//! What every control gives keyboard and assistive technology. gpui carries roles and labels to the system through
//! AccessKit for elements that have an id, and a focusable element in the tab order takes Enter and Space as a click.
//! Only a keyboard's focus shows: a 2 px accent ring inside the control's edge, over any border it has, which moves nothing, the same on every
//! control.

use gpui::prelude::*;
use gpui::{BoxShadow, Div, Role, SharedString, Stateful, Toggled, point, px};

use crate::tokens::{self, Tokens};

pub trait Control: Sized {
    /// In the tab order, with a role and a label for assistive technology.
    fn control(self, tokens: &Tokens, role: Role, label: impl Into<SharedString>) -> Self;

    /// A button, labelled because its text or icon is not a node of its own.
    fn button(self, tokens: &Tokens, label: impl Into<SharedString>) -> Self {
        self.control(tokens, Role::Button, label)
    }

    /// A button that stays on or off.
    fn toggle(self, tokens: &Tokens, label: impl Into<SharedString>, on: bool) -> Self;
}

impl Control for Stateful<Div> {
    fn control(self, tokens: &Tokens, role: Role, label: impl Into<SharedString>) -> Self {
        let ring = BoxShadow {
            color: tokens.accent.into(),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: tokens::FOCUS_RING,
            inset: true,
        };
        self.role(role)
            .aria_label(label)
            .focusable()
            .tab_stop(true)
            .focus_visible(move |style| style.shadow(vec![ring.clone()]))
    }

    fn toggle(self, tokens: &Tokens, label: impl Into<SharedString>, on: bool) -> Self {
        let toggled = if on { Toggled::True } else { Toggled::False };
        self.control(tokens, Role::Button, label).aria_toggled(toggled)
    }
}
