//! Right pane: the panel of the active tab, then the tab rail at the window's edge.

mod input;
mod strings;
mod style_property;
mod styles;

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, FontWeight, Subscription, Window, div, svg};

use styles::StylesPanel;
pub use styles::ThemeChanged;

use crate::tokens::{self, Tokens};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Selection,
    Styles,
    Fonts,
    Images,
    Details,
    Diagnostics,
    Export,
}

impl Tab {
    const TOP: [Self; 5] = [Self::Selection, Self::Styles, Self::Fonts, Self::Images, Self::Details];
    const BOTTOM: [Self; 2] = [Self::Diagnostics, Self::Export];

    fn id(self) -> &'static str {
        match self {
            Self::Selection => "tab-selection",
            Self::Styles => "tab-styles",
            Self::Fonts => "tab-fonts",
            Self::Images => "tab-images",
            Self::Details => "tab-details",
            Self::Diagnostics => "tab-diagnostics",
            Self::Export => "tab-export",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Selection => "icons/selection.svg",
            Self::Styles => "icons/styles.svg",
            Self::Fonts => "icons/fonts.svg",
            Self::Images => "icons/images.svg",
            Self::Details => "icons/details.svg",
            Self::Diagnostics => "icons/diagnostics.svg",
            Self::Export => "icons/export.svg",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Selection => "SELECTION",
            Self::Styles => "STYLES",
            Self::Fonts => "FONTS",
            Self::Images => "IMAGES",
            Self::Details => "DETAILS",
            Self::Diagnostics => "DIAGNOSTICS",
            Self::Export => "EXPORT",
        }
    }
}

pub struct InspectorView {
    active: Tab,
    styles: Entity<StylesPanel>,
    _subscription: Subscription,
}

/// The inspector announces what its panels change, for the shell to pass on to the galley and the preview.
impl EventEmitter<ThemeChanged> for InspectorView {}

impl InspectorView {
    /// `theme` is the JSON file the Styles tab edits.
    pub fn new(theme: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let styles = cx.new(|cx| StylesPanel::new(theme, window, cx));
        let subscription = cx.subscribe(&styles, |_, _, event: &ThemeChanged, cx| cx.emit(event.clone()));
        Self {
            active: Tab::Styles,
            styles,
            _subscription: subscription,
        }
    }

    /// Sets the body size as if typed into its field. For measurements.
    pub fn set_body_size(&mut self, text: &str, cx: &mut Context<Self>) {
        self.styles.update(cx, |styles, cx| styles.set_body_size(text, cx));
    }

    fn rail_button(&self, tab: Tab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx);
        let active = tab == self.active;
        div()
            .id(tab.id())
            .flex_none()
            .h(tokens::RAIL_WIDTH)
            .flex()
            .items_center()
            .justify_center()
            .when(active, |button| button.bg(tokens.raised))
            .hover(|button| button.bg(tokens.raised))
            .child(svg().path(tab.icon()).size(tokens::ICON_SIZE).text_color(if active {
                tokens.accent
            } else {
                tokens.text_quiet
            }))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.active = tab;
                cx.notify();
            }))
    }
}

impl Render for InspectorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top: Vec<_> = Tab::TOP.iter().map(|&tab| self.rail_button(tab, cx)).collect();
        let bottom: Vec<_> = Tab::BOTTOM.iter().map(|&tab| self.rail_button(tab, cx)).collect();
        let tokens = Tokens::get(cx);
        let panel = div()
            .flex_none()
            .w(tokens::INSPECTOR_WIDTH)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_l_1()
            .border_color(tokens.divider);
        let panel = if self.active == Tab::Styles {
            panel.child(self.styles.clone())
        } else {
            panel.child(
                div()
                    .flex_none()
                    .h(tokens::SECTION_HEIGHT)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(tokens.divider)
                    .text_size(tokens::TEXT_SMALL)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.text_quiet)
                    .child(self.active.label()),
            )
        };
        let rail = div()
            .flex_none()
            .w(tokens::RAIL_WIDTH)
            .flex()
            .flex_col()
            .justify_between()
            .bg(tokens.panel)
            .border_l_1()
            .border_color(tokens.divider)
            .child(div().flex().flex_col().children(top))
            .child(div().flex().flex_col().children(bottom));
        div().flex_none().flex().child(panel).child(rail)
    }
}
