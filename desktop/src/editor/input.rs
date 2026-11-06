//! What typing, deleting, the clipboard, and undo do to the text, and the system's input methods. Each change is one
//! edit to the buffer, after which the blocks around it are read again.

use super::*;

impl EditorView {
    pub(super) fn edit(
        &mut self,
        kind: EditKind,
        edits: Vec<Edit>,
        after: Selection,
        marked: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = self.trace.input();
        match self
            .session
            .active_buffer_mut()
            .transact(kind, edits, self.selection, after, started)
        {
            Ok(splice) => self.resegment(splice),
            Err(error) => {
                eprintln!("edit rejected: {error}");
                return;
            }
        }
        // A message over the galley does not outlast the next change.
        if matches!(&self.float, Float::Rich(float) if !float.takes_keys()) {
            self.float = Float::None;
        }
        self.marked = marked;
        self.gutter_shown = false;
        self.caret_by_keys = true;
        self.set_selection(after, false, window, cx);
        window.notify_text_input_changed(&self.focus);
        self.trace.handled(started);
        self.schedule_rich(window, cx);
        self.restart_spelling(window, cx);
        cx.emit(TextChanged);
    }

    pub(super) fn resegment(&mut self, splice: crate::source::buffer::Splice) {
        let delta = splice.new_end as isize - splice.old_end as isize;
        let anchor_start = self.blocks.as_slice()[self.scroll.block].range.start;
        let change = self.blocks.apply(self.session.active_buffer().text(), splice);
        let new_slots: Vec<Slot> = (0..change.inserted)
            .map(|at| Slot {
                // Blocks around an edit are scanned again. If the count stayed, an open comment stays open.
                expanded: change.removed == change.inserted && self.slots[change.start + at].expanded,
                ..Slot::default()
            })
            .collect();
        self.slots
            .splice(change.start..change.start + change.removed, new_slots);
        follow_blocks(&mut self.visible, change);
        let moved = if anchor_start > splice.start {
            anchor_start.saturating_add_signed(delta).max(splice.start)
        } else {
            anchor_start
        };
        self.scroll.block = self.blocks.index_at(moved);
        self.hovered = None;
        self.rich.atoms = rich::atoms(self.session.active_buffer().text(), &self.blocks);
        self.read_directives();
        review::shift(&mut self.review, &mut self.advice, &splice);
    }

    pub(super) fn replace_selection(
        &mut self,
        text: &str,
        kind: EditKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.selection.range();
        let after = Selection::caret(range.start + text.len());
        self.edit(kind, vec![Edit::new(range, text)], after, None, window, cx);
    }

    pub(super) fn delete_towards(
        &mut self,
        target: impl Fn(&str, usize) -> usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selection.is_empty() {
            let head = self.selection.head;
            let other = target(self.session.active_buffer().text(), head);
            // A key deletes a chip whole.
            let other = self.past_chip(other, other > head);
            // A comment hidden there is not deleted by a key that cannot show it: the caret goes past it. A fence line
            // at rest is not either: the caret goes onto it, which shows it.
            let visible = self.skip_hidden(other, other > head);
            let fence = self.blocks.index_at(other);
            if visible == other && fence != self.blocks.index_at(head) && self.fence_at_rest(fence) {
                self.set_selection(Selection::caret(other), false, window, cx);
                return;
            }
            if visible != other {
                if visible != head {
                    self.set_selection(Selection::caret(visible), false, window, cx);
                }
                return;
            }
            head.min(other)..head.max(other)
        } else {
            self.selection.range()
        };
        if !range.is_empty() {
            let after = Selection::caret(range.start);
            self.edit(EditKind::Deleting, vec![Edit::new(range, "")], after, None, window, cx);
        }
    }

    pub(super) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::left, window, cx);
    }

    pub(super) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::right, window, cx);
    }

    pub(super) fn delete_word_left(&mut self, _: &DeleteWordLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::prev_word, window, cx);
    }

    pub(super) fn delete_word_right(&mut self, _: &DeleteWordRight, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_towards(motion::next_word, window, cx);
    }

    pub(super) fn delete_to_line_start(&mut self, _: &DeleteToLineStart, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.row_edge(false, window).0;
        self.delete_towards(move |_, _| target, window, cx);
    }

    pub(super) fn new_block(&mut self, _: &NewBlock, window: &mut Window, cx: &mut Context<Self>) {
        if self.source {
            self.replace_selection("\n", EditKind::Discrete, window, cx);
            return;
        }
        let edit = if self.selection.is_empty() {
            typing::line_break(self.session.active_buffer().text(), &self.blocks, self.selection.head)
        } else {
            Edit::new(self.selection.range(), "\n\n")
        };
        let after = Selection::caret(edit.range.start + edit.text.len());
        self.edit(EditKind::Discrete, vec![edit], after, None, window, cx);
    }

    pub(super) fn new_line(&mut self, _: &NewLine, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_selection("\n", EditKind::Discrete, window, cx);
    }

    pub(super) fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_none() {
            let buffer = self.session.active_buffer_mut();
            let outside = buffer.undo_is_outside_change();
            let restored = buffer.undo();
            if outside {
                cx.emit(ExternalUndo {
                    done: restored.is_some(),
                });
            }
            self.restore(restored, window, cx);
        }
    }

    pub(super) fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_none() {
            let restored = self.session.active_buffer_mut().redo();
            self.restore(restored, window, cx);
        }
    }

    pub(super) fn restore(
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
        self.schedule_rich(window, cx);
        cx.emit(TextChanged);
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            let text = self.session.active_buffer().text()[self.selection.range()].to_owned();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(super) fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            let text = self.session.active_buffer().text()[self.selection.range()].to_owned();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.replace_selection("", EditKind::Discrete, window, cx);
        }
    }

    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_selection(&text.replace("\r\n", "\n"), EditKind::Discrete, window, cx);
        }
    }

    pub(super) fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SaveRequested);
    }

    pub(super) fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
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
        let range = self.session.active_buffer().range_from_utf16(&range_utf16);
        actual_range.replace(self.session.active_buffer().range_to_utf16(&range));
        Some(self.session.active_buffer().text()[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.session.active_buffer().range_to_utf16(&self.selection.range()),
            reversed: self.selection.reversed(),
        })
    }

    fn set_selected_text_range(&mut self, range_utf16: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.session.active_buffer().range_from_utf16(&range_utf16);
        let selection = Selection {
            anchor: range.start,
            head: range.end,
        };
        self.set_selection(selection, false, window, cx);
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.session.active_buffer().utf16_len())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|range| self.session.active_buffer().range_to_utf16(range))
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
        self.options_release_keys();
        let range = self.input_range(range_utf16);
        let typed_slash = new_text == "/" && kind == EditKind::Typing && !self.menu_is_open();
        let at = range.start;
        let after = Selection::caret(range.start + new_text.len());
        self.edit(kind, vec![Edit::new(range, new_text)], after, None, window, cx);
        if typed_slash {
            self.open_slash(at, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.options_release_keys();
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
        let range = self.session.active_buffer().range_from_utf16(&range_utf16);
        let start = self.offset_bounds(range.start, false, window)?;
        let end = self.offset_bounds(range.end, false, window).unwrap_or(start);
        let width = if end.origin.y == start.origin.y {
            (end.origin.x - start.origin.x).max(tokens::HAIRLINE)
        } else {
            tokens::HAIRLINE
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
        Some(self.session.active_buffer().utf16_of(offset))
    }
}

impl EditorView {
    /// The range an input method means: the one it names, else the marked text, else the selection.
    fn input_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range| self.session.active_buffer().range_from_utf16(&range))
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
