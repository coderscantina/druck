//! The window with no document: open one, or start one from a starter.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FontWeight, SharedString, Window, div, svg};

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
}

impl LaunchView {
    /// `error` says why what was asked for failed to open, and what that was.
    pub fn new(error: Option<(String, PathBuf)>) -> Self {
        let (error, asked) = error.unzip();
        Self {
            error: error.map(SharedString::from),
            offer: asked.as_deref().and_then(offer_for),
        }
    }

    /// Says why `asked` did not open, or with `None`, that nothing is wrong.
    pub fn set_error(&mut self, error: Option<String>, asked: Option<&Path>, cx: &mut Context<Self>) {
        self.error = error.map(SharedString::from);
        self.offer = asked.and_then(offer_for);
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
            .border_b_1()
            .border_color(tokens.divider)
            .cursor_pointer()
            .hover(|row| row.bg(tokens.raised))
            .child(
                svg()
                    .flex_none()
                    .path(icon)
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.accent),
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
                    _ => "icons/file.svg",
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
            .on_action(|_: &crate::workspace::FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &crate::workspace::FocusPrevious, window, cx| window.focus_prev(cx))
            .bg(tokens.canvas)
            .child(
                div()
                    .w(tokens::LAUNCH_WIDTH)
                    .flex()
                    .flex_col()
                    .bg(tokens.panel)
                    .border_1()
                    .border_color(tokens.divider)
                    .rounded(tokens::RADIUS)
                    .child(
                        div()
                            .p(tokens::SPACE_3)
                            .border_b_1()
                            .border_color(tokens.divider)
                            .child(
                                div()
                                    .text_size(tokens::TEXT_TITLE)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Druck"),
                            )
                            .child(div().text_color(tokens.text_quiet).child(crate::t!(Key::Tagline))),
                    )
                    .child(
                        Self::row(
                            &tokens,
                            "open",
                            "icons/file.svg",
                            crate::t!(Key::Open),
                            crate::t!(Key::OpenHint),
                        )
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(LaunchEvent::Open))),
                    )
                    .child(
                        div()
                            .px(tokens::SPACE_3)
                            .py(tokens::SPACE_2)
                            .border_b_1()
                            .border_color(tokens.divider)
                            .text_size(tokens::TEXT_SMALL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens.text_quiet)
                            .child(crate::t!(Key::NewProject)),
                    )
                    .children(starters)
                    .when_some(self.error.clone(), |card, error| {
                        card.child(div().p(tokens::SPACE_3).text_color(tokens.accent).child(error))
                    })
                    .when_some(self.offer.clone(), |card, (dir, offer)| {
                        card.child(
                            Self::row(&tokens, "make-project", "icons/book.svg", adopt::label(), offer.hint())
                                .text_color(tokens.accent)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(LaunchEvent::MakeProject(dir.clone(), offer.clone()));
                                })),
                        )
                    }),
            )
    }
}
