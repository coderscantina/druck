//! The Styles tab holds the theme's panels side by side: styles, tokens, the page, and the source view for the rest.
//! A strip at the top switches between them.

use gpui::prelude::*;
use gpui::{AnyView, Context, Role, SharedString, Window, div};

use super::Key;
use super::widgets;
use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

pub struct ThemeTabs {
    active: usize,
    panels: Vec<(Key, AnyView)>,
}

impl ThemeTabs {
    pub fn new(panels: Vec<(Key, AnyView)>) -> Self {
        // `DRUCK_SUBTAB` picks the panel for screenshots.
        let active = crate::gate::var("DRUCK_SUBTAB")
            .and_then(|index| index.parse().ok())
            .filter(|index| *index < panels.len())
            .unwrap_or(0);
        Self { active, panels }
    }

    /// Shows the panel at `index`.
    pub fn show(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.panels.len() {
            self.active = index;
            cx.notify();
        }
    }
}

impl Render for ThemeTabs {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let items = self
            .panels
            .iter()
            .enumerate()
            .map(|(index, (label, _))| {
                (
                    div()
                        .id(SharedString::from(format!("theme-tab-{index}")))
                        .control(&tokens, Role::Tab, *label)
                        .aria_selected(index == self.active)
                        .child(*label)
                        .on_click(cx.listener(move |tabs, _, _, cx| tabs.show(index, cx))),
                    index == self.active,
                )
            })
            .collect();
        let panel = self.panels.get(self.active).map(|(_, panel)| panel.clone());
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(tokens::SPACE_3)
                    .py(tokens::SPACE_2)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .child(widgets::segmented(&tokens, items)),
            )
            .children(panel.map(|panel| div().flex_1().min_h_0().flex().flex_col().child(panel)))
    }
}
