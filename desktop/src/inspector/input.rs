//! A minimal single-line text field for the inspector: typing, caret movement, paste, and commit on Enter or
//! blur. It takes no IME composition; the editor owns the real text element.

use gpui::prelude::*;
use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, MouseButton, Render, SharedString, Subscription,
    Window, div, px,
};

use crate::tokens::{self, Tokens};

/// The field's text, after the author finished editing it.
pub struct Committed(pub String);

/// The author pressed Enter in the field.
pub struct Submitted;

pub struct TextInput {
    text: String,
    /// What the field held when editing began, so an unchanged field commits nothing.
    original: String,
    /// Byte offset of the caret.
    caret: usize,
    /// Whether the text is the author's own or an inherited value shown for reference.
    customized: bool,
    /// The next typed character replaces everything, as after a focus or select all.
    replace_all: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Committed> for TextInput {}
impl EventEmitter<Submitted> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus_in(&focus, window, |input, _, cx| {
                input.replace_all = true;
                cx.notify();
            }),
            cx.on_focus_out(&focus, window, |input, _, _, cx| {
                input.replace_all = false;
                input.commit(cx);
            }),
        ];
        Self {
            text: String::new(),
            original: String::new(),
            caret: 0,
            customized: false,
            replace_all: false,
            focus,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_value(&mut self, text: String, customized: bool, cx: &mut Context<Self>) {
        self.caret = text.len();
        self.original.clone_from(&text);
        self.text = text;
        self.customized = customized;
        cx.notify();
    }

    /// The text as it stands, committed or not.
    pub fn text(&self) -> &str {
        &self.text
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        if self.text != self.original {
            self.original.clone_from(&self.text);
            cx.emit(Committed(self.text.clone()));
        }
        cx.notify();
    }

    fn insert(&mut self, text: &str) {
        if std::mem::take(&mut self.replace_all) {
            self.text.clear();
            self.caret = 0;
        }
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
    }

    fn previous(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    fn next(&self) -> usize {
        self.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let command = keystroke.modifiers.platform || keystroke.modifiers.control;
        if command {
            match keystroke.key.as_str() {
                "a" => self.replace_all = true,
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.insert(&text.replace('\n', " "));
                    }
                }
                _ => return,
            }
            cx.notify();
            return;
        }
        match keystroke.key.as_str() {
            "enter" => {
                self.replace_all = false;
                self.commit(cx);
                cx.emit(Submitted);
                window.blur(cx);
                return;
            }
            "escape" => {
                self.text.clone_from(&self.original);
                self.caret = self.text.len();
                self.replace_all = false;
                window.blur(cx);
            }
            "backspace" => {
                if std::mem::take(&mut self.replace_all) {
                    self.text.clear();
                    self.caret = 0;
                } else {
                    let start = self.previous();
                    self.text.replace_range(start..self.caret, "");
                    self.caret = start;
                }
            }
            "delete" => {
                let end = self.next();
                self.text.replace_range(self.caret..end, "");
            }
            "left" => {
                self.replace_all = false;
                self.caret = self.previous();
            }
            "right" => {
                self.replace_all = false;
                self.caret = self.next();
            }
            "home" => self.caret = 0,
            "end" => self.caret = self.text.len(),
            _ => match &keystroke.key_char {
                Some(text) if !text.chars().any(char::is_control) => self.insert(text),
                _ => return,
            },
        }
        cx.notify();
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let focused = self.focus.is_focused(window);
        let color = if self.customized || focused {
            tokens.text
        } else {
            tokens.text_faint
        };
        let (before, after) = self.text.split_at(self.caret);
        let text = |text: &str| div().flex_none().child(SharedString::from(text.to_owned()));
        let content = if focused && self.replace_all && !self.text.is_empty() {
            div().flex().bg(tokens.raised).child(text(&self.text))
        } else {
            div()
                .flex()
                .child(text(before))
                .when(focused, |row| {
                    row.child(div().flex_none().w(px(1.)).h(tokens::TEXT_BODY).bg(tokens.accent))
                })
                .child(text(after))
        };
        div()
            .id("text-input")
            .key_context("TextInput")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|input, _, window, cx| {
                    window.focus(&input.focus, cx);
                    input.replace_all = false;
                    input.caret = input.text.len();
                    cx.notify();
                }),
            )
            .h(tokens::FIELD_HEIGHT)
            .px(tokens::SPACE_2)
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(if focused { tokens.accent } else { tokens.divider })
            .bg(tokens.canvas)
            .text_color(color)
            .child(content)
    }
}
