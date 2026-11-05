//! The small pieces the versions views share, built from the tokens.

use gpui::prelude::*;
use gpui::{ElementId, FontWeight, SharedString, Stateful, div};

use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

/// The small label at the top of a card or panel, on a divider.
pub fn section_label(tokens: &Tokens, label: impl Into<SharedString>) -> gpui::Div {
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
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.text_quiet)
        .child(label.into())
}

/// A quiet group label in a panel.
pub fn group_label(tokens: &Tokens, label: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .px(tokens::SPACE_3)
        .pt(tokens::SPACE_3)
        .pb(tokens::SPACE_1)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_faint)
        .child(label.into())
}

/// A 22 px button with a thin border and quiet text. Nothing is filled.
pub fn button(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .button(tokens, label.clone())
        .flex_none()
        .h(tokens::VERSIONS_BUTTON_HEIGHT)
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .rounded(tokens::SETTINGS_SWATCH_RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .cursor_pointer()
        .hover(|button| button.bg(tokens.raised))
        .child(label)
}

/// A file's row in a list of changes: its title, and quiet words at the end.
pub fn change_row(tokens: &Tokens, title: String, words: String) -> gpui::Div {
    div()
        .flex_none()
        .min_h(tokens::VERSIONS_ROW_HEIGHT)
        .px(tokens::SPACE_3)
        .flex()
        .items_center()
        .justify_between()
        .gap(tokens::SPACE_2)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .child(div().flex_1().min_w_0().truncate().child(title))
        .child(div().flex_none().text_color(tokens.text_faint).child(words))
}

/// A line of text that says something is off, or what to do next, in the accent colour.
pub fn accent_line(tokens: &Tokens, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .px(tokens::SPACE_3)
        .py(tokens::SPACE_1)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.accent)
        .child(text.into())
}

/// A quiet line of explanation.
pub fn quiet_line(tokens: &Tokens, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .px(tokens::SPACE_3)
        .py(tokens::SPACE_1)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .child(text.into())
}
