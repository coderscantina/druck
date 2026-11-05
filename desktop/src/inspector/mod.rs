//! Publish's right side: the panel of the active tab, then the tab rail at the window's edge.

pub mod details;
mod fonts;
mod fork_prompt;
mod geometry;
pub mod input;
mod model;
mod picker;
mod running_heads;
mod source_view;
mod strings;
mod style_index;
mod style_property;
mod styles;
mod theme_tokens;
pub(crate) mod widgets;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{AnyView, Context, Entity, EventEmitter, Role, Subscription, Window, div, px, svg};

use fonts::FontsPanel;
pub use fork_prompt::ForkPrompt;
pub use model::{SpecimenRequested, ThemeChanged, ThemeModel, ThemeSwitched};
pub use picker::ThemePicker;
use style_index::{IndexEvent, StyleIndex};
use styles::StylesPanel;

use self::strings::Key;
use crate::a11y::Control as _;
use crate::theme_file::editor::Dirs;
use crate::tokens::{self, Tokens};

/// A style's name as the author reads it.
pub fn style_title(style: &str) -> String {
    style_property::style_label(style).to_owned()
}

/// The keys of the block styles the Styles tab lists.
#[cfg(test)]
pub fn style_keys() -> impl Iterator<Item = &'static str> {
    strings::STYLES.iter().map(|(key, _)| *key)
}

/// A tab of the rail. A panel takes one with [`InspectorView::mount`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Details,
    Styles,
    Fonts,
    Images,
    Agent,
    Git,
    Proof,
    Export,
}

impl Tab {
    const TOP: [Self; 4] = [Self::Details, Self::Styles, Self::Fonts, Self::Images];
    const BOTTOM: [Self; 4] = [Self::Agent, Self::Git, Self::Proof, Self::Export];

    /// The tab `DRUCK_TAB` names, for screenshots.
    fn from_env() -> Option<Self> {
        let name = crate::gate::var("DRUCK_TAB")?;
        Self::TOP
            .into_iter()
            .chain(Self::BOTTOM)
            .find(|tab| tab.id().strip_prefix("tab-") == Some(name.as_str()))
    }

    fn id(self) -> &'static str {
        match self {
            Self::Styles => "tab-styles",
            Self::Fonts => "tab-fonts",
            Self::Images => "tab-images",
            Self::Details => "tab-details",
            Self::Proof => "tab-proof",
            Self::Git => "tab-git",
            Self::Export => "tab-export",
            Self::Agent => "tab-agent",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Styles => "icons/styles.svg",
            Self::Fonts => "icons/fonts.svg",
            Self::Images => "icons/images.svg",
            Self::Details => "icons/details.svg",
            Self::Proof => "icons/diagnostics.svg",
            Self::Git => "icons/git.svg",
            Self::Export => "icons/export.svg",
            Self::Agent => "icons/agent.svg",
        }
    }

    fn label(self) -> &'static str {
        crate::t!(match self {
            Self::Styles => Key::TabStyles,
            Self::Fonts => Key::TabFonts,
            Self::Images => Key::TabImages,
            Self::Details => Key::TabDetails,
            Self::Proof => Key::TabProof,
            Self::Git => Key::TabGit,
            Self::Export => Key::TabExport,
            Self::Agent => Key::TabAgent,
        })
    }
}

pub struct InspectorView {
    active: Tab,
    /// The view each tab shows. A tab shows on the rail once its slice has mounted a panel with [`Self::mount`].
    panels: HashMap<Tab, AnyView>,
    /// Tabs that want the author's eye while another tab is open, shown as a dot on the rail.
    attention: HashSet<Tab>,
    styles: Entity<StylesPanel>,
    index: Entity<StyleIndex>,
    fonts: Entity<FontsPanel>,
    model: Entity<ThemeModel>,
    /// The theme picker, at the top of the Styles tab.
    picker: Option<Entity<ThemePicker>>,
    _subscriptions: Vec<Subscription>,
}

/// The inspector announces what its panels change, for the shell to pass on to the galley and the preview.
impl EventEmitter<ThemeChanged> for InspectorView {}

impl InspectorView {
    /// `theme` is the JSON file the theme tabs edit, and `dirs` the folders themes may be changed in.
    pub fn new(theme: Option<PathBuf>, dirs: Dirs, window: &mut Window, cx: &mut Context<Self>) -> Self {
        input::bind_keys(cx);
        let model = cx.new(|cx| ThemeModel::new(theme.as_deref(), dirs, cx));
        let styles = cx.new(|cx| StylesPanel::new(model.clone(), window, cx));
        let index = cx.new(|cx| StyleIndex::new(model.clone(), styles.clone(), window, cx));
        let fonts = cx.new(|cx| FontsPanel::new(model.clone(), window, cx));
        let subscriptions = vec![
            cx.subscribe(&model, |_, _, event: &ThemeChanged, cx| cx.emit(event.clone())),
            cx.subscribe(&model, |_, _, _: &ThemeSwitched, cx| cx.notify()),
            cx.subscribe(&index, |inspector, _, IndexEvent::ShowTab(tab), cx| {
                inspector.show(*tab, cx)
            }),
        ];
        // `DRUCK_PROBE_EDIT=1` makes one edit at the start, for screenshots of what an edit asks.
        if crate::gate::var("DRUCK_PROBE_EDIT").is_some() {
            let probe = model.clone();
            cx.defer(move |cx| {
                probe.update(cx, |model, cx| {
                    model
                        .change(
                            crate::theme_file::editor::Change::set(&["styles", "body", "size"], "11pt".into()),
                            cx,
                        )
                        .ok();
                });
            });
        }
        Self {
            active: Tab::from_env().unwrap_or(Tab::Details),
            panels: HashMap::from([(Tab::Styles, index.clone().into()), (Tab::Fonts, fonts.clone().into())]),
            attention: HashSet::new(),
            styles,
            index,
            fonts,
            model,
            picker: None,
            _subscriptions: subscriptions,
        }
    }

    /// Gives the Fonts tab the families the pages use.
    pub fn set_fonts(&mut self, fonts: Vec<crate::preview::FontUse>, cx: &mut Context<Self>) {
        self.fonts.update(cx, |panel, cx| panel.set_fonts(fonts, cx));
    }

    /// The theme the panels edit.
    pub fn model(&self) -> &Entity<ThemeModel> {
        &self.model
    }

    /// Gives a tab its panel.
    pub fn mount(&mut self, tab: Tab, panel: AnyView) {
        self.panels.insert(tab, panel);
    }

    /// Puts a dot on a tab's rail button, or takes it off.
    pub fn set_attention(&mut self, tab: Tab, wanted: bool, cx: &mut Context<Self>) {
        let changed = if wanted {
            self.attention.insert(tab)
        } else {
            self.attention.remove(&tab)
        };
        if changed {
            cx.notify();
        }
    }

    /// Opens the theme picker's menu on the Styles tab.
    pub fn choose_theme(&mut self, cx: &mut Context<Self>) {
        self.show(Tab::Styles, cx);
        if let Some(picker) = &self.picker {
            picker.update(cx, |picker, cx| picker.open_menu(cx));
        }
    }

    /// Shows a tab.
    pub fn show(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.active = tab;
        cx.notify();
    }

    /// Shows a style's place in the Style tab: its own section, or its row's detail.
    pub fn open_style(&mut self, style: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Tab::Styles, cx);
        self.index.update(cx, |index, cx| index.open_style(style, window, cx));
    }

    /// Reads the theme file again after it changed outside the app.
    pub fn reload_theme(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.reload(cx));
    }

    /// Sets the body size as if typed into its field. For measurements.
    pub fn set_body_size(&mut self, text: &str, cx: &mut Context<Self>) {
        self.styles.update(cx, |styles, cx| styles.set_body_size(text, cx));
    }

    fn rail_button(&self, tab: Tab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx);
        let active = tab == self.active;
        let attention = self.attention.contains(&tab) && !active;
        div()
            .id(tab.id())
            .control(tokens, Role::Tab, tab.label())
            .aria_selected(active)
            .relative()
            .flex_none()
            .h(tokens::RAIL_WIDTH)
            .flex()
            .items_center()
            .justify_center()
            .when(active, |button| button.bg(tokens.raised).rounded_r(tokens::RADIUS))
            .hover(|button| button.bg(tokens.raised))
            .child(svg().path(tab.icon()).size(tokens::ICON_SIZE).text_color(if active {
                tokens.text
            } else {
                tokens.text_quiet
            }))
            .when(attention, |button| {
                button.child(
                    div()
                        .absolute()
                        .top(px(10.))
                        .right(px(10.))
                        .size(tokens::DOT_SIZE)
                        .rounded_full()
                        .bg(tokens.accent),
                )
            })
            .on_click(cx.listener(move |view, _, _, cx| {
                view.active = tab;
                cx.notify();
            }))
    }
}

impl Render for InspectorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mounted = |tabs: &[Tab]| -> Vec<Tab> {
            tabs.iter()
                .copied()
                .filter(|tab| self.panels.contains_key(tab))
                .collect()
        };
        let (top, bottom) = (mounted(&Tab::TOP), mounted(&Tab::BOTTOM));
        let top: Vec<_> = top.into_iter().map(|tab| self.rail_button(tab, cx)).collect();
        let bottom: Vec<_> = bottom.into_iter().map(|tab| self.rail_button(tab, cx)).collect();
        let tokens = Tokens::get(cx);
        let panel = div()
            .id("inspector-panel")
            .role(Role::TabPanel)
            .aria_label(self.active.label())
            .flex_none()
            .w(tokens::INSPECTOR_WIDTH)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_l_1()
            .border_color(tokens.divider);
        let panel = panel.when(self.active == Tab::Styles, |panel| panel.children(self.picker.clone()));
        let panel = panel.children(self.panels.get(&self.active).cloned());
        let rail = div()
            .id("inspector-rail")
            .role(Role::TabList)
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

/// Mounts the theme picker at the top of the Styles tab and the question about common themes over the window.
pub fn mount_theme(parts: &crate::workspace::Parts, slots: &mut crate::workspace::Slots, cx: &mut gpui::App) {
    let model = parts.inspector.read(cx).model().clone();
    let picker = cx.new(|cx| ThemePicker::new(parts.editor.clone(), parts.preview.clone(), model.clone(), cx));
    parts
        .inspector
        .update(cx, |inspector, _| inspector.picker = Some(picker));
    slots
        .overlays
        .push(cx.new(|cx| ForkPrompt::new(model, parts.editor.clone(), cx)).into());
}
