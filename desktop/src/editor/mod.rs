//! Centre pane, writing side: one chapter as a galley on a sheet. The buffer holds the Markdown, every change is an
//! edit to it, and what shows is derived from it: blocks, hidden markers, and shaped lines, only for what is on screen.

mod display;
mod element;
mod layout;
mod trace;

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollEvent, SharedString, Task,
    UTF16Selection, Window, actions, div, point, px, size,
};

use crate::source::inline::{inline_spans, line_markers};
use crate::source::{BlockKind, Blocks, Buffer, Edit, EditKind, Selection, motion, typing};
use crate::tokens::{self, Tokens};
use display::{Syntax, display_lines, hidden_ranges};
use element::Galley;
use layout::{BlockLayout, GalleyStyle, ShapeRequest, shape_block};
use trace::Trace;

actions!(
    editor,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToLineStart,
        NewBlock,
        NewLine,
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
        PageUp,
        PageDown,
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
        SelectPageUp,
        SelectPageDown,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Save,
        ShowCharacterPalette,
    ]
);

const CONTEXT: &str = "Editor";

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("shift-backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("alt-backspace", DeleteWordLeft, context),
        KeyBinding::new("alt-delete", DeleteWordRight, context),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, context),
        KeyBinding::new("enter", NewBlock, context),
        KeyBinding::new("shift-enter", NewLine, context),
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
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
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
        KeyBinding::new("shift-pageup", SelectPageUp, context),
        KeyBinding::new("shift-pagedown", SelectPageDown, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-z", Undo, context),
        KeyBinding::new("cmd-shift-z", Redo, context),
        KeyBinding::new("cmd-s", Save, context),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, context),
    ]);
}

#[derive(Clone, Copy)]
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
    PageUp,
    PageDown,
}

/// Where the top of the viewport is: a distance into a block. Anchoring to a block rather than to a total height means
/// a block's estimated height turning into its real one moves nothing on screen.
#[derive(Clone, Copy, Default)]
struct Anchor {
    block: usize,
    offset: Pixels,
}

/// The cached derivations of one block. A change to the block replaces its slot.
#[derive(Default, Clone)]
struct Slot {
    syntax: Option<Rc<Syntax>>,
    layout: Option<Rc<BlockLayout>>,
}

/// The text column inside the pane.
#[derive(Clone, Copy, Default)]
struct Column {
    viewport: Bounds<Pixels>,
    sheet_left: Pixels,
    sheet_width: Pixels,
    text_left: Pixels,
    text_width: Pixels,
}

/// A mouse selection in progress. `unit` is what a double or triple click started with, which a drag extends by.
#[derive(Clone)]
struct Drag {
    origin: Range<usize>,
    unit: DragUnit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragUnit {
    Character,
    Word,
    Line,
}

/// A caret place in the shaped text.
#[derive(Clone, Copy)]
struct Located {
    block: usize,
    line: usize,
    row: usize,
    display: usize,
}

/// The text changed. Reading it is left to whoever decides when that is worth it.
pub struct TextChanged;

impl EventEmitter<TextChanged> for EditorView {}

/// The caret moved, to this byte of the text.
pub struct CaretMoved(pub usize);

impl EventEmitter<CaretMoved> for EditorView {}

pub struct EditorView {
    focus: FocusHandle,
    path: Option<PathBuf>,
    status: SharedString,
    style: GalleyStyle,
    buffer: Buffer,
    blocks: Blocks,
    slots: Vec<Slot>,
    selection: Selection,
    /// At a soft wrap, whether the caret shows at the end of the upper row.
    upstream: bool,
    marked: Option<Range<usize>>,
    /// The column up and down keep to, kept across lines that are too short for it.
    goal_x: Option<Pixels>,
    /// Where markers reveal. Follows the caret, but stays put during a drag, so text does not jump under the pointer.
    reveal_at: usize,
    drag: Option<Drag>,
    scroll: Anchor,
    autoscroll: bool,
    column: Column,
    /// Blocks in the current frame, with the window y of each one's top.
    visible: Vec<(usize, Pixels)>,
    hovered: Option<usize>,
    caret_on: bool,
    blink: Task<()>,
    trace: Trace,
    /// The newest revision and body style painted, and when each first was.
    painted_revision: (u64, Instant),
    painted_style: (u64, Instant),
    style_generation: u64,
}

const TOP_MARGIN: Pixels = px(16.);
const SHEET_PADDING_Y: Pixels = px(36.);
const GUTTER_WIDTH: Pixels = px(40.);
const SHEET_PADDING_X: Pixels = px(24.);
const MAX_SHEET_WIDTH: Pixels = px(680.);
const CARET_MARGIN: Pixels = px(24.);
const BLINK: Duration = Duration::from_millis(530);

impl EditorView {
    /// Opens `path`, keeping its bytes as they are. A folder or a file that cannot be read as text shows why, and has
    /// no path to save to, so nothing can overwrite it.
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (buffer, path, status) = match path {
            Some(path) => match open(&path) {
                Ok(buffer) => (buffer, Some(path), SharedString::default()),
                Err(message) => (Buffer::new(""), None, message.into()),
            },
            None => (Buffer::new(""), None, SharedString::default()),
        };
        let blocks = Blocks::parse(buffer.text());
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut view = Self {
            focus,
            path,
            status,
            style: GalleyStyle::new(),
            slots: vec![Slot::default(); blocks.as_slice().len()],
            buffer,
            blocks,
            selection: Selection::default(),
            upstream: false,
            marked: None,
            goal_x: None,
            reveal_at: 0,
            drag: None,
            scroll: Anchor {
                block: 0,
                offset: px(0.) - TOP_MARGIN - SHEET_PADDING_Y,
            },
            autoscroll: false,
            column: Column::default(),
            visible: Vec::new(),
            hovered: None,
            caret_on: true,
            blink: Task::ready(()),
            trace: Trace::from_env(),
            painted_revision: (0, Instant::now()),
            painted_style: (0, Instant::now()),
            style_generation: 0,
        };
        view.restart_blink(cx);
        view
    }

    /// The document's file, if it opened as one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn revision(&self) -> u64 {
        self.buffer.revision()
    }

    /// Puts the caret at `offset`, on a character boundary at or before it, and brings it into view.
    pub fn jump_to(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.buffer.text().floor_char_boundary(offset.min(self.buffer.len()));
        self.set_selection(Selection::caret(offset), false, window, cx);
    }

    /// Sets the galley's body from the theme. Every block is shaped again as it comes into view.
    pub fn set_body(&mut self, family: &str, size: f32, line_height: f32, cx: &mut Context<Self>) {
        self.style.set_body(family, size, line_height);
        self.style_generation += 1;
        for slot in &mut self.slots {
            slot.layout = None;
        }
        cx.notify();
    }

    pub fn style_generation(&self) -> u64 {
        self.style_generation
    }

    /// When `revision` or a newer one was first painted, if it has been.
    pub fn revision_painted_at(&self, revision: u64) -> Option<Instant> {
        (self.painted_revision.0 >= revision).then_some(self.painted_revision.1)
    }

    /// When body style `generation` or a newer one was first painted, if it has been.
    pub fn style_painted_at(&self, generation: u64) -> Option<Instant> {
        (self.painted_style.0 >= generation).then_some(self.painted_style.1)
    }

    /// Called at the end of each paint.
    fn note_painted(&mut self) {
        let now = Instant::now();
        if self.painted_revision.0 != self.buffer.revision() {
            self.painted_revision = (self.buffer.revision(), now);
        }
        if self.painted_style.0 != self.style_generation {
            self.painted_style = (self.style_generation, now);
        }
    }

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.caret_on = true;
        self.blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let toggled = this.update(cx, |this, cx| {
                    this.caret_on = !this.caret_on;
                    cx.notify();
                });
                if toggled.is_err() {
                    break;
                }
            }
        });
    }

    // Text changes

    fn edit(
        &mut self,
        kind: EditKind,
        edits: Vec<Edit>,
        after: Selection,
        marked: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = self.trace.input();
        match self.buffer.transact(kind, edits, self.selection, after, started) {
            Ok(splice) => self.resegment(splice),
            Err(error) => {
                eprintln!("edit rejected: {error}");
                return;
            }
        }
        self.marked = marked;
        self.set_selection(after, false, window, cx);
        window.notify_text_input_changed(&self.focus);
        self.trace.handled(started);
        cx.emit(TextChanged);
    }

    fn resegment(&mut self, splice: crate::source::buffer::Splice) {
        let delta = splice.new_end as isize - splice.old_end as isize;
        let anchor_start = self.blocks.as_slice()[self.scroll.block].range.start;
        let change = self.blocks.apply(self.buffer.text(), splice);
        self.slots.splice(
            change.start..change.start + change.removed,
            vec![Slot::default(); change.inserted],
        );
        let moved = if anchor_start > splice.start {
            anchor_start.saturating_add_signed(delta).max(splice.start)
        } else {
            anchor_start
        };
        self.scroll.block = self.blocks.index_at(moved);
        self.hovered = None;
    }

    fn set_selection(&mut self, selection: Selection, upstream: bool, window: &mut Window, cx: &mut Context<Self>) {
        if selection.head != self.selection.head {
            cx.emit(CaretMoved(selection.head));
        }
        self.selection = selection;
        self.upstream = upstream;
        self.goal_x = None;
        if self.drag.is_none() {
            self.reveal_at = selection.head;
        }
        self.autoscroll = true;
        self.restart_blink(cx);
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn replace_selection(&mut self, text: &str, kind: EditKind, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.selection.range();
        let after = Selection::caret(range.start + text.len());
        self.edit(kind, vec![Edit::new(range, text)], after, None, window, cx);
    }

    fn delete_towards(&mut self, target: impl Fn(&str, usize) -> usize, window: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selection.is_empty() {
            let head = self.selection.head;
            let other = target(self.buffer.text(), head);
            head.min(other)..head.max(other)
        } else {
            self.selection.range()
        };
        if !range.is_empty() {
            let after = Selection::caret(range.start);
            self.edit(EditKind::Deleting, vec![Edit::new(range, "")], after, None, window, cx);
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::left, window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::right, window, cx);
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::prev_word, window, cx);
    }

    fn delete_word_right(&mut self, _: &DeleteWordRight, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::next_word, window, cx);
    }

    fn delete_to_line_start(&mut self, _: &DeleteToLineStart, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.row_edge(false, window).0;
        self.delete_towards(move |_, _| target, window, cx);
    }

    fn new_block(&mut self, _: &NewBlock, window: &mut Window, cx: &mut Context<Self>) {
        let edit = if self.selection.is_empty() {
            typing::line_break(self.buffer.text(), &self.blocks, self.selection.head)
        } else {
            Edit::new(self.selection.range(), "\n\n")
        };
        let after = Selection::caret(edit.range.start + edit.text.len());
        self.edit(EditKind::Discrete, vec![edit], after, None, window, cx);
    }

    fn new_line(&mut self, _: &NewLine, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_selection("\n", EditKind::Discrete, window, cx);
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_none() {
            let restored = self.buffer.undo();
            self.restore(restored, window, cx);
        }
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_none() {
            let restored = self.buffer.redo();
            self.restore(restored, window, cx);
        }
    }

    fn restore(
        &mut self,
        restored: Option<crate::source::buffer::Restored>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(restored) = restored else {
            return;
        };
        let started = self.trace.input();
        self.resegment(restored.splice);
        self.set_selection(restored.selection, false, window, cx);
        window.notify_text_input_changed(&self.focus);
        self.trace.handled(started);
        cx.emit(TextChanged);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            let text = self.buffer.text()[self.selection.range()].to_owned();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            let text = self.buffer.text()[self.selection.range()].to_owned();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.replace_selection("", EditKind::Discrete, window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_selection(&text.replace("\r\n", "\n"), EditKind::Discrete, window, cx);
        }
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = &self.path else {
            return;
        };
        self.status = match std::fs::write(path, self.buffer.text()) {
            Ok(()) => "Saved".into(),
            Err(error) => format!("Cannot save: {error}").into(),
        };
        cx.notify();
    }

    fn show_character_palette(&mut self, _: &ShowCharacterPalette, window: &mut Window, _: &mut Context<Self>) {
        window.show_character_palette();
    }

    // Caret movement

    fn move_caret(&mut self, motion_kind: Motion, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let started = self.trace.input();
        let text = self.buffer.text();
        let head = self.selection.head;
        let range = self.selection.range();
        let collapsing = !extend && !self.selection.is_empty();
        let goal_x = self.goal_x;
        let mut new_goal = None;
        let (target, upstream) = match motion_kind {
            Motion::Left if collapsing => (range.start, false),
            Motion::Right if collapsing => (range.end, false),
            Motion::Left => (motion::left(text, head), false),
            Motion::Right => (motion::right(text, head), false),
            Motion::WordLeft => (motion::prev_word(text, head), false),
            Motion::WordRight => (motion::next_word(text, head), false),
            Motion::DocStart => (0, false),
            Motion::DocEnd => (text.len(), false),
            Motion::LineStart => self.row_edge(false, window),
            Motion::LineEnd => self.row_edge(true, window),
            Motion::Up | Motion::Down => {
                let (target, upstream, goal) =
                    self.vertical_target(matches!(motion_kind, Motion::Down), goal_x, window);
                new_goal = Some(goal);
                (target, upstream)
            }
            Motion::PageUp | Motion::PageDown => {
                let (target, upstream, goal) =
                    self.page_target(matches!(motion_kind, Motion::PageDown), goal_x, window);
                new_goal = Some(goal);
                (target, upstream)
            }
        };
        let selection = Selection {
            anchor: if extend { self.selection.anchor } else { target },
            head: target,
        };
        self.set_selection(selection, upstream, window, cx);
        self.goal_x = new_goal;
        self.trace.handled(started);
    }

    /// The source offset of the start or end of the caret's row.
    fn row_edge(&mut self, end: bool, window: &Window) -> (usize, bool) {
        let here = self.locate(self.selection.head, self.upstream, window);
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let row = &line.rows[here.row];
        let display = if end { row.end } else { row.start };
        let upstream = end && !line.is_last_row(here.row);
        let start = self.blocks.as_slice()[here.block].range.start;
        (start + line.display.to_source(display), upstream)
    }

    /// The target of up or down, and the column it aims for.
    fn vertical_target(&mut self, down: bool, goal: Option<Pixels>, window: &Window) -> (usize, bool, Pixels) {
        let here = self.locate(self.selection.head, self.upstream, window);
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let goal = goal.unwrap_or(line.x + line.x_in_row(here.display, here.row));
        let last_block = self.blocks.as_slice().len() - 1;
        let (mut block, mut line_index, mut row) = (here.block, here.line, here.row);
        let mut current = layout;
        loop {
            if down {
                if row + 1 < current.lines[line_index].rows.len() {
                    row += 1;
                } else if line_index + 1 < current.lines.len() {
                    line_index += 1;
                    row = 0;
                } else if block < last_block {
                    block += 1;
                    current = self.layout_block(block, window);
                    line_index = 0;
                    row = 0;
                } else {
                    return (self.buffer.len(), false, goal);
                }
            } else if row > 0 {
                row -= 1;
            } else if line_index > 0 {
                line_index -= 1;
                row = current.lines[line_index].rows.len() - 1;
            } else if block > 0 {
                block -= 1;
                current = self.layout_block(block, window);
                line_index = current.lines.len() - 1;
                row = current.lines[line_index].rows.len() - 1;
            } else {
                return (0, false, goal);
            }
            if !current.lines[line_index].skipped {
                break;
            }
        }
        let line = &current.lines[line_index];
        let display = line.display_at_x(row, goal - line.x);
        let upstream = display == line.rows[row].end && !line.is_last_row(row);
        let start = self.blocks.as_slice()[block].range.start;
        (start + line.display.to_source(display), upstream, goal)
    }

    /// Scrolls a page and returns the place at the caret's old height on screen.
    fn page_target(&mut self, down: bool, goal: Option<Pixels>, window: &Window) -> (usize, bool, Pixels) {
        let height = self.column.viewport.size.height;
        let distance = (height - px(48.)).max(px(48.));
        let caret = self.caret_bounds(window);
        let here = caret.map_or(self.column.viewport.origin, |caret| caret.origin);
        let goal = goal.unwrap_or(here.x);
        self.scroll_by(if down { distance } else { px(0.) - distance });
        self.lay_out_visible(window);
        let target = point(goal, here.y.max(self.column.viewport.top()));
        let (offset, upstream) = self.offset_at(target, window);
        (offset, upstream, goal)
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        let selection = Selection {
            anchor: 0,
            head: self.buffer.len(),
        };
        self.set_selection(selection, false, window, cx);
    }

    // Mouse

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (offset, upstream) = self.offset_at(event.position, window);
        let text = self.buffer.text();
        let (origin, unit) = match event.click_count {
            1 => (offset..offset, DragUnit::Character),
            2 => (motion::word_range(text, offset), DragUnit::Word),
            _ => (motion::line_range(text, offset), DragUnit::Line),
        };
        let selection = if event.modifiers.shift {
            Selection {
                anchor: self.selection.anchor,
                head: offset,
            }
        } else {
            Selection {
                anchor: origin.start,
                head: origin.end,
            }
        };
        self.drag = Some(Drag { origin, unit });
        self.reveal_at = selection.head;
        self.set_selection(selection, upstream, window, cx);
    }

    fn mouse_moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let viewport = self.column.viewport;
        let inside = viewport.contains(&event.position);
        let hovered = if inside {
            self.block_at_y(event.position.y)
        } else {
            None
        };
        if hovered != self.hovered {
            self.hovered = hovered;
            cx.notify();
        }
        let Some(drag) = self.drag.clone() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_drag(cx);
            return;
        }
        let above = viewport.top() - event.position.y;
        let below = event.position.y - viewport.bottom();
        if above > px(0.) {
            self.scroll_by(px(0.) - above);
            self.lay_out_visible(window);
        } else if below > px(0.) {
            self.scroll_by(below);
            self.lay_out_visible(window);
        }
        let (offset, upstream) = self.offset_at(event.position, window);
        let text = self.buffer.text();
        let unit = match drag.unit {
            DragUnit::Character => offset..offset,
            DragUnit::Word => motion::word_range(text, offset),
            DragUnit::Line => motion::line_range(text, offset),
        };
        let selection = if self.selection.anchor != drag.origin.start && drag.unit == DragUnit::Character {
            // A shift-click drag keeps the anchor it started from.
            Selection {
                anchor: self.selection.anchor,
                head: offset,
            }
        } else if unit.start < drag.origin.start {
            Selection {
                anchor: drag.origin.end,
                head: unit.start,
            }
        } else {
            Selection {
                anchor: drag.origin.start,
                head: unit.end.max(drag.origin.end),
            }
        };
        if selection != self.selection {
            self.set_selection(selection, upstream, window, cx);
            self.autoscroll = false;
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, cx: &mut Context<Self>) {
        self.end_drag(cx);
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.reveal_at = self.selection.head;
            cx.notify();
        }
    }

    fn scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.style.body_size * self.style.leading);
        self.scroll_by(px(0.) - delta.y);
        window.invalidate_character_coordinates();
        cx.notify();
    }

    // Layout and geometry

    /// The height of a block: real if it has been shaped for this width, otherwise estimated.
    fn block_height(&self, index: usize) -> Pixels {
        match &self.slots[index].layout {
            Some(layout) => layout.height,
            None => {
                let block = &self.blocks.as_slice()[index];
                self.style.estimate_height(
                    block.kind,
                    &self.buffer.text()[block.range.clone()],
                    self.column.text_width,
                )
            }
        }
    }

    /// Shapes a block for the current caret and width, or returns the shape it already has.
    fn layout_block(&mut self, index: usize, window: &Window) -> Rc<BlockLayout> {
        let block = self.blocks.as_slice()[index].clone();
        let text = self.buffer.text();
        let block_text = &text[block.range.clone()];
        let last = index + 1 == self.blocks.as_slice().len();
        let trailing_line = last && text.ends_with('\n');
        let syntax = self.slots[index]
            .syntax
            .get_or_insert_with(|| {
                Rc::new(Syntax {
                    spans: inline_spans(block.kind, block_text),
                    markers: line_markers(block.kind, block_text),
                })
            })
            .clone();
        let caret = (self.blocks.index_at(self.reveal_at) == index).then(|| self.reveal_at - block.range.start);
        let hidden = hidden_ranges(block.kind, block_text, &syntax, caret, trailing_line);
        let marked = self.marked.as_ref().and_then(|marked| {
            let start = marked.start.max(block.range.start);
            let end = marked.end.min(block.range.end);
            (start < end).then(|| start - block.range.start..end - block.range.start)
        });
        let width = self.column.text_width;
        if let Some(layout) = &self.slots[index].layout
            && layout.width == width
            && layout.hidden == hidden
            && layout.marked == marked
        {
            return layout.clone();
        }
        let lines = display_lines(block.kind, block_text, &syntax, &hidden, marked.clone(), trailing_line);
        let separator = block.kind == BlockKind::Blank && motion::is_separator(text, block.range.start);
        let layout = Rc::new(shape_block(
            ShapeRequest {
                kind: block.kind,
                lines,
                separator,
                hidden,
                marked,
                width,
                style: &self.style,
            },
            window,
        ));
        self.slots[index].layout = Some(layout.clone());
        layout
    }

    fn scroll_by(&mut self, delta: Pixels) {
        self.scroll.offset += delta;
        self.normalize_scroll();
    }

    fn normalize_scroll(&mut self) {
        while self.scroll.offset < px(0.) && self.scroll.block > 0 {
            self.scroll.block -= 1;
            self.scroll.offset += self.block_height(self.scroll.block);
        }
        if self.scroll.block == 0 {
            self.scroll.offset = self.scroll.offset.max(px(0.) - TOP_MARGIN - SHEET_PADDING_Y);
        }
        let last = self.blocks.as_slice().len() - 1;
        while self.scroll.block < last && self.scroll.offset >= self.block_height(self.scroll.block) {
            self.scroll.offset -= self.block_height(self.scroll.block);
            self.scroll.block += 1;
        }
    }

    fn set_viewport(&mut self, bounds: Bounds<Pixels>) {
        let sheet_width = (bounds.size.width - GUTTER_WIDTH - tokens::SPACE_3)
            .min(MAX_SHEET_WIDTH)
            .max(px(120.));
        let slack = (bounds.size.width - GUTTER_WIDTH - tokens::SPACE_3 - sheet_width).max(px(0.));
        let sheet_left = bounds.left() + GUTTER_WIDTH + slack * 0.5;
        self.column = Column {
            viewport: bounds,
            sheet_left,
            sheet_width,
            text_left: sheet_left + SHEET_PADDING_X,
            text_width: sheet_width - SHEET_PADDING_X * 2.,
        };
    }

    /// Finds and shapes the blocks on screen, moving the scroll position back if it ran past the end.
    fn lay_out_visible(&mut self, window: &Window) {
        let viewport = self.column.viewport;
        let block_count = self.blocks.as_slice().len();
        let mut retried = false;
        loop {
            self.visible.clear();
            let mut y = viewport.top() - self.scroll.offset;
            let mut index = self.scroll.block;
            while index < block_count && y < viewport.bottom() {
                let layout = self.layout_block(index, window);
                self.visible.push((index, y));
                y += layout.height;
                index += 1;
            }
            let end = y + SHEET_PADDING_Y + TOP_MARGIN;
            let past_end = index == block_count && end < viewport.bottom();
            let scrolled = self.scroll.block > 0 || self.scroll.offset > px(0.) - TOP_MARGIN - SHEET_PADDING_Y;
            if !past_end || !scrolled || retried {
                return;
            }
            self.scroll_by(end - viewport.bottom());
            retried = true;
        }
    }

    /// Moves the scroll position so the caret is on screen, if it is not.
    fn scroll_to_caret(&mut self, window: &Window) {
        let here = self.locate(self.selection.head, self.upstream, window);
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let caret_top = line.y + line.line_height * here.row as f32;
        let caret_bottom = caret_top + line.line_height;
        let height = self.column.viewport.size.height;

        let is_above =
            here.block < self.scroll.block || (here.block == self.scroll.block && caret_top < self.scroll.offset);
        if is_above {
            self.scroll = Anchor {
                block: here.block,
                offset: caret_top - CARET_MARGIN,
            };
            self.shape_back_from(here.block, CARET_MARGIN, window);
            self.normalize_scroll();
            return;
        }
        let mut distance = caret_bottom - self.scroll.offset;
        let mut index = self.scroll.block;
        while index < here.block && distance <= height {
            distance += self.layout_block(index, window).height;
            index += 1;
        }
        if index < here.block || distance > height {
            self.scroll = Anchor {
                block: here.block,
                offset: caret_bottom + CARET_MARGIN - height,
            };
            self.shape_back_from(here.block, height, window);
            self.normalize_scroll();
        }
    }

    /// Shapes the blocks above `index` that cover `distance`, so that scrolling back through them lands exactly.
    fn shape_back_from(&mut self, index: usize, distance: Pixels, window: &Window) {
        let mut covered = px(0.);
        let mut block = index;
        while block > 0 && covered < distance {
            block -= 1;
            covered += self.layout_block(block, window).height;
        }
    }

    fn locate(&mut self, offset: usize, upstream: bool, window: &Window) -> Located {
        let block = self.blocks.index_at(offset);
        let layout = self.layout_block(block, window);
        let relative = offset - self.blocks.as_slice()[block].range.start;
        let line = layout
            .lines
            .iter()
            .rposition(|line| line.display.source.start <= relative)
            .unwrap_or(0);
        let shaped = &layout.lines[line];
        let display = shaped.display.to_display(relative);
        Located {
            block,
            line,
            row: shaped.row_of(display, upstream),
            display,
        }
    }

    fn block_at_y(&self, y: Pixels) -> Option<usize> {
        let at = self.visible.partition_point(|(_, top)| *top <= y);
        at.checked_sub(1).map(|at| self.visible[at].0)
    }

    /// The source offset nearest a window position, and whether it shows at the end of an upper row.
    fn offset_at(&mut self, position: Point<Pixels>, window: &Window) -> (usize, bool) {
        let at = self
            .visible
            .partition_point(|(_, top)| *top <= position.y)
            .saturating_sub(1);
        let Some(&(index, top)) = self.visible.get(at) else {
            return (0, false);
        };
        let layout = self.layout_block(index, window);
        let range = self.blocks.as_slice()[index].range.clone();
        let relative_y = (position.y - top).max(px(0.));
        let line_index = layout
            .lines
            .iter()
            .rposition(|line| line.y <= relative_y && line.height > px(0.))
            .unwrap_or(0);
        let line = &layout.lines[line_index];
        if line.skipped && line.height > px(0.) {
            return if relative_y < line.y + line.height * 0.5 {
                (motion::prev_grapheme(self.buffer.text(), range.start), false)
            } else {
                (range.end, false)
            };
        }
        let row = (((relative_y - line.y) / line.line_height) as usize).min(line.rows.len() - 1);
        let x = position.x - self.column.text_left - line.x;
        let display = line.display_at_x(row, x.max(px(0.)));
        let upstream = display == line.rows[row].end && !line.is_last_row(row);
        (range.start + line.display.to_source(display), upstream)
    }

    fn caret_bounds(&mut self, window: &Window) -> Option<Bounds<Pixels>> {
        self.offset_bounds(self.selection.head, self.upstream, window)
    }

    /// The window rectangle of a caret at `offset`, if its block is on screen.
    fn offset_bounds(&mut self, offset: usize, upstream: bool, window: &Window) -> Option<Bounds<Pixels>> {
        let here = self.locate(offset, upstream, window);
        let (_, top) = *self.visible.iter().find(|(index, _)| *index == here.block)?;
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let origin = point(
            self.column.text_left + line.x + line.x_in_row(here.display, here.row),
            top + line.y + line.line_height * here.row as f32,
        );
        Some(Bounds::new(origin, size(px(1.5), line.line_height)))
    }
}

fn open(path: &Path) -> Result<Buffer, String> {
    if path.is_dir() {
        return Err("Folders open in a later version. Open a Markdown file.".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    Buffer::from_bytes(bytes).map_err(|_| format!("{} is not UTF-8 text.", path.display()))
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// One listener per caret motion, as a move and as a selection.
macro_rules! motions {
    ($element:expr, $cx:expr, $($action:ident => $motion:ident, $select:ident),* $(,)?) => {
        $element
            $(
                .on_action($cx.listener(|this, _: &$action, window, cx| {
                    this.move_caret(Motion::$motion, false, window, cx)
                }))
                .on_action($cx.listener(|this, _: &$select, window, cx| {
                    this.move_caret(Motion::$motion, true, window, cx)
                }))
            )*
    };
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let header = div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .flex()
            .items_center()
            .justify_center()
            .gap(tokens::SPACE_2)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
            .child("Write")
            .child(self.status.clone());
        let pane = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::new_block))
            .on_action(cx.listener(Self::new_line))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_scroll(cx.listener(Self::scroll));
        motions!(
            pane,
            cx,
            Left => Left, SelectLeft,
            Right => Right, SelectRight,
            Up => Up, SelectUp,
            Down => Down, SelectDown,
            WordLeft => WordLeft, SelectWordLeft,
            WordRight => WordRight, SelectWordRight,
            LineStart => LineStart, SelectLineStart,
            LineEnd => LineEnd, SelectLineEnd,
            DocStart => DocStart, SelectDocStart,
            DocEnd => DocEnd, SelectDocEnd,
            PageUp => PageUp, SelectPageUp,
            PageDown => PageDown, SelectPageDown,
        )
        .child(header)
        .child(Galley::new(cx.entity()))
    }
}

impl EntityInputHandler for EditorView {
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

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.selection.range()),
            reversed: self.selection.reversed(),
        })
    }

    fn set_selected_text_range(&mut self, range_utf16: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.buffer.range_from_utf16(&range_utf16);
        let selection = Selection {
            anchor: range.start,
            head: range.end,
        };
        self.set_selection(selection, false, window, cx);
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.buffer.utf16_len())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.buffer.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = if self.marked.is_some() {
            EditKind::Composing
        } else {
            EditKind::Typing
        };
        let range = self.input_range(range_utf16);
        let after = Selection::caret(range.start + new_text.len());
        self.edit(kind, vec![Edit::new(range, new_text)], after, None, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.input_range(range_utf16);
        let new_len_utf16 = new_text.encode_utf16().count();
        let marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        // The new selection is in UTF-16 units within the marked text, which is only known once it is written.
        let after_text = new_selected_range_utf16.map(|selected| {
            let (start, end) = (selected.start.min(new_len_utf16), selected.end.min(new_len_utf16));
            (byte_offset(new_text, start), byte_offset(new_text, end))
        });
        let after = match after_text {
            Some((start, end)) => Selection {
                anchor: range.start + start,
                head: range.start + end,
            },
            None => Selection::caret(range.start + new_text.len()),
        };
        self.edit(
            EditKind::Composing,
            vec![Edit::new(range, new_text)],
            after,
            marked,
            window,
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.buffer.range_from_utf16(&range_utf16);
        let start = self.offset_bounds(range.start, false, window)?;
        let end = self.offset_bounds(range.end, false, window).unwrap_or(start);
        let width = if end.origin.y == start.origin.y {
            (end.origin.x - start.origin.x).max(px(1.))
        } else {
            px(1.)
        };
        Some(Bounds::new(start.origin, size(width, start.size.height)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let (offset, _) = self.offset_at(point, window);
        Some(self.buffer.utf16_of(offset))
    }
}

impl EditorView {
    /// The range an input method means: the one it names, else the marked text, else the selection.
    fn input_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range| self.buffer.range_from_utf16(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.range())
    }
}

/// The byte offset of a UTF-16 offset in a short string.
fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (at, character) in text.char_indices() {
        if units >= utf16 {
            return at;
        }
        units += character.len_utf16();
    }
    text.len()
}
