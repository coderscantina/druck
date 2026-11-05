//! The small pieces the theme panels share: headers, rows, buttons, and the segmented choice. They take what to show
//! and return an element for the panel to attach its listeners to.

use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, SharedString, Stateful, div};

use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

/// A panel's top bar: the small section label on a divider.
pub fn header(tokens: &Tokens, title: impl Into<SharedString>) -> Div {
    bar(tokens).child(title.into())
}

/// The bar of a header, with nothing in it yet.
pub fn bar(tokens: &Tokens) -> Div {
    div()
        .flex_none()
        .h(tokens::SECTION_HEIGHT)
        .px(tokens::SPACE_3)
        .flex()
        .items_center()
        .gap(tokens::SPACE_2)
        .border_b_1()
        .border_color(tokens.divider)
        .text_size(tokens::TEXT_SMALL)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.text_quiet)
}

/// A label inside a panel, on a divider, as the details panel has it.
pub fn section(tokens: &Tokens, title: impl Into<SharedString>) -> Div {
    header(tokens, title).border_t_1().mt(tokens::SPACE_2)
}

/// A line of the list: the label, and a quiet tag at the end.
pub fn row(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .button(tokens, label.clone())
        .flex_none()
        .h(tokens::ROW_HEIGHT)
        .px(tokens::SPACE_3)
        .flex()
        .items_center()
        .justify_between()
        .gap(tokens::SPACE_2)
        .border_b_1()
        .border_color(tokens.divider)
        .cursor_pointer()
        .hover(|row| row.bg(tokens.raised))
        .child(div().min_w_0().truncate().child(label))
}

/// A quiet tag, in full ink when the theme itself sets what it stands for.
pub fn tag(tokens: &Tokens, text: impl Into<SharedString>, own: bool) -> Div {
    div()
        .flex_none()
        .text_size(tokens::TEXT_SMALL)
        .text_color(if own { tokens.text } else { tokens.text_faint })
        .child(text.into())
}

/// A small text button in the accent color, like the Reset link.
pub fn link(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .control(tokens, gpui::Role::Link, label.clone())
        .flex_none()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.accent)
        .cursor_pointer()
        .hover(|link| link.opacity(0.8))
        .child(label)
}

/// A small text button in the quiet color, for the way back or to another view.
pub fn quiet_link(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    let label = label.into();
    let hover = tokens.text;
    div()
        .id(id)
        .control(tokens, gpui::Role::Link, label.clone())
        .flex_none()
        .text_size(tokens::TEXT_SMALL)
        .font_weight(FontWeight::NORMAL)
        .text_color(tokens.text_quiet)
        .cursor_pointer()
        .hover(move |link| link.text_color(hover))
        .child(label)
}

/// A bordered button with a label, for actions.
pub fn button(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
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
        .text_color(tokens.text)
        .cursor_pointer()
        .hover(|button| button.bg(tokens.raised))
        .child(label)
}

pub fn error_line(tokens: &Tokens, message: impl Into<SharedString>) -> Div {
    div()
        .pt(tokens::SPACE_1)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.accent)
        .child(message.into())
}

pub fn note(tokens: &Tokens, message: impl Into<SharedString>) -> Div {
    div()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_faint)
        .child(message.into())
}

/// A field's label above its control, with `end` at the label row's end.
pub fn field(tokens: &Tokens, label: impl Into<SharedString>, end: impl IntoElement, control: impl IntoElement) -> Div {
    div()
        .px(tokens::SPACE_3)
        .pb(tokens::SPACE_2)
        .flex()
        .flex_col()
        .child(
            div()
                .h(tokens::ROW_HEIGHT)
                .flex()
                .items_center()
                .justify_between()
                .gap(tokens::SPACE_2)
                .text_color(tokens.text_quiet)
                .child(div().min_w_0().truncate().child(label.into()))
                .child(end),
        )
        .child(control)
}

/// A bordered group of choices, one active.
pub fn segmented(tokens: &Tokens, items: Vec<(Stateful<Div>, bool)>) -> Div {
    let children = items.into_iter().map(|(item, active)| {
        item.flex_1()
            .h(tokens::FIELD_HEIGHT)
            .flex()
            .items_center()
            .justify_center()
            .text_size(tokens::TEXT_SMALL)
            .cursor_pointer()
            .when(active, |button| button.bg(tokens.raised).text_color(tokens.text))
            .when(!active, |button| {
                button.text_color(tokens.text_quiet).hover(|b| b.bg(tokens.raised))
            })
    });
    div()
        .flex()
        .rounded(tokens::RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .overflow_hidden()
        .children(children)
}

/// An item of an open list: a label, and a quiet detail.
pub fn menu_item(
    tokens: &Tokens,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    detail: Option<String>,
    active: bool,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .control(tokens, gpui::Role::MenuItem, label.clone())
        .flex_none()
        .h(tokens::FIELD_HEIGHT)
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .justify_between()
        .gap(tokens::SPACE_2)
        .cursor_pointer()
        .hover(|item| item.bg(tokens.raised))
        .text_color(if active { tokens.text } else { tokens.text_quiet })
        .child(div().min_w_0().truncate().child(label))
        .when_some(detail, |item, detail| {
            item.child(
                div()
                    .flex_none()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(detail),
            )
        })
}

/// The frame around an open list.
pub fn menu(tokens: &Tokens, items: Vec<Stateful<Div>>) -> Stateful<Div> {
    div()
        .id("menu")
        .mt(tokens::SPACE_1)
        .max_h(tokens::DIALOG_LIST_HEIGHT / 2.)
        .overflow_y_scroll()
        .rounded(tokens::RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .bg(tokens.field)
        .flex()
        .flex_col()
        .children(items)
}
