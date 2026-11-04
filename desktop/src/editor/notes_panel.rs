//! The chapter's notes in the inspector: each comment that opens the chapter, with a field that edits its text where it
//! stands in the buffer, so undo and the bytes around it are the editor's.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, FontWeight,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollHandle, SharedString, Subscription, TextRun,
    UTF16Selection, Window, WrappedLine, canvas, div, fill, font, point, px, size,
};

use super::commands::notes::{Note, leading_notes};
use super::strings::Key;
use super::{
    Backspace, Copy, Cut, Delete, DeleteWordLeft, DeleteWordRight, DocEnd, DocStart, Down, EditorView, Left, LineEnd,
    LineStart, NewBlock, NewLine, Paste, Redo, Right, SelectAll, SelectDown, SelectLeft, SelectLineEnd,
    SelectLineStart, SelectRight, SelectUp, SelectWordLeft, SelectWordRight, SessionChanged, TextChanged, Undo, Up,
    WordLeft, WordRight,
};
use crate::source::{Selection, motion};
use crate::tokens::{self, Tokens};

const FIELD_CONTEXT: &str = "NoteField";
const LINE_HEIGHT: Pixels = px(19.);

pub struct NotesPanel {
    editor: Entity<EditorView>,
    /// What the panel shows, so a key elsewhere in the chapter draws nothing here.
    shown: Vec<String>,
    fields: Vec<Entity<NoteField>>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl NotesPanel {
    pub fn new(editor: Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&editor, |panel, _, _: &TextChanged, cx| panel.sync(cx)),
            cx.subscribe(&editor, |panel, _, _: &SessionChanged, cx| panel.sync(cx)),
        ];
        let mut panel = Self {
            editor,
            shown: Vec::new(),
            fields: Vec::new(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        panel.sync(cx);
        panel
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let shown = self.editor.read(cx).note_texts();
        if shown == self.shown {
            return;
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
}

impl Render for NotesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let header = div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .border_b_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tokens.text_quiet)
            .child(crate::t!(Key::NotesTitle));
        let body = div()
            .id("notes")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_3)
            .when(self.fields.is_empty(), |body| {
                body.child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_faint)
                        .child(crate::t!(Key::NotesEmpty)),
                )
            })
            .children(self.fields.iter().cloned());
        div().size_full().flex().flex_col().child(header).child(body)
    }
}

/// The width text wraps at in the panel.
fn text_width() -> Pixels {
    tokens::INSPECTOR_WIDTH - tokens::SPACE_3 * 2. - tokens::SPACE_2 * 2. - px(2.)
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
            focus: cx.focus_handle(),
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
        let color = tokens.text;
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
            .shape_text(shared, tokens::TEXT_BODY, &[run], Some(text_width()), None)
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
            (None, None)
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
                    window.paint_quad(fill(rectangle, accent.opacity(0.25)));
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
                    window.paint_quad(fill(Bounds::new(caret, size(px(1.), LINE_HEIGHT)), accent));
                }
            },
        )
        .w(text_width())
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
            .cursor_text()
            .p(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(if focused { tokens.accent } else { tokens.divider })
            .bg(tokens.canvas)
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
                            size((x1 - x0).max(px(4.)), LINE_HEIGHT),
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
        Some(Bounds::new(self.position_of(range.start), size(px(1.), LINE_HEIGHT)))
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
