//! The source view's button in the title bar. The view itself is the galley with its markers shown: same buffer, caret,
//! and undo.

use gpui::prelude::*;
use gpui::{Context, Entity, Subscription, div, svg};

use super::strings::Key;
use super::{EditorView, SessionChanged, SourceViewChanged};
use crate::tokens::{self, Tokens};

pub struct SourceToggle {
    editor: Entity<EditorView>,
    _subscriptions: Vec<Subscription>,
}

impl SourceToggle {
    pub fn new(editor: Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&editor, |_, _, _: &SessionChanged, cx| cx.notify()),
            cx.subscribe(&editor, |_, _, _: &SourceViewChanged, cx| cx.notify()),
        ];
        Self {
            editor,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for SourceToggle {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let on = self.editor.read(cx).shows_source();
        let color = if on { tokens.text } else { tokens.text_quiet };
        let raised = tokens.raised;
        div()
            .id("source-view")
            .px(tokens::SPACE_2)
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .text_color(color)
            .when(on, |button| button.bg(raised))
            .hover(move |button| button.bg(raised))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|toggle, _, window, cx| {
                toggle.editor.update(cx, |editor, cx| editor.toggle_source(window, cx));
            }))
            .child(svg().path("icons/source.svg").size(tokens::SPACE_3).text_color(color))
            .child(crate::t!(Key::SourceView))
    }
}
