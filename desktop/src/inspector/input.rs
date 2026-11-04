//! A text field for the inspector: one line, or several for the JSON view. It takes input method composition and
//! mouse selection, and keeps its text in the editor's `Buffer`, so it has the same undo and UTF-16 maps. The
//! galley's own element stays with the galley, since it is shaped for blocks and a session; this one shapes one
//! text.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    Render, SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div,
    fill, font, point, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::source::{Buffer, Edit, EditKind, Selection};
use crate::tokens::{self, Tokens};

const CONTEXT: &str = "InspectorInput";

actions!(
    inspector_input,
    [
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        LineStart,
        LineEnd,
        DocStart,
        DocEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectLineStart,
        SelectLineEnd,
        SelectDocStart,
        SelectDocEnd,
        SelectAll,
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToLineStart,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Enter,
        Commit,
        Escape,
    ]
);

/// Binds the field's keys. Once, however many fields there are.
pub fn bind_keys(cx: &mut App) {
    static BOUND: std::sync::Once = std::sync::Once::new();
    BOUND.call_once(|| bind(cx));
}

fn bind(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("alt-left", WordLeft, context),
        KeyBinding::new("alt-right", WordRight, context),
        KeyBinding::new("cmd-left", LineStart, context),
        KeyBinding::new("cmd-right", LineEnd, context),
        KeyBinding::new("home", LineStart, context),
        KeyBinding::new("end", LineEnd, context),
        KeyBinding::new("cmd-up", DocStart, context),
        KeyBinding::new("cmd-down", DocEnd, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("alt-shift-left", SelectWordLeft, context),
        KeyBinding::new("alt-shift-right", SelectWordRight, context),
        KeyBinding::new("cmd-shift-left", SelectLineStart, context),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, context),
        KeyBinding::new("shift-home", SelectLineStart, context),
        KeyBinding::new("shift-end", SelectLineEnd, context),
        KeyBinding::new("cmd-shift-up", SelectDocStart, context),
        KeyBinding::new("cmd-shift-down", SelectDocEnd, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("shift-backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("alt-backspace", DeleteWordLeft, context),
        KeyBinding::new("alt-delete", DeleteWordRight, context),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-z", Undo, context),
        KeyBinding::new("cmd-shift-z", Redo, context),
        KeyBinding::new("enter", Enter, context),
        KeyBinding::new("cmd-enter", Commit, context),
        KeyBinding::new("escape", Escape, context),
    ]);
}

/// The field's text, after the author finished editing it.
pub struct Committed(pub String);

/// The author pressed Enter in a one-line field.
pub struct Submitted;

/// The text changed. Sent for every change, for views that check the text as it is typed or search as it changes.
pub struct Edited;

/// The author pressed Escape in a field that keeps its focus on Enter.
pub struct Cancelled;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

/// What the last paint shaped, for questions about where text is.
struct Shaped {
    lines: Vec<ShapedLine>,
    line_height: Pixels,
    /// Where the text starts in the window.
    origin: Point<Pixels>,
}

struct ShapedLine {
    /// Byte range of this line in the text, without its newline.
    range: Range<usize>,
    wrapped: WrappedLine,
    /// Byte ranges of the rows the line wraps into.
    rows: Vec<Range<usize>>,
    /// Distance from the text's top.
    top: Pixels,
}

impl ShapedLine {
    fn row_of(&self, at: usize) -> usize {
        let local = at.saturating_sub(self.range.start);
        self.rows
            .iter()
            .position(|row| local < row.end)
            .unwrap_or(self.rows.len() - 1)
    }

    fn x_in_row(&self, local: usize, row: usize) -> Pixels {
        let layout = &self.wrapped.unwrapped_layout;
        layout.x_for_index(local) - layout.x_for_index(self.rows[row].start)
    }
}

pub struct TextInput {
    buffer: Buffer,
    selection: Selection,
    /// The text an input method is composing.
    marked: Option<Range<usize>>,
    /// What the field held when editing began, so an unchanged field commits nothing.
    original: String,
    /// Whether the text is the author's own or an inherited value shown for reference.
    customized: bool,
    multiline: bool,
    /// Enter keeps the focus here, for a field that is used again and again, such as the find bar's.
    keep_focus: bool,
    /// Shown in the quiet color while the field is empty.
    placeholder: SharedString,
    focus: FocusHandle,
    /// Inside a drag that started in the text.
    dragging: bool,
    /// How far a one-line field is scrolled to keep its caret in view.
    scroll_x: Pixels,
    shaped: Option<Shaped>,
    /// The column the caret keeps while moving up and down.
    goal_x: Option<Pixels>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<Committed> for TextInput {}
impl EventEmitter<Submitted> for TextInput {}
impl EventEmitter<Edited> for TextInput {}
impl EventEmitter<Cancelled> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(false, window, cx)
    }

    /// A field of several lines, in a monospaced font, that grows with its text.
    pub fn multiline(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(true, window, cx)
    }

    fn build(multiline: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus_in(&focus, window, |input, _, cx| {
                // A one-line field selects everything on focus, as a form field does.
                if !input.multiline && !input.dragging {
                    input.selection = Selection {
                        anchor: 0,
                        head: input.buffer.len(),
                    };
                }
                cx.notify();
            }),
            cx.on_focus_out(&focus, window, |input, _, _, cx| {
                input.marked = None;
                input.commit(cx);
            }),
        ];
        Self {
            buffer: Buffer::new(String::new()),
            selection: Selection::default(),
            marked: None,
            original: String::new(),
            customized: false,
            multiline,
            keep_focus: false,
            placeholder: SharedString::default(),
            focus,
            dragging: false,
            scroll_x: px(0.),
            shaped: None,
            goal_x: None,
            _subscriptions: subscriptions,
        }
    }

    /// Makes Enter submit without leaving the field, and Escape emit [`Cancelled`].
    pub fn keep_focus_on_enter(mut self) -> Self {
        self.keep_focus = true;
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    /// Selects the whole text, so the next typed character replaces it.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.set_selection(
            Selection {
                anchor: 0,
                head: self.buffer.len(),
            },
            cx,
        );
    }

    pub fn set_value(&mut self, text: String, customized: bool, cx: &mut Context<Self>) {
        self.original.clone_from(&text);
        self.buffer = Buffer::new(text);
        self.selection = Selection::caret(self.buffer.len());
        self.marked = None;
        self.customized = customized;
        cx.notify();
    }

    /// The text as it stands, committed or not.
    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        if self.buffer.text() != self.original {
            self.original = self.buffer.text().to_owned();
            cx.emit(Committed(self.original.clone()));
        }
        cx.notify();
    }

    fn edit(&mut self, kind: EditKind, range: Range<usize>, text: &str, marked: bool, cx: &mut Context<Self>) {
        let after = Selection::caret(range.start + text.len());
        if self
            .buffer
            .transact(
                kind,
                vec![Edit::new(range, text)],
                self.selection,
                after,
                Instant::now(),
            )
            .is_err()
        {
            return;
        }
        self.selection = after;
        self.marked = (marked && !text.is_empty()).then(|| after.head - text.len()..after.head);
        self.goal_x = None;
        cx.emit(Edited);
        cx.notify();
    }

    fn replace_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = if self.multiline {
            text.to_owned()
        } else {
            text.replace(['\n', '\r'], " ")
        };
        self.edit(EditKind::Discrete, self.selection.range(), &text, false, cx);
    }

    // Movement

    fn line_bounds(&self, at: usize) -> Range<usize> {
        let text = self.buffer.text();
        let start = text[..at].rfind('\n').map_or(0, |found| found + 1);
        let end = text[at..].find('\n').map_or(text.len(), |found| at + found);
        start..end
    }

    fn previous(&self, at: usize) -> usize {
        self.buffer.text()[..at]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(start, _)| start)
    }

    fn next(&self, at: usize) -> usize {
        self.buffer.text()[at..]
            .graphemes(true)
            .next()
            .map_or(at, |grapheme| at + grapheme.len())
    }

    fn word_left(&self, at: usize) -> usize {
        self.buffer.text()[..at]
            .split_word_bound_indices()
            .rev()
            .find(|(_, word)| word.chars().any(char::is_alphanumeric))
            .map_or(0, |(start, _)| start)
    }

    fn word_right(&self, at: usize) -> usize {
        let text = self.buffer.text();
        text[at..]
            .split_word_bound_indices()
            .find(|(_, word)| word.chars().any(char::is_alphanumeric))
            .map_or(text.len(), |(start, word)| at + start + word.len())
    }

    fn target(&mut self, motion: Motion) -> usize {
        let head = self.selection.head;
        let keeps_column = matches!(motion, Motion::Up | Motion::Down);
        let target = match motion {
            Motion::Left => self.previous(head),
            Motion::Right => self.next(head),
            Motion::WordLeft => self.word_left(head),
            Motion::WordRight => self.word_right(head),
            Motion::LineStart => self.row_edge(head, true),
            Motion::LineEnd => self.row_edge(head, false),
            Motion::DocStart => 0,
            Motion::DocEnd => self.buffer.len(),
            Motion::Up => self.vertical(head, -1),
            Motion::Down => self.vertical(head, 1),
        };
        if !keeps_column {
            self.goal_x = None;
        }
        target
    }

    /// The start or end of the row the offset is on.
    fn row_edge(&self, at: usize, start: bool) -> usize {
        let Some(shaped) = &self.shaped else {
            let line = self.line_bounds(at);
            return if start { line.start } else { line.end };
        };
        let Some(line) = shaped.lines.iter().find(|line| at <= line.range.end) else {
            return at;
        };
        let row = &line.rows[line.row_of(at)];
        if start {
            line.range.start + row.start
        } else {
            line.range.start + row.end
        }
    }

    /// One row up or down, at the column the caret had.
    fn vertical(&mut self, at: usize, direction: isize) -> usize {
        let Some(shaped) = &self.shaped else {
            return at;
        };
        let Some(line_index) = shaped.lines.iter().position(|line| at <= line.range.end) else {
            return at;
        };
        let line = &shaped.lines[line_index];
        let row = line.row_of(at);
        let x = *self
            .goal_x
            .get_or_insert_with(|| line.x_in_row(at - line.range.start, row));
        let (target_line, target_row) = match direction {
            -1 if row > 0 => (line_index, row - 1),
            -1 if line_index > 0 => (line_index - 1, shaped.lines[line_index - 1].rows.len() - 1),
            1 if row + 1 < line.rows.len() => (line_index, row + 1),
            1 if line_index + 1 < shaped.lines.len() => (line_index + 1, 0),
            -1 => return 0,
            _ => return self.buffer.len(),
        };
        let line = &shaped.lines[target_line];
        let layout = &line.wrapped.unwrapped_layout;
        let row_range = &line.rows[target_row];
        let local = layout
            .closest_index_for_x(layout.x_for_index(row_range.start) + x)
            .clamp(row_range.start, row_range.end);
        line.range.start + local
    }

    fn move_to(&mut self, motion: Motion, extend: bool, cx: &mut Context<Self>) {
        let target = self.target(motion);
        self.set_selection(
            if extend {
                Selection {
                    anchor: self.selection.anchor,
                    head: target,
                }
            } else if !self.selection.is_empty() && matches!(motion, Motion::Left | Motion::Right) {
                let range = self.selection.range();
                Selection::caret(if motion == Motion::Left { range.start } else { range.end })
            } else {
                Selection::caret(target)
            },
            cx,
        );
    }

    fn set_selection(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.selection = selection;
        cx.notify();
    }

    fn delete(&mut self, motion: Option<Motion>, cx: &mut Context<Self>) {
        let range = if self.selection.is_empty() {
            let Some(motion) = motion else { return };
            let target = self.target(motion);
            let head = self.selection.head;
            target.min(head)..target.max(head)
        } else {
            self.selection.range()
        };
        if range.is_empty() {
            return;
        }
        self.edit(EditKind::Deleting, range, "", false, cx);
    }

    fn copy(&mut self, cut: bool, cx: &mut Context<Self>) {
        let range = self.selection.range();
        if range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.text()[range.clone()].to_owned()));
        if cut {
            self.edit(EditKind::Discrete, range, "", false, cx);
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_selection(&text, cx);
        }
    }

    fn history(&mut self, undo: bool, cx: &mut Context<Self>) {
        let restored = if undo { self.buffer.undo() } else { self.buffer.redo() };
        if let Some(restored) = restored {
            self.selection = restored.selection;
            self.marked = None;
            cx.emit(Edited);
            cx.notify();
        }
    }

    // Mouse

    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let Some(shaped) = &self.shaped else {
            return self.buffer.len();
        };
        let y = position.y - shaped.origin.y;
        let Some(line) = shaped
            .lines
            .iter()
            .rev()
            .find(|line| line.top <= y)
            .or_else(|| shaped.lines.first())
        else {
            return 0;
        };
        let row = (((y - line.top) / shaped.line_height).floor().max(0.) as usize).min(line.rows.len() - 1);
        let layout = &line.wrapped.unwrapped_layout;
        let x = position.x - shaped.origin.x + self.scroll_x;
        let local = layout
            .closest_index_for_x(layout.x_for_index(line.rows[row].start) + x)
            .clamp(line.rows[row].start, line.rows[row].end);
        line.range.start + local
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let at = self.offset_at(event.position);
        self.dragging = true;
        self.goal_x = None;
        match event.click_count {
            1 if event.modifiers.shift => self.selection.head = at,
            1 => self.selection = Selection::caret(at),
            2 => {
                let text = self.buffer.text();
                let (start, word) = text
                    .split_word_bound_indices()
                    .find(|(start, word)| at >= *start && at <= start + word.len())
                    .unwrap_or((at, ""));
                self.selection = Selection {
                    anchor: start,
                    head: start + word.len(),
                };
            }
            _ => {
                let line = if self.multiline {
                    self.line_bounds(at)
                } else {
                    0..self.buffer.len()
                };
                self.selection = Selection {
                    anchor: line.start,
                    head: line.end,
                };
            }
        }
        cx.notify();
    }

    fn mouse_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.dragging && event.dragging() {
            self.selection.head = self.offset_at(event.position);
            cx.notify();
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Context<Self>) {
        self.dragging = false;
    }

    // Shaping

    fn position(shaped: &Shaped, at: usize) -> (Point<Pixels>, Pixels) {
        let line = shaped
            .lines
            .iter()
            .find(|line| at <= line.range.end)
            .or(shaped.lines.last());
        let Some(line) = line else {
            return (shaped.origin, shaped.line_height);
        };
        let local = at.saturating_sub(line.range.start).min(line.range.len());
        let row = line.row_of(line.range.start + local);
        let x = line.x_in_row(local, row);
        (
            point(
                shaped.origin.x + x,
                shaped.origin.y + line.top + shaped.line_height * row as f32,
            ),
            shaped.line_height,
        )
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.buffer.range_from_utf16(&range_utf16);
        actual_range.replace(self.buffer.range_to_utf16(&range));
        Some(self.buffer.text()[range].to_owned())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.selection.range()),
            reversed: self.selection.reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.buffer.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.buffer.range_from_utf16(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.range());
        let kind = if self.marked.is_some() {
            EditKind::Composing
        } else {
            EditKind::Typing
        };
        let text = if self.multiline {
            text.to_owned()
        } else {
            text.replace(['\n', '\r'], " ")
        };
        self.edit(kind, range, &text, false, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.buffer.range_from_utf16(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.range());
        let start = range.start;
        self.edit(EditKind::Composing, range, text, true, cx);
        if let Some(selected) = selected_utf16 {
            let within = |units: usize| {
                let mut seen = 0;
                for (at, character) in text.char_indices() {
                    if seen >= units {
                        return at;
                    }
                    seen += character.len_utf16();
                }
                text.len()
            };
            self.selection = Selection {
                anchor: start + within(selected.start),
                head: start + within(selected.end),
            };
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let shaped = self.shaped.as_ref()?;
        let range = self.buffer.range_from_utf16(&range_utf16);
        let (start, height) = Self::position(shaped, range.start);
        let (end, _) = Self::position(shaped, range.end);
        let width = if end.y == start.y {
            (end.x - start.x).max(px(1.))
        } else {
            px(1.)
        };
        Some(Bounds::new(start, size(width, height)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.buffer.utf16_of(self.offset_at(point)))
    }
}

/// What the element shaped, shared between its layout and its paint.
type Slot = Rc<RefCell<Option<Shaped>>>;

struct InputElement {
    input: Entity<TextInput>,
}

impl IntoElement for InputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct Frame {
    hitbox: Hitbox,
    slot: Slot,
}

impl Element for InputElement {
    type RequestLayoutState = Slot;
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, Slot) {
        let slot: Slot = Rc::new(RefCell::new(None));
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let input = self.input.clone();
        let measured = slot.clone();
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known
                .width
                .or(match available.width {
                    gpui::AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                })
                .unwrap_or(px(200.));
            let shaped = input.update(cx, |input, cx| input.shape(width, window, cx));
            let height = shaped.lines.last().map_or(shaped.line_height, |line| {
                line.top + shaped.line_height * line.rows.len() as f32
            });
            *measured.borrow_mut() = Some(shaped);
            size(width, height)
        });
        (id, slot)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        slot: &mut Slot,
        window: &mut Window,
        _: &mut App,
    ) -> Frame {
        Frame {
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
            slot: slot.clone(),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Slot,
        frame: &mut Frame,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(mut shaped) = frame.slot.borrow_mut().take() else {
            return;
        };
        shaped.origin = bounds.origin;
        let focus = self.input.read(cx).focus.clone();
        let focused = focus.is_focused(window);
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);
        window.set_cursor_style(CursorStyle::IBeam, &frame.hitbox);
        let (selection, multiline, scroll_x) = {
            let input = self.input.read(cx);
            (input.selection, input.multiline, input.scroll_x)
        };
        let accent = Hsla::from(Tokens::get(cx).accent);
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if focused && !selection.is_empty() {
                for rectangle in selection_rectangles(&shaped, selection.range(), bounds.size.width) {
                    window.paint_quad(fill(
                        Bounds::new(point(rectangle.origin.x - scroll_x, rectangle.origin.y), rectangle.size),
                        accent.opacity(0.25),
                    ));
                }
            }
            for line in &shaped.lines {
                let origin = point(bounds.origin.x - scroll_x, bounds.origin.y + line.top);
                line.wrapped
                    .paint(origin, shaped.line_height, TextAlign::Left, None, window, cx)
                    .ok();
            }
            if focused && selection.is_empty() {
                let (position, height) = TextInput::position(&shaped, selection.head);
                window.paint_quad(fill(
                    Bounds::new(point(position.x - scroll_x, position.y), size(px(1.), height)),
                    accent,
                ));
            }
        });
        shaped.origin = bounds.origin;
        self.input.update(cx, |input, cx| {
            let caret = TextInput::position(&shaped, input.selection.head).0.x - shaped.origin.x;
            let before = input.scroll_x;
            if !multiline {
                if caret - input.scroll_x > bounds.size.width - px(4.) {
                    input.scroll_x = caret - bounds.size.width + px(4.);
                } else if caret < input.scroll_x {
                    input.scroll_x = caret;
                }
                if shaped
                    .lines
                    .first()
                    .is_some_and(|line| line.wrapped.width() <= bounds.size.width)
                {
                    input.scroll_x = px(0.);
                }
            }
            input.shaped = Some(shaped);
            if input.scroll_x != before {
                cx.notify();
            }
        });
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase.bubble() {
                input.update(cx, |input, cx| input.mouse_moved(event, cx));
            }
        });
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase.bubble() {
                input.update(cx, |input, cx| input.mouse_up(event, cx));
            }
        });
    }
}

impl TextInput {
    /// Shapes the text for `width`, one wrapped line per written line. A one-line field never wraps.
    fn shape(&mut self, width: Pixels, window: &mut Window, cx: &mut Context<Self>) -> Shaped {
        let tokens = Tokens::get(cx);
        let showing_placeholder = self.buffer.text().is_empty() && !self.placeholder.is_empty();
        let color: Hsla = if showing_placeholder {
            tokens.text_faint
        } else if self.customized || self.focus.is_focused(window) {
            tokens.text
        } else {
            tokens.text_faint
        }
        .into();
        let family = if self.multiline { "Menlo" } else { tokens::FONT_UI };
        let font = font(family);
        let size = if self.multiline {
            tokens::TEXT_SMALL + px(1.)
        } else {
            tokens::TEXT_BODY
        };
        let line_height = size * 1.45;
        let wrap = self.multiline.then_some(width);
        let text = if showing_placeholder {
            self.placeholder.to_string()
        } else {
            self.buffer.text().to_owned()
        };
        let mut lines = Vec::new();
        let mut top = px(0.);
        let mut start = 0;
        for written in text.split('\n') {
            let range = start..start + written.len();
            let marked = self.marked.as_ref().and_then(|marked| {
                let (from, to) = (marked.start.max(range.start), marked.end.min(range.end));
                (from < to).then(|| from - range.start..to - range.start)
            });
            let runs = runs(written.len(), marked.filter(|_| !showing_placeholder), color, &font);
            let wrapped = window
                .text_system()
                .shape_text(SharedString::from(written.to_owned()), size, &runs, wrap, None)
                .ok()
                .and_then(|mut shaped| shaped.pop())
                .unwrap_or_default();
            let rows = rows_of(&wrapped, written.len());
            let height = line_height * rows.len() as f32;
            lines.push(ShapedLine {
                range,
                wrapped,
                rows,
                top,
            });
            top += height;
            start += written.len() + 1;
        }
        Shaped {
            lines,
            line_height,
            origin: point(px(0.), px(0.)),
        }
    }
}

/// The runs of a line of `length` bytes, with the composing text underlined where it falls in the line.
fn runs(length: usize, marked: Option<Range<usize>>, color: Hsla, font: &gpui::Font) -> Vec<TextRun> {
    let plain = |len: usize, underline: bool| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: underline.then_some(UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    };
    match marked {
        Some(marked) if marked.end <= length => [
            (marked.start, false),
            (marked.len(), true),
            (length - marked.end, false),
        ]
        .into_iter()
        .filter(|(len, _)| *len > 0)
        .map(|(len, underline)| plain(len, underline))
        .collect(),
        _ => vec![plain(length, false)],
    }
}

fn rows_of(wrapped: &WrappedLine, length: usize) -> Vec<Range<usize>> {
    let layout = &wrapped.unwrapped_layout;
    let mut rows = Vec::with_capacity(wrapped.wrap_boundaries.len() + 1);
    let mut start = 0;
    for boundary in &wrapped.wrap_boundaries {
        let end = layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index;
        rows.push(start..end);
        start = end;
    }
    rows.push(start..length);
    rows
}

fn selection_rectangles(shaped: &Shaped, range: Range<usize>, width: Pixels) -> Vec<Bounds<Pixels>> {
    let mut rectangles = Vec::new();
    for line in &shaped.lines {
        let from = range.start.max(line.range.start);
        let to = range.end.min(line.range.end + 1);
        if from >= to && !(range.start <= line.range.end && range.end > line.range.end) {
            continue;
        }
        let layout = &line.wrapped.unwrapped_layout;
        for (index, row) in line.rows.iter().enumerate() {
            let (row_start, row_end) = (line.range.start + row.start, line.range.start + row.end);
            let start = from.max(row_start);
            let end = to.min(row_end + usize::from(index + 1 == line.rows.len())).min(row_end);
            let continues = range.end > line.range.end && index + 1 == line.rows.len();
            if start > end || (start == end && !continues) {
                continue;
            }
            let left = layout.x_for_index(start - line.range.start) - layout.x_for_index(row.start);
            let right = if continues {
                (width.max(layout.x_for_index(end - line.range.start) - layout.x_for_index(row.start)) + px(4.))
                    .max(left)
            } else {
                layout.x_for_index(end - line.range.start) - layout.x_for_index(row.start)
            };
            rectangles.push(Bounds::new(
                point(
                    shaped.origin.x + left,
                    shaped.origin.y + line.top + shaped.line_height * index as f32,
                ),
                size((right - left).max(px(0.)), shaped.line_height),
            ));
        }
    }
    rectangles
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let focused = self.focus.is_focused(window);
        let multiline = self.multiline;
        let field = div()
            .id("text-input")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|input, _: &Left, _, cx| input.move_to(Motion::Left, false, cx)))
            .on_action(cx.listener(|input, _: &Right, _, cx| input.move_to(Motion::Right, false, cx)))
            .on_action(cx.listener(|input, _: &Up, _, cx| input.move_to(Motion::Up, false, cx)))
            .on_action(cx.listener(|input, _: &Down, _, cx| input.move_to(Motion::Down, false, cx)))
            .on_action(cx.listener(|input, _: &WordLeft, _, cx| input.move_to(Motion::WordLeft, false, cx)))
            .on_action(cx.listener(|input, _: &WordRight, _, cx| input.move_to(Motion::WordRight, false, cx)))
            .on_action(cx.listener(|input, _: &LineStart, _, cx| input.move_to(Motion::LineStart, false, cx)))
            .on_action(cx.listener(|input, _: &LineEnd, _, cx| input.move_to(Motion::LineEnd, false, cx)))
            .on_action(cx.listener(|input, _: &DocStart, _, cx| input.move_to(Motion::DocStart, false, cx)))
            .on_action(cx.listener(|input, _: &DocEnd, _, cx| input.move_to(Motion::DocEnd, false, cx)))
            .on_action(cx.listener(|input, _: &SelectLeft, _, cx| input.move_to(Motion::Left, true, cx)))
            .on_action(cx.listener(|input, _: &SelectRight, _, cx| input.move_to(Motion::Right, true, cx)))
            .on_action(cx.listener(|input, _: &SelectUp, _, cx| input.move_to(Motion::Up, true, cx)))
            .on_action(cx.listener(|input, _: &SelectDown, _, cx| input.move_to(Motion::Down, true, cx)))
            .on_action(cx.listener(|input, _: &SelectWordLeft, _, cx| input.move_to(Motion::WordLeft, true, cx)))
            .on_action(cx.listener(|input, _: &SelectWordRight, _, cx| input.move_to(Motion::WordRight, true, cx)))
            .on_action(cx.listener(|input, _: &SelectLineStart, _, cx| input.move_to(Motion::LineStart, true, cx)))
            .on_action(cx.listener(|input, _: &SelectLineEnd, _, cx| input.move_to(Motion::LineEnd, true, cx)))
            .on_action(cx.listener(|input, _: &SelectDocStart, _, cx| input.move_to(Motion::DocStart, true, cx)))
            .on_action(cx.listener(|input, _: &SelectDocEnd, _, cx| input.move_to(Motion::DocEnd, true, cx)))
            .on_action(cx.listener(|input, _: &SelectAll, _, cx| {
                input.set_selection(
                    Selection {
                        anchor: 0,
                        head: input.buffer.len(),
                    },
                    cx,
                );
            }))
            .on_action(cx.listener(|input, _: &Backspace, _, cx| input.delete(Some(Motion::Left), cx)))
            .on_action(cx.listener(|input, _: &Delete, _, cx| input.delete(Some(Motion::Right), cx)))
            .on_action(cx.listener(|input, _: &DeleteWordLeft, _, cx| input.delete(Some(Motion::WordLeft), cx)))
            .on_action(cx.listener(|input, _: &DeleteWordRight, _, cx| input.delete(Some(Motion::WordRight), cx)))
            .on_action(cx.listener(|input, _: &DeleteToLineStart, _, cx| input.delete(Some(Motion::LineStart), cx)))
            .on_action(cx.listener(|input, _: &Copy, _, cx| input.copy(false, cx)))
            .on_action(cx.listener(|input, _: &Cut, _, cx| input.copy(true, cx)))
            .on_action(cx.listener(|input, _: &Paste, _, cx| input.paste(cx)))
            .on_action(cx.listener(|input, _: &Undo, _, cx| input.history(true, cx)))
            .on_action(cx.listener(|input, _: &Redo, _, cx| input.history(false, cx)))
            .on_action(cx.listener(|input, _: &Enter, window, cx| {
                if input.multiline {
                    input.replace_selection("\n", cx);
                } else {
                    input.commit(cx);
                    cx.emit(Submitted);
                    if !input.keep_focus {
                        window.blur(cx);
                    }
                }
            }))
            .on_action(cx.listener(|input, _: &Commit, _, cx| input.commit(cx)))
            .on_action(cx.listener(|input, _: &Escape, window, cx| {
                if input.keep_focus {
                    cx.emit(Cancelled);
                    return;
                }
                input.buffer = Buffer::new(input.original.clone());
                input.selection = Selection::caret(input.buffer.len());
                input.marked = None;
                cx.emit(Edited);
                window.blur(cx);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .px(tokens::SPACE_2)
            .overflow_hidden()
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(if focused { tokens.accent } else { tokens.divider })
            .bg(tokens.canvas);
        if multiline {
            field
                .py(tokens::SPACE_2)
                .min_h(tokens::FIELD_HEIGHT)
                .child(InputElement { input: cx.entity() })
        } else {
            field
                .h(tokens::FIELD_HEIGHT)
                .flex()
                .items_center()
                .child(div().flex_1().min_w_0().child(InputElement { input: cx.entity() }))
        }
    }
}
