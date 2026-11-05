//! The layers around the conversation: the chat card over the text, the prompt under a selection, and the line at the foot
//! of the window that says what waits for review.

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, Window, div};

use super::prompt::PromptInput;
use super::strings::{self, Key};
use super::{AgentView, Workspace};
use crate::editor::review::small_button;
use crate::tokens::{self, Tokens};
use crate::workspace::Slots;

/// Mounts the card and the foot's line.
pub fn mount(agent: &Entity<AgentView>, slots: &mut Slots, _: &mut Window, cx: &mut Context<Workspace>) {
    let chat = cx.new(|cx| ChatCard::new(agent.clone(), cx));
    slots.overlays.push(chat.into());
    let foot = cx.new(|cx| ReviewFoot::new(agent.clone(), cx));
    // Before the saving state, whose line takes the room that is left, so a note coming and going moves nothing.
    slots.footer.insert(0, foot.into());
}

/// The small card at the bottom right, over the text and without a backdrop.
struct ChatCard {
    agent: Entity<AgentView>,
}

impl ChatCard {
    fn new(agent: Entity<AgentView>, cx: &mut Context<Self>) -> Self {
        cx.observe(&agent, |_, _, cx| cx.notify()).detach();
        Self { agent }
    }
}

impl Render for ChatCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.agent.read(cx).chat_is_on_screen() {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx);
        div()
            .absolute()
            .right(tokens::SPACE_3)
            .bottom(tokens::FOOTER_HEIGHT)
            .w(tokens::CHAT_CARD_WIDTH)
            .max_h(tokens::CHAT_CARD_MAX_HEIGHT)
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .bg(tokens.panel)
            .shadow_lg()
            .text_size(tokens::NOTE_TEXT)
            .text_color(tokens.text)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.agent.clone())
            .into_any_element()
    }
}

/// The one-line card under a selection. While it is open the selection stays tinted behind it.
pub struct SelectionCard {
    agent: Entity<AgentView>,
    input: Entity<PromptInput>,
}

impl SelectionCard {
    pub fn new(agent: Entity<AgentView>, input: Entity<PromptInput>, cx: &mut Context<Self>) -> Self {
        cx.observe(&agent, |_, _, cx| cx.notify()).detach();
        Self { agent, input }
    }
}

impl Render for SelectionCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let agent = self.agent.read(cx);
        let hint = if agent.is_looking_for_cli() {
            crate::t!(Key::Checking)
        } else if agent.is_working() {
            crate::t!(Key::Working)
        } else {
            "\u{23ce}"
        };
        div()
            .w(tokens::PROMPT_CARD_WIDTH)
            .min_h(tokens::PROMPT_CARD_HEIGHT)
            .px(tokens::SPACE_2)
            .py(tokens::SPACE_1)
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .bg(tokens.panel)
            .shadow_md()
            .font_family(tokens::FONT_UI)
            .text_size(tokens::NOTE_TEXT)
            .text_color(tokens.text)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex_1().min_w_0().child(self.input.clone()))
            .child(
                div()
                    .flex_none()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(hint),
            )
    }
}

/// How many changes wait for review, with Accept all and Reject all, and that a turn is running while the card is closed.
struct ReviewFoot {
    agent: Entity<AgentView>,
}

impl ReviewFoot {
    fn new(agent: Entity<AgentView>, cx: &mut Context<Self>) -> Self {
        cx.observe(&agent, |_, _, cx| cx.notify()).detach();
        Self { agent }
    }
}

impl Render for ReviewFoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let agent = self.agent.read(cx);
        let (here, elsewhere) = agent.review_counts(cx);
        let working = agent.is_working() && !agent.chat_is_on_screen();
        let has_next = agent.has_next_change(cx);
        if !agent.is_enabled() || (here + elsewhere == 0 && !working) {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let handle = self.agent.clone();
        let on = move |run: fn(&mut AgentView, &mut Window, &mut Context<AgentView>)| {
            let handle = handle.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                handle.update(cx, |agent, cx| run(agent, window, cx));
            }
        };
        div()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .px(tokens::SPACE_3)
            .bg(tokens.canvas)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(working, |foot| {
                foot.child(crate::t!(Key::Working)).child(
                    small_button("foot-stop", crate::t!(Key::Stop).to_owned(), &tokens)
                        .on_click(on(|agent, _, cx| agent.stop_turn(cx))),
                )
            })
            .when(here + elsewhere > 0, |foot| {
                foot.child(div().min_w_0().truncate().child(strings::review_foot(here, elsewhere)))
                    .when(has_next, |foot| {
                        foot.child(
                            small_button("foot-next", crate::t!(Key::NextChange).to_owned(), &tokens)
                                .on_click(on(|agent, window, cx| agent.next_change(window, cx))),
                        )
                    })
                    .child(
                        small_button("foot-accept", crate::t!(Key::AcceptAll).to_owned(), &tokens)
                            .text_color(tokens.accent)
                            .on_click(on(|agent, window, cx| agent.accept_all(window, cx))),
                    )
                    .child(
                        small_button("foot-reject", crate::t!(Key::RejectAll).to_owned(), &tokens)
                            .on_click(on(|agent, window, cx| agent.reject_all(window, cx))),
                    )
            })
            .into_any_element()
    }
}
