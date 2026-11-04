//! Right pane: the panel of the active tab, then the tab rail at the window's edge.

pub mod details;
mod fork_prompt;
pub mod input;
mod model;
mod picker;
mod strings;
mod style_property;
mod source_view;
mod styles;
mod theme_tabs;
mod theme_tokens;
mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{AnyView, Context, Entity, EventEmitter, FontWeight, Subscription, Window, div, svg};

pub use fork_prompt::ForkPrompt;
pub use model::{SpecimenRequested, ThemeChanged, ThemeModel, ThemeSwitched};
pub use picker::ThemePicker;
use source_view::SourceView;
use styles::StylesPanel;
use theme_tabs::ThemeTabs;
use theme_tokens::TokensPanel;

use crate::theme_file::editor::Dirs;
use crate::tokens::{self, Tokens};

/// A tab of the rail. A panel takes one with [`InspectorView::mount`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
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

    /// The tab `DRUCK_TAB` names, for screenshots.
    fn from_env() -> Option<Self> {
        let name = std::env::var("DRUCK_TAB").ok()?;
        Self::TOP.into_iter().chain(Self::BOTTOM).find(|tab| tab.label().eq_ignore_ascii_case(&name))
    }

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
    /// The view each tab shows. A tab without one shows its label until its slice mounts a panel.
    panels: HashMap<Tab, AnyView>,
    styles: Entity<StylesPanel>,
    model: Entity<ThemeModel>,
    _subscriptions: Vec<Subscription>,
}

/// The inspector announces what its panels change, for the shell to pass on to the galley and the preview.
impl EventEmitter<ThemeChanged> for InspectorView {}

impl InspectorView {
    /// `theme` is the JSON file the theme tabs edit, and `dirs` the folders themes may be changed in.
    pub fn new(theme: Option<PathBuf>, dirs: Dirs, window: &mut Window, cx: &mut Context<Self>) -> Self {
        input::bind_keys(cx);
        let model = cx.new(|_| ThemeModel::new(theme.as_deref(), dirs));
        let styles = cx.new(|cx| StylesPanel::new(model.clone(), window, cx));
        let tokens_panel = cx.new(|cx| TokensPanel::new(model.clone(), window, cx));
        let source = cx.new(|cx| SourceView::new(model.clone(), window, cx));
        let theme_tabs = cx.new(|_| {
            ThemeTabs::new(vec![
                (strings::STYLES_TITLE, styles.clone().into()),
                (strings::TOKENS_TITLE, tokens_panel.into()),
                (strings::JSON_TITLE, source.into()),
            ])
        });
        let subscriptions = vec![
            cx.subscribe(&model, |_, _, event: &ThemeChanged, cx| cx.emit(event.clone())),
            cx.subscribe(&model, |_, _, _: &ThemeSwitched, cx| cx.notify()),
        ];
        Self {
            active: Tab::from_env().unwrap_or(Tab::Details),
            panels: HashMap::from([(Tab::Styles, theme_tabs.into())]),
            styles,
            model,
            _subscriptions: subscriptions,
        }
    }

    /// The theme the panels edit.
    pub fn model(&self) -> &Entity<ThemeModel> {
        &self.model
    }

    /// Gives a tab its panel.
    pub fn mount(&mut self, tab: Tab, panel: AnyView) {
        self.panels.insert(tab, panel);
    }

    /// Shows a tab.
    pub fn show(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.active = tab;
        cx.notify();
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
        let panel = if let Some(mounted) = self.panels.get(&self.active) {
            panel.child(mounted.clone())
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
        div().flex_none().h_full().flex().child(panel).child(rail)
    }
}
