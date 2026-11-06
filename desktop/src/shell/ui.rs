//! Small pieces the shell's panels share, built from the tokens.

use gpui::prelude::*;
use gpui::{
    ElementId, FontWeight, Hsla, Pixels, ScrollHandle, SharedString, Stateful, TextRun, Window, div, font,
    linear_color_stop, linear_gradient,
};

use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

/// Whether a list that scrolls has more below what shows.
pub fn more_below(scroll: &ScrollHandle) -> bool {
    let (offset, most) = (scroll.offset().y, scroll.max_offset().y);
    most > tokens::HAIRLINE && -offset < most
}

/// The fade over the foot of a list on the panel colour that has more below. Its parent is `relative`.
pub fn foot_fade(tokens: &Tokens) -> gpui::Div {
    foot_fade_on(tokens.panel)
}

/// The fade over the foot of a list on `ground` that has more below. Its parent is `relative`.
pub fn foot_fade_on(ground: impl Into<Hsla>) -> gpui::Div {
    let solid: Hsla = ground.into();
    let clear = solid.opacity(tokens::CLEAR);
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
        ))
}

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

/// A 22 px button with a thin border and quiet text.
pub fn small_button(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .button(tokens, label.clone())
        .flex_none()
        .h(tokens::SMALL_BUTTON_HEIGHT)
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .rounded(tokens::CHIP_RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
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
                .size(tokens::CHECK_SIZE)
                .rounded(tokens::BOX_RADIUS)
                .border_1()
                .border_color(if on { tokens.text_quiet } else { tokens.text_faint })
                .when(on, |square| square.bg(tokens.text_quiet)),
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
        .border_color(tokens.divider)
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

/// How wide `text` is at the small size in the app's font, for a line that drops words it has no room for.
pub fn small_text_width(text: &str, window: &Window) -> Pixels {
    text_width(text, tokens::TEXT_SMALL, window)
}

/// How wide `text` is at `size` in the app's font.
pub fn text_width(text: &str, size: Pixels, window: &Window) -> Pixels {
    let run = TextRun {
        len: text.len(),
        font: font(tokens::FONT_UI),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    (window.text_system())
        .shape_line(text.to_owned().into(), size, &[run], None)
        .width
}

/// A small squared switch, drawn only: on is the raised grey with a light knob at the right, off an outline with a faint
/// knob at the left. The control around it takes the click and the keys.
pub fn switch_track(tokens: &Tokens, on: bool) -> gpui::Div {
    let inset = (tokens::SWITCH_HEIGHT - tokens::SWITCH_KNOB) / 2.;
    div()
        .flex_none()
        .relative()
        .w(tokens::SWITCH_WIDTH)
        .h(tokens::SWITCH_HEIGHT)
        .rounded(tokens::SWITCH_RADIUS)
        .when(on, |switch| switch.bg(tokens.switch_on))
        .when(!on, |switch| switch.border_1().border_color(tokens.divider))
        .child(
            div()
                .absolute()
                .top(inset)
                .when(on, |knob| knob.right(inset))
                .when(!on, |knob| knob.left(inset))
                .size(tokens::SWITCH_KNOB)
                .rounded(tokens::SWITCH_KNOB_RADIUS)
                .bg(if on { tokens.switch_knob } else { tokens.text_faint }),
        )
}
