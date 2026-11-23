//! What floats over the galley: the toolbar on a selection, the slash menu, a block's convert menu, and the link field.
//! Each is drawn by the galley's own frame, so it sits where the selection is in that same frame, and keyboard focus stays
//! in the text except in the link field.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, App, AvailableSpace, Bounds, Context, Entity, Focusable, Font, FontFeatures, MouseButton,
    MouseDownEvent, Pixels, Point, Render, ScrollHandle, SharedString, Size, Subscription, Window, div, point, px,
    size, svg,
};

use super::commands::Command;
use super::commands::convert::{self, BlockType, TARGETS};
use super::commands::links::{self, Reference};
use super::commands::marks::{self, Mark, MarkState, MarkStates};
use super::commands::notes;
use super::commands::slash::{self, Action, Item, type_icon, type_label};
use super::directives::edits::{self, PageSetting};
use super::directives::{self, Kind};
use super::picks::{Extra, Pick, Row};
use super::place::{self, Rect};
use super::placeholders::card::Completion;
use super::rich::view::RichFloat;
use super::rich::view::shapes::Place;
use super::strings::Key;
use super::{EditorView, MenuAccept, MenuComplete, MenuDown, MenuUp};
use crate::a11y::Control as _;
use crate::inspector::input::{Cancelled, Submitted, TextInput};
use crate::session::TEMPLATES;
use crate::source::{BlockKind, EditKind, Selection, motion};
use crate::tokens::{self, Tokens};

/// What is open over the galley.
#[derive(Default)]
pub(super) enum Float {
    #[default]
    None,
    Menu(Menu),
    Link(Link),
    /// What the rich blocks float: a note's text, a picker, a table's grid, a figure's card, and what they have to say.
    Rich(RichFloat),
    /// Completing a placeholder's name, with the keys in the text.
    Complete(Completion),
}

impl Float {
    /// The cell a table's menu was opened from, while that menu is what floats.
    pub(super) fn table_place(&self) -> Option<(usize, Place)> {
        let Self::Menu(Menu {
            kind: MenuKind::Pick { rows, .. },
            ..
        }) = self
        else {
            return None;
        };
        match rows.first()?.pick {
            Pick::Table { start, row, cell, .. } => Some((start, Place::Cell(row, cell))),
            _ => None,
        }
    }
}

pub(super) struct Menu {
    kind: MenuKind,
    /// The chosen row, counted among the rows shown.
    selected: usize,
    scroll: ScrollHandle,
    /// What was typed to narrow the block menu or a list.
    filter: Option<Filter>,
}

/// A query typed into a menu, and the rows it leaves by their place in the whole menu.
struct Filter {
    query: String,
    rows: Vec<usize>,
}

/// The rows whose label holds every word of `query`, by their place in `labels`. Case does not count.
fn matching<'a>(labels: impl Iterator<Item = &'a str>, query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    labels
        .enumerate()
        .filter(|(_, label)| {
            let label = label.to_lowercase();
            words.iter().all(|word| label.contains(word))
        })
        .map(|(at, _)| at)
        .collect()
}

enum MenuKind {
    /// `start` is where the typed slash is. `all` is what is valid here, `items` what the query leaves of it.
    Slash {
        start: usize,
        all: Vec<Item>,
        items: Vec<Item>,
    },
    /// What a block can become, and the regions its blocks can be wrapped in. Several selected blocks only wrap. With
    /// no anchor it stands at the selection.
    Convert {
        block: usize,
        targets: Vec<BlockType>,
        wraps: Vec<Kind>,
        /// The rows that open a list: the blocks' style and a code block's language.
        extras: Vec<Extra>,
        /// How the block stands on the pages, each with whether it is on, after a divider.
        pages: Vec<(PageSetting, bool)>,
        blocks: Range<usize>,
        anchor: Option<Bounds<Pixels>>,
    },
    /// A list that sets one thing about a block. See [`super::picks`].
    Pick {
        rows: Vec<Row>,
        anchor: Option<Bounds<Pixels>>,
    },
    /// The suggestions for a misspelled word, then a row that adds it to the project's words.
    Spelling {
        word: Range<usize>,
        suggestions: Vec<String>,
    },
}

impl Menu {
    fn new(kind: MenuKind, selected: usize, scroll: ScrollHandle) -> Self {
        Self {
            kind,
            selected,
            scroll,
            filter: None,
        }
    }

    /// The place in the whole menu of the row shown at `shown`.
    fn row(&self, shown: usize) -> usize {
        self.filter.as_ref().map_or(shown, |filter| filter.rows[shown])
    }

    /// The block menu and the lists narrow as the author types. The slash menu reads its query from the text.
    fn filters(&self) -> bool {
        matches!(self.kind, MenuKind::Convert { .. } | MenuKind::Pick { .. })
    }

    fn len(&self) -> usize {
        if let Some(filter) = &self.filter {
            return filter.rows.len();
        }
        match &self.kind {
            MenuKind::Slash { items, .. } => items.len(),
            MenuKind::Convert {
                targets,
                wraps,
                extras,
                pages,
                ..
            } => targets.len() + wraps.len() + extras.len() + pages.len(),
            MenuKind::Pick { rows, .. } => rows.len(),
            MenuKind::Spelling { suggestions, .. } => suggestions.len() + 1,
        }
    }
}

pub(super) struct Link {
    selection: Selection,
    input: Entity<TextInput>,
    has_link: bool,
    /// The reference link the selection stands in, whose address is in its definition.
    reference: Option<Reference>,
    _subscriptions: Vec<Subscription>,
}

/// What the toolbar shows for a selection in a text revision.
#[derive(Clone, Copy)]
pub(super) struct ToolbarState {
    revision: u64,
    selection: Selection,
    marks: MarkStates,
    block: Option<BlockType>,
    has_text: bool,
    /// The theme has a custom style for the selection's blocks, and an index mark may stand at its end.
    styles: bool,
    index: bool,
}

/// One button of the toolbar. The mockup's `Tt` is dropped: no case command exists in the language.
#[derive(Clone, Copy)]
enum Button {
    Mark(Mark),
    Block(BlockType),
    Link,
    Style,
    Index,
}

pub(super) const MARK_BUTTONS: [(Mark, &str, &str); 7] = [
    (Mark::Bold, "icons/mark-bold.svg", "\u{2318}B"),
    (Mark::Italic, "icons/mark-italic.svg", "\u{2318}I"),
    (Mark::Underline, "icons/mark-underline.svg", "\u{2318}U"),
    (Mark::Strike, "icons/mark-strike.svg", "\u{21e7}\u{2318}X"),
    (Mark::Superscript, "icons/mark-superscript.svg", "\u{2318}."),
    (Mark::Subscript, "icons/mark-subscript.svg", "\u{21e7}\u{2318}."),
    (Mark::SmallCaps, "icons/mark-smallcaps.svg", ""),
];

const BLOCK_BUTTONS: [(BlockType, &str, &str); 3] = [
    (BlockType::Quote, "icons/block-quote.svg", "\u{21e7}\u{2318}9"),
    (BlockType::Bullets, "icons/block-bullets.svg", "\u{21e7}\u{2318}8"),
    (BlockType::Numbers, "icons/block-numbers.svg", "\u{21e7}\u{2318}7"),
];

/// The toolbar's size with `extra` buttons after the link: the style list and the index mark, where they apply.
fn toolbar_size(extra: usize) -> Size<Pixels> {
    let buttons = MARK_BUTTONS.len() + BLOCK_BUTTONS.len() + 1 + extra;
    let separators = if extra > 0 { 2. } else { 1. };
    size(
        tokens::TOOLBAR_BUTTON * buttons as f32 + tokens::TOOLBAR_SEPARATOR * separators + tokens::SPACE_1 * 2.,
        tokens::TOOLBAR_HEIGHT,
    )
}

fn mark_label(mark: Mark) -> &'static str {
    crate::t!(match mark {
        Mark::Bold => Key::Bold,
        Mark::Italic => Key::Italic,
        Mark::Underline => Key::Underline,
        Mark::Strike => Key::Strike,
        Mark::Superscript => Key::Superscript,
        Mark::Subscript => Key::Subscript,
        Mark::SmallCaps => Key::SmallCaps,
    })
}

/// A short label on hover.
struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .px(tokens::SPACE_2)
            .py(tokens::SPACE_1)
            .rounded(tokens::RADIUS)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text)
            .child(self.0.clone())
    }
}

pub(crate) fn tooltip(text: String) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = SharedString::from(text);
    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

fn rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        left: f32::from(bounds.left()),
        top: f32::from(bounds.top()),
        right: f32::from(bounds.right()),
        bottom: f32::from(bounds.bottom()),
    }
}

/// The mark at the end of a row that is the choice in force.
const CHECK: &str = "\u{2713}";

/// One row of a floating list as it shows.
struct MenuRow {
    label: SharedString,
    /// A quiet word at its right, or a check mark.
    end: Option<SharedString>,
    icon: &'static str,
    /// A line under the label saying what it makes, in the faint small size.
    description: Option<&'static str>,
    /// A divider stands above it, and the name of the group it starts.
    divider: bool,
    section: Option<SharedString>,
}

impl MenuRow {
    fn new(label: impl Into<SharedString>, end: Option<SharedString>, icon: &'static str) -> Self {
        Self {
            label: label.into(),
            end,
            icon,
            description: None,
            divider: false,
            section: None,
        }
    }

    /// Its height in the list, with the divider and group name above it.
    fn height(&self) -> Pixels {
        let own = if self.description.is_some() {
            tokens::MENU_DESCRIBED_ROW_HEIGHT
        } else {
            tokens::MENU_ROW_HEIGHT
        };
        let divider = if self.divider {
            tokens::HAIRLINE + tokens::SPACE_1 * 2.
        } else {
            px(0.)
        };
        let section = self.section.as_ref().map_or(px(0.), |_| tokens::MENU_ROW_HEIGHT);
        own + divider + section
    }
}

/// A row that sets how a block stands on the pages: its label and icon.
fn page_row(setting: PageSetting) -> (&'static str, &'static str) {
    match setting {
        PageSetting::NewPage => (crate::t!(Key::StartOnNewPage), "icons/page-new.svg"),
        PageSetting::Foot => (crate::t!(Key::SetAtFoot), "icons/directive-bottom.svg"),
        PageSetting::Keep => (crate::t!(Key::Keep), "icons/directive-keep.svg"),
    }
}

/// Where a float goes.
pub(super) enum Spot {
    /// Its top left corner.
    At(Point<Pixels>),
    /// At the left edge of a caret, icon, or selection, below it or above it by the float's measured height, in an area.
    Caret(Bounds<Pixels>, Rect),
}

/// A list's place at a caret or icon, in `area`.
pub(super) fn placed_at_caret(target: Bounds<Pixels>, wanted: Size<Pixels>, area: Rect) -> Point<Pixels> {
    let (x, y) = place::at_caret(
        rect(target),
        f32::from(wanted.width),
        f32::from(wanted.height),
        area,
        f32::from(tokens::FLOAT_GAP),
    );
    point(px(x), px(y))
}

impl EditorView {
    pub(super) fn close_float(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.float, Float::None) {
            self.float = Float::None;
            cx.notify();
        }
    }

    pub(super) fn menu_is_open(&self) -> bool {
        matches!(self.float, Float::Menu(_))
    }

    /// Whether what floats has the keys: its own field or grid does, so the galley's bindings stand aside.
    pub(super) fn float_takes_keys(&self) -> bool {
        match &self.float {
            Float::Link(_) => true,
            Float::Rich(float) => float.takes_keys(),
            Float::None | Float::Menu(_) | Float::Complete(_) => false,
        }
    }

    /// Runs a command as one undo step.
    pub(super) fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.edit(EditKind::Discrete, command.edits, command.selection, None, window, cx);
    }

    // Toolbar

    /// What the toolbar shows. The answer lasts until the text or the selection changes.
    fn toolbar_state(&mut self) -> ToolbarState {
        let revision = self.session.active_buffer().revision();
        if let Some(seen) = self.toolbar_seen
            && seen.revision == revision
            && seen.selection == self.selection
        {
            return seen;
        }
        let text = self.session.active_buffer().text();
        let block = &self.blocks.as_slice()[self.blocks.index_at(self.selection.range().start)];
        let seen = ToolbarState {
            revision,
            selection: self.selection,
            marks: marks::states(text, &self.blocks, self.selection),
            block: convert::type_of(block.kind, &text[block.range.clone()]),
            has_text: marks::has_markable_text(text, &self.blocks, self.selection),
            styles: self.styles_offered(),
            index: self.index_offered(),
        };
        self.toolbar_seen = Some(seen);
        seen
    }

    pub(super) fn toggle_mark(&mut self, mark: Mark, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text();
        if let Some(command) = marks::toggle(text, &self.blocks, self.selection, mark) {
            self.run_command(command, window, cx);
        }
    }

    pub(super) fn toggle_block(&mut self, target: BlockType, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text();
        if let Some(command) = convert::toggle(text, &self.blocks, self.selection, target) {
            self.run_command(command, window, cx);
        }
    }

    fn toolbar_button(
        &self,
        button: Button,
        icon: &'static str,
        tip: String,
        state: MarkState,
        id: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx);
        let on = state == MarkState::On;
        div()
            .id(("toolbar", id))
            .role(gpui::Role::Button)
            .aria_label(tip.clone())
            .flex_none()
            .size(tokens::TOOLBAR_BUTTON)
            .flex()
            .items_center()
            .justify_center()
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .when(on, |button| button.bg(tokens.text))
            .when(!on, |button| button.hover(|button| button.bg(tokens.raised)))
            .tooltip(tooltip(tip))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| match button {
                Button::Mark(mark) => this.toggle_mark(mark, window, cx),
                Button::Block(target) => this.toggle_block(target, window, cx),
                Button::Link => this.open_link(window, cx),
                Button::Style => this.show_extra(Extra::Style, None, cx),
                Button::Index => this.open_index(window, cx),
            }))
            .child(svg().path(icon).size(tokens::ICON_SIZE).text_color(if on {
                tokens.panel
            } else if state == MarkState::Mixed {
                tokens.text_quiet
            } else {
                tokens.text
            }))
    }

    fn toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ToolbarState {
            marks: states,
            block,
            styles,
            index,
            ..
        } = self.toolbar_state();
        let mut row = div()
            .occlude()
            .h(tokens::TOOLBAR_HEIGHT)
            .px(tokens::SPACE_1)
            .flex()
            .items_center()
            .rounded(tokens::RADIUS)
            .shadow(tokens::float_shadow());
        let tokens_now = Tokens::get(cx).clone();
        row = row.bg(tokens_now.panel).border_1().border_color(tokens_now.divider);
        let hint = |label: &str, keys: &str| {
            if keys.is_empty() {
                label.to_owned()
            } else {
                format!("{label}  {keys}")
            }
        };
        let mut id = 0;
        for (mark, icon, keys) in MARK_BUTTONS {
            let tip = hint(mark_label(mark), keys);
            row = row.child(self.toolbar_button(Button::Mark(mark), icon, tip, states.of(mark), id, cx));
            id += 1;
        }
        let separator = || {
            div()
                .flex_none()
                .w(tokens::TOOLBAR_SEPARATOR)
                .flex()
                .justify_center()
                .child(div().w(tokens::HAIRLINE).h(tokens::SPACE_4).bg(tokens_now.divider))
        };
        row = row.child(separator());
        for (target, icon, keys) in BLOCK_BUTTONS {
            let state = if block == Some(target) {
                MarkState::On
            } else {
                MarkState::Off
            };
            let tip = hint(type_label(target, crate::shell::strings::language()), keys);
            row = row.child(self.toolbar_button(Button::Block(target), icon, tip, state, id, cx));
            id += 1;
        }
        let tip = hint(crate::t!(Key::Link), "\u{2318}K");
        row = row.child(self.toolbar_button(Button::Link, "icons/mark-link.svg", tip, states.link, id, cx));
        if styles || index {
            row = row.child(separator());
        }
        if styles {
            let tip = hint(crate::t!(Key::Style), "\u{2325}\u{2318}S");
            row = row.child(self.toolbar_button(Button::Style, "icons/styles.svg", tip, MarkState::Off, id + 1, cx));
        }
        if index {
            let tip = hint(crate::t!(Key::IndexMark), "\u{2325}\u{2318}I");
            row =
                row.child(self.toolbar_button(Button::Index, "icons/mark-index.svg", tip, MarkState::Off, id + 2, cx));
        }
        row
    }

    /// Whether the toolbar shows: a settled selection holding text it can mark.
    fn toolbar_wanted(&mut self) -> bool {
        if !self.writing.toolbar {
            return false;
        }
        !self.selection.is_empty()
            && self.drag.is_none()
            && self.marked.is_none()
            && !self.toolbar_dismissed
            && matches!(self.float, Float::None)
            && self.toolbar_state().has_text
    }

    /// The rectangle on screen the selection covers.
    fn selection_box(rectangles: &[Bounds<Pixels>]) -> Option<Bounds<Pixels>> {
        let first = rectangles.first()?;
        let (mut left, mut top, mut right, mut bottom) = (first.left(), first.top(), first.right(), first.bottom());
        for rectangle in rectangles {
            left = left.min(rectangle.left());
            top = top.min(rectangle.top());
            right = right.max(rectangle.right());
            bottom = bottom.max(rectangle.bottom());
        }
        Some(Bounds::from_corners(point(left, top), point(right, bottom)))
    }

    // Link

    pub(super) fn open_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text();
        let reference = links::reference_at(text, &self.blocks, self.selection);
        let address = match &reference {
            Some(reference) => Some(
                (reference.address.clone())
                    .map(|range| links::shown_address(&text[range]).to_owned())
                    .unwrap_or_default(),
            ),
            None => marks::link_at(text, &self.blocks, self.selection),
        };
        if !marks::has_markable_text(text, &self.blocks, self.command_range()) {
            return;
        }
        let has_link = address.is_some();
        let selection = self.selection;
        // A field that keeps its focus reports Escape, which closes the field, as the section dialog's does. Losing the
        // window's focus leaves it open.
        let input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(Key::LinkPlaceholder);
            input.set_value(address.unwrap_or_default(), true, cx);
            input
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, input, Submitted, window, cx| {
                let address = input.read(cx).text().to_owned();
                this.submit_link(&address, window, cx);
            }),
            cx.subscribe_in(&input, window, |this, _, Cancelled, window, cx| {
                this.close_link(window, cx);
            }),
        ];
        input.focus_handle(cx).focus(window, cx);
        self.float = Float::Link(Link {
            selection,
            input,
            has_link,
            reference,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// The selection a command would act on: a caret means the word it touches.
    fn command_range(&self) -> Selection {
        if !self.selection.is_empty() {
            return self.selection;
        }
        let range = motion::word_range(self.session.active_buffer().text(), self.selection.head);
        Selection {
            anchor: range.start,
            head: range.end,
        }
    }

    /// Closes the link field with nothing written, and gives the keys back to the text.
    fn close_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.float, Float::Link(_)) {
            self.close_float(cx);
            window.focus(&self.focus, cx);
        }
    }

    fn submit_link(&mut self, address: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Link(link) = &self.float else {
            return;
        };
        let selection = link.selection;
        let reference = link.reference.clone();
        self.float = Float::None;
        window.focus(&self.focus, cx);
        let text = self.session.active_buffer().text();
        let command = match reference {
            _ if address.trim().is_empty() => marks::remove_link(text, &self.blocks, selection),
            // A reference link keeps its form: the address changes in its definition, where that stands.
            Some(reference) => links::set_address(text, &reference, address, selection),
            None => marks::set_link(text, &self.blocks, selection, address),
        };
        if let Some(command) = command {
            self.run_command(command, window, cx);
        }
        cx.notify();
    }

    fn link_popup(&self, link: &Link, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx).clone();
        let has_link = link.has_link;
        div()
            .occlude()
            .w(tokens::LINK_POPUP_WIDTH)
            .p(tokens::SPACE_1)
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .rounded(tokens::RADIUS)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .shadow(tokens::float_shadow())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_link(window, cx)))
            .child(
                svg()
                    .flex_none()
                    .mx(tokens::SPACE_1)
                    .path("icons/mark-link.svg")
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.text_quiet),
            )
            .child(div().flex_1().min_w_0().child(link.input.clone()))
            .when(has_link, |row| {
                row.child(
                    div()
                        .id("remove-link")
                        .button(&tokens, crate::t!(Key::RemoveLink))
                        .flex_none()
                        .px(tokens::SPACE_2)
                        .h(tokens::FIELD_HEIGHT)
                        .flex()
                        .items_center()
                        .rounded(tokens::RADIUS)
                        .cursor_pointer()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_quiet)
                        .hover(|button| button.bg(tokens.raised))
                        .on_click(cx.listener(|this, _, window, cx| this.submit_link("", window, cx)))
                        .child(crate::t!(Key::RemoveLink)),
                )
            })
    }

    // Slash menu

    /// Opens the menu if the slash just typed at `at` may open it.
    pub(super) fn open_slash(&mut self, at: usize, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text();
        let kind = self.blocks.as_slice()[self.blocks.index_at(at)].kind;
        if !slash::opens_at(kind, text, at) {
            return;
        }
        let context = self.slash_context(at);
        let all = slash::items(&context, crate::shell::strings::language());
        if all.is_empty() {
            return;
        }
        self.float = Float::Menu(Menu::new(
            MenuKind::Slash {
                start: at,
                items: all.clone(),
                all,
            },
            0,
            ScrollHandle::new(),
        ));
        cx.notify();
    }

    fn slash_context(&self, at: usize) -> slash::Context {
        let text = self.session.active_buffer().text();
        let session = &self.session;
        let chapters = || {
            session
                .book()
                .iter()
                .filter_map(|id| session.chapter(*id))
                .map(|chapter| chapter.text())
        };
        slash::Context {
            open_directives: slash::open_directives(text, &self.blocks, at),
            has_toc: chapters().any(slash::holds_toc),
            has_bibliography: chapters().any(|text| slash::holds_directive(text, "bibliography")),
            has_index: chapters().any(|text| slash::holds_directive(text, "index")),
            has_works: !self.rich.works.is_empty(),
            has_labels: !self.rich.book.labels.is_empty(),
            block: Some(self.blocks.as_slice()[self.blocks.index_at(at)].kind),
            theme: session.theme_setting(),
        }
    }

    /// Follows the text and caret while the slash menu is open: the query filters it, and a caret that leaves the query,
    /// a line break, or no entry left closes it. The typed text stays.
    pub(super) fn follow_slash(&mut self) {
        let Float::Menu(Menu {
            kind: MenuKind::Slash { start, all, items },
            selected,
            ..
        }) = &mut self.float
        else {
            return;
        };
        let text = self.session.active_buffer().text();
        let caret = self.selection.head;
        let query = (self.selection.is_empty() && *start < text.len() && caret > *start)
            .then(|| &text[*start..caret])
            .and_then(|typed| typed.strip_prefix('/'))
            .filter(|query| !query.contains('\n'));
        let Some(query) = query else {
            self.float = Float::None;
            return;
        };
        *items = slash::filtered(all.clone(), query);
        *selected = 0;
        if items.is_empty() {
            let (at, query) = (*start, query.to_owned());
            self.float = Float::None;
            // What was searched for exists, but not here: the author is told why rather than left with nothing.
            let language = crate::shell::strings::language();
            if let Some(why) = slash::withheld(&self.slash_context(at), &query, language) {
                self.float = Float::Rich(super::rich::view::RichFloat::Notice(
                    super::rich::view::NoticeFloat::new(at, why),
                ));
            }
        }
    }

    pub(super) fn menu_move(&mut self, down: bool, cx: &mut Context<Self>) {
        if let Float::Menu(menu) = &mut self.float {
            let count = menu.len();
            if count > 0 {
                menu.selected = if down {
                    (menu.selected + 1) % count
                } else {
                    (menu.selected + count - 1) % count
                };
                menu.scroll.scroll_to_item(menu.selected);
                cx.notify();
            }
        }
    }

    /// Where the slash menu's slash is and what its chosen entry does, while it is open.
    pub(super) fn slash_choice(&self) -> Option<(usize, &Action)> {
        let Float::Menu(Menu {
            kind: MenuKind::Slash { start, items, .. },
            selected,
            ..
        }) = &self.float
        else {
            return None;
        };
        Some((*start, &items.get(*selected)?.action))
    }

    pub(super) fn menu_up(&mut self, _: &MenuUp, _: &mut Window, cx: &mut Context<Self>) {
        if !self.completion_move(false, cx) {
            self.menu_move(false, cx);
        }
    }

    pub(super) fn menu_down(&mut self, _: &MenuDown, _: &mut Window, cx: &mut Context<Self>) {
        if !self.completion_move(true, cx) {
            self.menu_move(true, cx);
        }
    }

    pub(super) fn menu_complete(&mut self, _: &MenuComplete, window: &mut Window, cx: &mut Context<Self>) {
        self.accept_completion(window, cx);
    }

    pub(super) fn menu_accept(&mut self, _: &MenuAccept, window: &mut Window, cx: &mut Context<Self>) {
        if self.accept_completion(window, cx) {
            return;
        }
        if let Float::Menu(menu) = &self.float
            && menu.len() > 0
        {
            let row = menu.row(menu.selected);
            self.choose(row, window, cx);
        }
    }

    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        match std::mem::take(&mut self.float) {
            Float::Menu(Menu {
                kind: MenuKind::Slash { start, items, .. },
                ..
            }) => {
                let Some(item) = items.get(index) else {
                    return;
                };
                self.apply_slash(start, &item.action, window, cx);
            }
            Float::Menu(Menu {
                kind:
                    MenuKind::Convert {
                        block,
                        targets,
                        wraps,
                        extras,
                        pages,
                        blocks,
                        anchor,
                    },
                ..
            }) => {
                // A row that leads to a list opens it where the menu stood. These rows come first.
                if let Some(extra) = extras.get(index) {
                    let rows = self.extra_rows(*extra, &blocks);
                    self.open_pick(rows, anchor, cx);
                    return;
                }
                let index = index - extras.len();
                let text = self.session.active_buffer().text();
                let structure = &self.directives.structure;
                let command = if let Some(target) = targets.get(index) {
                    convert::convert(text, &self.blocks, block, *target, self.selection)
                } else if let Some(kind) = wraps.get(index - targets.len()) {
                    edits::wrap(text, &self.blocks, structure, blocks, *kind, self.selection)
                } else {
                    let setting = pages
                        .get(index - targets.len() - wraps.len())
                        .map(|(setting, _)| *setting);
                    setting.and_then(|setting| {
                        edits::toggle_page_setting(text, &self.blocks, structure, block, setting, self.selection)
                    })
                };
                if let Some(command) = command {
                    self.run_command(command, window, cx);
                }
            }
            Float::Menu(Menu {
                kind: MenuKind::Pick { mut rows, .. },
                ..
            }) => {
                if index < rows.len() {
                    self.run_pick(rows.swap_remove(index).pick, window, cx);
                }
            }
            Float::Menu(Menu {
                kind: MenuKind::Spelling { word, suggestions },
                ..
            }) => match suggestions.get(index) {
                Some(suggestion) => self.apply_suggestion(word, suggestion, window, cx),
                None => self.add_to_word_list(word, cx),
            },
            other => self.float = other,
        }
        cx.notify();
    }

    /// Inserts a table at the caret and opens its grid, as the slash menu does.
    pub(super) fn insert_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.selection.head;
        self.apply_slash(caret, &Action::Insert(slash::Standalone::Table), window, cx);
    }

    /// Opens the menu of suggestions under a word.
    pub(super) fn open_spelling_menu(&mut self, word: Range<usize>, suggestions: Vec<String>, cx: &mut Context<Self>) {
        self.float = Float::Menu(Menu::new(
            MenuKind::Spelling { word, suggestions },
            0,
            ScrollHandle::new(),
        ));
        cx.notify();
    }

    fn apply_slash(&mut self, start: usize, action: &Action, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.selection.head;
        let text = self.session.active_buffer().text();
        // These open their editor, which writes the text and takes the typed slash with it.
        match action {
            Action::OpenFootnote => return self.insert_footnote(Some(start..caret), window, cx),
            Action::OpenCitation => return self.open_citation_picker(Some(start..caret), window, cx),
            Action::OpenReference => return self.open_reference_picker(Some(start..caret), window, cx),
            Action::OpenImage => return self.choose_image(Some(start..caret), window, cx),
            _ => {}
        }
        let Some(mut command) = slash::apply(
            text,
            &self.blocks,
            start..caret,
            action,
            crate::shell::strings::language(),
        ) else {
            return;
        };
        if matches!(action, Action::Insert(slash::Standalone::ListTable))
            && let Some(ghost) = self.directives.ghost_table.take()
            && ghost.options != super::directives::GhostTable::default().options
        {
            with_table_options(&mut command, &ghost.options.opening_line());
        }
        let table_at = matches!(
            action,
            Action::Insert(slash::Standalone::Table | slash::Standalone::ListTable)
        )
        .then_some(command.selection.head);
        self.run_command(command, window, cx);
        self.toolbar_dismissed = true;
        if let Some(at) = table_at {
            self.open_grid_at(at, window, cx);
        }
        if let Action::Section(slug) = action
            && let Some(template) = TEMPLATES.iter().find(|template| template.slug == *slug)
        {
            let made = self.update_session(
                |session| {
                    let after = session.active();
                    session.new_section(template, None, after).map(|_| ())
                },
                window,
                cx,
            );
            if let Err(problem) = made {
                self.status = problem.into();
            }
        }
    }

    // Convert menu

    /// Opens the menu of what a block can become, under its gutter icon. When the selection holds it and other blocks,
    /// the menu wraps them all.
    pub(super) fn open_convert(&mut self, block: usize, anchor: Bounds<Pixels>, cx: &mut Context<Self>) {
        let blocks = self.selected_blocks().filter(|blocks| blocks.contains(&block));
        self.open_block_menu(block, blocks, Some(anchor), cx);
    }

    /// The blocks a selection runs across, when it holds more than one.
    pub(super) fn selected_blocks(&self) -> Option<Range<usize>> {
        if self.selection.is_empty() || self.source {
            return None;
        }
        let range = self.selection.range();
        let first = self.blocks.index_at(range.start);
        let last = self.blocks.index_at(range.end - 1);
        (last > first).then_some(first..last + 1)
    }

    /// Opens the block menu: what `block` can become, or with several `blocks` only what they can be wrapped in.
    /// Returns whether it had anything to offer.
    pub(super) fn open_block_menu(
        &mut self,
        block: usize,
        blocks: Option<Range<usize>>,
        anchor: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.session.active_buffer().text();
        let targets: Vec<BlockType> = match blocks {
            Some(_) => Vec::new(),
            None => TARGETS
                .into_iter()
                .filter(|target| convert::convert(text, &self.blocks, block, *target, self.selection).is_some())
                .collect(),
        };
        let single = blocks.is_none();
        let blocks = blocks.unwrap_or(block..block + 1);
        let structure = &self.directives.structure;
        let mut wraps = directives::edits::wraps_offered(text, &self.blocks, structure, blocks.clone());
        let extras = self.extras_for(&blocks);
        let pages: Vec<(PageSetting, bool)> = if single {
            (PageSetting::ALL.into_iter())
                .filter(|setting| {
                    edits::toggle_page_setting(text, &self.blocks, structure, block, *setting, self.selection).is_some()
                })
                .map(|setting| {
                    let on = edits::page_setting(&self.blocks, structure, block, setting).is_some();
                    (setting, on)
                })
                .collect()
        } else {
            Vec::new()
        };
        // For one block, the rows that set how it stands on the pages wrap it in these.
        if single {
            wraps.retain(|kind| !matches!(kind, Kind::Keep | Kind::Bottom));
        }
        if targets.is_empty() && wraps.is_empty() && extras.is_empty() && pages.is_empty() {
            return false;
        }
        self.float = Float::Menu(Menu::new(
            MenuKind::Convert {
                block,
                targets,
                wraps,
                extras,
                pages,
                blocks,
                anchor,
            },
            0,
            ScrollHandle::new(),
        ));
        cx.notify();
        true
    }

    /// Opens a list that sets one thing, at `anchor` or the selection, on the choice the block has now.
    pub(super) fn open_pick(&mut self, rows: Vec<Row>, anchor: Option<Bounds<Pixels>>, cx: &mut Context<Self>) {
        // A list of choices opens on the one in force. A table's menu holds actions too, and opens at its top.
        let actions = matches!(rows.first().map(|row| &row.pick), Some(Pick::Table { .. }));
        let selected = rows.iter().position(|row| row.on && !actions).unwrap_or(0);
        let scroll = ScrollHandle::new();
        // The list shows ten rows, so a choice further down is brought into view by the rows' fixed height.
        let above = selected.saturating_sub(tokens::MENU_MAX_ROWS / 2);
        scroll.set_offset(point(px(0.), -tokens::MENU_ROW_HEIGHT * above as f32));
        self.float = Float::Menu(Menu::new(MenuKind::Pick { rows, anchor }, selected, scroll));
        cx.notify();
    }

    /// Whether a table's menu is open, which belongs to a cell, not to the caret in the text.
    pub(super) fn table_menu_open(&self) -> bool {
        matches!(&self.float, Float::Menu(Menu { kind: MenuKind::Pick { rows, .. }, .. })
            if matches!(rows.first().map(|row| &row.pick), Some(Pick::Table { .. })))
    }

    fn menu_rows(&self, menu: &Menu) -> Vec<MenuRow> {
        match &menu.kind {
            MenuKind::Slash { items, .. } => {
                let mut before: Option<slash::Section> = None;
                (items.iter().enumerate())
                    .map(|(at, item)| {
                        let starts = at > 0 && item.section != before;
                        before = item.section;
                        MenuRow {
                            description: item.description,
                            divider: starts,
                            section: (item.section.filter(|_| at == 0 || starts)).map(|section| section.label().into()),
                            ..MenuRow::new(item.label, item.group.map(SharedString::from), item.icon)
                        }
                    })
                    .collect()
            }
            MenuKind::Pick { rows, .. } => rows
                .iter()
                .map(|row| MenuRow::new(row.label.clone(), row.on.then(|| CHECK.into()), row.icon))
                .collect(),
            MenuKind::Convert {
                targets,
                wraps,
                extras,
                pages,
                blocks,
                ..
            } => {
                let wrap_in: SharedString = directives::wrap_in_label().into();
                let turned = targets.iter().map(|target| {
                    MenuRow::new(
                        type_label(*target, crate::shell::strings::language()),
                        None,
                        type_icon(*target),
                    )
                });
                let wrapped = wraps
                    .iter()
                    .map(|kind| MenuRow::new(kind.label(), Some(wrap_in.clone()), kind.icon()));
                let leading = extras.iter().map(|extra| {
                    let (label, end, icon) = self.extra_row(*extra, blocks);
                    MenuRow::new(label, end, icon)
                });
                let paged = pages.iter().enumerate().map(|(at, (setting, on))| {
                    let (label, icon) = page_row(*setting);
                    MenuRow {
                        divider: at == 0 && !(targets.is_empty() && wraps.is_empty() && extras.is_empty()),
                        ..MenuRow::new(label, on.then(|| CHECK.into()), icon)
                    }
                });
                leading.chain(turned).chain(wrapped).chain(paged).collect()
            }
            MenuKind::Spelling { suggestions, .. } => {
                let suggested = suggestions
                    .iter()
                    .map(|suggestion| MenuRow::new(suggestion.clone(), None, "icons/block-text.svg"));
                let add = MenuRow::new(crate::t!(Key::AddToWords), None, "icons/plus.svg");
                suggested.chain(std::iter::once(add)).collect()
            }
        }
    }

    /// The list's size: its rows' heights up to ten plain rows, which it scrolls past, and wider while rows describe
    /// what they make.
    fn menu_size<'a>(rows: impl Iterator<Item = &'a MenuRow> + Clone) -> Size<Pixels> {
        let most = tokens::MENU_ROW_HEIGHT * tokens::MENU_MAX_ROWS as f32;
        let height = rows.clone().fold(px(0.), |sum, row| sum + row.height()).min(most);
        let described = rows.into_iter().any(|row| row.description.is_some());
        size(
            if described {
                tokens::MENU_DESCRIBED_WIDTH
            } else {
                tokens::MENU_WIDTH
            },
            height + tokens::SPACE_1 * 2. + tokens::CARD_BORDERS,
        )
    }

    /// The rows that show, with their place in the whole menu. A typed query stands over the first of them.
    fn shown_rows(&self, menu: &Menu) -> Vec<(usize, MenuRow)> {
        let rows = self.menu_rows(menu).into_iter().enumerate();
        let Some(filter) = &menu.filter else {
            return rows.collect();
        };
        let mut shown: Vec<(usize, MenuRow)> = rows
            .filter(|(index, _)| filter.rows.contains(index))
            .map(|(index, row)| {
                let row = MenuRow {
                    divider: false,
                    section: None,
                    ..row
                };
                (index, row)
            })
            .collect();
        if let Some((_, first)) = shown.first_mut() {
            first.section = Some(filter.query.clone().into());
        }
        shown
    }

    /// Narrows an open block menu or list by what was typed, and says whether the menu took it. A query that leaves no
    /// row is not taken, so the list never empties.
    pub(super) fn type_into_menu(&mut self, typed: &str, cx: &mut Context<Self>) -> bool {
        let Float::Menu(menu) = &self.float else {
            return false;
        };
        if !menu.filters() {
            return false;
        }
        let query = format!(
            "{}{typed}",
            menu.filter.as_ref().map_or("", |filter| filter.query.as_str())
        );
        if query.trim().is_empty() {
            return true;
        }
        let labels: Vec<SharedString> = self.menu_rows(menu).into_iter().map(|row| row.label).collect();
        let rows = matching(labels.iter().map(SharedString::as_ref), &query);
        if !rows.is_empty()
            && let Float::Menu(menu) = &mut self.float
        {
            menu.filter = Some(Filter { query, rows });
            menu.selected = 0;
            menu.scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
        true
    }

    /// Takes the last typed letter off a menu's query, and says whether there was one.
    pub(super) fn erase_menu_query(&mut self, cx: &mut Context<Self>) -> bool {
        let Float::Menu(menu) = &self.float else {
            return false;
        };
        let Some(query) = menu.filter.as_ref().map(|filter| filter.query.clone()) else {
            return false;
        };
        let mut query = query;
        query.pop();
        let rows = (!query.trim().is_empty()).then(|| {
            let labels: Vec<SharedString> = self.menu_rows(menu).into_iter().map(|row| row.label).collect();
            matching(labels.iter().map(SharedString::as_ref), &query)
        });
        if let Float::Menu(menu) = &mut self.float {
            menu.filter = rows.map(|rows| Filter { query, rows });
            menu.selected = 0;
            menu.scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
        true
    }

    fn menu_element(&self, menu: &Menu, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx).clone();
        let shown = self.shown_rows(menu);
        let wanted = Self::menu_size(shown.iter().map(|(_, row)| row));
        let selected = menu.selected;
        let rows: Vec<_> = shown
            .into_iter()
            .enumerate()
            .map(|(at, (index, row))| {
                let chosen = at == selected;
                let MenuRow {
                    label,
                    end,
                    icon,
                    description,
                    divider,
                    section,
                } = row;
                let height = if description.is_some() {
                    tokens::MENU_DESCRIBED_ROW_HEIGHT
                } else {
                    tokens::MENU_ROW_HEIGHT
                };
                let words = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().child(label.clone()))
                    .children(description.map(|description| {
                        div()
                            .truncate()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(description)
                    }));
                // The check mark is the row's state; a quiet word at its end or a line under it is read after its name.
                let checked = end.as_ref().is_some_and(|end| end.as_ref() == CHECK);
                let spoken: Vec<&str> = (end.as_ref().filter(|_| !checked).map(SharedString::as_ref))
                    .into_iter()
                    .chain(description)
                    .collect();
                let item = div()
                    .id(("menu-row", index))
                    .role(gpui::Role::MenuItem)
                    .aria_label(label)
                    .aria_selected(chosen)
                    .when(checked, |row| row.aria_toggled(gpui::Toggled::True))
                    .when(!spoken.is_empty(), |row| row.aria_description(spoken.join(", ")))
                    .flex_none()
                    .h(height)
                    .px(tokens::SPACE_2)
                    .flex()
                    .when(description.is_some(), |row| {
                        row.items_start().pt(tokens::SPACE_1 + tokens::HAIRLINE)
                    })
                    .when(description.is_none(), |row| row.items_center())
                    .gap(tokens::SPACE_2)
                    .rounded(tokens::RADIUS)
                    .cursor_pointer()
                    .text_size(tokens::TEXT_BODY)
                    .text_color(if chosen { tokens.text } else { tokens.text_quiet })
                    .when(chosen, |row| row.bg(tokens.raised))
                    .hover(|row| row.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| this.choose(index, window, cx)))
                    .child(
                        svg()
                            .flex_none()
                            .when(description.is_some(), |icon| icon.mt(tokens::HAIRLINE * 2.))
                            .path(icon)
                            .size(tokens::ICON_SIZE)
                            .text_color(if chosen { tokens.text } else { tokens.text_faint }),
                    )
                    .child(words)
                    .children(end.map(|end| {
                        div()
                            .flex_none()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(end)
                    }));
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .when(divider, |group| {
                        group.child(
                            div()
                                .flex_none()
                                .my(tokens::SPACE_1)
                                .h(tokens::HAIRLINE)
                                .bg(tokens.divider),
                        )
                    })
                    .children(section.map(|section| {
                        div()
                            .flex_none()
                            .h(tokens::MENU_ROW_HEIGHT)
                            .px(tokens::SPACE_2)
                            .flex()
                            .items_center()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(section)
                    }))
                    .child(item)
            })
            .collect();
        div()
            .id("command-menu")
            .role(gpui::Role::Menu)
            .occlude()
            .w(wanted.width)
            .h(wanted.height)
            .p(tokens::SPACE_1)
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&menu.scroll)
            .rounded(tokens::RADIUS)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .shadow(tokens::float_shadow())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_float(cx)))
            .children(rows)
    }

    /// The chapter's opening notes for this frame: one line above its first text, standing in the room that text keeps
    /// above it, which grows for the notes when unfolded. They scroll away with the chapter's top. While the caret shows
    /// a note raw in the flow there is no line, which would stand over the note's own text.
    pub(super) fn margin_element(&self) -> Option<(AnyElement, Point<Pixels>)> {
        let margin = self.margin.clone()?;
        let last = self.opening_line_block()?;
        let (_, top) = self.visible.iter().find(|(index, _)| *index == last)?;
        Some((margin.into_any_element(), point(self.column.text_left, *top)))
    }

    /// The last hidden block of the chapter's opening notes while their line shows.
    fn opening_line_block(&self) -> Option<usize> {
        if self.source || self.margin.is_none() {
            return None;
        }
        let region = notes::leading_region(self.text(), &self.blocks)?;
        let first = self.blocks.index_at(region.start);
        let last = self.blocks.index_at(region.end.saturating_sub(1));
        let shown_raw = (first..=last)
            .any(|index| self.blocks.as_slice()[index].kind == BlockKind::Comment && !self.comment_is_hidden(index));
        (!shown_raw).then_some(last)
    }

    /// The chapter's first block of text, after its hidden notes and front matter.
    pub(super) fn first_text_block(&self) -> Option<usize> {
        (self.blocks.as_slice().iter()).position(|block| {
            !matches!(
                block.kind,
                BlockKind::Blank | BlockKind::Comment | BlockKind::FrontMatter
            )
        })
    }

    /// The room the chapter's first text keeps above it for the opening notes' line and the chapter's label, so the
    /// text's top margin stands above the whole opening. A body line stands between the notes' line and what follows,
    /// so the line reads apart from the title.
    pub(super) fn opening_room(&self, index: usize) -> Pixels {
        if self.source || self.first_text_block() != Some(index) {
            return px(0.);
        }
        let label = self.chapter_label_text().map_or(px(0.), |_| self.chapter_label_room());
        let line = self.opening_line_block().map_or(px(0.), |_| {
            tokens::NOTE_LINE_HEIGHT + self.style.body_size * self.style.leading
        });
        label + line
    }

    /// The chapter's number as the theme writes it, for a numbered chapter whose text opens with its level 1 heading.
    /// None in the source view and while the opening notes are unfolded above the title.
    fn chapter_label_text(&self) -> Option<String> {
        if self.source || self.note_room > px(0.) {
            return None;
        }
        let number = self.session.number_of(self.session.active())?;
        let title = self.first_text_block()?;
        if self.blocks.as_slice()[title].kind != (BlockKind::Heading { level: 1 }) {
            return None;
        }
        self.style.chapter_number(number)
    }

    /// The chapter's label with the top of its title's text, while the title is on screen.
    fn chapter_label(&self) -> Option<(String, Pixels)> {
        let label = self.chapter_label_text()?;
        let title = self.first_text_block()?;
        let (_, top) = self.visible.iter().find(|(index, _)| *index == title)?;
        let above = self.slots[title].layout.as_ref().map_or(px(0.), |layout| layout.above);
        Some((label, *top + above))
    }

    fn chapter_label_size(&self) -> Pixels {
        self.style.body_size * tokens::CHAPTER_LABEL_SCALE
    }

    /// The room the chapter's label takes above its title, with the gap between them.
    fn chapter_label_room(&self) -> Pixels {
        self.chapter_label_size() * tokens::CHAPTER_LABEL_LEADING + tokens::SPACE_1
    }

    /// The chapter's label above its title: quiet capitals in the book face, spaced apart, as the mockup has it.
    pub(super) fn chapter_label_element(&self, cx: &App) -> Option<(AnyElement, Point<Pixels>)> {
        let (label, top) = self.chapter_label()?;
        let tokens = Tokens::get(cx);
        let size = self.chapter_label_size();
        let spaced = tracked(&label.to_uppercase());
        // Lining figures stand as tall as the capitals, where a book face's old-style ones would sit at x-height.
        let font = Font {
            features: FontFeatures(std::sync::Arc::new(vec![("lnum".into(), 1)])),
            ..self.style.body.clone()
        };
        let element = div()
            .font(font)
            .text_size(size)
            .line_height(size * tokens::CHAPTER_LABEL_LEADING)
            .text_color(tokens.text_quiet)
            .whitespace_nowrap()
            .child(spaced)
            .into_any_element();
        Some((element, point(self.column.text_left, top - self.chapter_label_room())))
    }

    /// The left edge nearest `left` for something `width` wide over the row at `row`, inside `reach`, whose sides both
    /// stand in a word space or past the row's ends, so it cuts no glyph. Where none does, `left` itself.
    fn clear_of_glyphs(
        &mut self,
        left: Pixels,
        width: Pixels,
        row: Pixels,
        reach: (Pixels, Pixels),
        window: &Window,
    ) -> Pixels {
        let words = self.row_words(row, reach, window);
        let reach = (f32::from(reach.0), f32::from(reach.1));
        let room = f32::from(tokens::SPACE_1 * 0.5);
        place::clear_of(f32::from(left), f32::from(width), &words, reach, room).map_or(left, px)
    }

    /// The left and right of each word on the row at `row` between the sides of `reach`.
    fn row_words(&mut self, row: Pixels, reach: (Pixels, Pixels), window: &Window) -> Vec<(f32, f32)> {
        let (from, _) = self.offset_at(point(reach.0, row), window);
        let (to, _) = self.offset_at(point(reach.1, row), window);
        let text = self.session.active_buffer().text();
        let block = self.blocks.as_slice()[self.blocks.index_at(from)].range.clone();
        let (from, to) = (from.min(to).max(block.start), from.max(to).min(block.end));
        // Out to whole words, since the row's ends may fall inside one.
        let from = (text[block.start..from].rfind(char::is_whitespace)).map_or(block.start, |at| block.start + at + 1);
        let to = (text[to..block.end].find(char::is_whitespace)).map_or(block.end, |at| to + at);
        let mut spans = Vec::new();
        let mut start = None;
        for (at, ch) in text[from..to].char_indices().map(|(at, ch)| (from + at, ch)) {
            match (ch.is_whitespace(), start) {
                (true, Some(word)) => {
                    spans.push((word, at));
                    start = None;
                }
                (false, None) => start = Some(at),
                _ => {}
            }
        }
        spans.extend(start.map(|word| (word, to)));
        let on_row = |bounds: Bounds<Pixels>| bounds.top() <= row && row < bounds.bottom();
        spans
            .into_iter()
            .filter_map(|(start, end)| {
                let first = self
                    .offset_bounds(start, false, window)
                    .filter(|bounds| on_row(*bounds))?;
                let last = self.offset_bounds(end, true, window).filter(|bounds| on_row(*bounds))?;
                Some((f32::from(first.left()), f32::from(last.left())))
            })
            .collect()
    }

    // The frame's floats

    /// Builds what floats in this frame, each with where it goes. Selection rectangles and the caret are this frame's.
    pub(super) fn floats(
        &mut self,
        selection: &[Bounds<Pixels>],
        caret: Option<Bounds<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, Spot)> {
        let target = Self::selection_box(selection).or(caret);
        // Up and down it stays over the galley, and sideways inside the window.
        let viewport = self.column.viewport;
        let area = Rect {
            left: 0.,
            top: f32::from(viewport.top()),
            right: f32::from(window.viewport_size().width),
            bottom: f32::from(viewport.bottom()),
        };
        if let (Some(prompt), Some(target)) = (&self.selection_prompt, Self::selection_box(selection)) {
            let wanted = size(tokens::PROMPT_CARD_WIDTH, tokens::PROMPT_CARD_HEIGHT);
            let origin = point(self.column.text_left, placed_at_caret(target, wanted, area).y);
            return vec![(prompt.clone().into_any_element(), Spot::At(origin))];
        }
        match &self.float {
            Float::None => {
                if let Some(options) = self.options_float(cx) {
                    return vec![options];
                }
                if let Some(target) = Self::selection_box(selection)
                    && self.toolbar_wanted()
                    && !crate::palette::is_open(cx)
                {
                    // Inside the text column, so it never stands over the margin.
                    let column = Rect {
                        left: f32::from(self.column.text_left),
                        right: f32::from(self.column.text_left + self.column.text_width),
                        ..area
                    };
                    let state = self.toolbar_state();
                    let size = toolbar_size(usize::from(state.styles) + usize::from(state.index));
                    let gap = f32::from(tokens::FLOAT_GAP);
                    let (x, y) = place::above(rect(target), f32::from(size.width), f32::from(size.height), column, gap);
                    // Above the selection where there is room, its edges between lines rather than through glyphs.
                    let pitch = (selection.iter())
                        .min_by(|a, b| f32::from(a.top()).total_cmp(&f32::from(b.top())))
                        .map(|row| row.size.height);
                    let y = pitch
                        .map(|pitch| {
                            place::between_lines(f32::from(target.top()), f32::from(pitch), f32::from(size.height))
                        })
                        .filter(|snapped| y < f32::from(target.top()) && *snapped >= column.top + gap)
                        .unwrap_or(y);
                    let row = px(y) + size.height * 0.5;
                    let reach = (px(column.left + gap), px(column.right - gap));
                    let x = self.clear_of_glyphs(px(x), size.width, row, reach, window);
                    return vec![(self.toolbar(cx).into_any_element(), Spot::At(point(x, px(y))))];
                }
                Vec::new()
            }
            Float::Menu(menu) => {
                let target = match &menu.kind {
                    MenuKind::Slash { .. } => caret,
                    MenuKind::Convert { anchor, .. } | MenuKind::Pick { anchor, .. } => anchor.or(target),
                    MenuKind::Spelling { .. } => Self::selection_box(selection),
                };
                let Some(target) = target else {
                    return Vec::new();
                };
                let element = self.menu_element(menu, cx).into_any_element();
                // Above the caret while it lists directives, so their ghost shows in the text below it.
                let spot = match &menu.kind {
                    MenuKind::Slash { items, .. }
                        if items
                            .iter()
                            .any(|item| super::directives::ghost_kind(&item.action).is_some()) =>
                    {
                        let wanted = Self::menu_size(self.menu_rows(menu).iter());
                        let (width, height) = (f32::from(wanted.width), f32::from(wanted.height));
                        let gap = f32::from(tokens::FLOAT_GAP);
                        let (x, _) = place::at_caret(rect(target), width, height, area, gap);
                        let (_, y) = place::above(rect(target), width, height, area, gap);
                        Spot::At(point(px(x), px(y)))
                    }
                    _ => Spot::Caret(target, area),
                };
                let mut floats = vec![(element, spot)];
                floats.extend(self.options_float(cx));
                floats
            }
            Float::Link(link) => {
                let Some(target) = target else {
                    return Vec::new();
                };
                // At the selection's left edge, inside the text column.
                let column = Rect {
                    left: f32::from(self.column.text_left - tokens::FLOAT_GAP),
                    right: f32::from(self.column.text_left + self.column.text_width + tokens::FLOAT_GAP),
                    ..area
                };
                vec![(
                    self.link_popup(link, cx).into_any_element(),
                    Spot::Caret(target, column),
                )]
            }
            Float::Rich(_) => self.rich_float(area, window, cx).into_iter().collect(),
            Float::Complete(_) => {
                let Some(caret) = caret else {
                    return Vec::new();
                };
                // As tall as the room on the roomier side of the caret, so the card never covers the line it completes.
                let below = viewport.bottom() - caret.bottom();
                let above = caret.top() - viewport.top();
                let room = below.max(above) - tokens::FLOAT_GAP * 2.;
                self.completion_card(room, cx)
                    .map(|card| vec![(card, Spot::Caret(caret, area))])
                    .unwrap_or_default()
            }
        }
    }
}

/// Writes `opening` as the first line of the list table a command inserts, keeping its selection on the same text.
fn with_table_options(command: &mut Command, opening: &str) {
    let Some(edit) = command.edits.iter_mut().find(|edit| edit.text.contains("::: table")) else {
        return;
    };
    let at = edit.text.find("::: table").unwrap_or(0);
    let old_end = at + edit.text[at..].find(['\r', '\n']).unwrap_or(edit.text.len() - at);
    let grown = opening.len() as isize - (old_end - at) as isize;
    let written_at = edit.range.start + at;
    edit.text.replace_range(at..old_end, opening);
    let shift = |offset: usize| {
        if offset > written_at {
            offset.wrapping_add_signed(grown)
        } else {
            offset
        }
    };
    command.selection = Selection {
        anchor: shift(command.selection.anchor),
        head: shift(command.selection.head),
    };
}

/// Lays a float out and draws it above everything else, later in this same frame, where its measured size puts it.
pub(super) fn prepaint_float(mut element: AnyElement, spot: Spot, window: &mut Window, cx: &mut App) {
    let size = element.layout_as_root(AvailableSpace::min_size(), window, cx);
    let origin = match spot {
        Spot::At(origin) => origin,
        Spot::Caret(target, area) => placed_at_caret(target, size, area),
    };
    window.defer_draw(element, origin, 2, None);
}

/// Tracks a label's letters apart with thin spaces, about 0.15 em, since gpui sets no letter spacing. Digits keep
/// together and spaces stay as written: "KAPITEL 1", "11".
fn tracked(label: &str) -> String {
    let mut out = String::with_capacity(label.len() * 4);
    let mut previous = None::<char>;
    for character in label.chars() {
        if previous.is_some_and(char::is_alphabetic) && character.is_alphabetic() {
            out.push('\u{2009}');
        }
        out.push(character);
        previous = Some(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{matching, tracked};

    #[test]
    fn tracks_letters_and_keeps_numbers_tight() {
        assert_eq!(tracked("01"), "01");
        assert_eq!(
            tracked("KAPITEL 11"),
            "K\u{2009}A\u{2009}P\u{2009}I\u{2009}T\u{2009}E\u{2009}L 11"
        );
    }

    #[test]
    fn a_query_keeps_the_rows_that_hold_each_of_its_words() {
        let labels = ["Plain text", "JavaScript", "TypeScript", "Python"];
        assert_eq!(matching(labels.into_iter(), "script"), [1, 2]);
        assert_eq!(matching(labels.into_iter(), "TYPE s"), [2]);
        assert_eq!(matching(labels.into_iter(), "text plain"), [0]);
        assert!(matching(labels.into_iter(), "rust").is_empty());
    }
}
