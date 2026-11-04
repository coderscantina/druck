//! What every control gives keyboard and assistive technology. gpui carries roles and labels to the system through
//! AccessKit for elements that have an id, and a focusable element in the tab order takes Enter and Space as a click.
//! Only a keyboard's focus shows, in the accent color, the same way on every control.

use gpui::prelude::*;
use gpui::{Div, Role, SharedString, Stateful, Toggled};

use crate::tokens::Tokens;

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
        let accent = tokens.accent;
        self.role(role)
            .aria_label(label)
            .focusable()
            .tab_stop(true)
            .focus_visible(move |style| style.border_1().border_color(accent))
    }

    fn toggle(self, tokens: &Tokens, label: impl Into<SharedString>, on: bool) -> Self {
        let toggled = if on { Toggled::True } else { Toggled::False };
        self.control(tokens, Role::Button, label).aria_toggled(toggled)
    }
}
