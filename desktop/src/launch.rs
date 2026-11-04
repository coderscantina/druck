//! The window with no document: open one, or start one from a starter.

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FontWeight, SharedString, Window, div, svg};

use crate::session::{STARTERS, Starter};
use crate::tokens::{self, Tokens};

#[path = "launch_strings.rs"]
mod launch_strings;
use launch_strings::Key;

pub enum LaunchEvent {
    Open,
    New(&'static Starter),
}

impl EventEmitter<LaunchEvent> for LaunchView {}

pub struct LaunchView {
    /// Why the last attempt to open something failed.
    error: Option<SharedString>,
}

impl LaunchView {
    pub fn new(error: Option<String>) -> Self {
        Self {
            error: error.map(SharedString::from),
        }
    }

    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.error = error.map(SharedString::from);
        cx.notify();
    }

    fn row(
        tokens: &Tokens,
        id: &'static str,
        icon: &'static str,
        name: &'static str,
        description: &'static str,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
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
                        .child(description),
                ),
            )
    }
}

impl Render for LaunchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let starters: Vec<_> = STARTERS
            .iter()
            .map(|starter| {
                let icon = match starter.slug {
                    "book" => "icons/book.svg",
                    "report" => "icons/contents.svg",
                    _ => "icons/file.svg",
                };
                Self::row(&tokens, starter.slug, icon, starter.name, starter.description)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(LaunchEvent::New(starter))))
            })
            .collect();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
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
                    }),
            )
    }
}
