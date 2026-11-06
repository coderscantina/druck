//! A footnote's text in a popover at its reference. The field holds a plain copy, with the indent off, and closing it writes
//! the change as one undo step, in whichever chapter keeps the definition.

use std::ops::Range;

use gpui::Focusable;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, MouseDownEvent, Window, div};

use super::{CONTEXT, RichFloat, card, small_button};
use crate::editor::commands::marks::{self, Mark};
use crate::editor::commands::{normalized, selection_after};
use crate::editor::float::Float;
use crate::editor::rich::{Key, facts, footnotes};
use crate::editor::{EditorView, ToggleBold, ToggleItalic, ToggleStrike, ToggleUnderline};
use crate::inspector::input::{self, TextInput};
use crate::session::ChapterId;
use crate::source::{Blocks, Selection};
use crate::tokens::{self, Tokens};

pub struct NoteFloat {
    label: String,
    /// The reference in the active chapter.
    reference: Range<usize>,
    /// The chapter keeping the definition, and its title if that is not the active chapter.
    chapter: ChapterId,
    elsewhere: Option<String>,
    input: Entity<TextInput>,
    /// The field's text when it opened.
    original: String,
    /// The note was made just now and still holds its placeholder. Closing it unchanged takes it back.
    fresh: bool,
    /// Why the last write was refused.
    refused: Option<String>,
}

pub const WIDTH: gpui::Pixels = tokens::NOTE_CARD_WIDTH;

impl NoteFloat {
    /// About how tall the card comes out, to place it.
    pub fn height(&self, cx: &gpui::App) -> gpui::Pixels {
        let lines = self.input.read(cx).text().lines().count().max(1) as f32;
        tokens::NOTE_CARD_BASE + tokens::NOTE_CARD_LINE * lines
    }

    pub fn anchor(&self) -> usize {
        self.reference.start
    }

    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }
}

impl EditorView {
    /// Makes a new footnote at the caret, in place of the typed slash if there is one, and opens its popover.
    pub(in crate::editor) fn insert_footnote(
        &mut self,
        slash: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rich.refresh(&self.session);
        let replaced = slash.unwrap_or_else(|| self.selection.range());
        let text = self.session.active_buffer().text();
        let chapter = self.session.active_chapter();
        let stem = chapter
            .path()
            .file_stem()
            .map_or(String::new(), |stem| stem.to_string_lossy().into_owned());
        let facts = facts::scan(text);
        let label = footnotes::next_label(&stem, &facts, &self.rich.book.note_labels);
        let placeholder = crate::t!(Key::FootnotePlaceholder);
        let note = footnotes::insert(
            text,
            &self.blocks,
            &facts,
            replaced.clone(),
            &label,
            placeholder,
            self.rich.book.keeping,
        );
        let active = self.session.active();
        let selection = note.command.selection;
        match self.write_rich(active, note.command.edits, Some(selection), window, cx) {
            Ok(()) => {
                self.autoscroll = true;
                self.rich.refresh(&self.session);
                self.open_note_with(&label, note.reference, true, window, cx);
            }
            Err(why) => self.notify_rich(replaced.start, why, false, cx),
        }
    }

    /// Opens the popover of the note a chip at `reference` names.
    pub(in crate::editor) fn open_note(
        &mut self,
        label: &str,
        reference: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_note_with(label, reference, false, window, cx);
    }

    fn open_note_with(
        &mut self,
        label: &str,
        reference: Range<usize>,
        fresh: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found = self.rich.book.definitions.get(&footnotes::folded(label)).cloned();
        let chapter = found.as_ref().map_or(self.session.active(), |(chapter, _)| *chapter);
        let body = found.map_or(String::new(), |(chapter, _)| {
            let text = self.session.buffer(chapter).text();
            facts::scan(text)
                .definitions
                .iter()
                .find(|definition| footnotes::folded(&definition.label) == footnotes::folded(label))
                .map_or(String::new(), |definition| {
                    footnotes::read_body(text, &definition.range)
                })
        });
        let elsewhere = (chapter != self.session.active()).then(|| self.session.title_of(chapter));
        let original = body.clone();
        let input = cx.new(|cx| {
            let mut input = TextInput::multiline(window, cx).in_prose();
            input.set_value(body, true, cx);
            if fresh {
                input.select_all(cx);
            }
            input
        });
        input.focus_handle(cx).focus(window, cx);
        self.float = Float::Rich(RichFloat::Note(NoteFloat {
            label: label.to_owned(),
            reference,
            chapter,
            elsewhere,
            input,
            original,
            fresh,
            refused: None,
        }));
        cx.notify();
    }

    /// Writes what changed in a note's field. If the book would not stay valid, the popover stays with the reason.
    pub(super) fn commit_note(&mut self, mut note: NoteFloat, window: &mut Window, cx: &mut Context<Self>) {
        let new = note.input.read(cx).text().trim_end().to_owned();
        if new == note.original {
            if note.fresh {
                self.undo_fresh_note(window, cx);
            }
            return;
        }
        let text = self.session.buffer(note.chapter).text();
        let facts = facts::scan(text);
        let folded = footnotes::folded(&note.label);
        let definition = facts
            .definitions
            .iter()
            .find(|definition| footnotes::folded(&definition.label) == folded);
        let edit = match definition {
            Some(definition) => footnotes::write_body(text, &definition.range, &new),
            None => {
                let blocks = Blocks::parse(text);
                Some(footnotes::add_definition(
                    text,
                    &blocks,
                    &facts,
                    note.reference.start,
                    &note.label,
                    &new,
                    self.rich.book.keeping,
                ))
            }
        };
        let Some(edit) = edit else {
            return;
        };
        if let Err(why) = self.write_rich(note.chapter, vec![edit], None, window, cx) {
            note.refused = Some(why);
            self.float = Float::Rich(RichFloat::Note(note));
        }
    }

    /// Takes back a note just made, as one undo.
    fn undo_fresh_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restored = self.session.active_buffer_mut().undo();
        self.restore(restored, window, cx);
    }

    /// Removes the reference and its definition, each as written.
    fn remove_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Rich(RichFloat::Note(note)) = std::mem::take(&mut self.float) else {
            return;
        };
        window.focus(&self.focus, cx);
        let active = self.session.active();
        let reference = crate::source::Edit::new(note.reference.clone(), "");
        let text = self.session.buffer(note.chapter).text();
        let blocks = Blocks::parse(text);
        let facts = facts::scan(text);
        let folded = footnotes::folded(&note.label);
        let definition = facts
            .definitions
            .iter()
            .find(|definition| footnotes::folded(&definition.label) == folded)
            .map(|definition| footnotes::remove_definition(text, &blocks, definition));
        let result = if note.chapter == active {
            let mut edits = vec![reference];
            edits.extend(definition);
            let edits = normalized(edits, self.session.active_buffer().text());
            // A definition kept above the reference moves it back.
            let after = selection_after(Selection::caret(note.reference.start), &edits);
            self.write_rich(active, edits, Some(after), window, cx)
        } else {
            self.write_rich(
                active,
                vec![reference],
                Some(Selection::caret(note.reference.start)),
                window,
                cx,
            )
            .and_then(|()| match definition {
                Some(edit) => self.write_rich(note.chapter, vec![edit], None, window, cx),
                None => Ok(()),
            })
        };
        if let Err(why) = result {
            self.notify_rich(note.reference.start, why, false, cx);
        }
        cx.notify();
    }

    /// Applies a mark to the selection in a float's field, with the rules the galley's toolbar uses.
    pub(super) fn mark_in_field(&mut self, field: &Entity<TextInput>, mark: Mark, cx: &mut Context<Self>) {
        let (text, selection) = {
            let field = field.read(cx);
            (field.text().to_owned(), field.selection())
        };
        if let Some(command) = marks::toggle(&text, &Blocks::parse(&text), selection, mark) {
            field.update(cx, |field, cx| field.apply(command.edits, command.selection, cx));
        }
    }

    pub(super) fn note_element(&self, note: &NoteFloat, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx).clone();
        let input = note.input.clone();
        let title = crate::t!(Key::FootnoteTitle, label = note.label);
        card(&tokens)
            .id("note-float")
            .key_context(CONTEXT)
            .w(WIDTH)
            .p(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.commit_rich_float(window, cx)))
            .capture_action(cx.listener(|this, _: &input::Escape, window, cx| {
                this.commit_rich_float(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ToggleBold, _, cx| this.mark_note(Mark::Bold, cx)))
            .on_action(cx.listener(|this, _: &ToggleItalic, _, cx| this.mark_note(Mark::Italic, cx)))
            .on_action(cx.listener(|this, _: &ToggleUnderline, _, cx| this.mark_note(Mark::Underline, cx)))
            .on_action(cx.listener(|this, _: &ToggleStrike, _, cx| this.mark_note(Mark::Strike, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(tokens::SPACE_2)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(tokens::TEXT_SMALL)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(tokens.text_quiet)
                            .child(title),
                    )
                    .children(note.elsewhere.as_ref().map(|chapter| {
                        div()
                            .flex_none()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(crate::t!(Key::FootnoteElsewhere, chapter = chapter))
                    })),
            )
            .child(input)
            .children(note.refused.as_ref().map(|why| {
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.accent)
                    .child(why.clone())
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(tokens::SPACE_2)
                    .child(
                        div()
                            .min_w_0()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(crate::t!(Key::FootnoteHint)),
                    )
                    .child(
                        small_button(&tokens, "note-remove", crate::t!(Key::FootnoteRemove))
                            .on_click(cx.listener(|this, _, window, cx| this.remove_note(window, cx))),
                    ),
            )
            .into_any_element()
    }

    fn mark_note(&mut self, mark: Mark, cx: &mut Context<Self>) {
        if let Float::Rich(RichFloat::Note(note)) = &self.float {
            let field = note.input().clone();
            self.mark_in_field(&field, mark, cx);
        }
    }
}
