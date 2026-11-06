//! The Binder: the document's identity, then its contents list, the files not in the book, notes, and research. [`ticks`]
//! is the same contents at the edge of the bare page.
//!
//! It holds no document. The workspace gives it [`SidebarData`] and hears [`SidebarEvent`]s.

pub mod dialogs;
mod strings;
pub mod ticks;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    Context, ElementId, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, ObjectFit, Role,
    ScrollHandle, SharedString, Window, div, img, svg,
};

use self::strings::Key;
use crate::a11y::Control as _;
use crate::session::{ChapterId, Contents, Row, RowKind};
use crate::tokens::{self, Tokens};

/// What a dot on a row says: an error, such as a broken reference or citation, or an open placeholder. Page problems never
/// show here. It is derived, never stored: the shell sets these with [`SidebarView::set_marks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Problem,
    Placeholder,
}

/// The dot of a mark, with its slot at the end of a row.
pub(crate) fn mark_dot(tokens: &Tokens, mark: Option<Mark>) -> Option<gpui::Div> {
    let slot = div()
        .flex_none()
        .w(tokens::LIST_DOT_WIDTH)
        .flex()
        .items_center()
        .justify_center();
    match mark? {
        Mark::Problem => Some(slot.child(div().size(tokens::DOT_SIZE).rounded_full().bg(tokens.accent))),
        Mark::Placeholder => Some(
            slot.child(
                div()
                    .size(tokens::DOT_SIZE)
                    .rounded_full()
                    .border_1()
                    .border_color(tokens.text_faint),
            ),
        ),
    }
}

/// The number or icon a row starts with.
pub(crate) fn row_lead(tokens: &Tokens, row: &Row, plain_file: bool, selected: bool) -> gpui::Div {
    let color = if selected { tokens.text } else { tokens.text_faint };
    let lead = div()
        .flex_none()
        .w(tokens::LIST_LEAD_WIDTH)
        .flex()
        .items_center()
        .text_color(color);
    if let Some(number) = row.number {
        return lead.child(number.to_string());
    }
    let icon = match row.kind {
        _ if plain_file => "icons/file.svg",
        RowKind::Contents => "icons/contents.svg",
        RowKind::Unnumbered | RowKind::Numbered => "icons/book.svg",
        RowKind::Plain => "icons/title.svg",
    };
    lead.child(svg().path(icon).size(tokens::ICON_SIZE).text_color(color))
}

/// What the sidebar shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarData {
    pub title: String,
    pub authors: String,
    pub cover: Option<PathBuf>,
    pub contents: Contents,
    pub active: Option<ChapterId>,
    /// Why the document's settings cannot be read.
    pub problem: Option<String>,
}

pub enum SidebarEvent {
    Select(ChapterId),
    /// Open a chapter with the caret at an offset of its text, for a heading of the outline.
    Jump(ChapterId, usize),
    /// Give the keys back to the text.
    FocusText,
    /// Show a chapter in the galley while the keys stay in the sidebar, for arrowing through the list.
    Browse(ChapterId),
    /// Move a chapter so it stands before the one now at this place in the book.
    Move(ChapterId, usize),
    /// Put the book back in its order before the last move.
    UndoMove,
    /// Make the last undone move again.
    RedoMove,
    /// Set or clear a book chapter's writing state.
    SetStatus(ChapterId, Option<String>),
    AddToBook(ChapterId),
    Edit(ChapterId),
    NewSection,
    ShowDetails,
}

impl EventEmitter<SidebarEvent> for SidebarView {}

/// The lists that fold.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Fold {
    Notes,
    Research,
}

pub struct SidebarView {
    focus: FocusHandle,
    /// Whether the keys are in the sidebar, as of the last paint. Shows as a bar at the selected row.
    keyed: bool,
    _subscriptions: Vec<gpui::Subscription>,
    data: SidebarData,
    marks: HashMap<ChapterId, Mark>,
    notice: Option<SharedString>,
    folded: HashSet<Fold>,
    scroll: ScrollHandle,
}

/// A chapter being dragged.
#[derive(Clone)]
struct DraggedRow {
    id: ChapterId,
    title: SharedString,
}

impl Render for DraggedRow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .h(tokens::LIST_ROW_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .rounded(tokens::RADIUS)
            .bg(tokens.raised)
            .border_1()
            .border_color(tokens.divider)
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .text_color(tokens.text)
            .child(self.title.clone())
    }
}

/// The chapter a navigation key moves to from `current`: one step up or down, or the first or last of `ids`. Going
/// past either end stays on it.
fn neighbour(ids: &[ChapterId], current: Option<ChapterId>, key: &str) -> Option<ChapterId> {
    let at = current.and_then(|current| ids.iter().position(|id| *id == current));
    let target = match (key, at) {
        ("home", _) => 0,
        ("end", _) => ids.len().checked_sub(1)?,
        ("up", Some(at)) => at.saturating_sub(1),
        ("down", Some(at)) => (at + 1).min(ids.len().checked_sub(1)?),
        ("up" | "down", None) => 0,
        _ => return None,
    };
    ids.get(target).copied()
}

impl Focusable for SidebarView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SidebarView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
        ];
        Self {
            focus,
            keyed: false,
            _subscriptions: subscriptions,
            data: SidebarData::default(),
            marks: HashMap::new(),
            notice: None,
            folded: HashSet::new(),
            scroll: ScrollHandle::new(),
        }
    }

    /// The chapters the arrow keys visit, top to bottom: the book, files not in it, and the notes and research that
    /// are not folded.
    fn visible(&self) -> Vec<ChapterId> {
        let contents = &self.data.contents;
        let notes = (!self.folded.contains(&Fold::Notes)).then_some(&contents.notes);
        let research = (!self.folded.contains(&Fold::Research)).then_some(&contents.research);
        [Some(&contents.book), Some(&contents.not_in_book), notes, research]
            .into_iter()
            .flatten()
            .flatten()
            .map(|row| row.id)
            .collect()
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ids = self.visible();
        match event.keystroke.key.as_str() {
            // The keys for what a row's hover buttons and a drag do.
            "enter" if event.keystroke.modifiers.platform => {
                if let Some(id) = self.data.active {
                    cx.emit(if self.in_book(id) {
                        SidebarEvent::Edit(id)
                    } else {
                        SidebarEvent::AddToBook(id)
                    });
                }
            }
            key @ ("up" | "down") if event.keystroke.modifiers.alt => {
                if let Some((id, place)) = self.moved(key == "up") {
                    cx.emit(SidebarEvent::Move(id, place));
                }
            }
            "enter" | "escape" => {
                if let Some(id) = self.data.active {
                    cx.emit(SidebarEvent::Select(id));
                }
            }
            key @ ("up" | "down" | "home" | "end") => {
                let target = if event.keystroke.modifiers.platform && key == "up" {
                    "home"
                } else if event.keystroke.modifiers.platform && key == "down" {
                    "end"
                } else {
                    key
                };
                if let Some(id) = neighbour(&ids, self.data.active, target) {
                    cx.emit(SidebarEvent::Browse(id));
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The book's rows, in order.
    fn book_ids(&self) -> Vec<ChapterId> {
        self.data.contents.book.iter().map(|row| row.id).collect()
    }

    fn in_book(&self, id: ChapterId) -> bool {
        self.book_ids().contains(&id)
    }

    /// The active chapter and the place in the book that moves it one row up or down, if it can go.
    fn moved(&self, up: bool) -> Option<(ChapterId, usize)> {
        let id = self.data.active?;
        let book = self.book_ids();
        let at = book.iter().position(|other| *other == id)?;
        let place = if up { at.checked_sub(1)? } else { at + 2 };
        (place <= book.len()).then_some((id, place))
    }

    pub fn set_data(&mut self, data: SidebarData, cx: &mut Context<Self>) {
        if data != self.data {
            self.data = data;
            cx.notify();
        }
    }

    /// The dots of the rows. A chapter not in the map has none.
    pub fn set_marks(&mut self, marks: HashMap<ChapterId, Mark>, cx: &mut Context<Self>) {
        self.marks = marks;
        cx.notify();
    }

    /// A short note below the lists, such as why a change was not made.
    pub fn set_notice(&mut self, notice: Option<String>, cx: &mut Context<Self>) {
        self.notice = notice.map(SharedString::from);
        cx.notify();
    }

    fn label(tokens: &Tokens, text: &'static str) -> gpui::Div {
        div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tokens.text_quiet)
            .child(text)
    }

    /// The slot at the end of a row, which only a row with a dot has.
    fn dot(&self, tokens: &Tokens, id: ChapterId) -> Option<gpui::Div> {
        mark_dot(tokens, self.marks.get(&id).copied())
    }

    fn row(&self, row: &Row, plain_file: bool, tokens: &Tokens, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let id = row.id;
        let selected = self.data.active == Some(id);
        let keyed = selected && self.keyed;
        let color = if row.broken {
            tokens.accent
        } else if selected {
            tokens.text
        } else {
            tokens.text_quiet
        };
        div()
            .id(ElementId::from(("chapter", id.number())))
            .role(Role::ListItem)
            .aria_label(row.title.clone())
            .aria_selected(selected)
            .group("row")
            .flex_none()
            .h(tokens::LIST_ROW_HEIGHT)
            .pl(tokens::SPACE_3)
            .pr(tokens::SPACE_1)
            .flex()
            .items_center()
            .when(selected, |row| row.bg(tokens.raised))
            .relative()
            .when(keyed, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .w(tokens::KEY_BAR)
                        .bg(tokens.accent),
                )
            })
            .hover(|row| row.bg(tokens.raised))
            .cursor_pointer()
            .text_color(color)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(SidebarEvent::Select(id))))
            .child(row_lead(tokens, row, plain_file, selected))
            .child(div().flex_1().min_w_0().truncate().child(row.title.clone()))
    }

    /// A row in the book: it drags, takes a dragged row above it, and opens its edit dialog.
    fn book_row(&self, row: &Row, place: usize, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = row.id;
        let dragged = DraggedRow {
            id,
            title: row.title.clone().into(),
        };
        let accent = tokens.accent;
        let edit = div()
            .id(ElementId::from(("edit", id.number())))
            .absolute()
            .top_0()
            .right_0()
            .h_full()
            .w(tokens::LIST_DOT_WIDTH)
            .bg(tokens.raised)
            .invisible()
            .group_hover("row", |button| button.visible())
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                cx.emit(SidebarEvent::Edit(id));
            }))
            .child(
                svg()
                    .path("icons/more.svg")
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.text_quiet),
            );
        self.row(row, false, tokens, cx)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |_, _, _, cx| cx.emit(SidebarEvent::Edit(id))),
            )
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .drag_over::<DraggedRow>(move |style, _, _, _| style.border_t_2().border_color(accent))
            .on_drop(cx.listener(move |_, dragged: &DraggedRow, _, cx| {
                cx.emit(SidebarEvent::Move(dragged.id, place));
            }))
            .children(self.dot(tokens, id))
            .child(edit)
    }

    fn file_row(&self, row: &Row, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        self.row(row, true, tokens, cx).children(self.dot(tokens, row.id))
    }

    /// A file not in the book, with its one action.
    fn unlisted_row(&self, row: &Row, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = row.id;
        let add = div()
            .id(ElementId::from(("add", id.number())))
            .flex_none()
            .w(tokens::LIST_LEAD_WIDTH)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                cx.emit(SidebarEvent::AddToBook(id));
            }))
            .child(
                svg()
                    .path("icons/plus.svg")
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.text_quiet),
            );
        self.row(row, true, tokens, cx)
            .child(add)
            .children(self.dot(tokens, id))
    }

    fn fold_label(
        &self,
        fold: Fold,
        text: &'static str,
        count: usize,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = if fold == Fold::Notes {
            "fold-notes"
        } else {
            "fold-research"
        };
        Self::label(tokens, text)
            .id(id)
            .button(tokens, text)
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, _, cx| {
                if !view.folded.remove(&fold) {
                    view.folded.insert(fold);
                }
                cx.notify();
            }))
            .child(
                div()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(tokens.text_faint)
                    .child(count.to_string()),
            )
    }
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        self.keyed = self.focus.is_focused(window);
        let contents = self.data.contents.clone();
        let identity = div()
            .id("identity")
            .button(&tokens, crate::t!(Key::Details))
            .flex_none()
            .p(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_3)
            .bg(tokens.raised)
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| cx.emit(SidebarEvent::ShowDetails)))
            .when_some(self.data.cover.clone(), |identity, cover| {
                identity.child(
                    img(cover)
                        .flex_none()
                        .h(tokens::COVER_THUMB_HEIGHT)
                        .w(tokens::COVER_THUMB_WIDTH)
                        .object_fit(ObjectFit::Contain),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(self.data.title.clone()),
                    )
                    .child(
                        div()
                            .text_color(tokens.text_quiet)
                            .truncate()
                            .child(self.data.authors.clone()),
                    ),
            );
        let book: Vec<_> = (contents.book.iter().enumerate())
            .map(|(place, row)| self.book_row(row, place, &tokens, cx).into_any_element())
            .collect();
        let end = contents.book.len();
        let accent = tokens.accent;
        let drop_at_end = div()
            .id("drop-at-end")
            .flex_none()
            .h(tokens::SPACE_3)
            .drag_over::<DraggedRow>(move |style, _, _, _| style.border_t_2().border_color(accent))
            .on_drop(cx.listener(move |_, dragged: &DraggedRow, _, cx| {
                cx.emit(SidebarEvent::Move(dragged.id, end));
            }));
        let manifest: Vec<_> = (contents.manifest.iter())
            .map(|row| self.file_row(row, &tokens, cx).into_any_element())
            .collect();
        let unlisted: Vec<_> = (contents.not_in_book.iter())
            .map(|row| self.unlisted_row(row, &tokens, cx).into_any_element())
            .collect();
        let notes: Vec<_> = (contents.notes.iter())
            .filter(|_| !self.folded.contains(&Fold::Notes))
            .map(|row| self.file_row(row, &tokens, cx).into_any_element())
            .collect();
        let research: Vec<_> = (contents.research.iter())
            .filter(|_| !self.folded.contains(&Fold::Research))
            .map(|row| self.file_row(row, &tokens, cx).into_any_element())
            .collect();
        let notes_label = (!contents.notes.is_empty()).then(|| {
            self.fold_label(Fold::Notes, crate::t!(Key::Notes), contents.notes.len(), &tokens, cx)
                .into_any_element()
        });
        let research_label = (!contents.research.is_empty()).then(|| {
            self.fold_label(
                Fold::Research,
                crate::t!(Key::Research),
                contents.research.len(),
                &tokens,
                cx,
            )
            .into_any_element()
        });
        let list = div()
            .id("contents")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .when(!manifest.is_empty(), |list| {
                list.child(Self::label(&tokens, crate::t!(Key::Manifest)))
                    .children(manifest)
            })
            .child(Self::label(&tokens, crate::t!(Key::Contents)))
            .children(book)
            .child(drop_at_end)
            .when(!unlisted.is_empty(), |list| {
                list.child(Self::label(&tokens, crate::t!(Key::NotInBook)))
                    .children(unlisted)
            })
            .children(notes_label)
            .children(notes)
            .children(research_label)
            .children(research);
        let problem = self.data.problem.clone().map(SharedString::from);
        let notice = problem.or_else(|| self.notice.clone()).map(|notice| {
            div()
                .flex_none()
                .p(tokens::SPACE_2)
                .border_t_1()
                .border_color(tokens.divider)
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.accent)
                .child(notice)
        });
        let bottom = div()
            .flex_none()
            .h(tokens::FOOTER_HEIGHT)
            .flex()
            .justify_end()
            .border_t_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .id("new-section")
                    .button(&tokens, crate::t!(Key::NewSection))
                    .w(tokens::SECTION_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|button| button.bg(tokens.raised))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SidebarEvent::NewSection)))
                    .child(
                        svg()
                            .path("icons/plus.svg")
                            .size(tokens::ICON_SIZE)
                            .text_color(tokens.text_quiet),
                    ),
            );
        div()
            .flex_none()
            .w(tokens::SIDEBAR_WIDTH)
            .h_full()
            .flex()
            .flex_col()
            .id("sidebar")
            .role(Role::Navigation)
            .aria_label(crate::t!(Key::TheContents))
            .key_context("Sidebar")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|_, _: &crate::editor::Undo, _, cx| cx.emit(SidebarEvent::UndoMove)))
            .on_action(cx.listener(|_, _: &crate::editor::Redo, _, cx| cx.emit(SidebarEvent::RedoMove)))
            .bg(tokens.panel)
            .border_r_1()
            .border_color(tokens.divider)
            .child(identity)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(list)
                    .when(crate::shell::ui::more_below(&self.scroll), |list| {
                        list.child(crate::shell::ui::foot_fade(&tokens))
                    }),
            )
            .children(notice)
            .child(bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    fn ids() -> Vec<ChapterId> {
        let file = std::env::temp_dir().join(format!("druck-sidebar-{}.md", std::process::id()));
        std::fs::write(&file, "# A\n\n# B\n\n# C\n").expect("file");
        let session = Session::open(&file).expect("opens");
        std::fs::remove_file(&file).ok();
        session.book().to_vec()
    }

    #[test]
    fn arrows_step_and_stop_at_the_ends_and_home_and_end_jump() {
        let ids = ids();
        assert_eq!(neighbour(&ids, Some(ids[1]), "down"), Some(ids[2]));
        assert_eq!(neighbour(&ids, Some(ids[2]), "down"), Some(ids[2]));
        assert_eq!(neighbour(&ids, Some(ids[1]), "up"), Some(ids[0]));
        assert_eq!(neighbour(&ids, Some(ids[0]), "up"), Some(ids[0]));
        assert_eq!(neighbour(&ids, Some(ids[1]), "end"), Some(ids[2]));
        assert_eq!(neighbour(&ids, None, "down"), Some(ids[0]));
        assert_eq!(neighbour(&[], None, "home"), None);
    }
}
