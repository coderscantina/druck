//! Drawing the panel: the header, the conversation, the pending approval, the chips, and the prompt.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontStyle, FontWeight, HighlightStyle, Hsla, SharedString, Stateful, StyledText,
    UnderlineStyle, Window, div, px, svg,
};

use super::approval::{Approval, Preview};
use super::cli::{CliState, Login};
use super::context::ChipKind;
use super::conversation::{Entry, Status, ToolCall, ToolState};
use super::diff::{DiffLine, Mark};
use super::markdown::{self, Block, Inline};
use super::strings::{self, Key};
use super::{AgentView, HunkKey, MODELS, Standing};
use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

/// The tallest a diff in an approval grows before it scrolls.
const DIFF_HEIGHT: gpui::Pixels = px(180.);

impl Render for AgentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_checked(window, cx);
        let tokens = Tokens::get(cx).clone();
        div()
            .id("agent")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.header(&tokens, cx))
            .when(self.model_menu, |panel| panel.child(self.model_menu(&tokens, cx)))
            .child(self.body(&tokens, cx))
            .child(self.footer(&tokens, window, cx))
    }
}

impl AgentView {
    fn header(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.map_or(crate::t!(Key::ModelDefault), |model| model);
        let busy = !self.conversation.is_idle();
        div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(crate::t!(Key::Title)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_1)
                    .child(
                        small_button("agent-model", format!("{}: {model}", crate::t!(Key::Model)), tokens)
                            .when(busy, |button| button.opacity(0.5))
                            .when(!busy, |button| {
                                button.on_click(cx.listener(|view, _, _, cx| {
                                    view.model_menu = !view.model_menu;
                                    cx.notify();
                                }))
                            }),
                    )
                    .child(
                        small_button("agent-new", crate::t!(Key::NewConversation).to_owned(), tokens)
                            .when(busy, |button| button.opacity(0.5))
                            .when(!busy, |button| {
                                button.on_click(cx.listener(|view, _, _, cx| view.new_conversation(cx)))
                            }),
                    ),
            )
    }

    fn model_menu(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let options: Vec<_> = MODELS
            .iter()
            .copied()
            .map(|model| {
                let label = model.map_or(crate::t!(Key::ModelDefault), |model| model);
                div()
                    .id(label)
                    .button(tokens, label)
                    .h(tokens::LIST_ROW_HEIGHT)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|row| row.bg(tokens.raised))
                    .text_color(if model == self.model {
                        tokens.accent
                    } else {
                        tokens.text
                    })
                    .on_click(cx.listener(move |view, _, _, cx| view.choose_model(model, cx)))
                    .child(label)
            })
            .collect();
        div()
            .flex_none()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(tokens.divider)
            .bg(tokens.panel)
            .children(options)
    }

    fn body(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut rows: Vec<AnyElement> = Vec::new();
        rows.extend(self.status_rows(tokens));
        if self.cli_usable() {
            if self.conversation.entries().is_empty() {
                rows.push(quiet(
                    tokens,
                    if self.resumable {
                        crate::t!(Key::Resumed)
                    } else {
                        crate::t!(Key::Intro)
                    },
                ));
            }
            for index in 0..self.conversation.entries().len() {
                rows.push(self.entry(index, tokens, cx));
            }
            match self.conversation.status() {
                Status::Working if self.conversation.approvals().next().is_none() => {
                    rows.push(faint(tokens, crate::t!(Key::Working)));
                }
                Status::Stopping => rows.push(faint(tokens, crate::t!(Key::Stopping))),
                _ => {}
            }
        }
        div()
            .id("agent-conversation")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_2)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .children(rows)
    }

    /// What the panel says about the CLI: being looked for, missing, or what version and login it has.
    fn status_rows(&self, tokens: &Tokens) -> Vec<AnyElement> {
        let Some(cli) = &self.cli else {
            return vec![faint(tokens, crate::t!(Key::Checking))];
        };
        let mut rows = Vec::new();
        match &cli.state {
            CliState::Missing => {
                rows.push(card(
                    tokens,
                    crate::t!(Key::SetupTitle),
                    &[
                        crate::t!(Key::SetupMissing),
                        crate::t!(Key::SetupSteps),
                        crate::t!(Key::SetupOwner),
                    ],
                ));
            }
            CliState::Unreadable(_) => {
                rows.push(card(
                    tokens,
                    crate::t!(Key::SetupTitle),
                    &[crate::t!(Key::CliUnreadable), crate::t!(Key::SetupSteps)],
                ));
            }
            CliState::Untested(version) => {
                rows.push(quiet_owned(tokens, strings::cli_untested(version)));
                rows.extend(self.login_rows(&cli.login, tokens));
            }
            CliState::Ready(version) => {
                rows.push(faint_owned(tokens, strings::cli_version(version)));
                rows.extend(self.login_rows(&cli.login, tokens));
            }
        }
        rows
    }

    /// Which login the CLI reports, in plain words, and what is not settled about a subscription.
    fn login_rows(&self, login: &Login, tokens: &Tokens) -> Vec<AnyElement> {
        match login {
            Login::LoggedOut => vec![quiet(tokens, crate::t!(Key::LoggedOut))],
            Login::Unknown => vec![faint_owned(tokens, crate::t!(Key::LoginUnknown).to_owned())],
            Login::LoggedIn { method, plan } => {
                let mut rows = vec![faint_owned(tokens, strings::login_line(method, plan.as_deref()))];
                if method == "claude.ai" {
                    rows.push(faint(tokens, crate::t!(Key::SubscriptionNote)));
                }
                rows
            }
        }
    }

    fn entry(&self, index: usize, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        match &self.conversation.entries()[index] {
            Entry::User { text, context } => {
                let open = self.expanded.contains(&index);
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .p(tokens::SPACE_2)
                    .rounded(tokens::RADIUS)
                    .bg(tokens.raised)
                    .child(div().child(text.clone()))
                    .when_some(context.clone(), |row, context| {
                        row.child(
                            div()
                                .id(("sent-context", index))
                                .button(tokens, crate::t!(Key::Show))
                                .cursor_pointer()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    if !view.expanded.remove(&index) {
                                        view.expanded.insert(index);
                                    }
                                    cx.notify();
                                }))
                                .child(strings::context_sent_with(&context.labels))
                                .when(open, |row| row.child(mono_block(tokens, context.text.clone()))),
                        )
                    })
                    .into_any_element()
            }
            Entry::Assistant { text } => self.markdown(index, text, tokens),
            Entry::Tool(tool) => self.tool_row(index, tool, tokens, cx),
            Entry::Error(message) => div()
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.removed)
                .child(message.clone())
                .into_any_element(),
            Entry::Notice(message) => quiet_owned(tokens, message.clone()),
            Entry::Stopped => faint(tokens, crate::t!(Key::Stopped)),
            Entry::TurnEnd { cost_usd, changes } => {
                let cost = (*cost_usd).map(strings::turn_cost);
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .when_some(cost, |row, cost| {
                        row.child(
                            div()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .child(cost),
                        )
                    })
                    .when_some(*changes, |row, turn| row.child(self.summary(turn, tokens, cx)))
                    .into_any_element()
            }
        }
    }

    fn tool_row(&self, index: usize, tool: &ToolCall, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let (state, color) = match tool.state {
            ToolState::Running | ToolState::Done => ("", tokens.text_faint),
            ToolState::Waiting => (crate::t!(Key::ToolWaiting), tokens.accent),
            ToolState::Failed => (crate::t!(Key::ToolFailed), tokens.removed),
            ToolState::Denied => (crate::t!(Key::ToolDenied), tokens.removed),
        };
        let open = self.expanded.contains(&index);
        let has_output = !tool.output.is_empty();
        div()
            .flex()
            .flex_col()
            .text_size(tokens::TEXT_SMALL)
            .child(
                div()
                    .id(("tool", index))
                    .button(tokens, tool.name.clone())
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_1)
                    .min_w_0()
                    .text_color(tokens.text_quiet)
                    .when(has_output, |row| {
                        row.cursor_pointer().on_click(cx.listener(move |view, _, _, cx| {
                            if !view.expanded.remove(&index) {
                                view.expanded.insert(index);
                            }
                            cx.notify();
                        }))
                    })
                    .child(
                        svg()
                            .path("icons/more.svg")
                            .size(tokens::TEXT_SMALL)
                            .flex_none()
                            .text_color(tokens.text_faint),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tool.name.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_color(tokens.text_faint)
                            .child(tool.target.clone()),
                    )
                    .child(div().flex_none().text_color(color).child(state)),
            )
            .when(open && has_output, |row| {
                row.child(mono_block(tokens, tool.output.clone()))
            })
            .into_any_element()
    }

    /// A turn's changes: each hunk with where it is now and what it can do.
    fn summary(&self, turn: usize, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let Some(changes) = self.conversation.changes(turn) else {
            return div().into_any_element();
        };
        let mut files = Vec::new();
        for (file_at, file) in changes.files.iter().enumerate() {
            let path = super::relative(&self.base, &file.path);
            let hunks: Vec<_> = file
                .hunks
                .iter()
                .enumerate()
                .map(|(hunk_at, tracked)| {
                    let key: HunkKey = (turn, file_at, hunk_at);
                    let standing = self.standing.get(&key).copied().unwrap_or(Standing::NotOpen);
                    self.hunk_row(key, tracked, standing, tokens, cx)
                })
                .collect();
            files.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(
                        div()
                            .truncate()
                            .text_size(tokens::TEXT_SMALL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens.text_quiet)
                            .child(path),
                    )
                    .children(hunks),
            );
        }
        let created: Vec<_> = changes
            .created
            .iter()
            .map(|path| faint_owned(tokens, strings::created_file(&super::relative(&self.base, path))))
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .p(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(strings::changes_summary(changes.hunk_count(), changes.files.len())),
            )
            .children(files)
            .children(created)
            .into_any_element()
    }

    fn hunk_row(
        &self,
        key: HunkKey,
        tracked: &super::changes::Tracked,
        standing: Standing,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hunk = &tracked.hunk;
        let note = match standing {
            Standing::Applied => None,
            Standing::Reverted => Some(crate::t!(Key::HunkReverted)),
            Standing::Moved => Some(crate::t!(Key::HunkMoved)),
            Standing::NotOpen => Some(crate::t!(Key::HunkNotOpen)),
        };
        let shown: Vec<_> = hunk
            .lines
            .iter()
            .filter(|line| line.mark != Mark::Context)
            .take(4)
            .map(|line| {
                let (sign, color) = if line.mark == Mark::Added {
                    ("+", tokens.added)
                } else {
                    ("\u{2212}", tokens.removed)
                };
                div()
                    .flex()
                    .gap(tokens::SPACE_1)
                    .font_family(tokens::FONT_MONO)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(color)
                    .child(div().flex_none().child(sign))
                    .child(div().min_w_0().flex_1().truncate().child(line.text.clone()))
            })
            .collect();
        let id = key.0 * 10_000 + key.1 * 100 + key.2;
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(div().flex_1().child(format!(
                        "{}  +{} \u{2212}{}{}",
                        strings::hunk_line(hunk.first_changed_line_number()),
                        hunk.added(),
                        hunk.removed(),
                        note.map(|note| format!("  \u{b7} {note}")).unwrap_or_default()
                    )))
                    .when(standing != Standing::NotOpen && standing != Standing::Moved, |row| {
                        row.child(
                            small_button(("hunk-show", id), crate::t!(Key::Show).to_owned(), tokens)
                                .on_click(cx.listener(move |view, _, window, cx| view.reveal(key, window, cx))),
                        )
                    })
                    .when(standing == Standing::Applied, |row| {
                        row.child(
                            small_button(("hunk-revert", id), crate::t!(Key::Revert).to_owned(), tokens)
                                .on_click(cx.listener(move |view, _, window, cx| view.revert(key, window, cx))),
                        )
                    }),
            )
            .children(shown)
            .into_any_element()
    }

    /// A reply as blocks. A streamed reply only grows, so a parse is kept while its length is the same.
    fn markdown(&self, index: usize, source: &str, tokens: &Tokens) -> AnyElement {
        let blocks = {
            let mut parsed = self.parsed.borrow_mut();
            match parsed.get(&index) {
                Some((length, blocks)) if *length == source.len() => blocks.clone(),
                _ => {
                    let blocks = Rc::new(markdown::parse(source));
                    parsed.insert(index, (source.len(), blocks.clone()));
                    blocks
                }
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .children(blocks.iter().map(|block| markdown_block(block, tokens)))
            .into_any_element()
    }

    // The foot: the approval waiting, the chips, and the prompt

    fn footer(&mut self, tokens: &Tokens, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let _ = window;
        let waiting: Vec<Approval> = self.conversation.approvals().take(1).cloned().collect();
        let more = self.conversation.approvals().count().saturating_sub(1);
        let busy = !self.conversation.is_idle();
        let usable = self.cli_usable();
        let chips: Vec<_> = self
            .context
            .chips()
            .into_iter()
            .filter(|kind| !self.removed.contains(kind))
            .map(|kind| self.chip(kind, tokens, cx))
            .collect();
        let spent = self.conversation.spent();
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .p(tokens::SPACE_3)
            .border_t_1()
            .border_color(tokens.divider)
            .children(
                waiting
                    .into_iter()
                    .map(|approval| self.approval_card(&approval, more, tokens, cx)),
            )
            .when(usable && !chips.is_empty(), |footer| {
                footer.child(div().flex().flex_wrap().gap(tokens::SPACE_1).children(chips))
            })
            .when(usable, |footer| footer.child(self.prompt.clone()))
            .when(usable, |footer| {
                footer.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_faint)
                        .child(if spent > 0. {
                            strings::turn_cost(spent)
                        } else {
                            String::new()
                        })
                        .child(if busy {
                            small_button("agent-stop", crate::t!(Key::Stop).to_owned(), tokens)
                                .text_color(tokens.accent)
                                .on_click(cx.listener(|view, _, _, cx| view.stop(cx)))
                                .into_any_element()
                        } else {
                            small_button("agent-send", crate::t!(Key::Send).to_owned(), tokens)
                                .text_color(tokens.accent)
                                .on_click(cx.listener(|view, _, window, cx| view.send(window, cx)))
                                .into_any_element()
                        }),
                )
            })
    }

    fn chip(&self, kind: ChipKind, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let label = self.chip_label(kind);
        div()
            .id(match kind {
                ChipKind::Chapter => "chip-chapter",
                ChipKind::Selection => "chip-selection",
                ChipKind::Diagnostics => "chip-diagnostics",
            })
            .max_w_full()
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .pl(tokens::SPACE_2)
            .pr(px(4.))
            .h(px(22.))
            .rounded(px(4.))
            .bg(tokens.raised)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .child(div().min_w_0().truncate().child(label))
            .child(
                div()
                    .id(("chip-remove", kind as usize))
                    .button(tokens, crate::t!(Key::RemoveChip))
                    .flex_none()
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .rounded(px(3.))
                    .hover(|button| button.bg(tokens.divider))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.removed.insert(kind);
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path("icons/close.svg")
                            .size(px(10.))
                            .text_color(tokens.text_faint),
                    ),
            )
    }

    fn approval_card(&self, approval: &Approval, more: usize, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let id = approval.request.request_id.clone();
        let rule = approval.session_rule.clone();
        let denying = self.denying.as_deref() == Some(id.as_str());
        let dirty = approval.file.as_deref().is_some_and(|file| self.is_unsaved(file, cx));
        let title = strings::allow_tool(&approval.request.tool_name, &approval.target);
        let preview = match &approval.preview {
            Preview::Diff(lines) => Some(diff_view(lines, tokens).into_any_element()),
            Preview::Text(text) => Some(mono_block(tokens, text.clone()).into_any_element()),
            Preview::Nothing => Some(faint(tokens, crate::t!(Key::NewFile))),
        };
        let (once_id, deny_id, session_id, reason_id) = (id.clone(), id.clone(), id.clone(), id.clone());
        div()
            .id("agent-approval")
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .p(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.accent)
            .bg(tokens.raised)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .when(dirty, |card| card.child(quiet(tokens, crate::t!(Key::UnsavedWarning))))
            .when_some(preview, |card, preview| card.child(preview))
            .when(!denying, |card| {
                card.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(tokens::SPACE_1)
                        .child(
                            small_button("approve-once", crate::t!(Key::AllowOnce).to_owned(), tokens)
                                .text_color(tokens.accent)
                                .on_click(cx.listener(move |view, _, _, cx| view.allow(&once_id, false, cx))),
                        )
                        .child(
                            small_button("approve-deny", crate::t!(Key::Deny).to_owned(), tokens)
                                .on_click(cx.listener(move |view, _, _, cx| view.deny(&deny_id, "", cx))),
                        )
                        .child(
                            small_button("approve-reason", crate::t!(Key::DenyWithReason).to_owned(), tokens).on_click(
                                cx.listener(move |view, _, window, cx| {
                                    view.denying = Some(reason_id.clone());
                                    window.focus(&gpui::Focusable::focus_handle(view.reason.read(cx), cx), cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .when_some(rule, |row, rule| {
                            row.child(
                                small_button("approve-session", strings::session_button(&rule.label), tokens)
                                    .on_click(cx.listener(move |view, _, _, cx| view.allow(&session_id, true, cx))),
                            )
                        }),
                )
            })
            .when(denying, |card| {
                card.child(faint(tokens, crate::t!(Key::ReasonLabel)))
                    .child(self.reason.clone())
                    .child(
                        div()
                            .flex()
                            .gap(tokens::SPACE_1)
                            .child(
                                small_button("approve-send-denial", crate::t!(Key::SendDenial).to_owned(), tokens)
                                    .text_color(tokens.accent)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        let reason = view.reason.read(cx).text().to_owned();
                                        if let Some(id) = view.denying.clone() {
                                            view.deny(&id, &reason, cx);
                                        }
                                    })),
                            )
                            .child(
                                small_button("approve-back", crate::t!(Key::Back).to_owned(), tokens).on_click(
                                    cx.listener(|view, _, _, cx| {
                                        view.denying = None;
                                        cx.notify();
                                    }),
                                ),
                            ),
                    )
            })
            .child(faint(
                tokens,
                if approval.session_rule.is_some() {
                    crate::t!(Key::ScopeSession)
                } else {
                    crate::t!(Key::ScopeOnce)
                },
            ))
            .when(more > 0, |card| {
                card.child(faint_owned(tokens, strings::more_waiting(more)))
            })
            .into_any_element()
    }

    /// Whether the open chapter that is `file` has text the file does not.
    fn is_unsaved(&self, file: &std::path::Path, cx: &Context<Self>) -> bool {
        let editor = self.editor.read(cx);
        let session = editor.session();
        super::file_in_session(session, file)
            .and_then(|known| session.chapter_of_file(&known))
            .and_then(|id| session.chapter(id))
            .is_some_and(|chapter| chapter.is_dirty())
    }
}

// Pieces

fn small_button(id: impl Into<gpui::ElementId>, label: String, tokens: &Tokens) -> Stateful<Div> {
    div()
        .id(id)
        .button(tokens, label.clone())
        .flex_none()
        .h(px(22.))
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .rounded(tokens::RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .cursor_pointer()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .hover(|button| button.bg(tokens.raised).text_color(tokens.text))
        .child(label)
}

fn quiet(tokens: &Tokens, text: &'static str) -> AnyElement {
    quiet_owned(tokens, text.to_owned())
}

fn quiet_owned(tokens: &Tokens, text: String) -> AnyElement {
    div()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .child(text)
        .into_any_element()
}

fn faint(tokens: &Tokens, text: &'static str) -> AnyElement {
    faint_owned(tokens, text.to_owned())
}

fn faint_owned(tokens: &Tokens, text: String) -> AnyElement {
    div()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_faint)
        .child(text)
        .into_any_element()
}

/// A titled block of paragraphs, for the setup text.
fn card(tokens: &Tokens, title: &'static str, paragraphs: &[&'static str]) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(tokens::SPACE_2)
        .p(tokens::SPACE_3)
        .rounded(tokens::RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .children(paragraphs.iter().map(|text| {
            div()
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_quiet)
                .child(*text)
        }))
        .into_any_element()
}

fn mono_block(tokens: &Tokens, text: String) -> Div {
    div()
        .p(tokens::SPACE_2)
        .rounded(tokens::RADIUS)
        .bg(tokens.field)
        .font_family(tokens::FONT_MONO)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .child(text)
}

fn diff_view(lines: &[DiffLine], tokens: &Tokens) -> impl IntoElement + use<> {
    let rows: Vec<_> = lines
        .iter()
        .map(|line| match line {
            DiffLine::Gap(count) => div()
                .px(tokens::SPACE_2)
                .text_color(tokens.text_faint)
                .child(format!("\u{22ef} {count}"))
                .into_any_element(),
            DiffLine::Line {
                mark,
                number,
                text,
                emphasis,
            } => {
                let (sign, color) = match mark {
                    Mark::Added => ("+", Some(tokens.added)),
                    Mark::Removed => ("\u{2212}", Some(tokens.removed)),
                    Mark::Context => (" ", None),
                };
                let words: SharedString = if text.is_empty() {
                    " ".into()
                } else {
                    text.clone().into()
                };
                let shown = StyledText::new(words).with_highlights(emphasis.iter().cloned().map(|range| {
                    (
                        range,
                        HighlightStyle {
                            background_color: color.map(|color| Hsla::from(color).opacity(0.35)),
                            font_weight: Some(FontWeight::BOLD),
                            ..Default::default()
                        },
                    )
                }));
                div()
                    .flex()
                    .gap(tokens::SPACE_1)
                    .px(tokens::SPACE_1)
                    .when_some(color, |row, color| row.bg(Hsla::from(color).opacity(0.14)))
                    .text_color(color.unwrap_or(tokens.text_quiet))
                    .child(
                        div()
                            .flex_none()
                            .w(px(22.))
                            .text_color(tokens.text_faint)
                            .child(number.to_string()),
                    )
                    .child(div().flex_none().child(sign))
                    .child(div().min_w_0().flex_1().child(shown))
                    .into_any_element()
            }
        })
        .collect();
    div()
        .id("agent-diff")
        .max_h(DIFF_HEIGHT)
        .overflow_y_scroll()
        .py(px(4.))
        .rounded(tokens::RADIUS)
        .bg(tokens.field)
        .font_family(tokens::FONT_MONO)
        .text_size(tokens::TEXT_SMALL)
        .flex()
        .flex_col()
        .children(rows)
}

fn markdown_block(block: &Block, tokens: &Tokens) -> AnyElement {
    match block {
        Block::Paragraph(inline) => div().child(styled(inline, tokens)).into_any_element(),
        Block::Heading(level, inline) => div()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(match level {
                1 => px(15.),
                2 => px(14.),
                _ => tokens::TEXT_BODY,
            })
            .child(styled(inline, tokens))
            .into_any_element(),
        Block::Quote(inline) => div()
            .pl(tokens::SPACE_2)
            .border_l_2()
            .border_color(tokens.divider)
            .text_color(tokens.text_quiet)
            .child(styled(inline, tokens))
            .into_any_element(),
        Block::Item { depth, marker, text } => div()
            .flex()
            .gap(tokens::SPACE_1)
            .pl(tokens::SPACE_3 * *depth as f32)
            .child(
                div()
                    .flex_none()
                    .min_w(px(14.))
                    .text_color(tokens.text_quiet)
                    .child(marker.clone()),
            )
            .child(div().min_w_0().flex_1().child(styled(text, tokens)))
            .into_any_element(),
        Block::Code(code) => mono_block(tokens, code.clone()).into_any_element(),
        Block::Rule => div().h(px(1.)).bg(tokens.divider).into_any_element(),
    }
}

/// Text with its bold, italic, code, and link runs.
fn styled(inline: &Inline, tokens: &Tokens) -> StyledText {
    let text: SharedString = inline.text.clone().into();
    let mut highlights = Vec::new();
    let mut mono = Vec::new();
    for (range, flags) in inline.runs() {
        if flags.code {
            mono.push((range.clone(), SharedString::from(tokens::FONT_MONO)));
        }
        highlights.push((
            range,
            HighlightStyle {
                font_weight: flags.strong.then_some(FontWeight::BOLD),
                font_style: flags.emphasis.then_some(FontStyle::Italic),
                background_color: flags.code.then(|| Hsla::from(tokens.raised)),
                color: flags.link.then(|| Hsla::from(tokens.accent)),
                underline: flags.link.then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(Hsla::from(tokens.accent)),
                    wavy: false,
                }),
                ..Default::default()
            },
        ));
    }
    StyledText::new(text)
        .with_highlights(highlights)
        .with_font_family_overrides(mono)
}
