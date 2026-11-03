//! The optional agent panel. A status strip until the panel is designed.

// The protocol modules are proven by `examples/agent_probe.rs` and tests until the panel uses them.
#[allow(dead_code)]
mod cli;
#[allow(dead_code)]
mod session;

use gpui::prelude::*;
use gpui::{Context, Window, div};

use crate::tokens::{self, Tokens};

pub struct AgentView;

impl AgentView {
    pub fn new() -> Self {
        Self
    }
}

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .border_t_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
            .child("Agent: not connected")
    }
}
