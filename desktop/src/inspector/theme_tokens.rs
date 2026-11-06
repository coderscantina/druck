//! The Tokens tab: the fonts, sizes, spacing, and colors groups. A token can be created, given a new value,
//! renamed with every reference following, and deleted when nothing refers to it.

use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{Context, Entity, Focusable, SharedString, Subscription, Window, div};
use serde_json::Value;

use super::input::{Committed, Submitted, TextInput};
use super::model::ThemeModel;
use super::strings::Key;
use super::styles::parse_color;
use super::widgets;
use crate::a11y::Control as _;
use crate::theme_file::editor::Change;
use crate::theme_file::tokens::{self, Group};
use crate::tokens::{self as chrome, Tokens};

/// What a new token starts as.
fn starting_value(group: Group) -> &'static str {
    match group {
        Group::Fonts => "Libertinus Serif",
        Group::Sizes => "10pt",
        Group::Spacing => "1em",
        Group::Colors => "#444444",
    }
}

fn group_label(group: Group) -> Key {
    match group {
        Group::Fonts => Key::GroupFonts,
        Group::Sizes => Key::GroupSizes,
        Group::Spacing => Key::GroupSpacing,
        Group::Colors => Key::GroupColors,
    }
}

pub struct TokensPanel {
    model: Entity<ThemeModel>,
    group: Group,
    /// One value field per token shown, made when the token first appears.
    values: HashMap<(Group, String), Entity<TextInput>>,
    /// The token being renamed, with its field.
    renaming: Option<(String, Entity<TextInput>)>,
    creating: Option<Entity<TextInput>>,
    /// Why the last change was refused, by token name.
    errors: HashMap<String, String>,
    _subscriptions: Vec<Subscription>,
}

impl TokensPanel {
    pub fn new(model: Entity<ThemeModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe_in(&model, window, |panel, _, window, cx| panel.sync(window, cx));
        let mut panel = Self {
            model,
            group: Group::Sizes,
            values: HashMap::new(),
            renaming: None,
            creating: None,
            errors: HashMap::new(),
            _subscriptions: vec![subscription],
        };
        panel.sync(window, cx);
        panel
    }

    /// Makes a field for each token that has none, and shows the file's value in each one not being typed in.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let Ok(file) = model.file() else {
            cx.notify();
            return;
        };
        let mut shown = Vec::new();
        for group in Group::ALL {
            for (name, value) in tokens::of(file.merged(), group) {
                let own = file.get(&["tokens", group.key(), &name]).is_some();
                shown.push((group, name, super::style_property::Property::display(&value), own));
            }
        }
        let live: HashSet<(Group, String)> = shown.iter().map(|(group, name, ..)| (*group, name.clone())).collect();
        self.values.retain(|key, _| live.contains(key));
        for (group, name, text, own) in shown {
            let key = (group, name.clone());
            let input = match self.values.get(&key) {
                Some(input) => input.clone(),
                None => {
                    let input = cx.new(|cx| TextInput::new(window, cx));
                    self._subscriptions
                        .push(cx.subscribe(&input, move |panel, _, Committed(text), cx| {
                            panel.set_value(group, &name, text, cx);
                        }));
                    self.values.insert(key, input.clone());
                    input
                }
            };
            if !input.read(cx).focus_handle(cx).is_focused(window) {
                input.update(cx, |input, cx| input.set_value(text, own, cx));
            }
        }
        cx.notify();
    }

    fn set_value(&mut self, group: Group, name: &str, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        let path = ["tokens", group.key(), name];
        let change = if text.is_empty() {
            Change::remove(&path)
        } else {
            Change::set(&path, Value::String(text.to_owned()))
        };
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => self.errors.remove(name),
            Err(message) => self.errors.insert(name.to_owned(), message),
        };
        cx.notify();
    }

    fn create(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim().to_owned();
        self.creating = None;
        if name.is_empty() {
            cx.notify();
            return;
        }
        let change = Change::set(
            &["tokens", self.group.key(), &name],
            Value::String(starting_value(self.group).to_owned()),
        );
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => self.errors.remove(&name),
            Err(message) => self.errors.insert(name, message),
        };
        cx.notify();
    }

    fn rename(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        self.renaming = None;
        let to = to.trim();
        if to.is_empty() || to == from {
            cx.notify();
            return;
        }
        let change = Change::RenameToken(self.group, from.to_owned(), to.to_owned());
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => self.errors.remove(from),
            Err(message) => self.errors.insert(from.to_owned(), message),
        };
        cx.notify();
    }

    /// Deletes a token, or returns an overridden one to the default. A token that is in use stays, and the places
    /// that use it are named.
    fn delete(&mut self, name: &str, cx: &mut Context<Self>) {
        let reference = self.group.reference(name);
        let places = self
            .model
            .read(cx)
            .file()
            .map(|file| tokens::uses(file.merged(), &reference))
            .unwrap_or_default();
        let own_places: Vec<String> = places
            .into_iter()
            .filter(|place| *place != format!("tokens.{}.{name}", self.group.key()))
            .collect();
        let inherited = tokens::of(&default_theme(), self.group)
            .iter()
            .any(|(known, _)| known == name);
        if !own_places.is_empty() && !inherited {
            let shown = own_places.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let more = if own_places.len() > 3 { ", …" } else { "" };
            self.errors.insert(
                name.to_owned(),
                crate::t!(Key::TokenBlocked, value = &format!("{shown}{more}")),
            );
            cx.notify();
            return;
        }
        let change = Change::remove(&["tokens", self.group.key(), name]);
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => self.errors.remove(name),
            Err(message) => self.errors.insert(name.to_owned(), message),
        };
        cx.notify();
    }

    fn start_creating(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new(window, cx).with_placeholder(Key::NewTokenHint));
        self._subscriptions
            .push(cx.subscribe(&input, |panel, input, _: &Submitted, cx| {
                let name = input.read(cx).text().to_owned();
                panel.create(&name, cx);
            }));
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.creating = Some(input);
        cx.notify();
    }

    fn start_renaming(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new(window, cx));
        input.update(cx, |input, cx| input.set_value(name.to_owned(), true, cx));
        let from = name.to_owned();
        self._subscriptions
            .push(cx.subscribe(&input, move |panel, input, _: &Submitted, cx| {
                let to = input.read(cx).text().to_owned();
                panel.rename(&from, &to, cx);
            }));
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.renaming = Some((name.to_owned(), input));
        cx.notify();
    }

    fn groups(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let items = Group::ALL
            .into_iter()
            .map(|group| {
                (
                    div()
                        .id(SharedString::from(format!("group-{}", group.key())))
                        .control(tokens, gpui::Role::Tab, group_label(group))
                        .child(group_label(group))
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            panel.group = group;
                            panel.renaming = None;
                            panel.creating = None;
                            cx.notify();
                        })),
                    group == self.group,
                )
            })
            .collect();
        div()
            .px(chrome::SPACE_3)
            .py(chrome::SPACE_2)
            .child(widgets::segmented(tokens, items))
    }

    fn token_row(&self, name: &str, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let group = self.group;
        let Ok(file) = self.model.read(cx).file() else {
            return div().into_any_element();
        };
        let own = file.get(&["tokens", group.key(), name]).is_some();
        let inherited = tokens::of(&default_theme(), group)
            .iter()
            .any(|(known, _)| known == name);
        let uses = tokens::uses(file.merged(), &group.reference(name))
            .into_iter()
            .filter(|place| *place != format!("tokens.{}.{name}", group.key()))
            .count();
        let value = file
            .merged()
            .get("tokens")
            .and_then(|tokens| tokens.get(group.key()))
            .and_then(|tokens| tokens.get(name))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let swatch = (group == Group::Colors).then(|| parse_color(&value)).flatten();
        let input = self.values.get(&(group, name.to_owned())).cloned();
        let renaming = self
            .renaming
            .as_ref()
            .filter(|(renamed, _)| renamed == name)
            .map(|(_, input)| input.clone());
        let id = |what: &str| SharedString::from(format!("{what}-{name}"));
        let owned = name.to_owned();
        let title = match renaming {
            Some(input) => input.into_any_element(),
            None => div().text_color(tokens.text).child(name.to_owned()).into_any_element(),
        };
        let mut actions = div().flex().items_center().gap(chrome::SPACE_3);
        actions = actions.child(widgets::note(
            tokens,
            if uses == 0 {
                crate::t!(Key::TokenUnused).to_owned()
            } else {
                if uses == 1 {
                    crate::t!(Key::TokenUsedOnce).to_owned()
                } else {
                    crate::t!(Key::TokenUsed, value = &uses.to_string())
                }
            },
        ));
        if own && !inherited {
            let renamed = name.to_owned();
            actions = actions
                .child(
                    widgets::quiet_link(tokens, id("rename"), Key::Rename)
                        .on_click(cx.listener(move |panel, _, window, cx| panel.start_renaming(&renamed, window, cx))),
                )
                .child(
                    widgets::quiet_link(tokens, id("delete"), Key::Delete)
                        .on_click(cx.listener(move |panel, _, _, cx| panel.delete(&owned, cx))),
                );
        } else if own {
            actions = actions.child(
                widgets::quiet_link(tokens, id("reset"), Key::Reset)
                    .on_click(cx.listener(move |panel, _, _, cx| panel.delete(&owned, cx))),
            );
        }
        let error = self.errors.get(name).cloned();
        div()
            .px(chrome::SPACE_3)
            .py(chrome::SPACE_2)
            .flex()
            .flex_col()
            .gap(chrome::SPACE_1)
            .border_b_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(chrome::SPACE_2)
                    .child(title)
                    .child(actions),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(chrome::SPACE_2)
                    .when_some(swatch, |row, color| {
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
            )
            .when_some(error, |row, message| row.child(widgets::error_line(tokens, message)))
            .into_any_element()
    }
}

/// The bundled default theme, for telling an own token from one the file overrides.
fn default_theme() -> Value {
    druck::editor::theme_value::default_theme_value()
}

impl Render for TokensPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = &Tokens::get(cx).clone();
        let column = div().flex_1().min_w_0().h_full().flex().flex_col();
        let names: Vec<String> = match self.model.read(cx).file() {
            Ok(file) => tokens::of(file.merged(), self.group)
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            Err(message) => {
                let message = if message.is_empty() {
                    crate::t!(Key::NoTheme).to_owned()
                } else {
                    message.to_owned()
                };
                return column
                    .child(
                        div()
                            .p(chrome::SPACE_3)
                            .text_color(tokens.text_quiet)
                            .child(SharedString::from(message)),
                    )
                    .into_any_element();
            }
        };
        let rows: Vec<_> = names.iter().map(|name| self.token_row(name, tokens, cx)).collect();
        let creating = self.creating.clone();
        column
            .child(
                widgets::bar(tokens).justify_end().child(
                    widgets::quiet_link(tokens, "token-new", "+")
                        .text_size(chrome::TEXT_BODY)
                        .on_click(cx.listener(|panel, _, window, cx| panel.start_creating(window, cx))),
                ),
            )
            .child(self.groups(tokens, cx))
            .child(
                div()
                    .id("token-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when_some(creating, |list, input| {
                        list.child(div().px(chrome::SPACE_3).py(chrome::SPACE_2).child(input))
                    })
                    .children(rows),
            )
            .into_any_element()
    }
}
