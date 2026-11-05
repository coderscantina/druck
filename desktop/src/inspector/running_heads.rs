//! The Running heads section: what stands in the header and the footer of each kind of page. A theme replaces a band's
//! list as a whole, so an edit writes that one list and leaves everything else in the file as it was.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Focusable, SharedString, Subscription, Window, div};
use serde_json::{Map, Value, json};

use super::input::{Committed, TextInput};
use super::model::ThemeModel;
use super::strings::Key;
use super::widgets;
use crate::a11y::Control as _;
use crate::theme_file::editor::Change;
use crate::tokens::{self as chrome, Tokens};

/// The page variants, most used first, as the schema names them.
const VARIANTS: [(&str, Key); 6] = [
    ("odd", Key::PagesOdd),
    ("even", Key::PagesEven),
    ("opening", Key::PagesOpening),
    ("first", Key::PagesFirst),
    ("body", Key::PagesBody),
    ("title", Key::PagesTitle),
];
const BANDS: [(&str, Key); 2] = [("header", Key::BandHeader), ("footer", Key::BandFooter)];
const ALIGNS: [(&str, Key); 3] = [
    ("left", Key::AlignLeft),
    ("center", Key::AlignCenter),
    ("right", Key::AlignRight),
];
/// The groups a band shows fields for. A band with more keeps them, and they open as JSON.
const FIELDS_PER_BAND: usize = 3;

/// A band's groups as the theme has them, read from the merged theme. A missing or null band has none.
fn groups_of(theme: &Value, variant: &str, band: &str) -> Vec<Value> {
    theme
        .get("pages")
        .and_then(|pages| pages.get(variant))
        .and_then(|variant| variant.get(band))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// The first slot's text of a group.
fn text_of(group: &Value) -> &str {
    group
        .get("slots")
        .and_then(|slots| slots.get(0))
        .and_then(|slot| slot.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// A group with its first slot's text changed, and everything else as it was.
fn with_text(group: &Value, text: &str) -> Value {
    let mut group = group.clone();
    match group.pointer_mut("/slots/0/text") {
        Some(slot) => *slot = Value::String(text.to_owned()),
        None => group["slots"] = json!([{ "text": text }]),
    }
    group
}

/// A group with its alignment changed.
fn with_align(group: &Value, align: &str) -> Value {
    let mut group = group.clone();
    if let Value::Object(fields) = &mut group {
        fields.insert("align".to_owned(), Value::String(align.to_owned()));
    }
    group
}

/// A new group on the band's baseline, centred.
fn new_group(band: &str) -> Value {
    let anchor = if band == "header" { "bottom-left" } else { "top-left" };
    let mut group = Map::new();
    group.insert("anchor".to_owned(), json!(anchor));
    group.insert("align".to_owned(), json!("center"));
    group.insert("slots".to_owned(), json!([{ "text": "{page}" }]));
    Value::Object(group)
}

pub struct RunningHeads {
    model: Entity<ThemeModel>,
    variant: usize,
    /// One field per band and group, reused as the view changes.
    inputs: Vec<Entity<TextInput>>,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl RunningHeads {
    pub fn new(model: Entity<ThemeModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![cx.observe_in(&model, window, |panel, _, window, cx| panel.sync(window, cx))];
        let mut inputs = Vec::new();
        for band in 0..BANDS.len() {
            for group in 0..FIELDS_PER_BAND {
                let input = cx.new(|cx| TextInput::new(window, cx));
                subscriptions.push(cx.subscribe(&input, move |panel, _, Committed(text), cx| {
                    panel.set_text(band, group, text, cx);
                }));
                inputs.push(input);
            }
        }
        let mut panel = Self {
            model,
            variant: 0,
            inputs,
            error: None,
            _subscriptions: subscriptions,
        };
        panel.sync(window, cx);
        panel
    }

    fn input(&self, band: usize, group: usize) -> &Entity<TextInput> {
        &self.inputs[band * FIELDS_PER_BAND + group]
    }

    /// The groups of a band of the variant on screen.
    fn groups(&self, band: usize, cx: &Context<Self>) -> Vec<Value> {
        self.model.read(cx).file().map_or_else(
            |_| Vec::new(),
            |file| groups_of(file.merged(), VARIANTS[self.variant].0, BANDS[band].0),
        )
    }

    /// Shows what the theme says in every field the author is not typing in.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for band in 0..BANDS.len() {
            let groups = self.groups(band, cx);
            for at in 0..FIELDS_PER_BAND {
                let text = groups.get(at).map(text_of).unwrap_or_default().to_owned();
                let input = self.input(band, at).clone();
                if !input.read(cx).focus_handle(cx).is_focused(window) {
                    input.update(cx, |input, cx| input.set_value(text, true, cx));
                }
            }
        }
        cx.notify();
    }

    fn path(&self, band: usize) -> [&'static str; 3] {
        ["pages", VARIANTS[self.variant].0, BANDS[band].0]
    }

    /// Writes a band's whole list, which is the smallest a theme can set.
    fn write(&mut self, band: usize, groups: Vec<Value>, cx: &mut Context<Self>) {
        let change = Change::set(&self.path(band), Value::Array(groups));
        let result = self.model.update(cx, |model, cx| model.change(change, cx));
        self.error = result.err().map(SharedString::from);
        cx.notify();
    }

    fn set_text(&mut self, band: usize, at: usize, text: &str, cx: &mut Context<Self>) {
        let mut groups = self.groups(band, cx);
        let Some(group) = groups.get_mut(at) else {
            return;
        };
        if text_of(group) != text {
            *group = with_text(group, text);
            self.write(band, groups, cx);
        }
    }

    fn set_align(&mut self, band: usize, at: usize, align: &str, cx: &mut Context<Self>) {
        let mut groups = self.groups(band, cx);
        if let Some(group) = groups.get_mut(at) {
            *group = with_align(group, align);
            self.write(band, groups, cx);
        }
    }

    fn add(&mut self, band: usize, cx: &mut Context<Self>) {
        let mut groups = self.groups(band, cx);
        groups.push(new_group(BANDS[band].0));
        self.write(band, groups, cx);
    }

    fn remove(&mut self, band: usize, at: usize, cx: &mut Context<Self>) {
        let mut groups = self.groups(band, cx);
        if at < groups.len() {
            groups.remove(at);
            self.write(band, groups, cx);
        }
    }

    fn reset(&mut self, band: usize, cx: &mut Context<Self>) {
        let change = Change::remove(&self.path(band));
        let result = self.model.update(cx, |model, cx| model.change(change, cx));
        self.error = result.err().map(SharedString::from);
        cx.notify();
    }

    fn group_row(&self, band: usize, at: usize, group: &Value, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let current = group.get("align").and_then(Value::as_str).unwrap_or("left").to_owned();
        let items = ALIGNS
            .iter()
            .map(|&(align, label)| {
                let on = current == align;
                (
                    div()
                        .id(SharedString::from(format!("head-{band}-{at}-{align}")))
                        .toggle(tokens, label, on)
                        .child(label)
                        .on_click(cx.listener(move |panel, _, _, cx| panel.set_align(band, at, align, cx))),
                    on,
                )
            })
            .collect();
        let remove = widgets::quiet_link(
            tokens,
            SharedString::from(format!("head-remove-{band}-{at}")),
            Key::Remove,
        )
        .on_click(cx.listener(move |panel, _, _, cx| panel.remove(band, at, cx)));
        widgets::field(
            tokens,
            crate::t!(Key::SlotText),
            remove,
            div()
                .flex()
                .flex_col()
                .gap(chrome::SPACE_1)
                .child(self.input(band, at).clone())
                .child(widgets::segmented(tokens, items)),
        )
        .into_any_element()
    }

    fn band(&self, band: usize, tokens: &Tokens, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let groups = self.groups(band, cx);
        let (variant, name) = (VARIANTS[self.variant].0, BANDS[band].0);
        let own = self
            .model
            .read(cx)
            .file()
            .ok()
            .and_then(|file| file.get(&["pages", variant, name]))
            .is_some();
        let mut end = div().flex().items_center().gap(chrome::SPACE_2);
        if groups.len() < FIELDS_PER_BAND {
            end = end.child(
                widgets::quiet_link(tokens, SharedString::from(format!("head-add-{band}")), "+")
                    .text_size(chrome::TEXT_BODY)
                    .on_click(cx.listener(move |panel, _, _, cx| panel.add(band, cx))),
            );
        }
        if own {
            end = end.child(
                widgets::quiet_link(tokens, SharedString::from(format!("head-reset-{band}")), Key::Reset)
                    .on_click(cx.listener(move |panel, _, _, cx| panel.reset(band, cx))),
            );
        }
        let mut rows = vec![
            widgets::section(tokens, crate::t!(BANDS[band].1))
                .justify_between()
                .child(end)
                .into_any_element(),
        ];
        if groups.is_empty() {
            rows.push(
                div()
                    .px(chrome::SPACE_3)
                    .py(chrome::SPACE_2)
                    .child(widgets::note(tokens, Key::BandEmpty))
                    .into_any_element(),
            );
        }
        for (at, group) in groups.iter().take(FIELDS_PER_BAND).enumerate() {
            rows.push(self.group_row(band, at, group, tokens, cx));
        }
        rows
    }
}

impl Render for RunningHeads {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = &Tokens::get(cx).clone();
        let variants: Vec<_> = VARIANTS
            .iter()
            .enumerate()
            .map(|(at, &(name, label))| {
                let on = at == self.variant;
                div()
                    .id(name)
                    .toggle(tokens, label, on)
                    .flex_none()
                    .px(chrome::SPACE_2)
                    .h(chrome::ZOOM_BUTTON + chrome::SPACE_1)
                    .flex()
                    .items_center()
                    .rounded(chrome::RADIUS)
                    .text_size(chrome::TEXT_SMALL)
                    .cursor_pointer()
                    .when(on, |item| item.bg(tokens.raised).text_color(tokens.text))
                    .when(!on, |item| {
                        item.text_color(tokens.text_quiet).hover(|item| item.bg(tokens.raised))
                    })
                    .child(label)
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.variant = at;
                        panel.sync(window, cx);
                    }))
            })
            .collect();
        let mut rows = self.band(0, tokens, cx);
        rows.extend(self.band(1, tokens, cx));
        div()
            .id("running-heads")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pt(chrome::SPACE_2)
            .child(
                div()
                    .px(chrome::SPACE_3)
                    .flex()
                    .flex_wrap()
                    .gap(chrome::SPACE_1)
                    .children(variants),
            )
            .children(rows)
            .when_some(self.error.clone(), |list, error| {
                list.child(div().px(chrome::SPACE_3).child(widgets::error_line(tokens, error)))
            })
            .child(
                div()
                    .px(chrome::SPACE_3)
                    .pt(chrome::SPACE_2)
                    .pb(chrome::SPACE_3)
                    .child(widgets::note(tokens, Key::SlotHint)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_groups_text_changes_and_the_rest_of_it_stays() {
        let group =
            json!({"anchor": "bottom-left", "align": "center", "slots": [{"text": "{title}", "style": "header"}]});
        let changed = with_text(&group, "{section}");
        assert_eq!(text_of(&changed), "{section}");
        assert_eq!(changed["anchor"], "bottom-left");
        assert_eq!(changed["slots"][0]["style"], "header");
        assert_eq!(with_align(&group, "left")["align"], "left");
    }

    #[test]
    fn a_band_with_nothing_set_has_no_groups() {
        let theme = json!({"pages": {"odd": {"header": null, "footer": [{"anchor": "top-left", "slots": []}]}}});
        assert!(groups_of(&theme, "odd", "header").is_empty());
        assert_eq!(groups_of(&theme, "odd", "footer").len(), 1);
        assert!(groups_of(&theme, "even", "footer").is_empty());
    }

    #[test]
    fn the_variants_and_a_new_group_are_what_the_schema_has() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../schema/theme.v1.schema.json")).expect("the schema parses");
        for (variant, _) in VARIANTS {
            assert!(schema["properties"]["pages"]["properties"].get(variant).is_some());
        }
        let anchors = schema["$defs"]["anchor"]["enum"].as_array().expect("anchors");
        for (band, _) in BANDS {
            assert!(anchors.contains(&new_group(band)["anchor"]));
        }
    }
}
