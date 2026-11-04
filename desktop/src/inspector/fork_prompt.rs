//! The question a first edit to a common theme asks from inside a project: copy it into the project, which is the
//! safe choice, or change the shared file that other documents may use too.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, KeyDownEvent, MouseButton, Subscription, Window, div};

use super::model::ThemeModel;
use super::strings::Key;
use super::widgets;
use crate::editor::EditorView;
use crate::tokens::{self, Tokens};

pub struct ForkPrompt {
    model: Entity<ThemeModel>,
    editor: Entity<EditorView>,
    error: Option<String>,
    _subscription: Subscription,
}

impl ForkPrompt {
    pub fn new(model: Entity<ThemeModel>, editor: Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            editor,
            error: None,
            _subscription: subscription,
        }
    }

    fn decide(&mut self, fork: bool, cx: &mut Context<Self>) {
        let result = self.model.update(
            cx,
            |model, cx| {
                if fork { model.fork(cx) } else { model.edit_shared(cx) }
            },
        );
        self.error = result.err();
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.cancel(cx));
        self.error = None;
    }
}

impl Render for ForkPrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        if model.editor().pending().is_none() {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let name = model.editor().name().unwrap_or_default();
        let document = self
            .editor
            .read(cx)
            .session()
            .location()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let option = |id: &'static str, title: Key, note: Key, recommended: bool, fork: bool| {
            div()
                .id(id)
                .px(tokens::SPACE_3)
                .py(tokens::SPACE_2)
                .flex()
                .flex_col()
                .gap(tokens::SPACE_1)
                .cursor_pointer()
                .rounded(tokens::RADIUS)
                .border_1()
                .border_color(if recommended { tokens.accent } else { tokens.divider })
                .hover(|option| option.bg(tokens.raised))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(widgets::note(&tokens, note))
                .on_click(cx.listener(move |prompt, _, _, cx| prompt.decide(fork, cx)))
        };
        let card = div()
            .id("fork-prompt")
            .occlude()
            .absolute()
            .top(tokens::SPACE_4 * 3.)
            .left(tokens::SIDEBAR_WIDTH + tokens::SPACE_4)
            .w(tokens::DIALOG_WIDTH)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_3)
            .p(tokens::SPACE_3)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().font_weight(FontWeight::SEMIBOLD).child(crate::t!(Key::ForkTitle)))
            .child(
                div()
                    .text_color(tokens.text_quiet)
                    .child(crate::t!(Key::ForkBody, value = &name)),
            )
            .child(widgets::note(
                &tokens,
                format!(
                    "{} {}",
                    crate::t!(Key::ForkAffected, value = &document),
                    crate::t!(Key::ForkUnknown)
                ),
            ))
            .child(option("fork-copy", Key::ForkCopy, Key::ForkCopyNote, true, true))
            .child(option(
                "fork-shared",
                Key::ForkShared,
                Key::ForkSharedNote,
                false,
                false,
            ))
            .when_some(self.error.clone(), |card, message| {
                card.child(widgets::error_line(&tokens, message))
            })
            .child(
                div().flex().justify_end().child(
                    widgets::button(&tokens, "fork-cancel", Key::Cancel)
                        .on_click(cx.listener(|prompt, _, _, cx| prompt.cancel(cx))),
                ),
            );
        div()
            .id("fork-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_key_down(cx.listener(|prompt, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    prompt.cancel(cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|prompt, _, _, cx| prompt.cancel(cx)))
            .child(card)
            .into_any_element()
    }
}
