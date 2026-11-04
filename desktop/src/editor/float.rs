//! What floats over the galley: the toolbar on a selection, the slash menu, a block's convert menu, and the link field.
//! Each is drawn by the galley's own frame, so it sits where the selection is in that same frame, and keyboard focus stays
//! in the text except in the link field.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, App, AvailableSpace, Bounds, Context, Entity, Focusable, MouseButton, MouseDownEvent, Pixels,
    Point, Render, ScrollHandle, SharedString, Size, Subscription, Window, deferred, div, point, px, size, svg,
};

use super::commands::Command;
use super::commands::convert::{self, BlockType, TARGETS};
use super::commands::marks::{self, Mark, MarkState, MarkStates};
use super::commands::slash::{self, Action, Item, type_icon, type_label};
use super::place::{self, Rect};
use super::rich::view::RichFloat;
use super::strings::Key;
use super::{EditorView, MenuAccept, MenuDown, MenuUp};
use crate::a11y::Control as _;
use crate::inspector::input::{Submitted, TextInput};
use crate::session::TEMPLATES;
use crate::source::{EditKind, Selection, motion};
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
}

pub(super) struct Menu {
    kind: MenuKind,
    selected: usize,
    scroll: ScrollHandle,
}

enum MenuKind {
    /// `start` is where the typed slash is. `all` is what is valid here, `items` what the query leaves of it.
    Slash {
        start: usize,
        all: Vec<Item>,
        items: Vec<Item>,
    },
    Convert {
        block: usize,
        targets: Vec<BlockType>,
        anchor: Bounds<Pixels>,
    },
    /// The suggestions for a misspelled word, then a row that adds it to the project's words.
    Spelling {
        word: Range<usize>,
        suggestions: Vec<String>,
    },
}

impl Menu {
    fn len(&self) -> usize {
        match &self.kind {
            MenuKind::Slash { items, .. } => items.len(),
            MenuKind::Convert { targets, .. } => targets.len(),
            MenuKind::Spelling { suggestions, .. } => suggestions.len() + 1,
        }
    }
}

pub(super) struct Link {
    selection: Selection,
    input: Entity<TextInput>,
    has_link: bool,
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
}

/// One button of the toolbar. The mockup's `Tt` is dropped: no case command exists in the language.
#[derive(Clone, Copy)]
enum Button {
    Mark(Mark),
    Block(BlockType),
    Link,
}

const MARK_BUTTONS: [(Mark, &str, &str); 7] = [
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

fn toolbar_size() -> Size<Pixels> {
    let buttons = MARK_BUTTONS.len() + BLOCK_BUTTONS.len() + 1;
    size(
        tokens::TOOLBAR_BUTTON * buttons as f32 + tokens::TOOLBAR_SEPARATOR + tokens::SPACE_1 * 2.,
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

pub(super) fn tooltip(text: String) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
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

/// A float's place beside `target`, in `area`.
pub(super) fn placed(target: Bounds<Pixels>, wanted: Size<Pixels>, area: Rect) -> Point<Pixels> {
    let (x, y) = place::beside(
        rect(target),
        f32::from(wanted.width),
        f32::from(wanted.height),
        area,
        f32::from(tokens::FLOAT_GAP),
    );
    point(px(x), px(y))
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
            Float::None | Float::Menu(_) => false,
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
            }))
            .child(svg().path(icon).size(tokens::ICON_SIZE).text_color(if on {
                tokens.panel
            } else if state == MarkState::Mixed {
                tokens.accent
            } else {
                tokens.text
            }))
    }

    fn toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ToolbarState {
            marks: states, block, ..
        } = self.toolbar_state();
        let mut row = div()
            .occlude()
            .h(tokens::TOOLBAR_HEIGHT)
            .px(tokens::SPACE_1)
            .flex()
            .items_center()
            .rounded(tokens::RADIUS)
            .shadow_md();
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
        row = row.child(
            div()
                .flex_none()
                .w(tokens::TOOLBAR_SEPARATOR)
                .flex()
                .justify_center()
                .child(div().w(px(1.)).h(tokens::SPACE_4).bg(tokens_now.divider)),
        );
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
        row.child(self.toolbar_button(Button::Link, "icons/mark-link.svg", tip, states.link, id, cx))
    }

    /// Whether the toolbar shows: a settled selection holding text it can mark.
    fn toolbar_wanted(&mut self) -> bool {
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
        let address = marks::link_at(text, &self.blocks, self.selection);
        if !marks::has_markable_text(text, &self.blocks, self.command_range()) {
            return;
        }
        let has_link = address.is_some();
        let selection = self.selection;
        let input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx);
            input.set_value(address.unwrap_or_default(), true, cx);
            input
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, input, Submitted, window, cx| {
                let address = input.read(cx).text().to_owned();
                this.submit_link(&address, window, cx);
            }),
            cx.on_focus_out(&input.focus_handle(cx), window, |this, _, window, cx| {
                this.close_float(cx);
                // Escape blurs the field. A click on another field keeps its focus.
                if window.focused(cx).is_none() {
                    window.focus(&this.focus, cx);
                }
            }),
        ];
        input.focus_handle(cx).focus(window, cx);
        self.float = Float::Link(Link {
            selection,
            input,
            has_link,
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

    fn submit_link(&mut self, address: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Link(link) = &self.float else {
            return;
        };
        let selection = link.selection;
        self.float = Float::None;
        window.focus(&self.focus, cx);
        let text = self.session.active_buffer().text();
        let command = if address.trim().is_empty() {
            marks::remove_link(text, &self.blocks, selection)
        } else {
            marks::set_link(text, &self.blocks, selection, address)
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
            .shadow_md()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
        self.float = Float::Menu(Menu {
            kind: MenuKind::Slash {
                start: at,
                items: all.clone(),
                all,
            },
            selected: 0,
            scroll: ScrollHandle::new(),
        });
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
            has_works: !self.rich.works.is_empty(),
            has_labels: !self.rich.book.labels.is_empty(),
            block: Some(self.blocks.as_slice()[self.blocks.index_at(at)].kind),
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
            self.float = Float::None;
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

    pub(super) fn menu_up(&mut self, _: &MenuUp, _: &mut Window, cx: &mut Context<Self>) {
        self.menu_move(false, cx);
    }

    pub(super) fn menu_down(&mut self, _: &MenuDown, _: &mut Window, cx: &mut Context<Self>) {
        self.menu_move(true, cx);
    }

    pub(super) fn menu_accept(&mut self, _: &MenuAccept, window: &mut Window, cx: &mut Context<Self>) {
        if let Float::Menu(menu) = &self.float {
            let selected = menu.selected;
            self.choose(selected, window, cx);
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
                kind: MenuKind::Convert { block, targets, .. },
                ..
            }) => {
                let Some(target) = targets.get(index) else {
                    return;
                };
                let text = self.session.active_buffer().text();
                if let Some(command) = convert::convert(text, &self.blocks, block, *target, self.selection) {
                    self.run_command(command, window, cx);
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
        self.float = Float::Menu(Menu {
            kind: MenuKind::Spelling { word, suggestions },
            selected: 0,
            scroll: ScrollHandle::new(),
        });
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
        let Some(command) = slash::apply(
            text,
            &self.blocks,
            start..caret,
            action,
            crate::shell::strings::language(),
        ) else {
            return;
        };
        let table_at = matches!(action, Action::Insert(slash::Standalone::Table)).then_some(command.selection.head);
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

    /// Opens the menu of what a block can become, under its gutter icon.
    pub(super) fn open_convert(&mut self, block: usize, anchor: Bounds<Pixels>, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text();
        let targets: Vec<BlockType> = TARGETS
            .into_iter()
            .filter(|target| convert::convert(text, &self.blocks, block, *target, self.selection).is_some())
            .collect();
        if targets.is_empty() {
            return;
        }
        self.float = Float::Menu(Menu {
            kind: MenuKind::Convert { block, targets, anchor },
            selected: 0,
            scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    fn menu_rows(&self, menu: &Menu) -> Vec<(SharedString, Option<SharedString>, &'static str)> {
        match &menu.kind {
            MenuKind::Slash { items, .. } => items
                .iter()
                .map(|item| (item.label.into(), item.group.map(SharedString::from), item.icon))
                .collect(),
            MenuKind::Convert { targets, .. } => targets
                .iter()
                .map(|target| {
                    (
                        type_label(*target, crate::shell::strings::language()).into(),
                        None,
                        type_icon(*target),
                    )
                })
                .collect(),
            MenuKind::Spelling { suggestions, .. } => {
                let suggested = suggestions
                    .iter()
                    .map(|suggestion| (suggestion.clone().into(), None, "icons/block-text.svg"));
                let add = (crate::t!(Key::AddToWords).into(), None, "icons/plus.svg");
                suggested.chain(std::iter::once(add)).collect()
            }
        }
    }

    fn menu_size(count: usize) -> Size<Pixels> {
        let rows = count.min(tokens::MENU_MAX_ROWS) as f32;
        size(
            tokens::MENU_WIDTH,
            tokens::MENU_ROW_HEIGHT * rows + tokens::SPACE_1 * 2. + px(2.),
        )
    }

    fn menu_element(&self, menu: &Menu, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx).clone();
        let wanted = Self::menu_size(menu.len());
        let selected = menu.selected;
        let rows: Vec<_> = self
            .menu_rows(menu)
            .into_iter()
            .enumerate()
            .map(|(index, (label, group, icon))| {
                let chosen = index == selected;
                div()
                    .id(("menu-row", index))
                    .role(gpui::Role::MenuItem)
                    .aria_label(label.clone())
                    .aria_selected(chosen)
                    .flex_none()
                    .h(tokens::MENU_ROW_HEIGHT)
                    .px(tokens::SPACE_2)
                    .flex()
                    .items_center()
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
                            .path(icon)
                            .size(tokens::ICON_SIZE)
                            .text_color(if chosen { tokens.accent } else { tokens.text_faint }),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .children(group.map(|group| {
                        div()
                            .flex_none()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(group)
                    }))
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
            .shadow_md()
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_float(cx)))
            .children(rows)
    }

    // The frame's floats

    /// Builds what floats in this frame, each with where it goes. Selection rectangles and the caret are this frame's.
    pub(super) fn floats(
        &mut self,
        selection: &[Bounds<Pixels>],
        caret: Option<Bounds<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, Point<Pixels>)> {
        let target = Self::selection_box(selection).or(caret);
        // Up and down it stays over the galley, and sideways inside the window.
        let viewport = self.column.viewport;
        let area = Rect {
            left: 0.,
            top: f32::from(viewport.top()),
            right: f32::from(window.viewport_size().width),
            bottom: f32::from(viewport.bottom()),
        };
        match &self.float {
            Float::None => {
                if let Some(target) = Self::selection_box(selection)
                    && self.toolbar_wanted()
                {
                    let origin = placed(target, toolbar_size(), area);
                    return vec![(self.toolbar(cx).into_any_element(), origin)];
                }
                Vec::new()
            }
            Float::Menu(menu) => {
                let target = match &menu.kind {
                    MenuKind::Slash { .. } => caret,
                    MenuKind::Convert { anchor, .. } => Some(*anchor),
                    MenuKind::Spelling { .. } => Self::selection_box(selection),
                };
                let Some(target) = target else {
                    return Vec::new();
                };
                let origin = placed_at_caret(target, Self::menu_size(menu.len()), area);
                vec![(self.menu_element(menu, cx).into_any_element(), origin)]
            }
            Float::Link(link) => {
                let Some(target) = target else {
                    return Vec::new();
                };
                let wanted = size(
                    tokens::LINK_POPUP_WIDTH,
                    tokens::FIELD_HEIGHT + tokens::SPACE_2 + px(2.),
                );
                let origin = placed(target, wanted, area);
                vec![(self.link_popup(link, cx).into_any_element(), origin)]
            }
            Float::Rich(_) => self.rich_float(area, window, cx).into_iter().collect(),
        }
    }
}

/// Lays a float out and draws it above everything else, later in this same frame.
pub(super) fn prepaint_float(
    element: AnyElement,
    origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mut wrapped = deferred(element).with_priority(2).into_any_element();
    wrapped.prepaint_as_root(origin, AvailableSpace::min_size(), window, cx);
    wrapped
}
