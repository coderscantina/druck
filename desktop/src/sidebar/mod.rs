//! Left pane: the document's identity, then its contents list, the files not in the book, notes, and research.
//!
//! It holds no document. The workspace gives it [`SidebarData`] and hears [`SidebarEvent`]s.

pub mod dialogs;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, ElementId, EventEmitter, FontWeight, MouseButton, ObjectFit, SharedString, Window, div, img, svg};

use crate::session::{ChapterId, Contents, Row, RowKind};
use crate::tokens::{self, Tokens};

/// What a dot on a row says. It is derived, never stored: the Shell slice sets these with [`SidebarView::set_marks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "the Shell slice sets these")]
pub enum Mark {
    Problem,
    Placeholder,
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
    /// Move a chapter so it stands before the one now at this place in the book.
    Move(ChapterId, usize),
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
    data: SidebarData,
    marks: HashMap<ChapterId, Mark>,
    notice: Option<SharedString>,
    folded: HashSet<Fold>,
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

impl SidebarView {
    pub fn new() -> Self {
        Self {
            data: SidebarData::default(),
            marks: HashMap::new(),
            notice: None,
            folded: HashSet::new(),
        }
    }

    pub fn set_data(&mut self, data: SidebarData, cx: &mut Context<Self>) {
        if data != self.data {
            self.data = data;
            cx.notify();
        }
    }

    /// The dots of the rows. A chapter not in the map has none.
    #[allow(dead_code, reason = "the Shell slice calls this")]
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

    /// The number or icon a row starts with.
    fn lead(tokens: &Tokens, row: &Row, plain_file: bool, selected: bool) -> gpui::Div {
        let lead = div()
            .flex_none()
            .w(tokens::LIST_LEAD_WIDTH)
            .flex()
            .items_center()
            .text_color(if selected { tokens.text } else { tokens.text_faint });
        if let Some(number) = row.number {
            return lead.child(number.to_string());
        }
        let icon = match row.kind {
            _ if plain_file => "icons/file.svg",
            RowKind::Contents => "icons/contents.svg",
            RowKind::Unnumbered | RowKind::Numbered => "icons/book.svg",
            RowKind::Plain => "icons/title.svg",
        };
        let color = if selected { tokens.text } else { tokens.text_faint };
        lead.child(svg().path(icon).size(tokens::ICON_SIZE).text_color(color))
    }

    /// The slot at the end of a row. The Shell slice fills it through [`Self::set_marks`].
    fn dot(&self, tokens: &Tokens, id: ChapterId) -> gpui::Div {
        let slot = div()
            .flex_none()
            .w(tokens::LIST_DOT_WIDTH)
            .flex()
            .items_center()
            .justify_center();
        match self.marks.get(&id) {
            Some(Mark::Problem) => slot.child(div().size(tokens::DOT_SIZE).rounded_full().bg(tokens.text_quiet)),
            Some(Mark::Placeholder) => slot.child(
                div()
                    .size(tokens::DOT_SIZE)
                    .rounded_full()
                    .border_1()
                    .border_color(tokens.text_faint),
            ),
            None => slot,
        }
    }

    fn row(&self, row: &Row, plain_file: bool, tokens: &Tokens, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let id = row.id;
        let selected = self.data.active == Some(id);
        let color = if row.broken {
            tokens.accent
        } else if selected {
            tokens.text
        } else {
            tokens.text_quiet
        };
        div()
            .id(ElementId::from(("chapter", id.number())))
            .group("row")
            .flex_none()
            .h(tokens::LIST_ROW_HEIGHT)
            .pl(tokens::SPACE_3)
            .pr(tokens::SPACE_1)
            .flex()
            .items_center()
            .border_b_1()
            .border_color(tokens.divider)
            .when(selected, |row| row.bg(tokens.raised))
            .hover(|row| row.bg(tokens.raised))
            .cursor_pointer()
            .text_color(color)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(SidebarEvent::Select(id))))
            .child(Self::lead(tokens, row, plain_file, selected))
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
            .flex_none()
            .w(tokens::LIST_DOT_WIDTH)
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
            .child(edit)
            .child(self.dot(tokens, id))
    }

    fn file_row(&self, row: &Row, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        self.row(row, true, tokens, cx).child(self.dot(tokens, row.id))
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
        self.row(row, true, tokens, cx).child(add).child(self.dot(tokens, id))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let contents = self.data.contents.clone();
        let identity = div()
            .id("identity")
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
            self.fold_label(Fold::Notes, "NOTES", contents.notes.len(), &tokens, cx)
                .into_any_element()
        });
        let research_label = (!contents.research.is_empty()).then(|| {
            self.fold_label(Fold::Research, "RESEARCH", contents.research.len(), &tokens, cx)
                .into_any_element()
        });
        let list = div()
            .id("contents")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .when(!manifest.is_empty(), |list| {
                list.child(Self::label(&tokens, "MANIFEST")).children(manifest)
            })
            .child(Self::label(&tokens, "CONTENTS"))
            .children(book)
            .child(drop_at_end)
            .when(!unlisted.is_empty(), |list| {
                list.child(Self::label(&tokens, "NOT IN BOOK")).children(unlisted)
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
                    .w(tokens::SECTION_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_l_1()
                    .border_color(tokens.divider)
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
            .bg(tokens.panel)
            .border_r_1()
            .border_color(tokens.divider)
            .child(identity)
            .child(list)
            .children(notice)
            .child(bottom)
    }
}
