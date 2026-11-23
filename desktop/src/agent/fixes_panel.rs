//! The typesetter in the Proof tab: the command that starts a run, and the queue of kept fixes, each with its page
//! before and after, Accept, and Reject. It shows nothing with the agent off.

use gpui::prelude::*;
use gpui::{Context, Entity, Pixels, ScrollHandle, Window, div, img, px, relative};

use super::AgentView;
use super::fixing::{Entry, Shot};
use super::strings::{self, Key};
use crate::a11y::Control as _;
use crate::shell::ui::small_button;
use crate::tokens::{self, Tokens};

/// A page of an entry in the rail: half the panel, less its padding and the gap between the two.
const PAGE_WIDTH: Pixels = px(132.);
/// The queue takes at most this part of the panel's height, so the list of problems below it stays in view.
const QUEUE_HEIGHT: f32 = 0.7;

pub struct FixesPanel {
    agent: Entity<AgentView>,
    scroll: ScrollHandle,
}

impl FixesPanel {
    pub(super) fn new(agent: Entity<AgentView>, cx: &mut Context<Self>) -> Self {
        cx.observe(&agent, |_, _, cx| cx.notify()).detach();
        Self {
            agent,
            scroll: ScrollHandle::new(),
        }
    }

    /// One page of an entry under its label, with the fix's block outlined.
    fn page(
        &self,
        tokens: &Tokens,
        id: u64,
        side: &'static str,
        label: String,
        shot: &Shot,
    ) -> impl IntoElement + use<> {
        let size = shot.image.size(0);
        let height = PAGE_WIDTH * (size.height.0 as f32 / size.width.0 as f32);
        let agent = self.agent.clone();
        let mark = shot.mark.map(|[left, top, width, mark_height]| {
            div()
                .absolute()
                .left(PAGE_WIDTH * left - tokens::SPACE_1 / 2.)
                .top(height * top - tokens::SPACE_1 / 2.)
                .w(PAGE_WIDTH * width + tokens::SPACE_1)
                .h(height * mark_height + tokens::SPACE_1)
                .border_1()
                .border_color(tokens.accent)
        });
        div()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(label.clone()),
            )
            .child(
                div()
                    .id((side, id as usize))
                    .button(tokens, format!("{}: {label}", crate::t!(Key::FixShowPage)))
                    .relative()
                    .w(PAGE_WIDTH)
                    .h(height)
                    .border_1()
                    .border_color(tokens.divider)
                    .cursor_pointer()
                    .overflow_hidden()
                    .on_click(move |_, _, cx| agent.update(cx, |agent, cx| agent.show_fix(id, cx)))
                    .child(img(shot.image.clone()).w(PAGE_WIDTH - px(2.)).h(height - px(2.)))
                    .children(mark),
            )
    }

    fn entry(&self, tokens: &Tokens, entry: &Entry) -> impl IntoElement + use<> {
        let id = entry.id;
        let on = |run: fn(&mut AgentView, u64, &mut Window, &mut Context<AgentView>)| {
            let agent = self.agent.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                agent.update(cx, |agent, cx| run(agent, id, window, cx));
            }
        };
        let pages = entry.before.as_ref().zip(entry.after.as_ref()).map(|(before, after)| {
            div()
                .flex()
                .gap(tokens::SPACE_2)
                .child(self.page(
                    tokens,
                    id,
                    "fix-before",
                    crate::t!(Key::FixBefore, page = before.page),
                    before,
                ))
                .child(self.page(
                    tokens,
                    id,
                    "fix-after",
                    crate::t!(Key::FixAfter, page = after.page),
                    after,
                ))
        });
        div()
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_2)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .border_b_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().child(entry.title()))
                    .child(
                        div()
                            .truncate()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_quiet)
                            .child(entry.place()),
                    )
                    .children(strings::fix_counts(&entry.counts).into_iter().map(|line| {
                        div()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(line)
                    })),
            )
            .child(
                div()
                    .flex()
                    .gap(tokens::SPACE_2)
                    .child(
                        small_button(tokens, ("fix-accept", id as usize), crate::t!(Key::FixAccept))
                            .text_color(tokens.accent)
                            .on_click(on(|agent, id, window, cx| agent.accept_fix(id, window, cx))),
                    )
                    .child(
                        small_button(tokens, ("fix-reject", id as usize), crate::t!(Key::FixReject))
                            .on_click(on(|agent, id, window, cx| agent.reject_fix(id, window, cx))),
                    ),
            )
            .children(pages)
    }
}

impl Render for FixesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.agent.update(cx, |agent, _| agent.take_stale_fix_pages()) {
            window.drop_image(image).ok();
        }
        let agent = self.agent.read(cx);
        let line = agent.fix_line();
        let offered = line.is_none() && agent.fixes().is_empty() && agent.warnings_to_fix(cx) > 0;
        if !agent.is_enabled() || (line.is_none() && agent.fixes().is_empty() && !offered) {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let count = agent.fixes().len();
        let handle = self.agent.clone();
        let on = move |run: fn(&mut AgentView, &mut Window, &mut Context<AgentView>)| {
            let handle = handle.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                handle.update(cx, |agent, cx| run(agent, window, cx));
            }
        };
        let row = || {
            div()
                .flex_none()
                .px(tokens::SPACE_3)
                .py(tokens::SPACE_2)
                .flex()
                .items_center()
                .justify_between()
                .gap(tokens::SPACE_2)
                .border_b_1()
                .border_color(tokens.divider)
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_quiet)
        };
        let agent = self.agent.clone();
        let entries: Vec<_> = (agent.read(cx).fixes().iter())
            .map(|entry| self.entry(&tokens, entry).into_any_element())
            .collect();
        div()
            .id("fixes")
            .flex_none()
            .max_h(relative(QUEUE_HEIGHT))
            .flex()
            .flex_col()
            .when_some(line, |panel, line| {
                panel.child(
                    row().child(div().min_w_0().child(line)).child(
                        small_button(&tokens, "fix-stop", crate::t!(Key::Stop))
                            .on_click(on(|agent, _, cx| agent.stop_turn(cx))),
                    ),
                )
            })
            .when(offered, |panel| {
                panel.child(
                    row()
                        .flex_col()
                        .items_start()
                        .child(div().child(crate::t!(Key::FixOffer)))
                        .child(
                            small_button(&tokens, "fix-proof", crate::t!(Key::FixProof))
                                .text_color(tokens.accent)
                                .on_click(on(|agent, window, cx| agent.fix_proof(window, cx))),
                        ),
                )
            })
            .when(count > 0, |panel| {
                panel
                    .child(
                        row()
                            // The buttons go under the count where both do not fit on one line, as in German.
                            .flex_wrap()
                            .child(div().flex_none().child(strings::fixes_to_review(count)))
                            .child(
                                div()
                                    .flex()
                                    .gap(tokens::SPACE_2)
                                    .child(
                                        small_button(&tokens, "fixes-accept", crate::t!(Key::AcceptAll))
                                            .text_color(tokens.accent)
                                            .on_click(on(|agent, window, cx| agent.accept_all_fixes(window, cx))),
                                    )
                                    .child(
                                        small_button(&tokens, "fixes-reject", crate::t!(Key::RejectAll))
                                            .on_click(on(|agent, _, cx| agent.reject_all_fixes(cx))),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("fixes-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .children(entries),
                    )
            })
            .into_any_element()
    }
}

/// Puts the panel above the Proof tab's list.
pub(super) fn mount(agent: &Entity<AgentView>, cx: &mut gpui::App) {
    let panel = cx.new(|cx| FixesPanel::new(agent.clone(), cx));
    crate::shell::mount_above_proof(panel.into(), cx);
}
