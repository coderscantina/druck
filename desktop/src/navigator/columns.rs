//! The columns a path segment opens over the text: parts, then chapters with their words, then headings, and the chosen
//! chapter's opening lines last. Files outside the book stand at the foot of the parts column under a divider. Arrow
//! keys walk them, typing selects by title, Return opens, and Escape closes.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{App, Context, ElementId, FontWeight, KeyDownEvent, MouseButton, Pixels, Role, ScrollHandle, Window, div};

use super::book::{Book, Kind, Place};
use super::strings::Key;
use super::{Level, Navigator, group_label, grouped, hint};
use crate::a11y::Control as _;
use crate::session::ChapterId;
use crate::tokens::{self, Tokens};

/// How long typed letters add up to one title.
const TYPE_PAUSE: Duration = Duration::from_millis(1000);

/// What the columns have chosen, and which has the keys.
pub struct Columns {
    pub level: Level,
    /// The chosen list of the parts column: a group, or a shelf after the groups.
    list: usize,
    chapter: Option<ChapterId>,
    heading: Option<usize>,
    typed: String,
    typed_at: Option<Instant>,
    scrolls: [ScrollHandle; 3],
}

impl Columns {
    /// Opens on the caret's place, with the keys at `level`.
    pub fn new(book: &Book, place: Option<&Place>, level: Level) -> Self {
        let list = place.map_or(0, |place| place.group);
        let in_list = |id: ChapterId| book.entries(list).iter().any(|entry| entry.row.id == id);
        let chapter = place.map(|place| place.chapter).filter(|id| in_list(*id));
        let mut columns = Self {
            level,
            list,
            chapter,
            heading: chapter.and(place.and_then(|place| place.section)),
            typed: String::new(),
            typed_at: None,
            scrolls: Default::default(),
        };
        columns.settle(book);
        columns
    }

    /// Keeps the chosen file after the book was read again, wherever it went.
    pub fn keep(&mut self, book: &Book) {
        if let Some(id) = self.chapter
            && let Some(list) = (0..book.lists()).find(|list| book.entries(*list).iter().any(|e| e.row.id == id))
        {
            self.list = list;
        }
        self.settle(book);
    }

    /// Fills in what a column with the keys needs, and drops what no longer exists.
    fn settle(&mut self, book: &Book) {
        self.list = self.list.min(book.lists().saturating_sub(1));
        let entries = book.entries(self.list);
        if !(self.chapter).is_some_and(|id| entries.iter().any(|entry| entry.row.id == id)) {
            self.chapter = None;
            self.heading = None;
        }
        if self.level >= Level::Chapters && self.chapter.is_none() {
            self.chapter = entries.first().map(|entry| entry.row.id);
        }
        let headings = self
            .chapter
            .and_then(|id| book.entry(id))
            .map_or(0, |entry| entry.headings.len());
        self.heading = self.heading.filter(|at| *at < headings);
        if self.level == Level::Headings && self.heading.is_none() {
            self.heading = (headings > 0).then_some(0);
        }
        if self.level == Level::Headings && headings == 0 {
            self.level = Level::Chapters;
        }
        if self.level == Level::Chapters && self.chapter.is_none() {
            self.level = Level::Parts;
        }
    }

    fn chapter_index(&self, book: &Book) -> Option<usize> {
        let id = self.chapter?;
        book.entries(self.list).iter().position(|entry| entry.row.id == id)
    }

    /// How many rows the column with the keys has, and which is chosen.
    fn rows(&self, book: &Book) -> (usize, Option<usize>) {
        match self.level {
            Level::Parts => (book.lists(), Some(self.list)),
            Level::Chapters => (book.entries(self.list).len(), self.chapter_index(book)),
            Level::Headings => (
                self.chapter
                    .and_then(|id| book.entry(id))
                    .map_or(0, |entry| entry.headings.len()),
                self.heading,
            ),
        }
    }

    /// Chooses row `at` of the column with the keys.
    fn choose(&mut self, book: &Book, at: usize) {
        match self.level {
            Level::Parts => {
                if at != self.list {
                    self.list = at;
                    self.chapter = None;
                    self.heading = None;
                }
            }
            Level::Chapters => {
                let id = book.entries(self.list).get(at).map(|entry| entry.row.id);
                if id != self.chapter {
                    self.chapter = id;
                    self.heading = None;
                }
            }
            Level::Headings => self.heading = Some(at),
        }
        self.scrolls[self.level as usize].scroll_to_item(at);
    }

    /// Moves one row down or up in the column with the keys, staying at its ends.
    pub fn step(&mut self, book: &Book, forward: bool) {
        let (count, chosen) = self.rows(book);
        let Some(last) = count.checked_sub(1) else {
            return;
        };
        let at = match (chosen, forward) {
            (None, _) => 0,
            (Some(at), true) => (at + 1).min(last),
            (Some(at), false) => at.saturating_sub(1),
        };
        self.choose(book, at);
    }

    /// Gives the keys to the column at the left or right, if it has rows.
    fn side(&mut self, book: &Book, right: bool) {
        let level = match (self.level, right) {
            (Level::Parts, true) => Level::Chapters,
            (Level::Chapters, true) => Level::Headings,
            (Level::Headings, false) => Level::Chapters,
            (Level::Chapters, false) => Level::Parts,
            (level, _) => level,
        };
        let before = self.level;
        self.level = level;
        if self.rows(book).0 == 0 {
            self.level = before;
            return;
        }
        self.settle(book);
    }

    /// Chooses the first row whose title starts with what was typed, or else holds it.
    fn type_select(&mut self, book: &Book, typed: &str) {
        let now = Instant::now();
        if self.typed_at.is_none_or(|at| now.duration_since(at) > TYPE_PAUSE) {
            self.typed.clear();
        }
        self.typed_at = Some(now);
        self.typed.push_str(&typed.to_lowercase());
        let titles: Vec<String> = match self.level {
            Level::Parts => (0..book.lists())
                .map(|list| group_label(book, list, true).unwrap_or_default())
                .collect(),
            Level::Chapters => book
                .entries(self.list)
                .iter()
                .map(|entry| entry.row.title.clone())
                .collect(),
            Level::Headings => (self.chapter.and_then(|id| book.entry(id)))
                .map(|entry| entry.headings.iter().map(|heading| heading.title.clone()).collect())
                .unwrap_or_default(),
        };
        if let Some(at) = select_by_title(&titles, &self.typed) {
            self.choose(book, at);
        }
    }

    /// What Return opens: a part's opening file, a file, or a heading's place.
    fn target(&self, book: &Book) -> Option<(ChapterId, Option<usize>)> {
        let chapter = self
            .chapter
            .or_else(|| book.entries(self.list).first().map(|entry| entry.row.id));
        match self.level {
            Level::Parts => {
                let opener = (book.groups.get(self.list)).and_then(|group| group.opener.as_ref());
                opener.map(|opener| opener.row.id).or(chapter).map(|id| (id, None))
            }
            Level::Chapters => chapter.map(|id| (id, None)),
            Level::Headings => {
                let id = chapter?;
                let offset = self
                    .heading
                    .and_then(|at| Some(book.entry(id)?.headings.get(at)?.offset));
                Some((id, offset))
            }
        }
    }
}

/// The first title that starts with `typed`, ignoring case, or else the first that holds it.
fn select_by_title(titles: &[String], typed: &str) -> Option<usize> {
    let lower: Vec<String> = titles.iter().map(|title| title.to_lowercase()).collect();
    (lower.iter().position(|title| title.starts_with(typed)))
        .or_else(|| lower.iter().position(|title| title.contains(typed)))
}

impl Navigator {
    pub(super) fn columns_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let book = self.book.clone();
        let super::Shown::Columns(columns) = &mut self.shown else {
            return false;
        };
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        match event.keystroke.key.as_str() {
            "escape" => self.close(true, window, cx),
            "enter" => {
                if let Some((id, at)) = columns.target(&book) {
                    self.open(id, at, window, cx);
                }
            }
            "up" => columns.step(&book, false),
            "down" => columns.step(&book, true),
            "left" => columns.side(&book, false),
            "right" => columns.side(&book, true),
            "home" => columns.choose(&book, 0),
            "end" => {
                if let Some(last) = columns.rows(&book).0.checked_sub(1) {
                    columns.choose(&book, last);
                }
            }
            _ => match event.keystroke.key_char.as_deref() {
                Some(typed) if !typed.trim().is_empty() => columns.type_select(&book, typed),
                _ => return false,
            },
        }
        true
    }

    /// A click on a row: it takes the keys to its column and is chosen. A heading opens at once, a file on a second click.
    fn clicked(&mut self, level: Level, at: usize, count: usize, window: &mut Window, cx: &mut Context<Self>) {
        let book = self.book.clone();
        let super::Shown::Columns(columns) = &mut self.shown else {
            return;
        };
        columns.level = level;
        columns.choose(&book, at);
        columns.settle(&book);
        if (level == Level::Headings || (level == Level::Chapters && count > 1))
            && let Some((id, at)) = columns.target(&book)
        {
            self.open(id, at, window, cx);
        }
        cx.notify();
    }

    pub(super) fn render_columns(
        &mut self,
        tokens: &Tokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let book = self.book.clone();
        let super::Shown::Columns(columns) = &self.shown else {
            return div().id("navigator-columns");
        };
        let viewport = window.viewport_size();
        let room = viewport.width - tokens::NAV_CARD_LEFT - tokens::SPACE_4;
        let all = tokens::NAV_PARTS_WIDTH + tokens::NAV_CHAPTERS_WIDTH + tokens::NAV_HEADINGS_WIDTH;
        let with_opening = all + tokens::NAV_OPENING_WIDTH <= room;
        let height = viewport.height
            - tokens::TITLEBAR_HEIGHT
            - tokens::SPACE_1
            - tokens::NAV_FOOT_HEIGHT
            - tokens::FOOTER_HEIGHT;
        let font = self.parts.editor.read(cx).body_font().family;
        let parts = parts_column(&book, columns, tokens, height, cx);
        let chapters = chapters_column(&book, columns, tokens, height, cx);
        let headings = headings_column(&book, columns, self.place.as_ref(), tokens, height, cx);
        let opening = with_opening
            .then(|| columns.chapter.and_then(|id| book.entry(id)))
            .flatten()
            .map(|entry| opening(entry, font, tokens, cx));
        div()
            .id("navigator-columns")
            .occlude()
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT + tokens::SPACE_1)
            .left(tokens::NAV_CARD_LEFT)
            .max_w(room)
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow(tokens::card_shadow())
            .role(Role::Group)
            .aria_label(crate::t!(Key::Columns))
            .on_mouse_down_out(cx.listener(|navigator, _, window, cx| navigator.pressed_outside(window, cx)))
            .child(
                div()
                    .flex()
                    .min_h_0()
                    .child(parts)
                    .child(chapters)
                    .child(headings)
                    .children(opening),
            )
            .child(foot(tokens, cx))
    }
}

/// A column: rows that scroll, with a divider at its right.
fn column(
    id: &'static str,
    width: Pixels,
    height: Pixels,
    scroll: &ScrollHandle,
    tokens: &Tokens,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::List)
        .w(width)
        .flex_shrink(1.)
        .min_w_0()
        .max_h(height)
        .py(tokens::NAV_COLUMN_Y)
        .border_r_1()
        .border_color(tokens.divider)
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .track_scroll(scroll)
}

/// A row of a column: chosen in the column with the keys, it has the raised grey and the bar of the keys; chosen in
/// another, the path's lighter grey.
fn row(
    tokens: &Tokens,
    id: ElementId,
    label: &str,
    chosen: bool,
    keyed: bool,
    click: impl Fn(&mut Navigator, usize, &mut Window, &mut Context<Navigator>) + 'static,
    cx: &mut Context<Navigator>,
) -> gpui::Stateful<gpui::Div> {
    let path_tint = gpui::Rgba {
        a: tokens::NAV_PATH_TINT,
        ..tokens.raised
    };
    div()
        .id(id)
        .role(Role::ListItem)
        .aria_label(label.to_owned())
        .aria_selected(chosen)
        .flex_none()
        .relative()
        .h(tokens::MENU_ROW_HEIGHT)
        .mx(tokens::SPACE_1)
        .px(tokens::SPACE_2)
        .rounded(tokens::CHIP_RADIUS)
        .flex()
        .items_center()
        .gap(tokens::SPACE_2)
        .cursor_pointer()
        .text_color(if chosen { tokens.text } else { tokens.text_quiet })
        .when(chosen && keyed, |row| {
            row.bg(tokens.raised).child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .h_full()
                    .w(tokens::KEY_BAR)
                    .bg(tokens.accent),
            )
        })
        .when(chosen && !keyed, |row| row.bg(path_tint))
        .hover(|row| row.bg(tokens.raised))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |navigator, event: &gpui::ClickEvent, window, cx| {
            click(navigator, event.click_count(), window, cx)
        }))
}

/// A faint count at a row's end.
fn count(tokens: &Tokens, text: String) -> gpui::Div {
    div()
        .flex_none()
        .ml_auto()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_faint)
        .child(text)
}

fn parts_column(
    book: &Book,
    columns: &Columns,
    tokens: &Tokens,
    height: Pixels,
    cx: &mut Context<Navigator>,
) -> impl IntoElement + use<> {
    let keyed = columns.level == Level::Parts;
    let mut list = column(
        "navigator-parts",
        tokens::NAV_PARTS_WIDTH,
        height,
        &columns.scrolls[0],
        tokens,
    );
    for at in 0..book.lists() {
        if at == book.groups.len() && at > 0 {
            list = list.child(
                div()
                    .flex_none()
                    .h(tokens::HAIRLINE)
                    .mx(tokens::SPACE_3)
                    .my(tokens::SPACE_1)
                    .bg(tokens.divider),
            );
        }
        let label = group_label(book, at, true).unwrap_or_default();
        let shelf = at >= book.groups.len();
        let counted = shelf || book.groups[at].kind != Kind::Part;
        let shown = book.entries(at).len();
        let mark = div()
            .flex_none()
            .size(tokens::NAV_MARK)
            .border_1()
            .border_color(tokens.text_faint)
            .when(shelf, |mark| mark.rounded_full());
        list = list.child(
            row(
                tokens,
                ElementId::from(("navigator-part", at)),
                &label,
                at == columns.list,
                keyed,
                move |navigator, clicks, window, cx| navigator.clicked(Level::Parts, at, clicks, window, cx),
                cx,
            )
            .child(mark)
            .child(div().flex_1().min_w_0().truncate().child(label))
            .when(counted, |row| row.child(count(tokens, shown.to_string()))),
        );
    }
    list
}

fn chapters_column(
    book: &Book,
    columns: &Columns,
    tokens: &Tokens,
    height: Pixels,
    cx: &mut Context<Navigator>,
) -> impl IntoElement + use<> {
    let keyed = columns.level == Level::Chapters;
    let shelf = columns.list >= book.groups.len();
    let mut list = column(
        "navigator-chapters",
        tokens::NAV_CHAPTERS_WIDTH,
        height,
        &columns.scrolls[1],
        tokens,
    );
    for (at, entry) in book.entries(columns.list).iter().enumerate() {
        let chosen = columns.chapter == Some(entry.row.id);
        let words = Navigator::words(entry.row.id, cx).map(grouped);
        list = list.child(
            row(
                tokens,
                ElementId::from(("navigator-chapter", entry.row.id.number())),
                &entry.row.title,
                chosen,
                keyed,
                move |navigator, clicks, window, cx| navigator.clicked(Level::Chapters, at, clicks, window, cx),
                cx,
            )
            .child(crate::sidebar::row_lead(tokens, &entry.row, shelf, chosen))
            .child(div().flex_1().min_w_0().truncate().child(entry.row.title.clone()))
            .children(words.map(|words| count(tokens, words))),
        );
    }
    list
}

fn headings_column(
    book: &Book,
    columns: &Columns,
    place: Option<&Place>,
    tokens: &Tokens,
    height: Pixels,
    cx: &mut Context<Navigator>,
) -> impl IntoElement + use<> {
    let keyed = columns.level == Level::Headings;
    let entry = columns.chapter.and_then(|id| book.entry(id));
    let caret = (place.zip(entry))
        .filter(|(place, entry)| place.chapter == entry.row.id)
        .and_then(|(place, _)| place.section);
    let mut list = column(
        "navigator-headings",
        tokens::NAV_HEADINGS_WIDTH,
        height,
        &columns.scrolls[2],
        tokens,
    );
    for (at, heading) in entry.map_or(&[][..], |entry| &entry.headings).iter().enumerate() {
        let depth = f32::from(heading.level.saturating_sub(2));
        list = list.child(
            row(
                tokens,
                ElementId::from(("navigator-heading", at)),
                &heading.title,
                columns.heading == Some(at),
                keyed,
                move |navigator, clicks, window, cx| navigator.clicked(Level::Headings, at, clicks, window, cx),
                cx,
            )
            .pl(tokens::SPACE_2 + tokens::NAV_INDENT * depth)
            .child(div().flex_1().min_w_0().truncate().child(heading.title.clone()))
            .when(caret == Some(at), |row| {
                row.child(div().flex_none().size(tokens::DOT_SIZE).rounded_full().bg(tokens.text))
            }),
        );
    }
    list
}

/// The chosen file's title and opening lines in the book face, and its words and open placeholders.
fn opening(
    entry: &super::book::Entry,
    font: gpui::SharedString,
    tokens: &Tokens,
    cx: &App,
) -> impl IntoElement + use<> {
    let words = Navigator::words(entry.row.id, cx).map(|words| crate::t!(Key::Words, count = grouped(words)));
    let open = Navigator::open_placeholders(entry.row.id, cx);
    div()
        .w(tokens::NAV_OPENING_WIDTH)
        .flex_shrink(1.)
        .min_w_0()
        .px(tokens::SPACE_4)
        .py(tokens::NAV_OPENING_Y)
        .flex()
        .flex_col()
        .gap(tokens::SPACE_2)
        .font_family(font)
        .child(
            div()
                .text_size(tokens::NAV_OPENING_TITLE)
                .font_weight(FontWeight::BOLD)
                .text_color(tokens.text)
                .child(entry.numbered_title()),
        )
        .children(entry.opening.iter().map(|paragraph| {
            div()
                .text_size(tokens::TEXT_BODY)
                .line_height(tokens::NAV_OPENING_LEADING)
                .text_color(tokens.text_quiet)
                .line_clamp(tokens::NAV_OPENING_LINES)
                .text_ellipsis()
                .child(paragraph.clone())
        }))
        .child(
            div()
                .flex()
                .gap(tokens::SPACE_2)
                .font_family(tokens::FONT_UI)
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_faint)
                .children(words)
                .when(open > 0, |line| {
                    line.child(
                        div()
                            .text_color(tokens.accent)
                            .child(crate::t!(Key::Open, count = open)),
                    )
                }),
        )
}

/// The keys at the foot, and the way to the overview at its right.
fn foot(tokens: &Tokens, cx: &mut Context<Navigator>) -> impl IntoElement + use<> {
    let label = crate::t!(Key::Overview);
    div()
        .flex_none()
        .h(tokens::NAV_FOOT_HEIGHT)
        .px(tokens::SPACE_3)
        .border_t_1()
        .border_color(tokens.divider)
        .flex()
        .items_center()
        .gap(tokens::SPACE_4)
        .overflow_hidden()
        .text_size(tokens::TEXT_SMALL)
        .child(hint(tokens, crate::t!(Key::HintArrows), crate::t!(Key::HintMove)))
        .child(hint(tokens, crate::t!(Key::HintReturn), crate::t!(Key::HintOpen)))
        .child(hint(tokens, crate::t!(Key::HintType), crate::t!(Key::HintSelect)))
        .child(hint(tokens, crate::t!(Key::HintEscape), crate::t!(Key::HintClose)))
        .child(
            div()
                .id("navigator-overview")
                .button(tokens, label)
                .flex_none()
                .ml_auto()
                .flex()
                .items_center()
                .gap(tokens::SPACE_1)
                .px(tokens::SPACE_1)
                .rounded(tokens::CHIP_RADIUS)
                .cursor_pointer()
                .text_color(tokens.text_quiet)
                .hover(|button| button.bg(tokens.raised))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|navigator, _, window, cx| navigator.toggle_overview(window, cx)))
                .child(
                    gpui::svg()
                        .path("icons/overview.svg")
                        .size(tokens::NAV_ICON)
                        .text_color(tokens.text_faint),
                )
                .child(label)
                .children(
                    crate::shell::commands::keys_of(&super::ShowOverview, cx)
                        .map(|keys| div().text_color(tokens.text_faint).child(keys)),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_selects_the_first_title_that_starts_with_it_then_one_that_holds_it() {
        let titles: Vec<String> = ["Over the passes", "Tolls and tariffs", "The river route"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(select_by_title(&titles, "t"), Some(1));
        assert_eq!(select_by_title(&titles, "th"), Some(2));
        assert_eq!(select_by_title(&titles, "river"), Some(2));
        assert_eq!(select_by_title(&titles, "x"), None);
    }
}
