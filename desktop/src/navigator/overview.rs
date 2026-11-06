//! The overview: the whole book laid out like its contents page, one entry per file in reading order, grouped by part,
//! each with its title, first line, and words, and the files outside the book set aside after the back matter. It is
//! the one place where the book's order changes: a dragged entry drops where a 2 px accent line shows, and Option+Up
//! and Option+Down move the focused one a place. The keys walk the entries, and Return opens the focused one. A chapter
//! of the book shows its writing state, which S moves to the next and a click on it chooses from a menu.

use gpui::prelude::*;
use gpui::{
    Anchor, Context, ElementId, FontWeight, KeyDownEvent, MouseButton, Role, ScrollHandle, SharedString, Window,
    anchored, deferred, div,
};

use super::book::{Book, Entry, Place, Shelf};
use super::strings::Key;
use super::{Navigator, group_label, grouped, hint};
use crate::session::{ChapterId, status_label};
use crate::sidebar::SidebarEvent;
use crate::tokens::{self, Tokens};

/// A file the keys can stop at: a part's opener in its label, or an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stop {
    id: ChapterId,
    /// Its place in the book, for a file in it.
    place: Option<usize>,
}

pub struct Overview {
    stops: Vec<Stop>,
    focus: usize,
    /// The stops of each line on screen, with their slots from the left, the label being slot 0. From the last paint,
    /// for Up and Down.
    lines: Vec<Vec<(usize, usize)>>,
    scroll: ScrollHandle,
    /// The open menu of writing states: whose, and which row the keys are on.
    menu: Option<(ChapterId, usize)>,
}

/// The stops in reading order: each group's opener, then its entries, then the files not in the book.
fn stops(book: &Book) -> Vec<Stop> {
    let stop = |entry: &Entry| Stop {
        id: entry.row.id,
        place: entry.place,
    };
    let grouped = book
        .groups
        .iter()
        .flat_map(|group| group.opener.iter().chain(&group.entries));
    let aside = (book.shelves.iter())
        .filter(|(shelf, _)| *shelf == Shelf::NotInBook)
        .flat_map(|(_, entries)| entries);
    grouped.chain(aside).map(stop).collect()
}

impl Overview {
    pub fn new(book: &Book, place: Option<&Place>) -> Self {
        let stops = stops(book);
        let focus = place
            .and_then(|place| stops.iter().position(|stop| stop.id == place.chapter))
            .unwrap_or(0);
        Self {
            stops,
            focus,
            lines: Vec::new(),
            scroll: ScrollHandle::new(),
            menu: None,
        }
    }

    /// Keeps the focus on the same file after the book was read again, as after a move.
    pub fn keep(&mut self, book: &Book) {
        let focused = self.stops.get(self.focus).map(|stop| stop.id);
        self.stops = stops(book);
        self.focus = focused
            .and_then(|id| self.stops.iter().position(|stop| stop.id == id))
            .unwrap_or(self.focus)
            .min(self.stops.len().saturating_sub(1));
        self.menu = self
            .menu
            .filter(|(id, _)| self.stops.iter().any(|stop| stop.id == *id && stop.place.is_some()));
    }

    pub fn step(&mut self, forward: bool) {
        self.focus_on(if forward {
            (self.focus + 1).min(self.stops.len().saturating_sub(1))
        } else {
            self.focus.saturating_sub(1)
        });
    }

    fn focus_on(&mut self, at: usize) {
        self.focus = at;
        if let Some(line) = self
            .lines
            .iter()
            .position(|line| line.iter().any(|(_, stop)| *stop == at))
        {
            // The header comes first among the scrolled children.
            self.scroll.scroll_to_item(line + 1);
        }
    }

    /// The stop in the line above or below, nearest to the focused one's slot.
    fn vertical(&self, down: bool) -> Option<usize> {
        let line = self
            .lines
            .iter()
            .position(|line| line.iter().any(|(_, stop)| *stop == self.focus))?;
        let slot = self.lines[line].iter().find(|(_, stop)| *stop == self.focus)?.0;
        let target = if down { line + 1 } else { line.checked_sub(1)? };
        let candidates = self.lines.get(target)?;
        candidates
            .iter()
            .min_by_key(|(other, _)| other.abs_diff(slot))
            .map(|(_, stop)| *stop)
    }

    /// Where the focused file goes one place earlier or later in the book, as the Binder's Option+arrows do.
    fn moved(&self, later: bool) -> Option<(ChapterId, usize)> {
        let stop = self.stops.get(self.focus)?;
        let place = stop.place?;
        let length = self.stops.iter().filter(|stop| stop.place.is_some()).count();
        let to = if later { place + 2 } else { place.checked_sub(1)? };
        (to <= length).then_some((stop.id, to))
    }
}

/// An entry being dragged.
#[derive(Clone)]
struct DraggedEntry {
    id: ChapterId,
    title: SharedString,
}

impl Render for DraggedEntry {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .bg(tokens.raised)
            .border_1()
            .border_color(tokens.divider)
            .shadow(tokens::float_shadow())
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .text_color(tokens.text)
            .child(self.title.clone())
    }
}

impl Navigator {
    pub(super) fn overview_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let super::Shown::Overview(overview) = &mut self.shown else {
            return false;
        };
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if let Some((id, row)) = overview.menu {
            let rows = self.parts.editor.read(cx).session().statuses().len() + 1;
            match key {
                "up" => overview.menu = Some((id, row.checked_sub(1).unwrap_or(rows - 1))),
                "down" => overview.menu = Some((id, (row + 1) % rows)),
                "enter" | "space" => self.choose_status(id, row, window, cx),
                "escape" => overview.menu = None,
                _ => {}
            }
            cx.notify();
            return true;
        }
        if key == "s" && !modifiers.alt && !modifiers.shift {
            if let Some(Stop { id, place: Some(_) }) = overview.stops.get(overview.focus).copied() {
                let next = self.parts.editor.read(cx).session().next_status(id);
                self.workspace_event(SidebarEvent::SetStatus(id, next), window, cx);
            }
            return true;
        }
        match (key, modifiers.alt) {
            (key @ ("up" | "down"), true) => {
                if let Some((id, place)) = overview.moved(key == "down") {
                    self.move_to(id, place, window, cx);
                }
            }
            (_, true) => return false,
            ("escape", _) => self.close(true, window, cx),
            ("enter", _) => {
                if let Some(stop) = overview.stops.get(overview.focus).copied() {
                    self.open(stop.id, None, window, cx);
                }
            }
            ("left", _) => overview.step(false),
            ("right", _) => overview.step(true),
            (key @ ("up" | "down"), _) => {
                if let Some(at) = overview.vertical(key == "down") {
                    overview.focus_on(at);
                }
            }
            ("home", _) => overview.focus_on(0),
            ("end", _) => overview.focus_on(overview.stops.len().saturating_sub(1)),
            _ => return false,
        }
        true
    }

    /// Opens the menu of writing states on a chapter's card, with the keys on its current state.
    fn open_status_menu(&mut self, id: ChapterId, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let statuses = session.statuses();
        let current = session.chapter(id).and_then(|chapter| chapter.status());
        let row = current
            .and_then(|status| statuses.iter().position(|known| known == status))
            .unwrap_or(statuses.len());
        if let super::Shown::Overview(overview) = &mut self.shown {
            if let Some(at) = overview.stops.iter().position(|stop| stop.id == id) {
                overview.focus = at;
            }
            overview.menu = Some((id, row));
        }
        cx.notify();
    }

    /// Opens the menu of writing states on the focused entry, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub(super) fn open_focused_status_menu(&mut self, cx: &mut Context<Self>) {
        let super::Shown::Overview(overview) = &self.shown else {
            return;
        };
        if let Some(Stop { id, place: Some(_) }) = overview.stops.get(overview.focus).copied() {
            self.open_status_menu(id, cx);
        }
    }

    /// Sets the state at a row of the menu, the last row being none, and closes the menu.
    fn choose_status(&mut self, id: ChapterId, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let super::Shown::Overview(overview) = &mut self.shown {
            overview.menu = None;
        }
        let status = self.parts.editor.read(cx).session().statuses().get(row).cloned();
        self.workspace_event(SidebarEvent::SetStatus(id, status), window, cx);
        cx.notify();
    }

    /// A click focuses an entry, and a second one opens it.
    fn entry_clicked(&mut self, id: ChapterId, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        let super::Shown::Overview(overview) = &mut self.shown else {
            return;
        };
        if let Some(at) = overview.stops.iter().position(|stop| stop.id == id) {
            overview.focus = at;
        }
        self.focus.focus(window, cx);
        if clicks > 1 {
            self.open(id, None, window, cx);
        }
        cx.notify();
    }

    pub(super) fn render_overview(
        &mut self,
        tokens: &Tokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let book = self.book.clone();
        let font = self.parts.editor.read(cx).body_font().family;
        let super::Shown::Overview(overview) = &self.shown else {
            return div().id("navigator-overview");
        };
        let width = window.viewport_size().width;
        let margin = if width < tokens::NAV_DESK_WIDE {
            tokens::SPACE_4
        } else {
            tokens::NAV_DESK_MARGIN
        };
        let room = width - margin * 2. - tokens::NAV_DESK_LABEL;
        let per_line = ((room / (tokens::NAV_DESK_CARD_MIN + tokens::SPACE_4)).floor() as usize)
            .clamp(1, tokens::NAV_DESK_COLUMNS);
        let focused = overview.stops.get(overview.focus).map(|stop| stop.id);
        let session = self.parts.editor.read(cx).session();
        let statuses = session.statuses();
        let desk = Desk {
            tokens,
            font: font.clone(),
            focused,
            per_line,
            statuses,
            menu: overview.menu,
        };
        let mut lines = Vec::new();
        let mut elements = Vec::new();
        let stop_of = |id: ChapterId| overview.stops.iter().position(|stop| stop.id == id);
        for (at, group) in book.groups.iter().enumerate() {
            let label = group_label(&book, at, true).unwrap_or_default();
            let cards: Vec<&Entry> = group.entries.iter().collect();
            let chunks: Vec<&[&Entry]> = if cards.is_empty() {
                vec![&[]]
            } else {
                cards.chunks(per_line).collect()
            };
            for (index, chunk) in chunks.iter().enumerate() {
                let first = index == 0;
                let last = index + 1 == chunks.len();
                let mut line = Vec::new();
                if first && let Some(stop) = group.opener.as_ref().and_then(|opener| stop_of(opener.row.id)) {
                    line.push((0, stop));
                }
                line.extend(
                    (chunk.iter().enumerate()).filter_map(|(slot, entry)| Some((slot + 1, stop_of(entry.row.id)?))),
                );
                lines.push(line);
                let label = first.then(|| desk.label(group, &label, cx));
                let end = last.then(|| group.end()).flatten();
                elements.push(desk.line(lines.len(), label, chunk, end, cx).into_any_element());
            }
        }
        let aside: Vec<&Entry> = (book.shelves.iter())
            .filter(|(shelf, _)| *shelf == Shelf::NotInBook)
            .flat_map(|(_, entries)| entries)
            .collect();
        let lists: Vec<(Shelf, usize)> = (book.shelves.iter())
            .filter(|(shelf, _)| *shelf != Shelf::NotInBook)
            .map(|(shelf, entries)| (*shelf, entries.len()))
            .collect();
        if !aside.is_empty() || !lists.is_empty() {
            let chunks: Vec<&[&Entry]> = if aside.is_empty() {
                vec![&[]]
            } else {
                aside.chunks(per_line).collect()
            };
            let count = chunks.len();
            for (index, chunk) in chunks.into_iter().enumerate() {
                lines.push(
                    (chunk.iter().enumerate())
                        .filter_map(|(slot, entry)| Some((slot + 1, stop_of(entry.row.id)?)))
                        .collect(),
                );
                let list = (index + 1 == count).then(|| desk.list(&lists));
                elements.push(desk.aside(chunk, list, cx).into_any_element());
            }
        }
        let details = self.parts.editor.read(cx).session().details().clone();
        let header = desk.header(&book, &details, cx);
        let scroll = overview.scroll.clone();
        if let super::Shown::Overview(overview) = &mut self.shown {
            overview.lines = lines;
        }
        div()
            .id("navigator-overview")
            .occlude()
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT)
            .left_0()
            .right_0()
            .bottom_0()
            .bg(tokens.canvas)
            .role(Role::List)
            .aria_label(crate::t!(Key::Overview))
            .child(
                div()
                    .id("navigator-overview-scroll")
                    // Ends above the line of keys, so a focused entry scrolled into view is never under it.
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom(tokens::FOOTER_HEIGHT)
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .px(margin)
                    .pt(tokens::NAV_DESK_TOP)
                    .pb(tokens::SPACE_4)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_3)
                    .child(header)
                    .children(elements),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(tokens::FOOTER_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(tokens::SPACE_4)
                    .bg(tokens.canvas)
                    .text_size(tokens::TEXT_SMALL)
                    .child(hint(tokens, crate::t!(Key::HintReturn), crate::t!(Key::HintOpen)))
                    .child(hint(tokens, "\u{2325}\u{2191}\u{2193}", crate::t!(Key::HintReorder)))
                    .child(hint(tokens, "S", crate::t!(Key::HintState)))
                    .child(hint(tokens, crate::t!(Key::HintEscape), crate::t!(Key::HintBack))),
            )
    }
}

/// What every part of the overview is drawn with.
struct Desk<'a> {
    tokens: &'a Tokens,
    font: SharedString,
    focused: Option<ChapterId>,
    per_line: usize,
    /// The book's writing states, for the menu.
    statuses: Vec<String>,
    menu: Option<(ChapterId, usize)>,
}

impl Desk<'_> {
    fn header(
        &self,
        book: &Book,
        details: &crate::session::Details,
        cx: &Context<Navigator>,
    ) -> impl IntoElement + use<> {
        let tokens = self.tokens;
        let words: Option<usize> = (book.groups.iter())
            .flat_map(|group| group.opener.iter().chain(&group.entries))
            .map(|entry| Navigator::words(entry.row.id, cx))
            .sum();
        let line = match (words, details.authors.is_empty()) {
            (Some(words), false) => crate::t!(Key::BookWords, authors = details.authors, count = grouped(words)),
            (Some(words), true) => crate::t!(Key::BookWordsAlone, count = grouped(words)),
            (None, _) => details.authors.clone(),
        };
        div()
            .flex()
            .items_baseline()
            .gap(tokens::SPACE_3)
            .pb(tokens::SPACE_3)
            .child(
                div()
                    .font_family(self.font.clone())
                    .text_size(tokens::NAV_DESK_TITLE)
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens.text)
                    .child(details.title.clone()),
            )
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(line),
            )
    }

    /// One line of a group: its label in the first, then its cards, empty slots, and after the group's last card the
    /// place a drop lands at its end.
    fn line(
        &self,
        key: usize,
        label: Option<gpui::AnyElement>,
        cards: &[&Entry],
        end: Option<usize>,
        cx: &mut Context<Navigator>,
    ) -> impl IntoElement + use<> {
        let line = div()
            .flex()
            .child(div().flex_none().w(tokens::NAV_DESK_LABEL).children(label))
            .children(cards.iter().map(|entry| self.card(entry, false, cx).into_any_element()));
        line.children((cards.len()..self.per_line).map(|_| div().flex_1().ml(tokens::SPACE_4).into_any_element()))
            .child(self.drop_slot(ElementId::from(("desk-end", key)), end, cx))
    }

    /// The files set aside, and the lists of research, notes, and probes after the last of them.
    fn aside(
        &self,
        cards: &[&Entry],
        list: Option<gpui::Div>,
        cx: &mut Context<Navigator>,
    ) -> impl IntoElement + use<> {
        let slots = cards.len() + usize::from(list.is_some());
        div()
            .flex()
            .child(div().flex_none().w(tokens::NAV_DESK_LABEL))
            .children(cards.iter().map(|entry| self.card(entry, true, cx).into_any_element()))
            .children(list.map(|list| {
                div()
                    .flex_1()
                    .min_w_0()
                    .ml(tokens::SPACE_4)
                    .child(list)
                    .into_any_element()
            }))
            .children((slots..self.per_line).map(|_| div().flex_1().ml(tokens::SPACE_4).into_any_element()))
            .child(div().flex_none().w(tokens::SPACE_4).ml(tokens::SPACE_2))
    }

    /// A group's label. A part's names its opening file, which the keys stop at, opens, and drags like an entry.
    fn label(&self, group: &super::book::Group, label: &str, cx: &mut Context<Navigator>) -> gpui::AnyElement {
        let tokens = self.tokens;
        let text = div()
            .font_family(self.font.clone())
            .text_size(tokens::NAV_DESK_LABEL_TEXT)
            .text_color(tokens.text_quiet)
            .line_clamp(2)
            .child(label.to_owned());
        let Some(opener) = &group.opener else {
            return div().py(tokens::NAV_DESK_CARD_Y).child(text).into_any_element();
        };
        let id = opener.row.id;
        let focused = self.focused == Some(id);
        self.draggable(
            div()
                .id(ElementId::from(("desk-part", id.number())))
                .role(Role::ListItem)
                .aria_label(label.to_owned())
                .aria_selected(focused)
                .p(tokens::NAV_DESK_CARD_Y)
                // Its words stand where the other labels' do, the grey around them reaching into the margin.
                .ml(-tokens::NAV_DESK_CARD_Y)
                .rounded(tokens::RADIUS)
                .cursor_pointer()
                .when(focused, |label| {
                    label.bg(tokens.raised).shadow(vec![crate::a11y::ring(tokens)])
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |navigator, event: &gpui::ClickEvent, window, cx| {
                    navigator.entry_clicked(id, event.click_count(), window, cx)
                }))
                .child(text),
            opener,
        )
        .into_any_element()
    }

    /// A slot of the line, with its share of the gap before it. With `place`, it takes a dropped entry there, which the
    /// 2 px line at its left shows.
    fn cell(&self, id: ElementId, place: Option<usize>, cx: &mut Context<Navigator>) -> gpui::Stateful<gpui::Div> {
        let tokens = self.tokens;
        let accent = tokens.accent;
        div()
            .id(id)
            .flex_1()
            .min_w_0()
            .ml(tokens::SPACE_2)
            .border_l(tokens::KEY_BAR)
            .border_color(gpui::transparent_black())
            .pl(tokens::SPACE_2 - tokens::KEY_BAR)
            .when_some(place, |cell, place| {
                cell.drag_over::<DraggedEntry>(move |style, _, _, _| style.border_color(accent))
                    .on_drop(cx.listener(move |navigator, dragged: &DraggedEntry, window, cx| {
                        navigator.move_to(dragged.id, place, window, cx);
                    }))
            })
    }

    /// The slot after a line's last card, which takes a drop at the group's end on the group's last line.
    fn drop_slot(&self, id: ElementId, end: Option<usize>, cx: &mut Context<Navigator>) -> impl IntoElement + use<> {
        self.cell(id, end, cx)
            .flex_none()
            .flex_basis(tokens::SPACE_4)
            .h(tokens::NAV_DESK_CARD_HEIGHT)
    }

    /// Lets a file of the book be dragged.
    fn draggable(&self, element: gpui::Stateful<gpui::Div>, entry: &Entry) -> gpui::Stateful<gpui::Div> {
        if entry.place.is_none() {
            return element;
        }
        let dragged = DraggedEntry {
            id: entry.row.id,
            title: entry.numbered_title().into(),
        };
        element.on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
    }

    /// An entry: its number and title, its first line, and its words, with its open placeholders in the accent color or
    /// that it is not in the book.
    fn card(&self, entry: &Entry, aside: bool, cx: &mut Context<Navigator>) -> impl IntoElement + use<> {
        let tokens = self.tokens;
        let id = entry.row.id;
        let focused = self.focused == Some(id);
        let words = Navigator::words(id, cx).map(|words| crate::t!(Key::Words, count = grouped(words)));
        let open = Navigator::open_placeholders(id, cx);
        let state = if aside {
            Some(div().child(crate::t!(Key::NotInBook)))
        } else {
            let open = (open > 0).then(|| {
                div()
                    .text_color(tokens.accent)
                    .child(crate::t!(Key::Open, count = open))
            });
            let status = self.status_word(id, entry.row.status.as_deref(), focused, cx);
            (open.is_some() || status.is_some())
                .then(|| div().flex().gap(tokens::SPACE_2).children(status).children(open))
        };
        let card = div()
            .id(ElementId::from(("desk-card", id.number())))
            .role(Role::ListItem)
            .aria_label(entry.row.title.clone())
            .aria_selected(focused)
            .h(tokens::NAV_DESK_CARD_HEIGHT)
            .px(tokens::NAV_DESK_CARD_X)
            .pt(tokens::NAV_DESK_CARD_Y)
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(if focused {
                gpui::transparent_black().into()
            } else {
                tokens.divider
            })
            .when(aside && !focused, |card| card.border_dashed())
            .when(focused, |card| {
                card.bg(tokens.raised)
                    .rounded(tokens::RADIUS)
                    .shadow(vec![crate::a11y::ring(tokens)])
            })
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |navigator, event: &gpui::ClickEvent, window, cx| {
                navigator.entry_clicked(id, event.click_count(), window, cx)
            }))
            .child(
                div()
                    .flex()
                    .gap(tokens::NAV_DESK_NUMBER_GAP)
                    .font_family(self.font.clone())
                    .text_size(tokens::NAV_DESK_CARD_TITLE)
                    .line_height(tokens::NAV_DESK_TITLE_LEADING)
                    .children(entry.row.number.map(|number| {
                        div()
                            .flex_none()
                            .text_color(tokens.text_faint)
                            .child(number.to_string())
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(if aside { tokens.text_quiet } else { tokens.text })
                            .child(entry.row.title.clone()),
                    ),
            )
            .child(
                div()
                    .mt(tokens::HAIR_GAP)
                    .font_family(self.font.clone())
                    .flex_none()
                    .text_size(tokens::NAV_DESK_FIRST_LINE)
                    .line_height(tokens::NAV_DESK_FIRST_LINE_LEADING)
                    .text_color(tokens.text_quiet)
                    .line_clamp(2)
                    .text_ellipsis()
                    .children(entry.opening.first().cloned()),
            )
            .child(
                div()
                    .mt_auto()
                    .pb(tokens::NAV_DESK_CARD_Y)
                    .flex()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .children(words)
                    .children(state.map(|state| state.ml_auto())),
            );
        let place = (!aside).then_some(entry.place).flatten();
        self.cell(ElementId::from(("desk-cell", id.number())), place, cx)
            .child(self.draggable(card, entry))
    }

    /// A chapter's writing state in the quiet colour, and on the focused card without one a fainter offer to set it. A
    /// click opens the menu of states below it.
    fn status_word(
        &self,
        id: ChapterId,
        status: Option<&str>,
        focused: bool,
        cx: &mut Context<Navigator>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let tokens = self.tokens;
        let (word, color) = match status {
            Some(status) => (status_label(status), tokens.text_quiet),
            None if focused || self.menu.is_some_and(|(menu, _)| menu == id) => {
                (crate::t!(Key::NoState).to_owned(), tokens.text_faint)
            }
            None => return None,
        };
        let menu = (self.menu.filter(|(menu, _)| *menu == id)).map(|(_, row)| self.status_menu(id, status, row, cx));
        Some(
            div()
                .id(ElementId::from(("desk-status", id.number())))
                .relative()
                .text_color(color)
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |navigator, _, _, cx| navigator.open_status_menu(id, cx)))
                .child(word)
                .children(menu.map(|menu| {
                    div().absolute().top_full().right_0().child(
                        deferred(anchored().anchor(Anchor::TopRight).child(menu))
                            .with_priority(tokens::LAYER_QUESTION + 1),
                    )
                })),
        )
    }

    /// The book's states and none, the chapter's own or the row the keys are on raised.
    fn status_menu(
        &self,
        id: ChapterId,
        current: Option<&str>,
        row: usize,
        cx: &mut Context<Navigator>,
    ) -> impl IntoElement + use<> {
        let tokens = self.tokens;
        let names = (self.statuses.iter())
            .map(|status| (status_label(status), current == Some(status.as_str())))
            .chain([(crate::t!(Key::NoState).to_owned(), current.is_none())]);
        let rows = names.enumerate().map(|(at, (name, active))| {
            crate::inspector::widgets::menu_item(tokens, ("desk-state", at), name, None, active)
                .when(at == row, |item| item.bg(tokens.raised))
                .on_click(cx.listener(move |navigator, _, window, cx| navigator.choose_status(id, at, window, cx)))
        });
        div()
            .id("desk-status-menu")
            .occlude()
            .role(Role::Menu)
            .min_w(tokens::NAV_DESK_CARD_MIN / 2.)
            .mt(tokens::SPACE_1)
            .py(tokens::SPACE_1)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow(tokens::card_shadow())
            .text_size(tokens::TEXT_BODY)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|navigator, _, _, cx| {
                if let super::Shown::Overview(overview) = &mut navigator.shown {
                    overview.menu = None;
                }
                cx.notify();
            }))
            .children(rows)
    }

    /// Research, notes, and probes, each with how many files it holds.
    fn list(&self, lists: &[(Shelf, usize)]) -> gpui::Div {
        let tokens = self.tokens;
        div()
            .h(tokens::NAV_DESK_CARD_HEIGHT)
            .px(tokens::NAV_DESK_CARD_X)
            .pt(tokens::NAV_DESK_CARD_Y)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .border_t_1()
            .border_dashed()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .children(lists.iter().map(|(shelf, count)| {
                let name = crate::t!(match shelf {
                    Shelf::NotInBook => Key::NotInBook,
                    Shelf::Research => Key::Research,
                    Shelf::Notes => Key::Notes,
                    Shelf::Probes => Key::Probes,
                });
                div().flex().text_color(tokens.text_quiet).child(name).child(
                    div().ml_auto().text_color(tokens.text_faint).child(if *count == 1 {
                        crate::t!(Key::OneFile).to_owned()
                    } else {
                        crate::t!(Key::Files, count = count)
                    }),
                )
            }))
    }
}
