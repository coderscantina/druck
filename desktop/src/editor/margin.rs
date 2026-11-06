//! Comments beside the text. A chapter's opening notes stand in one faint line above its first text, in the flow, and
//! unfold there on a click or keyboard focus into fields that edit their text where it stands in the buffer, so undo and
//! the bytes around it are the editor's. Later comments stand in the right margin beside their passages, cut to a few
//! lines. An agent's notes stand in the same margin, see [`super::review`], and [`flow`] places them all, so none is
//! painted over another. Nothing here changes a byte unless the author types in a note.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, Role, SharedString, Subscription, TextRun,
    UTF16Selection, Window, WrappedLine, canvas, div, fill, font, point, px, size, svg,
};

use super::commands::notes::{Note, comments_in, first_meaningful_line, leading_notes, leading_region, shown_range};
use super::strings::Key;
use super::{
    Backspace, Cancel, CaretMoved, Copy, Cut, Delete, DeleteWordLeft, DeleteWordRight, DocEnd, DocStart, Down,
    EditorView, Left, LineEnd, LineStart, NewBlock, NewLine, Paste, Redo, Right, SelectAll, SelectDown, SelectLeft,
    SelectLineEnd, SelectLineStart, SelectRight, SelectUp, SelectWordLeft, SelectWordRight, SessionChanged,
    TextChanged, Undo, Up, WordLeft, WordRight,
};
use crate::session::ChapterId;
use crate::source::inline::SpanKind;
use crate::source::{BlockKind, Selection, motion};
use crate::tokens::{self, Tokens};

const FIELD_CONTEXT: &str = "NoteField";
const LINE_HEIGHT: Pixels = tokens::NOTE_LINE_HEIGHT;

/// Where something in the margin wants to stand, and how tall it is.
#[derive(Clone, Copy, Debug)]
pub(super) struct Wanted {
    pub y: Pixels,
    pub height: Pixels,
}

/// Lays the margin out as one flow, down from `top`, the view's top. `notes` are in the order they stand: each as near its
/// passage as the note above it allows.
pub(super) fn flow(notes: &[Wanted], top: Pixels) -> Vec<Pixels> {
    let mut next = top;
    notes
        .iter()
        .map(|note| {
            let y = note.y.max(next);
            next = y + note.height + tokens::SPACE_3;
            y
        })
        .collect()
}

/// A chapter's opening notes: folded to one line, or unfolded into their fields.
pub struct ChapterNotes {
    editor: Entity<EditorView>,
    /// Holds the keys while they are in the notes, so leaving folds them.
    focus: FocusHandle,
    unfolded: bool,
    /// The text's selection is inside a note, as a find leaves it, so the notes stay open and nothing changes out of sight.
    held_open: bool,
    /// What the notes show, so a key elsewhere in the chapter draws nothing here.
    shown: Vec<String>,
    /// The line that stands for them when folded.
    line: Option<String>,
    chapter: Option<ChapterId>,
    fields: Vec<Entity<NoteField>>,
    _subscriptions: Vec<Subscription>,
}

impl ChapterNotes {
    pub fn new(editor: Entity<EditorView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A stop of Control+Tab: it unfolds as it takes the keys and hands them to the first note.
        let focus = cx.focus_handle().tab_stop(true);
        let subscriptions = vec![
            cx.subscribe(&editor, |notes, _, _: &TextChanged, cx| notes.sync(cx)),
            cx.subscribe(&editor, |notes, _, _: &SessionChanged, cx| notes.sync(cx)),
            cx.subscribe(&editor, |notes, _, _: &CaretMoved, cx| notes.hold_for_selection(cx)),
            cx.on_focus_in(&focus, window, |notes, window, cx| notes.unfold(window, cx)),
            cx.on_focus_out(&focus, window, |notes, _, _, cx| notes.set_unfolded(false, cx)),
        ];
        let mut notes = Self {
            editor,
            focus,
            unfolded: false,
            held_open: false,
            shown: Vec::new(),
            line: None,
            chapter: None,
            fields: Vec::new(),
            _subscriptions: subscriptions,
        };
        notes.sync(cx);
        notes
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let chapter = editor.session().active();
        let shown = editor.note_texts();
        if shown == self.shown && self.chapter == Some(chapter) {
            return;
        }
        let title = editor.session().title_of(chapter);
        self.line = first_meaningful_line(shown.iter().map(String::as_str), &title).map(str::to_owned);
        if self.chapter != Some(chapter) {
            self.chapter = Some(chapter);
            self.set_unfolded(false, cx);
        }
        while self.fields.len() < shown.len() {
            let index = self.fields.len();
            let editor = self.editor.clone();
            self.fields.push(cx.new(|cx| NoteField::new(editor, index, cx)));
        }
        self.fields.truncate(shown.len());
        self.shown = shown;
        for field in &self.fields {
            field.update(cx, |field, cx| field.text_changed(cx));
        }
        cx.notify();
    }

    fn hold_for_selection(&mut self, cx: &mut Context<Self>) {
        let held = self.editor.read(cx).selection_in_note().is_some();
        if held != self.held_open || held {
            self.held_open = held;
            cx.notify();
            for field in &self.fields {
                field.update(cx, |_, cx| cx.notify());
            }
        }
    }

    fn set_unfolded(&mut self, unfolded: bool, cx: &mut Context<Self>) {
        if self.unfolded != unfolded {
            self.unfolded = unfolded;
            cx.notify();
        }
    }

    /// Opens the notes and gives the keys to the first, unless they are in one already. The field takes focus once it has
    /// been drawn, since until then it is in no part of the window.
    fn unfold(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unfolded {
            return;
        }
        self.set_unfolded(true, cx);
        cx.on_next_frame(window, |notes, window, cx| {
            let in_a_field = (notes.fields.iter()).any(|field| field.read(cx).focus.is_focused(window));
            if !in_a_field && let Some(first) = notes.fields.first() {
                let first = first.read(cx).focus.clone();
                window.focus(&first, cx);
            }
        });
    }

    /// Folds the notes and returns the keys to the text.
    fn fold(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_unfolded(false, cx);
        let text = self.editor.read(cx).focus_handle(cx);
        window.focus(&text, cx);
    }
}

impl Render for ChapterNotes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let line = self.line.clone().unwrap_or_default();
        let label = crate::t!(Key::ChapterNotes);
        let unfolded = self.unfolded || self.held_open;
        let icon = svg()
            .flex_none()
            .path("icons/block-comment.svg")
            .size(tokens::NOTE_ICON)
            .text_color(tokens.text_faint);
        let row = div()
            .id("chapter-note")
            .h(LINE_HEIGHT)
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .cursor_pointer()
            .text_size(tokens::NOTE_TEXT)
            .text_color(tokens.text_faint)
            .child(icon)
            .when(!unfolded, |row| row.child(div().min_w_0().truncate().child(line)))
            .when(unfolded, |row| {
                row.on_click(cx.listener(|notes, _, window, cx| notes.fold(window, cx)))
            });
        div()
            .id("chapter-notes")
            .role(Role::Button)
            .aria_label(label)
            .track_focus(&self.focus)
            .on_action(cx.listener(|notes, _: &Cancel, window, cx| notes.fold(window, cx)))
            .w(self.editor.read(cx).text_width())
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(row)
            .when(unfolded, |notes| notes.children(self.fields.iter().cloned()))
    }
}

/// How many lines of a later comment show until it is opened.
const COMMENT_LINES: usize = 6;

impl EditorView {
    /// The later comments on screen, as where they stand in the text and the height of their passage.
    fn comments_on_screen(&self) -> Vec<(Range<usize>, Pixels)> {
        let text = self.text();
        let leading = leading_region(text, &self.blocks);
        let mut found = Vec::new();
        for (index, top) in &self.visible {
            let block = &self.blocks.as_slice()[*index];
            if leading
                .as_ref()
                .is_some_and(|leading| leading.contains(&block.range.start))
            {
                continue;
            }
            let slot = &self.slots[*index];
            if block.kind == BlockKind::Comment {
                if self.comment_is_hidden(*index) {
                    found.extend(
                        comments_in(text, &block.range)
                            .into_iter()
                            .map(|comment| (comment, *top)),
                    );
                }
                continue;
            }
            let (Some(syntax), Some(layout)) = (&slot.syntax, &slot.layout) else {
                continue;
            };
            for span in syntax.spans.iter().filter(|span| span.kind == SpanKind::Comment) {
                let line = (layout.lines.iter())
                    .rfind(|line| line.display.source.start <= span.range.start)
                    .filter(|line| line.height > px(0.));
                if let Some(line) = line {
                    let row = line.row_of(line.display.to_display(span.range.start), false);
                    let comment = block.range.start + span.range.start..block.range.start + span.range.end;
                    found.push((comment, *top + line.y + line.line_height * row as f32));
                }
            }
        }
        found
    }

    /// The later comments for this frame's margin, each with the height it wants to stand at.
    pub(super) fn comment_notes(&mut self, window: &Window, cx: &mut Context<Self>) -> Vec<(AnyElement, Pixels)> {
        if self.column.note_left.is_none() || self.source {
            return Vec::new();
        }
        let tokens = Tokens::get(cx).clone();
        let on_screen = self.comments_on_screen();
        let mut notes = Vec::with_capacity(on_screen.len());
        for (comment, y) in on_screen {
            let text = self.text();
            if !text[comment.clone()].starts_with("<!--") || !text[comment.clone()].ends_with("-->") {
                continue;
            }
            let shown = text[shown_range(text, &comment)].to_owned();
            if shown.is_empty() {
                continue;
            }
            let rows = rows_of(&shown, window, &tokens);
            let focus = (self.comment_focus)
                .entry(shown.clone())
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let open = self.open_comments.contains(&shown) || focus.is_focused(window);
            let element = self.comment_note(shown, rows, open, focus, &tokens, cx);
            notes.push((element.into_any_element(), y));
        }
        notes
    }

    fn comment_note(
        &self,
        shown: String,
        rows: usize,
        open: bool,
        focus: FocusHandle,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let clipped = rows > COMMENT_LINES && !open;
        let fade = gpui::linear_gradient(
            180.,
            gpui::linear_color_stop(gpui::Hsla::from(tokens.canvas).opacity(tokens::CLEAR), 0.),
            gpui::linear_color_stop(tokens.canvas, 1.),
        );
        let toggled = shown.clone();
        let more = (rows > COMMENT_LINES).then(|| {
            let label = crate::t!(if open { Key::NoteLess } else { Key::NoteMore });
            div()
                .id(SharedString::from(format!("comment-more-{shown}")))
                .role(Role::Button)
                .aria_label(label)
                .track_focus(&focus)
                .mt(tokens::SPACE_1)
                .cursor_pointer()
                .text_color(tokens.text_quiet)
                .hover(|link| link.text_color(tokens.text))
                .on_click(cx.listener(move |editor, _, window, cx| {
                    if !editor.open_comments.remove(&toggled) {
                        editor.open_comments.insert(toggled.clone());
                    } else {
                        // The field stays open while it has the keys, so closing hands them back.
                        window.focus(&editor.focus, cx);
                    }
                    cx.notify();
                }))
                .child(label)
        });
        div()
            .w(tokens::NOTE_WIDTH)
            .pl(tokens::SPACE_2)
            .border_l_1()
            .border_color(tokens.divider)
            .font_family(tokens::FONT_UI)
            .text_size(tokens::NOTE_TEXT)
            .line_height(LINE_HEIGHT)
            .text_color(tokens.text_faint)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .when(clipped, |text| {
                        text.max_h(LINE_HEIGHT * COMMENT_LINES as f32).overflow_hidden()
                    })
                    .child(shown)
                    .when(clipped, |text| {
                        text.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(LINE_HEIGHT * 2.)
                                .bg(fade),
                        )
                    }),
            )
            .children(more)
    }

    /// Whether anything but an agent's notes wants the margin: a comment after the chapter's opening notes.
    pub(super) fn has_later_comment(&self) -> bool {
        let text = self.text();
        let from = leading_region(text, &self.blocks).map_or(0, |leading| leading.end);
        text[from..].contains("<!--")
    }

    /// The room the unfolded opening notes need above the chapter's first text, past the line the folded ones stand in.
    /// Which opening note the selection is inside, and where in its shown text. The source view shows notes as text.
    pub(super) fn selection_in_note(&self) -> Option<(usize, Range<usize>)> {
        if self.source {
            return None;
        }
        let range = self.selection.range();
        leading_notes(self.text(), &self.blocks)
            .into_iter()
            .enumerate()
            .find(|(_, note)| note.shown.start <= range.start && range.end <= note.shown.end)
            .map(|(index, note)| (index, range.start - note.shown.start..range.end - note.shown.start))
    }

    /// Takes the height the opening notes were measured at while the text was drawn, and returns whether their room
    /// changed. A notify while drawing asks for no frame, so the caller asks for the next one.
    pub(super) fn set_note_room(&mut self, height: Pixels) -> bool {
        let room = (height - LINE_HEIGHT).max(px(0.));
        let changed = self.note_room != room;
        self.note_room = room;
        changed
    }
}

/// How many rows a comment takes in the margin.
fn rows_of(shown: &str, window: &Window, tokens: &Tokens) -> usize {
    let run = TextRun {
        len: shown.len(),
        font: font(tokens::FONT_UI),
        color: tokens.text_faint.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_text(
            SharedString::from(shown.to_owned()),
            tokens::NOTE_TEXT,
            &[run],
            Some(text_width()),
            None,
        )
        .map_or(0, |lines| lines.iter().map(|line| line.wrap_boundaries.len() + 1).sum())
}

/// The width a comment wraps at in the margin.
fn text_width() -> Pixels {
    tokens::NOTE_WIDTH - tokens::SPACE_2 - tokens::NOTE_RULE
}

/// One note, edited in place. Offsets are into the note's shown text.
pub struct NoteField {
    editor: Entity<EditorView>,
    index: usize,
    focus: FocusHandle,
    selection: Selection,
    marked: Option<Range<usize>>,
    refused: bool,
    drag_anchor: Option<usize>,
    lines: Rc<Vec<WrappedLine>>,
    /// Where the text is on screen, from the last frame.
    origin: Rc<Cell<Point<Pixels>>>,
}

impl NoteField {
    fn new(editor: Entity<EditorView>, index: usize, cx: &mut Context<Self>) -> Self {
        Self {
            editor,
            index,
            focus: cx.focus_handle().tab_stop(true),
            selection: Selection::default(),
            marked: None,
            refused: false,
            drag_anchor: None,
            lines: Rc::new(Vec::new()),
            origin: Rc::new(Cell::new(Point::default())),
        }
    }

    fn note(&self, cx: &App) -> Option<(Note, String)> {
        let editor = self.editor.read(cx);
        let note = leading_notes(editor.text(), &editor.blocks)
            .into_iter()
            .nth(self.index)?;
        let text = editor.text()[note.shown.clone()].to_owned();
        Some((note, text))
    }

    fn shown(&self, cx: &App) -> String {
        self.note(cx).map(|(_, text)| text).unwrap_or_default()
    }

    /// Keeps the caret inside the text after a change from outside, such as undo.
    fn text_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.shown(cx);
        let clamp = |offset: usize| text.floor_char_boundary(offset.min(text.len()));
        self.selection = Selection {
            anchor: clamp(self.selection.anchor),
            head: clamp(self.selection.head),
        };
        cx.notify();
    }

    /// Writes a change of `range` to the buffer. Text that would close the comment is refused and shown as such.
    fn replace(&mut self, range: Range<usize>, new: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let index = self.index;
        let written = self
            .editor
            .update(cx, |editor, cx| editor.edit_note(index, range.clone(), new, window, cx));
        self.refused = !written;
        if written {
            self.selection = Selection::caret(range.start + new.len());
        }
        cx.notify();
        written
    }

    fn insert(&mut self, new: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.replace(self.selection.range(), new, window, cx);
    }

    fn delete_to(&mut self, target: impl Fn(&str, usize) -> usize, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.shown(cx);
        let range = if self.selection.is_empty() {
            let other = target(&text, self.selection.head);
            other.min(self.selection.head)..other.max(self.selection.head)
        } else {
            self.selection.range()
        };
        if !range.is_empty() {
            self.replace(range, "", window, cx);
        }
    }

    fn go(&mut self, target: impl Fn(&str, usize) -> usize, extend: bool, cx: &mut Context<Self>) {
        let text = self.shown(cx);
        let head = target(&text, self.selection.head);
        self.selection = Selection {
            anchor: if extend { self.selection.anchor } else { head },
            head,
        };
        cx.notify();
    }

    /// The window position of an offset, from the last shaped lines.
    fn position_of(&self, offset: usize) -> Point<Pixels> {
        let mut start = 0;
        let mut y = px(0.);
        for line in self.lines.iter() {
            if offset <= start + line.len() {
                let local = line.position_for_index(offset - start, LINE_HEIGHT).unwrap_or_default();
                return self.origin.get() + point(local.x, y + local.y);
            }
            start += line.len() + 1;
            y += line.size(LINE_HEIGHT).height;
        }
        self.origin.get() + point(px(0.), y)
    }

    /// The offset at a window position.
    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let local = position - self.origin.get();
        let mut start = 0;
        let mut y = px(0.);
        for (index, line) in self.lines.iter().enumerate() {
            let height = line.size(LINE_HEIGHT).height;
            if local.y < y + height || index + 1 == self.lines.len() {
                let inside = point(local.x.max(px(0.)), (local.y - y).max(px(0.)).min(height - px(1.)));
                let found = line.closest_index_for_position(inside, LINE_HEIGHT);
                return start + found.unwrap_or_else(|closest| closest);
            }
            start += line.len() + 1;
            y += height;
        }
        0
    }

    fn vertical(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let here = self.position_of(self.selection.head);
        let y = if down {
            here.y + LINE_HEIGHT
        } else {
            here.y - LINE_HEIGHT
        };
        let head = self.offset_at(point(here.x, y));
        self.selection = Selection {
            anchor: if extend { self.selection.anchor } else { head },
            head,
        };
        cx.notify();
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        window.focus(&self.focus, cx);
        let offset = self.offset_at(event.position);
        self.selection = Selection::caret(offset);
        self.drag_anchor = Some(offset);
        cx.notify();
    }

    fn mouse_moved(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let (Some(anchor), Some(MouseButton::Left)) = (self.drag_anchor, event.pressed_button) {
            self.selection = Selection {
                anchor,
                head: self.offset_at(event.position),
            };
            cx.notify();
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            let text = self.shown(cx)[self.selection.range()].to_owned();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, action: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        let _ = action;
        if !self.selection.is_empty() {
            let text = self.shown(cx)[self.selection.range()].to_owned();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            self.insert("", window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert(&text.replace("\r\n", "\n"), window, cx);
        }
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| editor.undo_from_notes(window, cx));
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| editor.redo_from_notes(window, cx));
    }
}

impl Focusable for NoteField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NoteField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let text = self.shown(cx);
        let color = tokens.text_quiet;
        let width = self.editor.read(cx).text_width();
        let run = TextRun {
            len: text.len(),
            font: font(tokens::FONT_UI),
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shared: SharedString = text.clone().into();
        let shaped = window
            .text_system()
            .shape_text(shared, tokens::NOTE_TEXT, &[run], Some(width), None)
            .map(|lines| lines.into_vec())
            .unwrap_or_default();
        self.lines = Rc::new(shaped);
        let height: Pixels = self
            .lines
            .iter()
            .map(|line| line.size(LINE_HEIGHT).height)
            .fold(px(0.), |total, line| total + line)
            .max(LINE_HEIGHT);

        let focused = self.focus.is_focused(window);
        let lines = self.lines.clone();
        let origin = self.origin.clone();
        let (caret, selection) = if focused {
            let range = self.selection.range();
            (
                range.is_empty().then(|| self.position_of(range.start)),
                (!range.is_empty()).then(|| self.selection_rectangles(range)),
            )
        } else {
            // What the text has selected in this note, as a find leaves it, shows here.
            let found = (self.editor.read(cx).selection_in_note())
                .filter(|(index, range)| *index == self.index && !range.is_empty());
            (None, found.map(|(_, range)| self.selection_rectangles(range)))
        };
        let accent = gpui::Hsla::from(tokens.accent);
        let this = cx.entity();
        let focus = self.focus.clone();
        let surface = canvas(
            {
                let origin = origin.clone();
                move |bounds, _, _| origin.set(bounds.origin)
            },
            move |bounds: Bounds<Pixels>, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, this.clone()), cx);
                for rectangle in selection.into_iter().flatten() {
                    window.paint_quad(fill(rectangle, accent.opacity(tokens::SELECTION_TINT)));
                }
                let mut y = bounds.top();
                for line in lines.iter() {
                    line.paint(
                        point(bounds.left(), y),
                        LINE_HEIGHT,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
                    y += line.size(LINE_HEIGHT).height;
                }
                if let Some(caret) = caret {
                    window.paint_quad(fill(Bounds::new(caret, size(tokens::HAIRLINE, LINE_HEIGHT)), accent));
                }
            },
        )
        .w(width)
        .h(height);

        let field = div()
            .id(("note-field", self.index))
            .key_context(FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Backspace, window, cx| this.delete_to(motion::prev_grapheme, window, cx)))
            .on_action(cx.listener(|this, _: &Delete, window, cx| this.delete_to(motion::next_grapheme, window, cx)))
            .on_action(
                cx.listener(|this, _: &DeleteWordLeft, window, cx| this.delete_to(motion::prev_word, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &DeleteWordRight, window, cx| this.delete_to(motion::next_word, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NewBlock, window, cx| this.insert("\n", window, cx)))
            .on_action(cx.listener(|this, _: &NewLine, window, cx| this.insert("\n", window, cx)))
            .on_action(cx.listener(|this, _: &Left, _, cx| this.go(motion::prev_grapheme, false, cx)))
            .on_action(cx.listener(|this, _: &Right, _, cx| this.go(motion::next_grapheme, false, cx)))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.go(motion::prev_grapheme, true, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.go(motion::next_grapheme, true, cx)))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| this.go(motion::prev_word, false, cx)))
            .on_action(cx.listener(|this, _: &WordRight, _, cx| this.go(motion::next_word, false, cx)))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| this.go(motion::prev_word, true, cx)))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| this.go(motion::next_word, true, cx)))
            .on_action(cx.listener(|this, _: &LineStart, _, cx| {
                this.go(|text, at| motion::line_range(text, at).start, false, cx)
            }))
            .on_action(
                cx.listener(|this, _: &LineEnd, _, cx| this.go(|text, at| motion::line_range(text, at).end, false, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| {
                this.go(|text, at| motion::line_range(text, at).start, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| {
                this.go(|text, at| motion::line_range(text, at).end, true, cx)
            }))
            .on_action(cx.listener(|this, _: &DocStart, _, cx| this.go(|_, _| 0, false, cx)))
            .on_action(cx.listener(|this, _: &DocEnd, _, cx| this.go(|text, _| text.len(), false, cx)))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.vertical(false, false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.vertical(true, false, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.vertical(false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.vertical(true, true, cx)))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                let len = this.shown(cx).len();
                this.selection = Selection { anchor: 0, head: len };
                cx.notify();
            }))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_moved))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.drag_anchor = None;
                }),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    let text = this.editor.read(cx).focus_handle(cx);
                    window.focus(&text, cx);
                }
            }))
            .cursor_text()
            .child(surface);
        div()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .when(self.refused, |column| {
                column.child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.accent)
                        .child(crate::t!(Key::NotesRefused)),
                )
            })
            .child(field)
    }
}

impl NoteField {
    fn selection_rectangles(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut rectangles = Vec::new();
        let mut start = 0;
        let origin = self.origin.get();
        let mut y = origin.y;
        for line in self.lines.iter() {
            let line_end = start + line.len();
            if range.start <= line_end && range.end >= start {
                let from = range.start.max(start) - start;
                let to = range.end.min(line_end) - start;
                let mut row_start = 0;
                let boundaries: Vec<usize> = line
                    .wrap_boundaries
                    .iter()
                    .map(|boundary| line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index)
                    .chain(std::iter::once(line.len()))
                    .collect();
                for (row, row_end) in boundaries.into_iter().enumerate() {
                    let a = from.max(row_start);
                    let b = to.min(row_end);
                    if a < b || (a == b && row_end == line.len() && range.end > line_end) {
                        let layout = &line.unwrapped_layout;
                        let row_x = layout.x_for_index(row_start);
                        let x0 = layout.x_for_index(a) - row_x;
                        let x1 = layout.x_for_index(b) - row_x;
                        rectangles.push(Bounds::new(
                            point(origin.x + x0, y + LINE_HEIGHT * row as f32),
                            size((x1 - x0).max(tokens::NOTE_SELECTION_MIN), LINE_HEIGHT),
                        ));
                    }
                    row_start = row_end;
                }
            }
            start = line_end + 1;
            y += line.size(LINE_HEIGHT).height;
        }
        rectangles
    }
}

impl EntityInputHandler for NoteField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.shown(cx);
        let range = from_utf16(&text, &range_utf16);
        actual_range.replace(to_utf16(&text, &range));
        Some(text[range].to_owned())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, cx: &mut Context<Self>) -> Option<UTF16Selection> {
        let text = self.shown(cx);
        Some(UTF16Selection {
            range: to_utf16(&text, &self.selection.range()),
            reversed: self.selection.reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let text = self.shown(cx);
        self.marked.as_ref().map(|range| to_utf16(&text, range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.shown(cx);
        let range = range_utf16
            .map(|range| from_utf16(&text, &range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.range());
        self.marked = None;
        self.replace(range, new_text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.shown(cx);
        let range = range_utf16
            .map(|range| from_utf16(&text, &range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.range());
        let start = range.start;
        if self.replace(range, new_text, window, cx) {
            self.marked = (!new_text.is_empty()).then(|| start..start + new_text.len());
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let text = self.shown(cx);
        let range = from_utf16(&text, &range_utf16);
        Some(Bounds::new(
            self.position_of(range.start),
            size(tokens::HAIRLINE, LINE_HEIGHT),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let text = self.shown(cx);
        Some(text[..self.offset_at(position)].encode_utf16().count())
    }
}

fn to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    text[..range.start].encode_utf16().count()..text[..range.end].encode_utf16().count()
}

fn from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    let offset = |target: usize| {
        let mut units = 0;
        for (at, character) in text.char_indices() {
            if units >= target {
                return at;
            }
            units += character.len_utf16();
        }
        text.len()
    };
    offset(range.start)..offset(range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(y: f32, height: f32) -> Wanted {
        Wanted {
            y: px(y),
            height: px(height),
        }
    }

    #[test]
    fn notes_stand_beside_their_passages_while_there_is_room() {
        assert_eq!(
            flow(&[wanted(40., 60.), wanted(450., 120.)], px(28.)),
            [px(40.), px(450.)]
        );
    }

    #[test]
    fn nothing_in_the_flow_overlaps() {
        let notes = [wanted(30., 80.), wanted(50., 80.), wanted(60., 80.)];
        let placed = flow(&notes, px(28.));
        for (pair, note) in placed.windows(2).zip(&notes) {
            assert!(pair[0] + note.height <= pair[1], "{placed:?}");
        }
    }

    #[test]
    fn a_note_above_the_view_stays_in_it() {
        assert_eq!(flow(&[wanted(-300., 100.)], px(28.)), [px(28.)]);
    }
}
