//! The card of an index mark: its entry and sub-entry as two fields. Return writes the mark as one undo step, and
//! Escape leaves the text as it was.

use gpui::Focusable;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, MouseDownEvent, SharedString, Subscription, Window, div};

use super::{CONTEXT, NextField, PreviousField, RichFloat, card, field_label, small_button};
use crate::editor::EditorView;
use crate::editor::float::Float;
use crate::editor::rich::Key;
use crate::editor::rich::index::{self, Found, Terms};
use crate::inspector::input::{Cancelled, Submitted, TextInput};
use crate::source::Selection;
use crate::tokens::{self, Tokens};

pub struct IndexFloat {
    /// Where a new mark goes, or where the mark being changed starts.
    at: usize,
    /// The mark being changed: where its block starts, and the mark in it.
    existing: Option<(usize, Found)>,
    entry: Entity<TextInput>,
    sub: Entity<TextInput>,
    refused: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl IndexFloat {
    pub fn anchor(&self) -> usize {
        self.at
    }
}

impl EditorView {
    /// Opens the card for the mark at the caret, or for a new mark on the selection or the caret's word.
    pub(in crate::editor) fn open_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source {
            return;
        }
        let text = self.session.active_buffer().text();
        let head = self.selection.head;
        let existing = (self.selection.is_empty())
            .then(|| index::mark_at(text, &self.blocks, head))
            .flatten();
        let (at, (entry, sub)) = match &existing {
            Some((start, found)) => {
                let bracketed = found
                    .brackets
                    .as_ref()
                    .map(|brackets| &text[start + brackets.start + 1..start + brackets.end - 1]);
                let written = &text[start + found.braces.start..start + found.braces.end];
                let terms = Terms::read(written, bracketed);
                (start + found.braces.start, (terms.entry, terms.sub))
            }
            None => {
                let at = index::place_for(text, self.selection);
                if !index::allowed(&self.blocks, at) {
                    self.notify_rich(head, crate::t!(Key::IndexNotHere).to_owned(), false, cx);
                    return;
                }
                (at, (index::entry_for(text, self.selection), String::new()))
            }
        };
        let make = |value: String, label: Key, placeholder: Key, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(window, cx)
                    .keep_focus_on_enter()
                    .with_label(label)
                    .with_placeholder(crate::t!(placeholder));
                input.set_value(value, true, cx);
                input
            })
        };
        let entry = make(entry, Key::IndexEntry, Key::IndexEntryPlaceholder, window, cx);
        let sub = make(sub, Key::IndexSub, Key::IndexSubPlaceholder, window, cx);
        let subscriptions = [&entry, &sub]
            .into_iter()
            .flat_map(|field| {
                [
                    cx.subscribe_in(field, window, |this, _, _: &Submitted, window, cx| {
                        this.submit_index(false, window, cx);
                    }),
                    cx.subscribe_in(field, window, |this, _, _: &Cancelled, window, cx| {
                        this.close_index(window, cx);
                    }),
                ]
            })
            .collect();
        entry.update(cx, |entry, cx| entry.select_all(cx));
        entry.focus_handle(cx).focus(window, cx);
        self.float = Float::Rich(RichFloat::Index(IndexFloat {
            at,
            existing,
            entry,
            sub,
            refused: None,
            _subscriptions: subscriptions,
        }));
        cx.notify();
    }

    /// Closes the card with nothing written, and gives the keys back to the text.
    fn close_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.float, Float::Rich(RichFloat::Index(_))) {
            self.close_float(cx);
            window.focus(&self.focus, cx);
        }
    }

    /// Writes the card's mark, or with `remove` or an entry left empty takes the mark out.
    fn submit_index(&mut self, remove: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Rich(RichFloat::Index(mut float)) = std::mem::take(&mut self.float) else {
            return;
        };
        let entry = float.entry.read(cx).text().to_owned();
        let sub = float.sub.read(cx).text().to_owned();
        let text = self.session.active_buffer().text();
        let remove = remove || (float.existing.is_some() && entry.trim().is_empty());
        let (edits, caret) = match (&float.existing, remove) {
            (Some((start, found)), true) => {
                let edits = index::remove(*start, found);
                let lost: usize = edits
                    .iter()
                    .filter(|edit| edit.range.start < float.at)
                    .map(|edit| edit.range.len())
                    .sum();
                (edits, float.at - lost)
            }
            _ => {
                if let Some(why) = index::problem(&entry, &sub) {
                    float.refused = Some(crate::t!(why).into());
                    self.float = Float::Rich(RichFloat::Index(float));
                    cx.notify();
                    return;
                }
                let edit = match &float.existing {
                    Some((start, found)) => index::change(text, *start, found, &entry, &sub),
                    None => index::insert(float.at, &entry, &sub),
                };
                let caret = edit.range.start + edit.text.len();
                (vec![edit], caret)
            }
        };
        let active = self.session.active();
        match self.write_rich(active, edits, Some(Selection::caret(caret)), window, cx) {
            Ok(()) => {
                self.toolbar_dismissed = true;
                window.focus(&self.focus, cx);
            }
            Err(why) => {
                float.refused = Some(why.into());
                self.float = Float::Rich(RichFloat::Index(float));
            }
        }
        cx.notify();
    }

    fn index_switch_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Rich(RichFloat::Index(float)) = &self.float else {
            return;
        };
        let (entry, sub) = (float.entry.clone(), float.sub.clone());
        let target = if entry.focus_handle(cx).is_focused(window) {
            sub
        } else {
            entry
        };
        target.focus_handle(cx).focus(window, cx);
    }

    pub(super) fn index_element(&self, float: &IndexFloat, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx).clone();
        let field = |label: &'static str, input: &Entity<TextInput>| {
            div()
                .flex()
                .flex_col()
                .gap(tokens::SPACE_1)
                .child(field_label(&tokens, label))
                .child(input.clone())
        };
        let changing = float.existing.is_some();
        let accept = if changing { Key::CloseLabel } else { Key::IndexMark };
        card(&tokens)
            .id("index-float")
            .key_context(CONTEXT)
            .w(tokens::INDEX_CARD_WIDTH)
            .p(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_index(window, cx)))
            .on_action(cx.listener(|this, _: &NextField, window, cx| this.index_switch_field(window, cx)))
            .on_action(cx.listener(|this, _: &PreviousField, window, cx| this.index_switch_field(window, cx)))
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(crate::t!(Key::IndexTitle)),
            )
            .child(field(crate::t!(Key::IndexEntry), &float.entry))
            .child(field(crate::t!(Key::IndexSub), &float.sub))
            .children(
                float
                    .refused
                    .clone()
                    .map(|why| div().text_size(tokens::TEXT_SMALL).text_color(tokens.accent).child(why)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_1)
                    .when(changing, |row| {
                        row.child(
                            small_button(&tokens, "index-remove", crate::t!(Key::IndexRemove))
                                .on_click(cx.listener(|this, _, window, cx| this.submit_index(true, window, cx))),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        small_button(&tokens, "index-accept", crate::t!(accept))
                            .text_color(tokens.accent)
                            .on_click(cx.listener(|this, _, window, cx| this.submit_index(false, window, cx))),
                    ),
            )
            .into_any_element()
    }
}
