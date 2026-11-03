//! Left pane: the document's identity, then its contents list.

use gpui::prelude::*;
use gpui::{Context, FontWeight, SharedString, Window, div};

use crate::document::Opened;
use crate::tokens::{self, Tokens};

pub struct SidebarView {
    title: SharedString,
    authors: SharedString,
}

impl SidebarView {
    pub fn new(opened: Option<Opened>) -> Self {
        let (title, authors) = opened.map_or_else(
            || ("No document".to_owned(), String::new()),
            |opened| (opened.title, opened.authors),
        );
        Self {
            title: title.into(),
            authors: authors.into(),
        }
    }
}

impl Render for SidebarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .flex_none()
            .w(tokens::SIDEBAR_WIDTH)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_r_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .flex_none()
                    .p(tokens::SPACE_3)
                    .bg(tokens.raised)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .text_color(tokens.text_quiet)
                            .truncate()
                            .child(self.authors.clone()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .h(tokens::SECTION_HEIGHT)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .border_y_1()
                    .border_color(tokens.divider)
                    .text_size(tokens::TEXT_SMALL)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.text_quiet)
                    .child("CONTENTS"),
            )
    }
}
