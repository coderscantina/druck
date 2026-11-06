//! What every control gives keyboard and assistive technology. gpui carries roles and labels to the system through
//! AccessKit for elements that have an id, and a focusable element in the tab order takes Enter and Space as a click.
//! Only a keyboard's focus shows: a 2 px accent ring inside the control's edge, over any border it has, which moves nothing, the same on every
//! control.

use gpui::prelude::*;
use gpui::{App, BoxShadow, Div, FocusHandle, Global, Rgba, Role, SharedString, Stateful, Toggled, Window, point, px};

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

    /// The ring shows whenever the control has the keys, also when the app gave them to it as a view opened, since Enter
    /// acts there. gpui shows a keyboard's focus only after a key, and hides it when the pointer moves.
    fn ring_while_focused(self, tokens: &Tokens) -> Self;
}

/// Draws the keyboard's focus ring on `element`. A border paints over the ring, so it takes the accent too, and a
/// bordered control shows the same 2 px as one without.
pub(crate) fn ringed<E: Styled>(element: E, accent: Rgba) -> E {
    element.shadow(vec![ring_of(accent)]).border_color(accent)
}

/// The keyboard's focus ring, for a control that draws it itself.
pub(crate) fn ring(tokens: &Tokens) -> BoxShadow {
    ring_of(tokens.accent)
}

fn ring_of(accent: Rgba) -> BoxShadow {
    BoxShadow {
        color: accent.into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: tokens::FOCUS_RING,
        inset: true,
    }
}

impl Control for Stateful<Div> {
    fn control(self, tokens: &Tokens, role: Role, label: impl Into<SharedString>) -> Self {
        let accent = tokens.accent;
        self.role(role)
            .aria_label(label)
            .focusable()
            .tab_stop(true)
            .focus_visible(move |style| ringed(style, accent))
    }

    fn ring_while_focused(self, tokens: &Tokens) -> Self {
        let accent = tokens.accent;
        self.focus(move |style| ringed(style, accent))
    }

    fn toggle(self, tokens: &Tokens, label: impl Into<SharedString>, on: bool) -> Self {
        let toggled = if on { Toggled::True } else { Toggled::False };
        self.control(tokens, Role::Button, label).aria_toggled(toggled)
    }
}

/// The stop Tab or Shift+Tab last gave the keys. A field shows its ring only there, not when a view gives it the keys
/// as it opens, which gpui would also count as a keyboard's focus.
#[derive(Default)]
struct Tabbed(Option<FocusHandle>);

impl Global for Tabbed {}

/// Moves the keys to the next stop in the window, or the previous one.
pub fn tab(forward: bool, window: &mut Window, cx: &mut App) {
    if forward {
        window.focus_next(cx);
    } else {
        window.focus_prev(cx);
    }
    cx.set_global(Tabbed(window.focused(cx)));
}

/// Whether Tab gave `focus` the keys.
pub(crate) fn tabbed_to(focus: &FocusHandle, cx: &App) -> bool {
    cx.try_global::<Tabbed>().and_then(|tabbed| tabbed.0.as_ref()) == Some(focus)
}

/// Forgets that Tab gave `focus` the keys, once it has lost them, so a view that gives them back shows no ring.
pub(crate) fn untab(focus: &FocusHandle, cx: &mut App) {
    if tabbed_to(focus, cx) {
        cx.set_global(Tabbed(None));
    }
}

/// Moves the keys to the next stop inside `card`, or the previous one, around its ends, so Tab stays in a card that
/// asks. `card` is the handle its outer element tracks.
pub fn cycle_within(card: &FocusHandle, forward: bool, window: &mut Window, cx: &mut App) {
    // The window's stops form one loop, so this comes back into the card unless it has none.
    const MOST_STOPS: usize = 1000;
    for _ in 0..MOST_STOPS {
        tab(forward, window, cx);
        if card.contains_focused(window, cx) {
            return;
        }
    }
}
