//! The Styles tab: a list of the theme's block styles and a detail view for one. Every edit goes through
//! `ThemeFile`, which checks it with the engine and keeps the file's other bytes as they were.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Window, div};

use super::input::{Committed, TextInput};
use super::strings::{self, Key};
use super::style_property::{ALIGNMENTS, Property, style_label};
use crate::theme_file::ThemeFile;
use crate::tokens::{self, Tokens};

/// A property of the theme changed and was saved. The preview and the galley update on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeChanged {
    pub path: PathBuf,
    /// Dotted path of the property, such as `styles.heading-1.size`.
    pub property: String,
}

pub struct StylesPanel {
    theme: Result<ThemeFile, String>,
    /// The style open in the detail view.
    selected: Option<&'static str>,
    inputs: HashMap<Property, Entity<TextInput>>,
    errors: HashMap<Property, String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThemeChanged> for StylesPanel {}

impl StylesPanel {
    pub fn new(theme: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = match theme {
            Some(path) => ThemeFile::open(&path),
            None => Err(crate::t!(Key::NoTheme).to_owned()),
        };
        let mut inputs = HashMap::new();
        let mut subscriptions = Vec::new();
        for property in Property::ALL {
            if property == Property::Align {
                continue;
            }
            let input = cx.new(|cx| TextInput::new(window, cx));
            subscriptions.push(cx.subscribe(&input, move |panel, _, Committed(text), cx| {
                panel.commit(property, text, cx);
            }));
            inputs.insert(property, input);
        }
        Self {
            theme,
            selected: None,
            inputs,
            errors: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn set_body_size(&mut self, text: &str, cx: &mut Context<Self>) {
        self.select(Some("body"), cx);
        self.commit(Property::Size, text, cx);
    }

    fn path(&self, style: &'static str, property: Property) -> [&'static str; 3] {
        ["styles", style, property.key()]
    }

    /// Whether the theme itself sets anything on a style, as opposed to inheriting all of it.
    fn is_customized(file: &ThemeFile, style: &str) -> bool {
        file.get(&["styles", style])
            .and_then(|value| value.as_object())
            .is_some_and(|fields| !fields.is_empty())
    }

    fn select(&mut self, style: Option<&'static str>, cx: &mut Context<Self>) {
        self.selected = style;
        self.errors.clear();
        self.sync_inputs(cx);
        cx.notify();
    }

    /// Shows the effective value in every field, marked as the theme's own when it sets it.
    fn sync_inputs(&mut self, cx: &mut Context<Self>) {
        let (Some(style), Ok(file)) = (self.selected, &self.theme) else {
            return;
        };
        for (property, input) in &self.inputs {
            let path = ["styles", style, property.key()];
            let text = file.effective(&path).map(Property::display).unwrap_or_default();
            let customized = file.get(&path).is_some();
            input.update(cx, |input, cx| input.set_value(text, customized, cx));
        }
    }

    /// Empty text returns the property to the inherited value.
    fn commit(&mut self, property: Property, text: &str, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            self.reset(property, cx);
            return;
        }
        match property.parse(text) {
            Ok(value) => self.apply(property, Some(value), cx),
            Err(message) => {
                self.errors.insert(property, crate::t!(message).to_owned());
                cx.notify();
            }
        }
    }

    fn reset(&mut self, property: Property, cx: &mut Context<Self>) {
        self.apply(property, None, cx);
    }

    /// Sets or removes a property, saves, and announces the change. A rejected edit leaves file and disk as
    /// they were and shows the engine's reason beside the field.
    fn apply(&mut self, property: Property, value: Option<serde_json::Value>, cx: &mut Context<Self>) {
        let (Some(style), Ok(file)) = (self.selected, &mut self.theme) else {
            return;
        };
        let path = ["styles", style, property.key()];
        if file.get(&path) == value.as_ref() {
            self.errors.remove(&property);
            cx.notify();
            return;
        }
        let result = match value {
            Some(value) => file.set(&path, value),
            None => file.remove(&path),
        };
        match result {
            Err(problems) => {
                let message = problems
                    .first()
                    .map(|problem| problem.message.clone())
                    .unwrap_or_default();
                self.errors.insert(property, message);
                cx.notify();
                return;
            }
            Ok(()) => match file.save() {
                Err(error) => {
                    self.errors
                        .insert(property, format!("{}: {error}", crate::t!(Key::SaveFailed)));
                }
                Ok(()) => {
                    self.errors.remove(&property);
                    cx.emit(ThemeChanged {
                        path: file.path().to_owned(),
                        property: path.join("."),
                    });
                }
            },
        }
        self.sync_inputs(cx);
        cx.notify();
    }

    fn header(&self, title: &str, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .border_b_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tokens.text_quiet)
            .when(self.selected.is_some(), |header| {
                header.child(
                    div()
                        .id("styles-back")
                        .cursor_pointer()
                        .hover(|back| back.text_color(tokens.text))
                        .child(format!("‹ {}", crate::t!(Key::Back).to_uppercase()))
                        .on_click(cx.listener(|panel, _, _, cx| panel.select(None, cx))),
                )
            })
            .child(SharedString::from(title.to_uppercase()))
    }

    fn list(&self, file: &ThemeFile, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows: Vec<_> = strings::STYLES
            .iter()
            .map(|&(key, label)| {
                let customized = Self::is_customized(file, key);
                let label = crate::t!(label);
                div()
                    .id(key)
                    .h(tokens::ROW_HEIGHT)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(tokens.divider)
                    .cursor_pointer()
                    .hover(|row| row.bg(tokens.raised))
                    .child(label)
                    .child(
                        div()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(if customized { tokens.accent } else { tokens.text_faint })
                            .child(crate::t!(if customized { Key::Customized } else { Key::Inherited })),
                    )
                    .on_click(cx.listener(move |panel, _, _, cx| panel.select(Some(key), cx)))
            })
            .collect();
        div()
            .id("styles-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows)
    }

    fn field(
        &self,
        property: Property,
        file: &ThemeFile,
        style: &'static str,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let customized = file.get(&self.path(style, property)).is_some();
        let label = div()
            .h(tokens::ROW_HEIGHT)
            .flex()
            .items_center()
            .justify_between()
            .text_color(tokens.text_quiet)
            .child(property.label())
            .when(customized, |row| {
                row.child(
                    div()
                        .id(SharedString::from(format!("reset-{}", property.key())))
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.accent)
                        .cursor_pointer()
                        .child(crate::t!(Key::Reset))
                        .on_click(cx.listener(move |panel, _, _, cx| panel.reset(property, cx))),
                )
            });
        let control = if property == Property::Align {
            self.alignment(property, file, style, tokens, cx).into_any_element()
        } else {
            self.inputs[&property].clone().into_any_element()
        };
        div()
            .px(tokens::SPACE_3)
            .pb(tokens::SPACE_2)
            .flex()
            .flex_col()
            .child(label)
            .child(control)
            .when_some(self.errors.get(&property), |field, message| {
                field.child(
                    div()
                        .pt(tokens::SPACE_2 / 2.)
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.accent)
                        .child(SharedString::from(message.clone())),
                )
            })
    }

    fn alignment(
        &self,
        property: Property,
        file: &ThemeFile,
        style: &'static str,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let path = self.path(style, property);
        let current = file
            .effective(&path)
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let buttons: Vec<_> = ALIGNMENTS
            .iter()
            .map(|&(value, label)| {
                let active = current.as_deref() == Some(value);
                let label = crate::t!(label);
                div()
                    .id(value)
                    .flex_1()
                    .h(tokens::FIELD_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(tokens::TEXT_SMALL)
                    .cursor_pointer()
                    .when(active, |button| button.bg(tokens.raised).text_color(tokens.accent))
                    .when(!active, |button| {
                        button.text_color(tokens.text_quiet).hover(|b| b.bg(tokens.raised))
                    })
                    .child(label)
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.apply(Property::Align, Some(value.into()), cx);
                    }))
            })
            .collect();
        div()
            .flex()
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .overflow_hidden()
            .children(buttons)
    }
}

impl Render for StylesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = &Tokens::get(cx).clone();
        let column = div().flex_1().min_w_0().flex().flex_col();
        let file = match &self.theme {
            Ok(file) => file,
            Err(message) => {
                return column
                    .child(self.header(crate::t!(Key::StylesTitle), tokens, cx))
                    .child(
                        div()
                            .p(tokens::SPACE_3)
                            .text_color(tokens.text_quiet)
                            .child(SharedString::from(message.clone())),
                    );
            }
        };
        let Some(style) = self.selected else {
            return column
                .child(self.header(crate::t!(Key::StylesTitle), tokens, cx))
                .child(self.list(file, tokens, cx));
        };
        let fields: Vec<_> = Property::ALL
            .iter()
            .map(|&property| self.field(property, file, style, tokens, cx))
            .collect();
        column.child(self.header(style_label(style), tokens, cx)).child(
            div()
                .id("style-detail")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pt(tokens::SPACE_2)
                .children(fields),
        )
    }
}
