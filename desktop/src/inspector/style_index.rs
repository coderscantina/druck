//! The Style tab: a searchable index of the theme's 30 sections, then one section at a time with a quiet back link. A
//! section with a designed panel opens in it, and any other as JSON in the same place. A new panel is a line in
//! [`SECTIONS`] and a form in [`super::style_property`].

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Role, SharedString, Subscription, Window, div};
use serde_json::Value;

use super::input::{Cancelled, Edited, TextInput};
use super::model::ThemeModel;
use super::running_heads::RunningHeads;
use super::source_view::SourceView;
use super::strings::{Key, STYLES};
use super::style_property::{CHAPTERS, CONTENTS, Control, Form};
use super::styles::{self, Selected, StylesPanel};
use super::{Tab, geometry::GeometryPanel, theme_tokens::TokensPanel, widgets};
use crate::a11y::Control as _;
use crate::tokens::{self as chrome, Tokens};

/// How a section is edited.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Panel {
    Body,
    Headings,
    BlockStyles,
    CustomStyles,
    Geometry,
    /// The Fonts tab has its own place on the rail.
    Fonts,
    Tokens,
    RunningHeads,
    Form(Form),
    Json,
    /// The whole file.
    Source,
}

/// A line of the index. `key` is the section's name in the theme, which the JSON view opens.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Section {
    key: Option<&'static str>,
    label: Key,
    group: Key,
    panel: Panel,
}

const fn section(key: &'static str, label: Key, group: Key, panel: Panel) -> Section {
    Section {
        key: Some(key),
        label,
        group,
        panel,
    }
}

use Key::{GroupBookParts, GroupNotes, GroupPage, GroupText, GroupTheme};
use Panel::Json;

/// The schema's 30 top-level sections, with body text and headings, the parts of `styles` authors reach for most, and the
/// whole file.
pub const SECTIONS: [Section; 33] = [
    section("page", Key::SectionGeometry, GroupPage, Panel::Geometry),
    section("pages", Key::SectionRunningHeads, GroupPage, Panel::RunningHeads),
    section("title-page", Key::SectionTitlePage, GroupPage, Json),
    section("title-block", Key::SectionTitleBlock, GroupPage, Json),
    section("watermark", Key::SectionWatermark, GroupPage, Json),
    section("placement", Key::SectionPlacement, GroupPage, Json),
    Section {
        key: None,
        label: Key::SectionBody,
        group: GroupText,
        panel: Panel::Body,
    },
    Section {
        key: None,
        label: Key::SectionHeadings,
        group: GroupText,
        panel: Panel::Headings,
    },
    section("styles", Key::SectionStyles, GroupText, Panel::BlockStyles),
    section(
        "custom-styles",
        Key::SectionCustomStyles,
        GroupText,
        Panel::CustomStyles,
    ),
    section("inline", Key::SectionInline, GroupText, Json),
    section("lists", Key::SectionLists, GroupText, Json),
    section("tables", Key::SectionTables, GroupText, Json),
    section("callouts", Key::SectionCallouts, GroupText, Json),
    section("captions", Key::SectionCaptions, GroupText, Json),
    section("highlight", Key::SectionHighlight, GroupText, Json),
    section("typesetting", Key::SectionTypesetting, GroupText, Json),
    section("chapters", Key::SectionChapters, GroupBookParts, Panel::Form(CHAPTERS)),
    section("toc", Key::SectionContents, GroupBookParts, Panel::Form(CONTENTS)),
    section("matter", Key::SectionMatter, GroupBookParts, Json),
    section("scene-break", Key::SectionSceneBreak, GroupBookParts, Json),
    section("bibliography", Key::SectionBibliography, GroupBookParts, Json),
    section("index", Key::SectionIndex, GroupBookParts, Json),
    section("footnotes", Key::SectionFootnotes, GroupNotes, Json),
    section("margin-notes", Key::SectionMarginNotes, GroupNotes, Json),
    section("fonts", Key::SectionFonts, GroupTheme, Panel::Fonts),
    section("tokens", Key::SectionTokens, GroupTheme, Panel::Tokens),
    section("images", Key::SectionImages, GroupTheme, Json),
    section("document", Key::SectionDocument, GroupTheme, Json),
    section("labels", Key::SectionLabels, GroupTheme, Json),
    section("version", Key::SectionVersion, GroupTheme, Json),
    section("$schema", Key::SectionSchema, GroupTheme, Json),
    Section {
        key: None,
        label: Key::SectionSource,
        group: GroupTheme,
        panel: Panel::Source,
    },
];

/// The groups, in the order the index shows them.
const GROUPS: [Key; 5] = [GroupPage, GroupText, GroupBookParts, GroupNotes, GroupTheme];

impl Section {
    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || crate::t!(self.label).to_lowercase().contains(query)
            || self.key.is_some_and(|key| key.to_lowercase().contains(query))
    }

    /// The section's main value, at the end of its row.
    fn value(&self, theme: &Value) -> Option<String> {
        let read = |path: &[&str]| {
            let found = path.iter().try_fold(theme, |value, key| value.get(*key))?;
            Some(styles::resolve_value(theme, found))
        };
        match self.panel {
            Panel::Body => read(&["styles", "body", "size"]),
            Panel::Headings => read(&["styles", "heading-1", "size"]),
            Panel::Geometry => {
                let size = theme.get("page")?.get("size")?;
                match (size.get("width"), size.get("height")) {
                    (Some(width), Some(height)) => Some(format!(
                        "{} × {}",
                        styles::resolve_value(theme, width),
                        styles::resolve_value(theme, height)
                    )),
                    _ => read(&["page", "size"]).map(|paper| paper.to_uppercase()),
                }
            }
            Panel::Form(form) if form == CHAPTERS => {
                let written = read(&["chapters", "break-before"])?;
                match form.fields[0].control {
                    Control::Choice(choices) => choices
                        .iter()
                        .find(|(value, _)| *value == written)
                        .map(|(_, label)| crate::t!(*label).to_owned()),
                    _ => None,
                }
            }
            Panel::Form(form) if form == CONTENTS => read(&["document", "toc-depth"]),
            _ => None,
        }
    }
}

/// What the rail must do for the index.
pub enum IndexEvent {
    ShowTab(Tab),
}

impl gpui::EventEmitter<IndexEvent> for StyleIndex {}

pub struct StyleIndex {
    model: Entity<ThemeModel>,
    search: Entity<TextInput>,
    open: Option<Section>,
    /// The section's JSON instead of its panel.
    as_json: bool,
    heading: usize,
    styles: Entity<StylesPanel>,
    tokens_panel: Entity<TokensPanel>,
    geometry: Entity<GeometryPanel>,
    heads: Entity<RunningHeads>,
    source: Entity<SourceView>,
    _subscriptions: Vec<Subscription>,
}

impl StyleIndex {
    pub fn new(
        model: Entity<ThemeModel>,
        styles: Entity<StylesPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(Key::SearchSections)
                .with_label(Key::SearchSections)
        });
        let tokens_panel = cx.new(|cx| TokensPanel::new(model.clone(), window, cx));
        let geometry = cx.new(|cx| GeometryPanel::new(model.clone(), window, cx));
        let heads = cx.new(|cx| RunningHeads::new(model.clone(), window, cx));
        let source = cx.new(|cx| SourceView::new(model.clone(), window, cx));
        let subscriptions = vec![
            cx.subscribe(&search, |_, _, _: &Edited, cx| cx.notify()),
            cx.subscribe(&search, |_, search, _: &Cancelled, cx| {
                search.update(cx, |search, cx| search.set_value(String::new(), true, cx));
                cx.notify();
            }),
            cx.observe(&model, |_, _, cx| cx.notify()),
        ];
        Self {
            model,
            search,
            open: None,
            as_json: false,
            heading: 1,
            styles,
            tokens_panel,
            geometry,
            heads,
            source,
            _subscriptions: subscriptions,
        }
    }

    /// Opens a section by its name in the theme, or `body`, `headings`, or `source`. For screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn open_section_named(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let found = SECTIONS.iter().find(|section| match section.panel {
            Panel::Body => name == "body",
            Panel::Headings => name == "headings",
            Panel::Source => name == "source",
            _ => section.key == Some(name),
        });
        if let Some(found) = found {
            self.open_section(*found, window, cx);
        }
    }

    /// Opens the section a style belongs to, at that style.
    pub fn open_style(&mut self, style: &str, window: &mut Window, cx: &mut Context<Self>) {
        let of_panel = |panel: Panel| SECTIONS.iter().find(|section| section.panel == panel).copied();
        let level = style.strip_prefix("heading-").and_then(|level| level.parse().ok());
        let section = match (style, level) {
            ("body", _) => of_panel(Panel::Body),
            (_, Some(level)) => {
                self.heading = level;
                of_panel(Panel::Headings)
            }
            _ if STYLES.iter().any(|(key, _)| *key == style) => of_panel(Panel::BlockStyles),
            _ => of_panel(Panel::CustomStyles),
        };
        if let Some(section) = section {
            self.open_section(section, window, cx);
        }
        if !matches!(style, "body") && level.is_none() {
            let selected = styles::selected_of(style);
            self.styles.update(cx, |styles, cx| styles.go_to(selected, window, cx));
        }
    }

    fn open_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.as_json = false;
        match section.panel {
            Panel::Fonts => {
                cx.emit(IndexEvent::ShowTab(Tab::Fonts));
                return;
            }
            Panel::Json => self.show_json(section.key, window, cx),
            Panel::Source => self.show_json(None, window, cx),
            Panel::Body => self.show_style(Selected::Builtin("body"), window, cx),
            Panel::Headings => self.show_heading(self.heading, window, cx),
            Panel::Form(form) => self.show_style(Selected::Form(form), window, cx),
            Panel::BlockStyles => self.styles.update(cx, |styles, cx| styles.show_list(false, window, cx)),
            Panel::CustomStyles => self.styles.update(cx, |styles, cx| styles.show_list(true, window, cx)),
            Panel::Geometry | Panel::Tokens | Panel::RunningHeads => {}
        }
        self.open = Some(section);
        cx.notify();
    }

    fn show_style(&mut self, selected: Selected, window: &mut Window, cx: &mut Context<Self>) {
        self.styles
            .update(cx, |styles, cx| styles.show_root(selected, window, cx));
    }

    fn show_heading(&mut self, level: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.heading = level;
        self.show_style(styles::selected_of(&format!("heading-{level}")), window, cx);
        cx.notify();
    }

    fn show_json(&mut self, section: Option<&'static str>, window: &mut Window, cx: &mut Context<Self>) {
        self.source.update(cx, |source, cx| source.open(section, window, cx));
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        self.as_json = false;
        cx.notify();
    }

    fn row(&self, section: Section, theme: Option<&Value>, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let label = crate::t!(section.label);
        div()
            .id(SharedString::from(format!("section-{label}")))
            .control(tokens, Role::Button, label)
            .flex_none()
            .h(chrome::LIST_ROW_HEIGHT)
            .px(chrome::SPACE_3)
            .flex()
            .items_center()
            .justify_between()
            .gap(chrome::SPACE_2)
            .text_color(tokens.text_quiet)
            .cursor_pointer()
            .hover(|row| row.bg(tokens.raised).text_color(tokens.text))
            .on_click(cx.listener(move |index, _, window, cx| index.open_section(section, window, cx)))
            .child(div().min_w_0().truncate().child(label))
            .when_some(theme.and_then(|theme| section.value(theme)), |row, value| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(chrome::TEXT_SMALL)
                        .text_color(tokens.text_faint)
                        .child(value),
                )
            })
            .into_any_element()
    }

    fn index(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let theme = self.model.read(cx).file().ok().map(|file| file.merged().clone());
        let mut rows: Vec<AnyElement> = Vec::new();
        for group in GROUPS {
            let found: Vec<Section> = SECTIONS
                .iter()
                .filter(|section| section.group == group && section.matches(&query))
                .copied()
                .collect();
            if found.is_empty() {
                continue;
            }
            rows.push(
                div()
                    .flex_none()
                    .px(chrome::SPACE_3)
                    .pt(chrome::SPACE_3)
                    .pb(chrome::SPACE_1)
                    .text_size(chrome::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(crate::t!(group))
                    .into_any_element(),
            );
            for section in found {
                rows.push(self.row(section, theme.as_ref(), tokens, cx));
            }
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(div().flex_none().p(chrome::SPACE_3).child(self.search.clone()))
            .child(
                div()
                    .id("section-index")
                    .flex_1()
                    .min_h_0()
                    .pb(chrome::SPACE_2)
                    .overflow_y_scroll()
                    .children(rows),
            )
    }

    /// The bar over an open section: the way back, what it is, and the way to its JSON.
    fn bar(&self, section: Section, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let json = section.key.filter(|_| section.panel != Panel::Json).map(|key| {
            let (label, on) = if self.as_json {
                (crate::t!(Key::SectionForm), false)
            } else {
                (crate::t!(Key::SectionJson), true)
            };
            widgets::quiet_link(tokens, "section-json", label).on_click(cx.listener(move |index, _, window, cx| {
                index.as_json = on;
                if on {
                    index.show_json(Some(key), window, cx);
                }
                cx.notify();
            }))
        });
        widgets::bar(tokens)
            .justify_between()
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(chrome::SPACE_2)
                    .child(
                        widgets::quiet_link(tokens, "section-back", format!("‹ {}", crate::t!(Key::AllSections)))
                            .on_click(cx.listener(|index, _, _, cx| index.close(cx))),
                    )
                    .child(div().min_w_0().truncate().child(crate::t!(section.label))),
            )
            .children(json)
    }

    fn heading_levels(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let items = (1..=6)
            .map(|level| {
                let label = format!("{level}");
                (
                    div()
                        .id(("heading-level", level))
                        .toggle(tokens, label.clone(), level == self.heading)
                        .child(label)
                        .on_click(cx.listener(move |index, _, window, cx| index.show_heading(level, window, cx))),
                    level == self.heading,
                )
            })
            .collect();
        div()
            .flex_none()
            .px(chrome::SPACE_3)
            .py(chrome::SPACE_2)
            .child(widgets::segmented(tokens, items))
    }

    fn panel(&self, section: Section, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        if self.as_json {
            return self.source.clone().into_any_element();
        }
        match section.panel {
            Panel::Json | Panel::Source => self.source.clone().into_any_element(),
            Panel::Geometry => self.geometry.clone().into_any_element(),
            Panel::Tokens => self.tokens_panel.clone().into_any_element(),
            Panel::RunningHeads => self.heads.clone().into_any_element(),
            Panel::Headings => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.heading_levels(tokens, cx))
                .child(self.styles.clone())
                .into_any_element(),
            Panel::Body | Panel::BlockStyles | Panel::CustomStyles | Panel::Form(_) | Panel::Fonts => {
                self.styles.clone().into_any_element()
            }
        }
    }
}

impl Render for StyleIndex {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let column = div().flex_1().min_w_0().min_h_0().flex().flex_col();
        match self.open {
            None => column.child(self.index(&tokens, cx)).into_any_element(),
            Some(section) => column
                .child(self.bar(section, &tokens, cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(self.panel(section, &tokens, cx)),
                )
                .into_any_element(),
        }
    }
}

/// The fields a section's panel shows, for what asks whether each is in the schema.
#[cfg(test)]
fn form_fields() -> Vec<super::style_property::Property> {
    [CHAPTERS, CONTENTS]
        .iter()
        .flat_map(|form| form.fields.iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn schema() -> Value {
        serde_json::from_str(include_str!("../../../schema/theme.v1.schema.json")).expect("the schema parses")
    }

    #[test]
    fn every_section_of_the_schema_is_in_the_index_once() {
        let schema = schema();
        let in_schema: HashSet<&str> = schema["properties"]
            .as_object()
            .expect("sections")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(in_schema.len(), 30);
        let listed: Vec<&str> = SECTIONS.iter().filter_map(|section| section.key).collect();
        let unique: HashSet<&str> = listed.iter().copied().collect();
        assert_eq!(listed.len(), unique.len());
        assert_eq!(unique, in_schema);
    }

    #[test]
    fn every_section_stands_in_a_group() {
        for section in SECTIONS {
            assert!(GROUPS.contains(&section.group));
        }
    }

    #[test]
    fn search_finds_a_section_by_its_label_or_its_name_in_the_theme() {
        let _guard = crate::shell::strings::LANGUAGE_TEST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::shell::strings::set_language(crate::shell::strings::Language::English);
        let names = |query: &str| {
            SECTIONS
                .iter()
                .filter(|section| section.matches(query))
                .map(|section| crate::t!(section.label))
                .collect::<Vec<_>>()
        };
        assert_eq!(names("running"), ["Running heads"]);
        assert_eq!(names("scene-break"), ["Scene break"]);
        assert_eq!(names("toc"), ["Contents"]);
        assert_eq!(
            SECTIONS.iter().filter(|section| section.matches("")).count(),
            SECTIONS.len()
        );
    }

    #[test]
    fn every_field_of_a_designed_form_is_in_the_schema() {
        let schema = schema();
        for field in form_fields() {
            let section = &schema["properties"][field.path[0]]["properties"];
            assert!(
                section.get(field.path[1]).is_some(),
                "{} is not in the schema",
                field.key()
            );
        }
    }
}
