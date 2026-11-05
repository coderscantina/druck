//! `Cmd+Shift+S`: a card at the top of the text, with no backdrop. A message, the changed chapters with word counts, then
//! the buttons. In a folder with no repo it asks once whether to make one.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, Hsla, KeyBinding, ScrollHandle, Subscription, WeakEntity, Window,
    actions, div, linear_color_stop, linear_gradient, relative,
};

use super::repo::{self, Overview};
use super::strings::Key;
use super::{RepoState, Versions, change_words, ui, unreviewed_changes};
use crate::inspector::input::{Cancelled, Submitted, TextInput};
use crate::tokens::{self, Tokens};

actions!(commit_card, [Close]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("CommitCard"))]);
}

/// What gets the keys when the card opens.
pub enum CardField {
    Message,
    Identity,
    /// Nothing to type in: the card holds them, for Escape.
    None,
}

pub struct CommitCard {
    view: WeakEntity<Versions>,
    open: bool,
    /// Holds the keys while no field is on the card.
    focus: FocusHandle,
    /// What had the keys, which get them back.
    before: Option<FocusHandle>,
    message: Entity<TextInput>,
    name: Entity<TextInput>,
    email: Entity<TextInput>,
    /// This is the first commit asked for in a folder with no repo, and the question stands.
    first_asking: bool,
    changes_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl CommitCard {
    pub fn new(view: WeakEntity<Versions>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = |label: Key, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                TextInput::new(window, cx)
                    .keep_focus_on_enter()
                    .with_label(crate::t!(label))
                    .with_placeholder(crate::t!(label))
            })
        };
        let message = field(Key::Message, window, cx);
        let name = field(Key::YourName, window, cx);
        let email = field(Key::YourEmail, window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&message, window, |card, _, _: &Submitted, window, cx| {
                card.commit(false, window, cx)
            }),
            cx.subscribe_in(&message, window, |card, _, _: &Cancelled, window, cx| {
                card.close(window, cx)
            }),
            cx.subscribe_in(&email, window, |card, _, _: &Submitted, window, cx| {
                card.save_identity(window, cx)
            }),
            cx.subscribe_in(&name, window, |card, _, _: &Cancelled, window, cx| {
                card.close(window, cx)
            }),
            cx.subscribe_in(&email, window, |card, _, _: &Cancelled, window, cx| {
                card.close(window, cx)
            }),
        ];
        Self {
            view,
            open: false,
            focus: cx.focus_handle(),
            before: None,
            message,
            name,
            email,
            first_asking: false,
            changes_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Puts words in the message field. For screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn preset_message(&mut self, message: String, cx: &mut Context<Self>) {
        self.message.update(cx, |input, cx| input.set_value(message, true, cx));
    }

    /// Opens the card, remembering what had the keys.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.before = window.focused(cx);
        self.open = true;
        self.first_asking = false;
        cx.notify();
    }

    /// Takes what is known of the repo. `message` is what a merge suggests, for an empty field, and `asking` says the
    /// question about a repo stands.
    pub fn prepare(
        &mut self,
        message: Option<String>,
        asking: bool,
        field: CardField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.first_asking |= asking;
        if let Some(message) = message.filter(|message| !message.is_empty())
            && self.message.read(cx).text().is_empty()
        {
            self.message.update(cx, |input, cx| input.set_value(message, true, cx));
        }
        match field {
            CardField::Message => self.message.read(cx).focus_handle(cx).focus(window, cx),
            CardField::Identity => self.name.read(cx).focus_handle(cx).focus(window, cx),
            CardField::None => self.focus.focus(window, cx),
        }
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        if let Some(before) = self.before.take() {
            window.focus(&before, cx);
        }
        cx.notify();
    }

    /// A commit landed. With `all_done` nothing is left to say, so the card goes.
    pub fn committed(&mut self, all_done: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.message
            .update(cx, |input, cx| input.set_value(String::new(), true, cx));
        if self.open && all_done {
            self.close(window, cx);
        }
    }

    fn commit(&mut self, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).text().to_owned();
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |view, cx| view.commit(message, push, window, cx));
        }
    }

    fn save_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, email) = (
            self.name.read(cx).text().trim().to_owned(),
            self.email.read(cx).text().trim().to_owned(),
        );
        if name.is_empty() || email.is_empty() {
            return;
        }
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |view, cx| view.set_identity(name, email, window, cx));
        }
        self.message.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn field(input: &Entity<TextInput>) -> impl IntoElement + use<> {
        div()
            .flex_none()
            .px(tokens::SPACE_3)
            .pt(tokens::SPACE_2)
            .child(input.clone())
    }

    fn buttons() -> gpui::Div {
        div()
            .flex_none()
            .flex()
            .justify_end()
            .gap(tokens::SPACE_2)
            .px(tokens::SPACE_3)
            .pt(tokens::SPACE_2)
            .pb(tokens::SPACE_3)
    }

    fn no_repo(&self, tokens: &Tokens, view: &Entity<Versions>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let mut parts = Vec::new();
        let text = if self.first_asking {
            crate::t!(Key::FirstCommitQuestion)
        } else {
            crate::t!(Key::NoRepoLine)
        };
        parts.push(ui::quiet_line(tokens, text).pt(tokens::SPACE_2).into_any_element());
        if let Some(service) = view.read(cx).book_folder(cx).as_deref().and_then(repo::cloud_service) {
            parts.push(ui::accent_line(tokens, crate::t!(Key::CloudWarning, service = service)).into_any_element());
        }
        let actions = Self::buttons()
            .when(self.first_asking, |row| {
                row.child(
                    ui::button(tokens, "card-not-now", crate::t!(Key::NotNow))
                        .on_click(cx.listener(|card, _, window, cx| card.close(window, cx))),
                )
            })
            .child(
                ui::button(tokens, "card-make-repo", crate::t!(Key::MakeRepo))
                    .text_color(tokens.accent)
                    .on_click(cx.listener(|card, _, window, cx| {
                        if let Some(view) = card.view.upgrade() {
                            view.update(cx, |view, cx| view.make_repo(window, cx));
                        }
                        card.message.read(cx).focus_handle(cx).focus(window, cx);
                    })),
            );
        parts.push(actions.into_any_element());
        parts
    }

    fn ready(
        &self,
        tokens: &Tokens,
        view: &Entity<Versions>,
        overview: &Overview,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let mut parts = Vec::new();
        if !overview.has_identity {
            parts.push(
                ui::quiet_line(tokens, crate::t!(Key::IdentityQuestion))
                    .pt(tokens::SPACE_2)
                    .into_any_element(),
            );
            parts.push(Self::field(&self.name).into_any_element());
            parts.push(Self::field(&self.email).into_any_element());
            parts.push(
                Self::buttons()
                    .child(
                        ui::button(tokens, "card-identity", crate::t!(Key::Continue))
                            .text_color(tokens.accent)
                            .on_click(cx.listener(|card, _, window, cx| card.save_identity(window, cx))),
                    )
                    .into_any_element(),
            );
            return parts;
        }
        parts.push(Self::field(&self.message).into_any_element());
        if overview.merging.is_some() {
            parts.push(ui::quiet_line(tokens, crate::t!(Key::MergingNote)).into_any_element());
            if let Some((_, chapter)) = &view.read(cx).both_stand_in {
                parts.push(ui::accent_line(tokens, crate::t!(Key::BothStand, chapter = chapter)).into_any_element());
            }
        }
        if overview.changes.is_empty() && overview.merging.is_none() {
            parts.push(
                ui::quiet_line(tokens, crate::t!(Key::NothingChanged))
                    .pt(tokens::SPACE_2)
                    .into_any_element(),
            );
        } else {
            let rows = overview.listed().map(|change| {
                let view = view.read(cx);
                let title = view.title_of(&change.path, &change.name, cx);
                ui::change_row(tokens, title, change_words(change, view.in_conflict(&change.path, cx)))
                    .into_any_element()
            });
            let more_below = {
                let (offset, most) = (self.changes_scroll.offset().y, self.changes_scroll.max_offset().y);
                overview.listed().count() > tokens::COMMIT_CARD_MAX_ROWS && -offset < most.max(tokens::HAIRLINE)
            };
            parts.push(
                div()
                    .flex_none()
                    .mt(tokens::SPACE_2)
                    .relative()
                    .child(
                        div()
                            .id("card-changes")
                            .max_h(tokens::VERSIONS_ROW_HEIGHT * tokens::COMMIT_CARD_MAX_ROWS as f32)
                            .overflow_y_scroll()
                            .track_scroll(&self.changes_scroll)
                            .flex()
                            .flex_col()
                            .children(rows.collect::<Vec<_>>()),
                    )
                    .when(more_below, |list| {
                        let (clear, solid) = (
                            Hsla::from(tokens.panel).opacity(tokens::CLEAR),
                            Hsla::from(tokens.panel),
                        );
                        list.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(tokens::VERSIONS_ROW_HEIGHT)
                                .bg(linear_gradient(
                                    tokens::FADE_DOWN,
                                    linear_color_stop(clear, 0.),
                                    linear_color_stop(solid, 1.),
                                )),
                        )
                    })
                    .into_any_element(),
            );
        }
        let unreviewed = unreviewed_changes(cx);
        if unreviewed > 0 {
            parts.push(ui::accent_line(tokens, crate::t!(Key::Unreviewed, count = unreviewed)).into_any_element());
        }
        let (installed, working) = (view.read(cx).git_installed(), view.read(cx).is_working());
        if overview.signs && !installed {
            parts.push(ui::quiet_line(tokens, crate::t!(Key::Unsigned)).into_any_element());
        }
        let can_push = overview.remote.is_some() && installed;
        let row = Self::buttons()
            .when(working, |row| {
                row.child(
                    div()
                        .flex_1()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_faint)
                        .child(crate::t!(Key::Working)),
                )
            })
            .when(can_push, |row| {
                row.child(
                    ui::button(tokens, "card-commit-push", crate::t!(Key::CommitAndPush))
                        .on_click(cx.listener(|card, _, window, cx| card.commit(true, window, cx))),
                )
            })
            .child(
                ui::button(tokens, "card-commit", crate::t!(Key::Commit))
                    .text_color(tokens.accent)
                    .on_click(cx.listener(|card, _, window, cx| card.commit(false, window, cx))),
            );
        parts.push(row.into_any_element());
        parts
    }
}

impl Render for CommitCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view.upgrade();
        let (true, Some(view)) = (self.open, view) else {
            return div().into_any_element();
        };
        let tokens = Tokens::get(cx).clone();
        let state = view.read(cx).state().clone();
        let mut parts = match &state {
            RepoState::Single => vec![
                ui::quiet_line(&tokens, crate::t!(Key::SingleFile))
                    .py(tokens::SPACE_3)
                    .into_any_element(),
            ],
            RepoState::Reading => vec![
                ui::quiet_line(&tokens, crate::t!(Key::Reading))
                    .py(tokens::SPACE_3)
                    .into_any_element(),
            ],
            RepoState::NoRepo => self.no_repo(&tokens, &view, cx),
            RepoState::Broken(reason) => vec![
                ui::accent_line(&tokens, reason.clone())
                    .py(tokens::SPACE_3)
                    .into_any_element(),
            ],
            RepoState::Ready(overview) => self.ready(&tokens, &view, overview, cx),
        };
        if let Some(error) = view.read(cx).error() {
            parts.insert(
                parts.len().saturating_sub(1),
                ui::accent_line(&tokens, error.to_owned()).into_any_element(),
            );
        }
        div()
            .id("commit-card")
            .occlude()
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT + tokens::SPACE_4)
            .left(relative(0.5))
            .ml(tokens::COMMIT_CARD_WIDTH / -2.)
            .w(tokens::COMMIT_CARD_WIDTH)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow_lg()
            .track_focus(&self.focus)
            .key_context("CommitCard")
            .on_action(cx.listener(|card, _: &Close, window, cx| card.close(window, cx)))
            .child(ui::section_label(&tokens, crate::t!(Key::CommitLabel)))
            .children(parts)
            .into_any_element()
    }
}
