//! Centre pane, writing side: the galley with its custom text element.

use gpui::prelude::*;
use gpui::{Context, Window, div};

use crate::tokens::{self, Tokens};

pub struct EditorView;

impl EditorView {
    pub fn new() -> Self {
        Self
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(tokens::SPACE_2)
            .pt(tokens::SPACE_4)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
            .child("Write")
            .child(div().w(tokens::SHEET_WIDTH).h(tokens::SHEET_HEIGHT).bg(tokens.sheet))
    }
}
