//! Small pieces the shell's panels share, built from the tokens.

use gpui::prelude::*;
use gpui::{ElementId, FontWeight, SharedString, Stateful, div, px};

use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

/// The small label at the top of a panel, on a divider.
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

/// A bordered button with a text label.
pub fn button(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .button(tokens, label.clone())
        .flex_none()
        .h(tokens::FIELD_HEIGHT)
        .px(tokens::SPACE_3)
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .cursor_pointer()
        .hover(|button| button.bg(tokens.raised))
        .child(label)
}

/// A small square switch with its label, filled when on.
pub fn check(
    tokens: &Tokens,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
) -> Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .control(tokens, gpui::Role::CheckBox, label.clone())
        .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
        .flex()
        .items_center()
        .gap(tokens::SPACE_2)
        .cursor_pointer()
        .child(
            div()
                .flex_none()
                .size(px(14.))
                .rounded(px(3.))
                .border_1()
                .border_color(if on { tokens.accent } else { tokens.text_faint })
                .when(on, |square| square.bg(tokens.accent)),
        )
        .child(div().min_w_0().child(label))
}

/// A one-line toggle in a row of options, such as the search panel's: raised when on.
pub fn chip(
    tokens: &Tokens,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
) -> Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .toggle(tokens, label.clone(), on)
        .flex_none()
        .h(tokens::FIELD_HEIGHT)
        .min_w(tokens::FIELD_HEIGHT)
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .border_1()
        .border_color(if on { tokens.accent } else { tokens.divider })
        .text_color(if on { tokens.text } else { tokens.text_quiet })
        .when(on, |chip| chip.bg(tokens.raised))
        .cursor_pointer()
        .hover(|chip| chip.bg(tokens.raised))
        .child(label)
}

/// The dot a problem, a warning, or an open placeholder shows, as in the sidebar.
pub fn dot(tokens: &Tokens, kind: super::report::Kind) -> gpui::Div {
    use super::report::Kind;
    let dot = div().flex_none().size(tokens::DOT_SIZE).rounded_full();
    match kind {
        Kind::Error => dot.bg(tokens.accent),
        Kind::Warning => dot.bg(tokens.text_quiet),
        Kind::Todo => dot.border_1().border_color(tokens.text_faint),
    }
}
