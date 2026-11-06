//! The bare page's outline: one short tick per heading of the open chapter at the left edge, the section holding the
//! caret in full ink. As the pointer comes near, the ticks under it grow and the nearest title shows. Resting on them
//! unfolds the strip in place into the outline in the book's face, with the part and the neighbouring chapters. A tick
//! never marks a page problem.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, KeyDownEvent, MouseButton,
    Pixels, Role, ScrollHandle, Subscription, Task, Window, div, px,
};

use super::strings::Key;
use super::{SidebarData, SidebarEvent};
use crate::editor::EditorView;
use crate::navigator::book;
use crate::session::outline::{self, Heading};
use crate::session::{ChapterId, Row};
use crate::tokens::{self, Tokens};

/// How long the pointer rests on one tick before the strip unfolds, so passing the edge opens nothing.
const DWELL: Duration = Duration::from_millis(350);

impl EventEmitter<SidebarEvent> for TicksView {}

/// A line of the unfolded outline.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Part(String),
    /// The open chapter's name, when its text has no level 1 heading to show it.
    Title(String),
    Chapter(ChapterId, String),
    Heading {
        level: u8,
        title: String,
        start: usize,
        current: bool,
    },
}

impl Entry {
    fn selectable(&self) -> bool {
        !matches!(self, Entry::Part(_) | Entry::Title(_))
    }
}

/// The headings of the open chapter, read again when its text or the chapter changes.
struct Read {
    chapter: ChapterId,
    revision: u64,
    headings: Vec<Heading>,
    /// The heading that holds the caret.
    caret: Option<usize>,
}

pub struct TicksView {
    editor: Entity<EditorView>,
    data: SidebarData,
    read: Option<Read>,
    /// The tick the pointer is nearest to.
    near: Option<usize>,
    dwell: Task<()>,
    /// The unfolded outline's lines, empty while the strip is folded.
    entries: Vec<Entry>,
    /// The selectable entry the pointer or the keys are on.
    lit: Option<usize>,
    /// Whether the keys are in the outline.
    keyed: bool,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
    /// Stays unfolded whatever the pointer does, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pinned: Option<Pin>,
}

/// What a screenshot holds the strip in.
#[cfg(any(debug_assertions, feature = "measure"))]
#[derive(Clone, Copy)]
enum Pin {
    Unfolded,
    Near,
}

impl Focusable for TicksView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TicksView {
    pub fn new(editor: &Entity<EditorView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.observe(editor, |_, _, cx| cx.notify()),
            cx.on_focus_out(&focus, window, |ticks, _, _, cx| {
                if ticks.keyed {
                    ticks.fold(cx);
                }
            }),
        ];
        Self {
            editor: editor.clone(),
            data: SidebarData::default(),
            read: None,
            near: None,
            dwell: Task::ready(()),
            entries: Vec::new(),
            lit: None,
            keyed: false,
            focus,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
            #[cfg(any(debug_assertions, feature = "measure"))]
            pinned: None,
        }
    }

    pub fn set_data(&mut self, data: SidebarData, cx: &mut Context<Self>) {
        if data != self.data {
            self.data = data;
            #[cfg(any(debug_assertions, feature = "measure"))]
            self.repin(cx);
            cx.notify();
        }
    }

    fn unfolded(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Reads the headings again when the chapter or its text is not the one last read.
    fn refresh(&mut self, cx: &Context<Self>) -> bool {
        let editor = self.editor.read(cx);
        let chapter = editor.session().active();
        let revision = editor.revision();
        let fresh = self
            .read
            .as_ref()
            .is_some_and(|read| read.chapter == chapter && read.revision == revision);
        let headings = if fresh {
            None
        } else {
            Some(outline::headings(editor.text()))
        };
        let caret = editor.selection_range().start;
        let read = self.read.take();
        let headings = headings.unwrap_or_else(|| read.map(|read| read.headings).unwrap_or_default());
        let at = headings.iter().rposition(|heading| heading.start <= caret);
        if !fresh && self.unfolded() {
            self.entries.clear();
            self.lit = None;
            self.keyed = false;
        }
        self.read = Some(Read {
            chapter,
            revision,
            headings,
            caret: at,
        });
        fresh
    }

    fn headings(&self) -> &[Heading] {
        self.read.as_ref().map_or(&[], |read| &read.headings)
    }

    fn fold(&mut self, cx: &mut Context<Self>) {
        #[cfg(any(debug_assertions, feature = "measure"))]
        if self.pinned.is_some() {
            return;
        }
        self.dwell = Task::ready(());
        self.entries.clear();
        self.lit = None;
        self.near = None;
        self.keyed = false;
        cx.notify();
    }

    /// Opens the outline at `heading`, or at the caret's.
    fn unfold(&mut self, heading: Option<usize>, cx: &mut Context<Self>) {
        let Some(read) = self.read.as_ref() else {
            return;
        };
        let session = self.editor.read(cx).session();
        let book: Vec<&Row> = self.data.contents.book.iter().collect();
        let at = book.iter().position(|row| row.id == read.chapter);
        let label = |row: &Row| match row.number {
            Some(number) => format!("{number} {}", row.title),
            None => row.title.clone(),
        };
        // The part as the path bar reads it, unless the open chapter is the file that opens it.
        let structure = book::structure(&self.data.contents.book, &|id| session.buffer(id).text());
        let group = (structure.groups.iter())
            .find(|group| group.holds(read.chapter))
            .filter(|group| group.opener.as_ref().is_none_or(|opener| opener.row.id != read.chapter));
        let opener = group
            .and_then(|group| group.opener.as_ref())
            .map(|opener| opener.row.id);
        let mut entries = Vec::new();
        entries.extend(
            group
                .and_then(|group| group.title())
                .map(|title| Entry::Part(title.to_owned())),
        );
        // The part's own page is the part line already, so it does not stand again as the chapter before.
        if let Some(row) = (at.and_then(|at| at.checked_sub(1)).map(|at| book[at])).filter(|row| Some(row.id) != opener)
        {
            entries.push(Entry::Chapter(row.id, label(row)));
        }
        let open_row = at.map(|at| book[at]);
        if read.headings.first().is_none_or(|first| first.level != 1) {
            entries.extend(open_row.map(|row| Entry::Title(label(row))));
        }
        let first_heading = entries.len();
        for (index, heading) in read.headings.iter().enumerate() {
            let title = match (index, heading.level, open_row) {
                (0, 1, Some(row)) => match row.number {
                    Some(number) => format!("{number} {}", heading.title),
                    None => heading.title.clone(),
                },
                _ => heading.title.clone(),
            };
            entries.push(Entry::Heading {
                level: heading.level,
                title,
                start: heading.start,
                current: read.caret == Some(index),
            });
        }
        if let Some(row) = at.map(|at| at + 1).and_then(|next| book.get(next)) {
            entries.push(Entry::Chapter(row.id, label(row)));
        }
        let wanted = heading.or(read.caret).map(|index| first_heading + index);
        self.lit = wanted
            .filter(|index| entries.get(*index).is_some_and(Entry::selectable))
            .or_else(|| entries.iter().position(Entry::selectable));
        self.entries = entries;
        if let Some(lit) = self.lit {
            self.scroll.scroll_to_item(lit);
        }
        cx.notify();
    }

    /// Opens the outline for the keys: they walk it until Return or Escape.
    pub fn open_with_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
        self.unfold(None, cx);
        if self.unfolded() {
            self.keyed = true;
            window.focus(&self.focus, cx);
        }
    }

    /// Unfolds and keeps the outline open, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn pin_open(&mut self, cx: &mut Context<Self>) {
        self.pinned = Some(Pin::Unfolded);
        self.repin(cx);
    }

    /// Shows the approach, the pointer near the second tick after the caret's, and keeps it, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn pin_near(&mut self, cx: &mut Context<Self>) {
        self.pinned = Some(Pin::Near);
        self.repin(cx);
    }

    /// Puts a screenshot's state back after the chapter or the contents changed under it.
    #[cfg(any(debug_assertions, feature = "measure"))]
    fn repin(&mut self, cx: &mut Context<Self>) {
        let Some(pin) = self.pinned else {
            return;
        };
        self.refresh(cx);
        self.entries.clear();
        match pin {
            Pin::Unfolded => self.unfold(None, cx),
            Pin::Near => {
                let caret = self.read.as_ref().and_then(|read| read.caret).unwrap_or(0);
                self.near = Some((caret + 2).min(self.headings().len().saturating_sub(1)));
            }
        }
        cx.notify();
    }

    /// The pointer is on tick `index`: the strip grows around it, and unfolds when the pointer stays.
    fn approached(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.near == Some(index) {
            return;
        }
        self.near = Some(index);
        self.dwell = cx.spawn(async move |ticks, cx| {
            cx.background_executor().timer(DWELL).await;
            ticks
                .update(cx, |ticks, cx| {
                    if ticks.near == Some(index) && !ticks.unfolded() {
                        ticks.unfold(Some(index), cx);
                    }
                })
                .ok();
        });
        cx.notify();
    }

    fn activate(&mut self, entry: usize, cx: &mut Context<Self>) {
        let chapter = self.read.as_ref().map(|read| read.chapter);
        match (self.entries.get(entry).cloned(), chapter) {
            (Some(Entry::Chapter(id, _)), _) => {
                self.finish(cx);
                cx.emit(SidebarEvent::Select(id));
            }
            (Some(Entry::Heading { start, .. }), Some(chapter)) => {
                self.finish(cx);
                cx.emit(SidebarEvent::Jump(chapter, start));
            }
            _ => {}
        }
    }

    /// Folds even a pinned outline, once a choice is made.
    fn finish(&mut self, cx: &mut Context<Self>) {
        #[cfg(any(debug_assertions, feature = "measure"))]
        {
            self.pinned = None;
        }
        self.fold(cx);
    }

    fn walk(&mut self, step: isize, cx: &mut Context<Self>) {
        let selectable: Vec<usize> = (0..self.entries.len())
            .filter(|index| self.entries[*index].selectable())
            .collect();
        let Some(at) = self
            .lit
            .and_then(|lit| selectable.iter().position(|index| *index == lit))
        else {
            self.lit = selectable.first().copied();
            cx.notify();
            return;
        };
        let to = (at as isize + step).clamp(0, selectable.len() as isize - 1) as usize;
        self.lit = Some(selectable[to]);
        self.scroll.scroll_to_item(selectable[to]);
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.walk(1, cx),
            "up" => self.walk(-1, cx),
            "home" => self.walk(isize::MIN / 2, cx),
            "end" => self.walk(isize::MAX / 2, cx),
            "enter" => {
                if let Some(lit) = self.lit {
                    self.activate(lit, cx);
                }
            }
            "escape" => {
                self.finish(cx);
                cx.emit(SidebarEvent::FocusText);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// How wide the tick at `index` is, against how near the pointer's tick stands.
    fn width(&self, index: usize, level: u8, step: Pixels) -> Pixels {
        let base = tokens::TICK_WIDTHS[usize::from(level.clamp(1, 3)) - 1];
        let Some(near) = self.near else {
            return base;
        };
        let distance = step * near.abs_diff(index) as f32;
        let reach = (1. - distance / tokens::TICK_REACH).max(0.);
        base * (1. + (tokens::TICK_GROWTH - 1.) * reach)
    }

    fn ticks(&self, room: Pixels, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let headings = self.headings();
        let caret = self.read.as_ref().and_then(|read| read.caret);
        let step = tokens::TICK_STEP.min((room - tokens::SPACE_4 * 2.) / headings.len().max(1) as f32);
        let rows = headings.iter().enumerate().map(|(index, heading)| {
            let current = caret == Some(index);
            let near = self.near == Some(index);
            let tick = div()
                .h(if near { px(2.) } else { tokens::TICK_HEIGHT })
                .w(self.width(index, heading.level, step))
                .rounded(px(1.))
                .bg(if current {
                    tokens.text
                } else if near {
                    tokens.text_quiet
                } else {
                    tokens.text_faint
                })
                .when(!current && !near, |tick| tick.opacity(tokens::TICK_OPACITY));
            div()
                .id(ElementId::from(("heading-tick", index)))
                .relative()
                .flex_none()
                .h(step)
                .w(tokens::TICK_ZONE_WIDTH)
                .pl(tokens::TICK_LEFT)
                .flex()
                .items_center()
                .on_hover(cx.listener(move |ticks, hovered: &bool, _, cx| {
                    if *hovered {
                        ticks.approached(index, cx);
                    }
                }))
                .child(tick)
                .when(near, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left(px(46.))
                            .whitespace_nowrap()
                            .px(tokens::SPACE_1)
                            .rounded(tokens::RADIUS)
                            .bg(tokens.canvas)
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_quiet)
                            .child(heading.title.clone()),
                    )
                })
        });
        div().flex().flex_col().children(rows)
    }

    fn outline(&self, room: Pixels, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let editor = self.editor.read(cx);
        let book_face = editor.body_font();
        let rows: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let lit = self.lit == Some(index);
                let row = div()
                    .id(ElementId::from(("outline-row", index)))
                    .relative()
                    .flex_none()
                    .h(tokens::OUTLINE_ROW_HEIGHT)
                    .w_full()
                    .pl(tokens::TICK_ZONE_WIDTH)
                    .pr(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .overflow_hidden();
                match entry {
                    Entry::Part(name) => row
                        .font_family(tokens::FONT_UI)
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_faint)
                        .child(name.clone()),
                    Entry::Title(name) => row
                        .text_color(tokens.text)
                        .font_weight(FontWeight::BOLD)
                        .child(name.clone()),
                    Entry::Chapter(id, name) => row
                        .role(Role::ListItem)
                        .aria_label(name.clone())
                        .cursor_pointer()
                        .text_color(if lit { tokens.text } else { tokens.text_faint })
                        .on_hover(cx.listener(move |ticks, hovered: &bool, _, cx| {
                            if *hovered {
                                ticks.lit = Some(index);
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener({
                            let id = *id;
                            move |ticks, _, _, cx| {
                                ticks.finish(cx);
                                cx.emit(SidebarEvent::Select(id));
                            }
                        }))
                        .child(name.clone()),
                    Entry::Heading {
                        level, title, current, ..
                    } => {
                        let first = *level == 1;
                        let width = tokens::TICK_WIDTHS[usize::from((*level).clamp(1, 3)) - 1];
                        let indent = tokens::OUTLINE_INDENT * f32::from(level.saturating_sub(2));
                        row.role(Role::ListItem)
                            .aria_label(title.clone())
                            .aria_selected(*current)
                            .cursor_pointer()
                            .pl(tokens::TICK_ZONE_WIDTH + indent)
                            .text_color(if lit || first { tokens.text } else { tokens.text_quiet })
                            .when(first, |row| row.font_weight(FontWeight::BOLD))
                            .on_hover(cx.listener(move |ticks, hovered: &bool, _, cx| {
                                if *hovered {
                                    ticks.lit = Some(index);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |ticks, _, _, cx| ticks.activate(index, cx)))
                            .child(
                                div()
                                    .absolute()
                                    .left(tokens::TICK_LEFT)
                                    .h(if lit { px(2.) } else { tokens::TICK_HEIGHT })
                                    .w(width)
                                    .rounded(px(1.))
                                    .bg(if *current {
                                        tokens.text
                                    } else if lit {
                                        tokens.text_quiet
                                    } else {
                                        tokens.text_faint
                                    }),
                            )
                            .child(title.clone())
                    }
                }
            })
            .collect();
        let height = tokens::OUTLINE_ROW_HEIGHT * rows.len() as f32 + tokens::SPACE_2;
        let available = room - tokens::SPACE_4 * 2.;
        let top = ((room - height) / 2.).max(tokens::SPACE_4);
        div()
            .id("outline")
            .role(Role::List)
            .aria_label(crate::t!(Key::Outline))
            .absolute()
            .top(top)
            .left_0()
            .w(tokens::OUTLINE_WIDTH)
            .max_h(available)
            .py(tokens::SPACE_1)
            .bg(tokens.canvas)
            .font(book_face)
            .text_size(tokens::TEXT_BODY)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .on_hover(cx.listener(|ticks, hovered: &bool, _, cx| {
                if !*hovered && !ticks.keyed {
                    ticks.fold(cx);
                }
            }))
            .children(rows)
    }
}

impl Render for TicksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fresh = self.refresh(cx);
        #[cfg(any(debug_assertions, feature = "measure"))]
        if !fresh && self.pinned.is_some() {
            self.repin(cx);
        }
        #[cfg(not(any(debug_assertions, feature = "measure")))]
        let _ = fresh;
        let tokens = Tokens::get(cx).clone();
        let room = window.viewport_size().height - tokens::TITLEBAR_HEIGHT - tokens::FOOTER_HEIGHT;
        let count = self.headings().len();
        let root = div().id("heading-ticks").absolute().top_0().left_0();
        if self.unfolded() {
            let veil = Hsla::from(tokens.canvas).opacity(1. - tokens::OUTLINE_TEXT_OPACITY);
            return root
                .size_full()
                .occlude()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::on_key))
                .child(
                    div()
                        .id("outline-veil")
                        .absolute()
                        .size_full()
                        .bg(veil)
                        .on_mouse_down(MouseButton::Left, cx.listener(|ticks, _, _, cx| ticks.fold(cx))),
                )
                .child(self.outline(room, &tokens, cx))
                .into_any_element();
        }
        if count == 0 {
            return root.into_any_element();
        }
        let step = tokens::TICK_STEP.min((room - tokens::SPACE_4 * 2.) / count as f32);
        let span = step * count as f32;
        root.top(((room - span) / 2.).max(tokens::SPACE_4))
            .w(tokens::TICK_ZONE_WIDTH)
            .h(span)
            .on_hover(cx.listener(|ticks, hovered: &bool, _, cx| {
                if !*hovered {
                    ticks.fold(cx);
                }
            }))
            .child(self.ticks(room, &tokens, cx))
            .into_any_element()
    }
}
