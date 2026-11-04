//! The Styles tab: a list of the theme's block styles and custom styles, and a detail view for one with every
//! property the schema has. Every edit goes through the shared [`ThemeModel`], which checks it with the engine and
//! keeps the file's other bytes as they were.

use std::collections::HashMap;

use gpui::prelude::*;
use gpui::{Context, Entity, Focusable, SharedString, Subscription, Window, div};
use serde_json::Value;

use super::input::{Committed, Submitted, TextInput};
use super::model::ThemeModel;
use super::strings::{Key, STYLES};
use super::style_property::{self, ALIGNMENTS, Control, Property, style_label};
use super::widgets;
use crate::theme_file::editor::Change;
use crate::theme_file::tokens::{self, Group};
use crate::tokens::{self as chrome, Tokens};

/// A style open in the detail view.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Selected {
    Builtin(&'static str),
    Custom(String),
}

impl Selected {
    fn prefix(&self) -> [&str; 2] {
        match self {
            Self::Builtin(key) => ["styles", key],
            Self::Custom(name) => ["custom-styles", name],
        }
    }

    /// The style's key in the theme.
    fn key(&self) -> &str {
        match self {
            Self::Builtin(key) => key,
            Self::Custom(name) => name,
        }
    }

    fn title(&self) -> String {
        match self {
            Self::Builtin(key) => style_label(key).to_owned(),
            Self::Custom(name) => name.clone(),
        }
    }
}

pub struct StylesPanel {
    model: Entity<ThemeModel>,
    selected: Option<Selected>,
    inputs: HashMap<String, Entity<TextInput>>,
    errors: HashMap<String, String>,
    /// The property whose list of tokens or choices is open.
    open: Option<String>,
    /// The field for a new custom style's name, while one is being made.
    creating: Option<Entity<TextInput>>,
    /// The field for the open custom style's new name.
    renaming: Option<Entity<TextInput>>,
    notice: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl StylesPanel {
    pub fn new(model: Entity<ThemeModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut inputs = HashMap::new();
        let mut subscriptions = vec![cx.observe_in(&model, window, |panel, _, window, cx| panel.sync(window, cx))];
        // Every property a style or a custom style can have that is typed, once, since they share keys.
        let every = style_property::of_custom()
            .into_iter()
            .chain(style_property::BLOCK.iter().copied());
        for property in every {
            if matches!(
                property.control,
                Control::Reference(_) | Control::Number | Control::Weight | Control::List
            ) {
                inputs.entry(property.key()).or_insert_with(|| {
                    let input = cx.new(|cx| TextInput::new(window, cx));
                    subscriptions.push(cx.subscribe(&input, move |panel, _, Committed(text), cx| {
                        panel.commit(property, text, cx);
                    }));
                    input
                });
            }
        }
        // `DRUCK_STYLE` opens a style for screenshots.
        let selected = crate::gate::var("DRUCK_STYLE").and_then(|name| {
            STYLES
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(key, _)| Selected::Builtin(key))
                .or(Some(Selected::Custom(name)))
        });
        let mut panel = Self {
            model,
            selected,
            inputs,
            errors: HashMap::new(),
            open: None,
            creating: None,
            renaming: None,
            notice: None,
            _subscriptions: subscriptions,
        };
        panel.sync(window, cx);
        panel
    }

    /// Sets the body size as if typed into its field. For measurements.
    pub fn set_body_size(&mut self, text: &str, cx: &mut Context<Self>) {
        self.selected = Some(Selected::Builtin("body"));
        self.commit(style_property::SIZE, text, cx);
    }

    /// The style the panel shows, for others that want to know what is being edited.
    #[allow(dead_code, reason = "the jump from a theme edit reads it in a later pass")]
    pub fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    fn select(&mut self, style: Option<Selected>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = style;
        self.errors.clear();
        self.open = None;
        self.renaming = None;
        self.notice = None;
        self.sync(window, cx);
    }

    fn path(&self, property: Property) -> Vec<String> {
        match &self.selected {
            Some(selected) => property.path_in(&selected.prefix()),
            None => Vec::new(),
        }
    }

    /// Shows what the file says in every field the author is not typing in.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selected.clone() else {
            cx.notify();
            return;
        };
        let model = self.model.read(cx);
        let Ok(file) = model.file() else {
            cx.notify();
            return;
        };
        let shown: Vec<_> = self
            .inputs
            .iter()
            .map(|(key, input)| {
                let path = [&selected.prefix()[..], &key.split('.').collect::<Vec<_>>()].concat();
                let own = file.get(&path);
                let value = own.or_else(|| inherited(file.merged(), file.value(), &selected, key));
                (
                    input.clone(),
                    value.map(Property::display).unwrap_or_default(),
                    own.is_some(),
                )
            })
            .collect();
        for (input, text, customized) in shown {
            if !input.read(cx).focus_handle(cx).is_focused(window) {
                input.update(cx, |input, cx| input.set_value(text, customized, cx));
            }
        }
        cx.notify();
    }

    /// Empty text returns the property to the inherited value.
    fn commit(&mut self, property: Property, text: &str, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            self.apply(property, None, cx);
            return;
        }
        match property.parse(text) {
            Ok(value) => self.apply(property, Some(value), cx),
            Err(message) => {
                self.errors.insert(property.key(), message.to_owned());
                cx.notify();
            }
        }
    }

    /// Sets or removes a property and announces the change. A rejected edit leaves file and disk as they were and
    /// shows the engine's reason beside the field.
    fn apply(&mut self, property: Property, value: Option<Value>, cx: &mut Context<Self>) {
        let path = self.path(property);
        if path.is_empty() {
            return;
        }
        let change = match value {
            Some(value) => Change::Set(path, value),
            None => Change::Remove(path),
        };
        let result = self.model.update(cx, |model, cx| model.change(change, cx));
        match result {
            Ok(_) => self.errors.remove(&property.key()),
            Err(message) => self.errors.insert(property.key(), message),
        };
        cx.notify();
    }

    fn create_style(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        self.creating = None;
        if name.is_empty() {
            cx.notify();
            return;
        }
        let change = Change::set(&["custom-styles", name, "based-on"], Value::String("body".to_owned()));
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => {
                self.selected = Some(Selected::Custom(name.to_owned()));
                self.notice = None;
            }
            Err(message) => self.notice = Some(message),
        }
        cx.notify();
    }

    fn rename_style(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        self.renaming = None;
        let to = to.trim();
        if to.is_empty() || to == from {
            cx.notify();
            return;
        }
        let change = Change::RenameCustomStyle(from.to_owned(), to.to_owned());
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => {
                self.selected = Some(Selected::Custom(to.to_owned()));
                self.notice = Some(
                    crate::t!(Key::RenameStyleNote)
                        .replace(".old", &format!(".{from}"))
                        .replace(".new", &format!(".{to}")),
                );
            }
            Err(message) => self.notice = Some(message),
        }
        cx.notify();
    }

    fn delete_style(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let change = Change::remove(&["custom-styles", name]);
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => self.select(None, window, cx),
            Err(message) => {
                self.notice = Some(message);
                cx.notify();
            }
        }
    }

    fn start_creating(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new(window, cx).with_placeholder(crate::t!(Key::NewStyleHint)));
        self._subscriptions
            .push(cx.subscribe(&input, |panel, input, _: &Submitted, cx| {
                let name = input.read(cx).text().to_owned();
                panel.create_style(&name, cx);
            }));
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.creating = Some(input);
        cx.notify();
    }

    fn start_renaming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Selected::Custom(name)) = self.selected.clone() else {
            return;
        };
        let input = cx.new(|cx| TextInput::new(window, cx));
        input.update(cx, |input, cx| input.set_value(name.clone(), true, cx));
        self._subscriptions
            .push(cx.subscribe(&input, move |panel, input, _: &Submitted, cx| {
                let to = input.read(cx).text().to_owned();
                panel.rename_style(&name, &to, cx);
            }));
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.renaming = Some(input);
        cx.notify();
    }

    // Rendering

    fn top(&self, title: SharedString, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let bar = widgets::bar(tokens);
        if self.selected.is_some() {
            bar.child(
                widgets::link(tokens, "styles-back", format!("‹ {}", crate::t!(Key::Back)))
                    .on_click(cx.listener(|panel, _, window, cx| panel.select(None, window, cx))),
            )
            .child(title)
        } else {
            bar.justify_between().child(title).child(
                widgets::link(tokens, "styles-new", "+")
                    .text_size(chrome::TEXT_BODY)
                    .on_click(cx.listener(|panel, _, window, cx| panel.start_creating(window, cx))),
            )
        }
    }

    fn list(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let Ok(file) = model.file() else {
            return div().into_any_element();
        };
        let customized = |prefix: &[&str]| {
            file.get(prefix)
                .and_then(|value| value.as_object())
                .is_some_and(|fields| !fields.is_empty())
        };
        let mut rows: Vec<gpui::AnyElement> = STYLES
            .iter()
            .map(|&(key, label)| {
                let used = customized(&["styles", key]);
                widgets::row(tokens, SharedString::from(format!("style-{key}")), crate::t!(label))
                    .child(widgets::tag(
                        tokens,
                        if used { Key::Customized } else { Key::Inherited },
                        used,
                    ))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.select(Some(Selected::Builtin(key)), window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let custom: Vec<String> = file
            .value()
            .get("custom-styles")
            .and_then(Value::as_object)
            .map(|styles| styles.keys().cloned().collect())
            .unwrap_or_default();
        rows.push(widgets::section(tokens, crate::t!(Key::CustomStyles).to_uppercase()).into_any_element());
        if let Some(input) = &self.creating {
            rows.push(
                div()
                    .px(chrome::SPACE_3)
                    .py(chrome::SPACE_2)
                    .child(input.clone())
                    .into_any_element(),
            );
        }
        for name in custom {
            let based = file
                .get(&["custom-styles", &name, "based-on"])
                .map(Property::display)
                .unwrap_or_default();
            let selected = Selected::Custom(name.clone());
            rows.push(
                widgets::row(tokens, SharedString::from(format!("custom-{name}")), name.clone())
                    .child(widgets::tag(tokens, based, false))
                    .on_click(cx.listener(move |panel, _, window, cx| panel.select(Some(selected.clone()), window, cx)))
                    .into_any_element(),
            );
        }
        if let Some(message) = &self.notice {
            rows.push(
                div()
                    .px(chrome::SPACE_3)
                    .child(widgets::error_line(tokens, message.clone()))
                    .into_any_element(),
            );
        }
        div()
            .id("styles-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }

    fn detail(&self, selected: &Selected, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        if self.model.read(cx).file().is_err() {
            return div().into_any_element();
        }
        let properties = match selected {
            Selected::Builtin(key) => style_property::of_builtin(key),
            Selected::Custom(_) => style_property::of_custom(),
        };
        let mut fields: Vec<gpui::AnyElement> = Vec::new();
        if let Selected::Custom(name) = selected {
            fields.push(self.custom_header(name, tokens, cx).into_any_element());
        }
        if let Some(message) = &self.notice {
            fields.push(
                div()
                    .px(chrome::SPACE_3)
                    .child(widgets::error_line(tokens, message.clone()))
                    .into_any_element(),
            );
        }
        if self.model.read(cx).missing() == Some(selected.key()) {
            fields.push(
                div()
                    .px(chrome::SPACE_3)
                    .pb(chrome::SPACE_2)
                    .flex()
                    .flex_col()
                    .gap(chrome::SPACE_1)
                    .child(widgets::note(
                        tokens,
                        crate::t!(Key::MissingStyleBlock, value = &selected.title()),
                    ))
                    .child(
                        widgets::link(tokens, "show-specimen", Key::ShowSpecimen).on_click(cx.listener(
                            |panel, _, _, cx| {
                                panel.model.update(cx, |model, cx| model.request_specimen(cx));
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }
        for property in properties {
            fields.push(self.field(property, selected, tokens, cx).into_any_element());
        }
        div()
            .id("style-detail")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pt(chrome::SPACE_2)
            .children(fields)
            .into_any_element()
    }

    fn custom_header(&self, name: &str, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let owned = name.to_owned();
        div()
            .px(chrome::SPACE_3)
            .pb(chrome::SPACE_2)
            .flex()
            .flex_col()
            .gap(chrome::SPACE_1)
            .child(match &self.renaming {
                Some(input) => input.clone().into_any_element(),
                None => div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_color(tokens.text).child(name.to_owned()))
                    .child(
                        div()
                            .flex()
                            .gap(chrome::SPACE_3)
                            .child(
                                widgets::link(tokens, "custom-rename", Key::Rename)
                                    .on_click(cx.listener(|panel, _, window, cx| panel.start_renaming(window, cx))),
                            )
                            .child(widgets::link(tokens, "custom-delete", Key::Delete).on_click(
                                cx.listener(move |panel, _, window, cx| panel.delete_style(&owned, window, cx)),
                            )),
                    )
                    .into_any_element(),
            })
            .child(widgets::note(
                tokens,
                crate::t!(Key::UsedAs).replace(".name", &format!(".{name}")),
            ))
    }

    /// One property: its label with what it resolves to and Reset, then its control, then why the engine refuses it.
    fn field(
        &self,
        property: Property,
        selected: &Selected,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let key = property.key();
        let path = property.path_in(&selected.prefix());
        let dotted = path.join(".");
        let open = self.open.as_deref() == Some(&key);
        let Some(data) = FieldData::read(self.model.read(cx), property, selected, &path, open) else {
            return div().into_any_element();
        };
        let id = |what: &str| SharedString::from(format!("{what}-{dotted}"));
        let current = data.current.as_ref();

        let mut end = div().flex().items_center().gap(chrome::SPACE_2);
        if let Some(resolved) = data.resolved.clone() {
            end = end.child(widgets::note(tokens, resolved));
        }
        if let Control::Reference(_) = property.control {
            let key = key.clone();
            end = end.child(
                widgets::link(tokens, id("pick"), if open { "▴ $" } else { "▾ $" }).on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.open = if panel.open.as_deref() == Some(&key) {
                            None
                        } else {
                            Some(key.clone())
                        };
                        cx.notify();
                    },
                )),
            );
        }
        if data.customized {
            end = end.child(
                widgets::link(tokens, id("reset"), Key::Reset)
                    .on_click(cx.listener(move |panel, _, _, cx| panel.apply(property, None, cx))),
            );
        }

        let mut control = div().flex().flex_col();
        match property.control {
            Control::Align => control = control.child(self.alignment(property, current, tokens, cx)),
            Control::Toggle => control = control.child(self.toggle(property, current, tokens, cx)),
            Control::Choice(choices) => control = control.child(self.choice(property, choices, current, tokens, cx)),
            Control::StyleName => control = control.child(self.style_names(property, current, &data.names, tokens, cx)),
            _ => {
                let input = self.inputs.get(&key).cloned();
                control = control.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(chrome::SPACE_2)
                        .when_some(data.swatch, |row, color| {
                            row.child(
                                div()
                                    .flex_none()
                                    .size(chrome::FIELD_HEIGHT)
                                    .rounded(chrome::RADIUS)
                                    .border_1()
                                    .border_color(tokens.divider)
                                    .bg(color),
                            )
                        })
                        .when_some(input, |row, input| row.child(div().flex_1().min_w_0().child(input))),
                );
            }
        }
        if open && matches!(property.control, Control::Reference(_)) {
            control = control.child(self.token_list(property, &data.tokens, current, tokens, cx));
        }
        let error = self.errors.get(&key).cloned().or(data.problem);
        widgets::field(
            tokens,
            property.label,
            end,
            control.when_some(error, |control, message| {
                control.child(widgets::error_line(tokens, message))
            }),
        )
        .into_any_element()
    }

    fn token_list(
        &self,
        property: Property,
        listed: &[(String, String, Option<String>)],
        current: Option<&Value>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let items = listed
            .iter()
            .map(|(written, label, detail)| {
                let active = current.and_then(Value::as_str) == Some(written.as_str());
                let value = written.clone();
                widgets::menu_item(
                    tokens,
                    SharedString::from(format!("token-{written}")),
                    label.clone(),
                    detail.clone(),
                    active,
                )
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.open = None;
                    panel.apply(property, Some(Value::String(value.clone())), cx);
                }))
            })
            .collect();
        widgets::menu(tokens, items)
    }

    fn alignment(
        &self,
        property: Property,
        current: Option<&Value>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let current = current.and_then(Value::as_str).map(str::to_owned);
        let items = ALIGNMENTS
            .iter()
            .map(|&(value, label)| {
                let active = current.as_deref() == Some(value);
                (
                    div()
                        .id(value)
                        .child(label)
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            panel.apply(property, Some(value.into()), cx);
                        })),
                    active,
                )
            })
            .collect();
        widgets::segmented(tokens, items)
    }

    fn toggle(
        &self,
        property: Property,
        current: Option<&Value>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let on = current.and_then(Value::as_bool).unwrap_or(false);
        let key = property.key();
        let items = [(true, Key::Yes), (false, Key::No)]
            .into_iter()
            .map(|(value, label)| {
                (
                    div()
                        .id(SharedString::from(format!("{key}-{value}")))
                        .child(label)
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            panel.apply(property, Some(Value::Bool(value)), cx);
                        })),
                    on == value,
                )
            })
            .collect();
        widgets::segmented(tokens, items)
    }

    fn choice(
        &self,
        property: Property,
        choices: &'static [(&'static str, Key)],
        current: Option<&Value>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let current = current.and_then(Value::as_str).map(str::to_owned);
        let key = property.key();
        let items = choices
            .iter()
            .map(|&(value, label)| {
                (
                    div()
                        .id(SharedString::from(format!("{key}-{value}")))
                        .child(label)
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            panel.apply(property, Some(value.into()), cx);
                        })),
                    current.as_deref() == Some(value),
                )
            })
            .collect();
        widgets::segmented(tokens, items)
    }

    /// The names of styles to pick as a base or a marker, in the open list.
    fn style_names(
        &self,
        property: Property,
        current: Option<&Value>,
        names: &[String],
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let key = property.key();
        let shown = current.and_then(Value::as_str).unwrap_or_default().to_owned();
        let button = widgets::button(
            tokens,
            SharedString::from(format!("names-{key}")),
            format!("{shown}  ▾"),
        )
        .justify_start()
        .on_click({
            let key = key.clone();
            cx.listener(move |panel, _, _, cx| {
                panel.open = if panel.open.as_deref() == Some(&key) {
                    None
                } else {
                    Some(key.clone())
                };
                cx.notify();
            })
        });
        let mut column = div().flex().flex_col().child(button);
        if self.open.as_deref() == Some(&key) {
            let items = names
                .iter()
                .map(|name| {
                    let value = name.clone();
                    widgets::menu_item(
                        tokens,
                        SharedString::from(format!("name-{name}")),
                        name.clone(),
                        None,
                        &shown == name,
                    )
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.open = None;
                        panel.apply(property, Some(Value::String(value.clone())), cx);
                    }))
                })
                .collect();
            column = column.child(widgets::menu(tokens, items));
        }
        column
    }
}

/// What a field shows, read from the file once so building the field does not hold the model.
struct FieldData {
    current: Option<Value>,
    customized: bool,
    resolved: Option<String>,
    swatch: Option<gpui::Rgba>,
    problem: Option<String>,
    /// For a reference: written form, label, and what it comes to.
    tokens: Vec<(String, String, Option<String>)>,
    /// For a style name: the styles to pick from, when the list is open.
    names: Vec<String>,
}

impl FieldData {
    fn read(model: &ThemeModel, property: Property, selected: &Selected, path: &[String], open: bool) -> Option<Self> {
        let file = model.file().ok()?;
        let key = property.key();
        let path_refs: Vec<&str> = path.iter().map(String::as_str).collect();
        let own = file.get(&path_refs);
        let current = own
            .or_else(|| inherited(file.merged(), file.value(), selected, &key))
            .cloned();
        let theme = file.merged();
        let resolved = match (property.control, &current) {
            (Control::Reference(_), Some(Value::String(text))) if text.starts_with('$') => resolve(theme, text),
            _ => None,
        };
        let swatch = (property.control == Control::Reference(Group::Colors))
            .then(|| {
                let text = current.as_ref()?.as_str()?;
                let resolved = resolve(theme, text).unwrap_or_else(|| text.to_owned());
                parse_color(&resolved)
            })
            .flatten();
        let mut listed = Vec::new();
        if let (true, Control::Reference(group)) = (open, property.control) {
            for (name, value) in tokens::of(theme, group) {
                listed.push((group.reference(&name), name, Some(resolve_value(theme, &value))));
            }
            if group == Group::Fonts {
                let families = theme.get("fonts").and_then(Value::as_object);
                for family in families.into_iter().flat_map(|families| families.keys()) {
                    listed.push((family.clone(), family.clone(), None));
                }
            }
        }
        let mut names = Vec::new();
        if open && property.control == Control::StyleName {
            names = STYLES.iter().map(|(key, _)| (*key).to_owned()).collect();
            names.extend(
                file.value()
                    .get("custom-styles")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(|styles| styles.keys().cloned()),
            );
            if let Selected::Custom(own) = selected {
                names.retain(|name| name != own);
            }
        }
        Some(Self {
            customized: own.is_some(),
            current,
            resolved,
            swatch,
            problem: model.problem_at(&path.join(".")).map(|problem| problem.message.clone()),
            tokens: listed,
            names,
        })
    }
}

/// What a style gets from the styles it is based on, or from the theme's own value, for a property it does not set.
fn inherited<'a>(merged: &'a Value, own: &'a Value, selected: &Selected, key: &str) -> Option<&'a Value> {
    let path: Vec<&str> = key.split('.').collect();
    let find = |root: &'a Value, prefix: [&str; 2]| {
        let mut value = root.get(prefix[0])?.get(prefix[1])?;
        for part in &path {
            value = value.get(*part)?;
        }
        Some(value)
    };
    match selected {
        Selected::Builtin(style) => find(merged, ["styles", style]),
        Selected::Custom(name) => {
            let mut base = own.get("custom-styles")?.get(name)?.get("based-on")?.as_str()?;
            for _ in 0..16 {
                if let Some(value) = find(merged, ["styles", base]) {
                    return Some(value);
                }
                let next = own.get("custom-styles")?.get(base)?;
                if let Some(value) = path.iter().try_fold(next, |value, part| value.get(*part)) {
                    return Some(value);
                }
                base = next.get("based-on")?.as_str()?;
            }
            None
        }
    }
}

/// What a `$group.name` reference comes to, following references between tokens.
fn resolve(theme: &Value, reference: &str) -> Option<String> {
    let mut current = reference.to_owned();
    for _ in 0..8 {
        let (group, name) = Group::parse(&current)?;
        let value = theme.get("tokens")?.get(group.key())?.get(name)?.as_str()?;
        if !value.starts_with('$') {
            return Some(value.to_owned());
        }
        current = value.to_owned();
    }
    None
}

fn resolve_value(theme: &Value, value: &Value) -> String {
    let text = Property::display(value);
    if text.starts_with('$') {
        resolve(theme, &text).unwrap_or(text)
    } else {
        text
    }
}

/// A `#rrggbb` color.
pub fn parse_color(text: &str) -> Option<gpui::Rgba> {
    let digits = text.strip_prefix('#')?;
    (digits.len() == 6)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
        .map(gpui::rgb)
}

impl Render for StylesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = &Tokens::get(cx).clone();
        let column = div().flex_1().min_w_0().h_full().flex().flex_col();
        let model = self.model.read(cx);
        if let Err(message) = model.file() {
            let message = if message.is_empty() {
                crate::t!(Key::NoTheme).to_owned()
            } else {
                message.to_owned()
            };
            return column
                .child(widgets::header(tokens, crate::t!(Key::StylesTitle).to_uppercase()))
                .child(
                    div()
                        .p(chrome::SPACE_3)
                        .text_color(tokens.text_quiet)
                        .child(SharedString::from(message)),
                )
                .into_any_element();
        }
        match self.selected.clone() {
            None => column
                .child(self.top(crate::t!(Key::StylesTitle).into(), tokens, cx))
                .child(self.list(tokens, cx))
                .into_any_element(),
            Some(selected) => column
                .child(self.top(SharedString::from(selected.title().to_uppercase()), tokens, cx))
                .child(self.detail(&selected, tokens, cx))
                .into_any_element(),
        }
    }
}
