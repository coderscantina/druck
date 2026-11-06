//! The window with no document: open one, or start one from a starter.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FocusHandle, FontWeight, SharedString, Window, div, svg};

use crate::a11y::Control as _;
use crate::session::adopt::{self, Offer};
use crate::session::{STARTERS, Starter};
use crate::tokens::{self, Tokens};

#[path = "launch_strings.rs"]
mod launch_strings;
use launch_strings::Key;

pub enum LaunchEvent {
    Open,
    New(&'static Starter),
    MakeProject(PathBuf, Offer),
}

impl EventEmitter<LaunchEvent> for LaunchView {}

pub struct LaunchView {
    /// Why the last attempt to open something failed.
    error: Option<SharedString>,
    /// A folder of Markdown that failed for having no manifest, and what making it a project would do.
    offer: Option<(PathBuf, Offer)>,
    /// With nothing focused Tab has nowhere to start from, so the card holds the keys.
    focus: FocusHandle,
    /// The offer's row, which gets the keys while it shows.
    offer_focus: FocusHandle,
}

impl LaunchView {
    /// `error` says why what was asked for failed to open, and what that was.
    pub fn new(error: Option<(String, PathBuf)>, cx: &mut Context<Self>) -> Self {
        let (error, asked) = error.unzip();
        let mut view = Self {
            error: None,
            offer: None,
            focus: cx.focus_handle(),
            offer_focus: cx.focus_handle().tab_stop(true),
        };
        view.take_error(error, asked.as_deref());
        view
    }

    /// A folder that only lacks a manifest is no error: the offer's own line says what is missing.
    fn take_error(&mut self, error: Option<String>, asked: Option<&Path>) {
        self.offer = asked.and_then(offer_for);
        self.error = error.filter(|_| self.offer.is_none()).map(SharedString::from);
    }

    /// Gives the keys to the offer if there is one, or the card, as the window shows it.
    pub fn take_focus(view: &gpui::Entity<Self>, window: &mut Window, cx: &mut gpui::App) {
        let view = view.read(cx);
        let focus = if view.offer.is_some() {
            view.offer_focus.clone()
        } else {
            view.focus.clone()
        };
        window.focus(&focus, cx);
    }

    /// Says why `asked` did not open, or with `None`, that nothing is wrong.
    pub fn set_error(&mut self, error: Option<String>, asked: Option<&Path>, cx: &mut Context<Self>) {
        self.take_error(error, asked);
        cx.notify();
    }

    /// Says why the offer did not work. The offer stays, to try again.
    pub fn offer_failed(&mut self, problem: String, cx: &mut Context<Self>) {
        self.error = Some(problem.into());
        cx.notify();
    }

    fn row(
        tokens: &Tokens,
        id: &'static str,
        icon: &'static str,
        name: &'static str,
        description: impl Into<SharedString>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .button(tokens, name)
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_2)
            .flex()
            .items_center()
            .gap(tokens::SPACE_3)
            .cursor_pointer()
            .hover(|row| row.bg(tokens.raised))
            .child(
                svg()
                    .flex_none()
                    .path(icon)
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.text_quiet),
            )
            .child(
                div().flex_1().min_w_0().child(name).child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_quiet)
                        .child(description.into()),
                ),
            )
    }
}

fn offer_for(path: &Path) -> Option<(PathBuf, Offer)> {
    adopt::offer(path).map(|offer| (path.to_owned(), offer))
}

impl Render for LaunchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let starters: Vec<_> = STARTERS
            .iter()
            .map(|starter| {
                let icon = match starter.slug {
                    "book" => "icons/book.svg",
                    "paper" => "icons/contents.svg",
                    _ => "icons/article.svg",
                };
                Self::row(
                    &tokens,
                    starter.slug,
                    icon,
                    crate::t!(starter.name),
                    crate::t!(starter.description),
                )
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(LaunchEvent::New(starter))))
            })
            .collect();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(|_: &crate::workspace::FocusNext, window, cx| crate::a11y::tab(true, window, cx))
            .on_action(|_: &crate::workspace::FocusPrevious, window, cx| crate::a11y::tab(false, window, cx))
            .bg(tokens.canvas)
            .child(
                div()
                    .w(tokens::LAUNCH_WIDTH)
                    .flex()
                    .flex_col()
                    .bg(tokens.canvas)
                    .border_1()
                    .border_color(tokens.divider)
                    .rounded(tokens::RADIUS)
                    .child(
                        div()
                            .px(tokens::SPACE_3)
                            .py(tokens::SPACE_2)
                            .border_b_1()
                            .border_color(tokens.divider)
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_quiet)
                            .child("Druck"),
                    )
                    .when_some(self.offer.clone(), |card, (dir, offer)| {
                        card.child(
                            div()
                                .px(tokens::SPACE_3)
                                .pt(tokens::SPACE_2)
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_quiet)
                                .child(adopt::reason(&dir)),
                        )
                        .child(
                            Self::row(&tokens, "make-project", "icons/book.svg", adopt::label(), offer.hint())
                                .track_focus(&self.offer_focus)
                                .ring_while_focused(&tokens)
                                .border_b_1()
                                .border_color(tokens.divider)
                                .text_color(tokens.accent)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(LaunchEvent::MakeProject(dir.clone(), offer.clone()));
                                })),
                        )
                    })
                    .when_some(self.error.clone(), |card, error| {
                        card.child(div().p(tokens::SPACE_3).text_color(tokens.accent).child(error))
                    })
                    .child(
                        Self::row(
                            &tokens,
                            "open",
                            "icons/file.svg",
                            crate::t!(Key::Open),
                            crate::t!(Key::OpenHint),
                        )
                        .border_b_1()
                        .border_color(tokens.divider)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(LaunchEvent::Open))),
                    )
                    .child(
                        div()
                            .px(tokens::SPACE_3)
                            .pt(tokens::SPACE_3)
                            .pb(tokens::SPACE_1)
                            .text_size(tokens::TEXT_SMALL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens.text_quiet)
                            .child(crate::t!(Key::NewProject)),
                    )
                    .children(starters),
            )
    }
}
