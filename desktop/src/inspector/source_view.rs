//! The source view: every top-level section of the theme as JSON text, for the sections that have no panel of their
//! own yet. An edit applies once the text parses and the engine accepts the whole theme, and only that section's
//! bytes change in the file.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, Entity, Focusable, SharedString, Subscription, Task, Window, div};
use serde_json::Value;

use super::input::{Committed, Edited, TextInput};
use super::model::ThemeModel;
use super::strings::Key;
use super::widgets;
use crate::theme_file::editor::Change;
use crate::theme_file::patch;
use crate::tokens::{self as chrome, Tokens};

/// How long typing must stop before a valid text is applied.
const PAUSE: Duration = Duration::from_millis(400);
/// The longest default shown for reference, in lines.
const DEFAULT_LINES: usize = 60;

/// The 30 top-level sections of the schema, in its order, with the panel that edits what it can. A section with no
/// panel is edited here only. Milestone 17 gives each one a panel.
pub const SECTIONS: [(&str, Option<Key>); 30] = [
    ("$schema", None),
    ("version", None),
    ("document", None),
    ("fonts", Some(Key::FontsTitle)),
    ("images", None),
    ("tokens", Some(Key::TokensTitle)),
    ("page", Some(Key::GeometryTitle)),
    ("styles", Some(Key::StylesTitle)),
    ("custom-styles", Some(Key::StylesTitle)),
    ("inline", None),
    ("lists", None),
    ("tables", None),
    ("callouts", None),
    ("footnotes", None),
    ("margin-notes", None),
    ("placement", None),
    ("captions", None),
    ("bibliography", None),
    ("toc", None),
    ("index", None),
    ("matter", None),
    ("chapters", None),
    ("scene-break", None),
    ("typesetting", None),
    ("title-block", None),
    ("title-page", None),
    ("pages", None),
    ("watermark", None),
    ("highlight", None),
    ("labels", None),
];

pub struct SourceView {
    model: Entity<ThemeModel>,
    section: Option<&'static str>,
    input: Entity<TextInput>,
    /// What the engine or the parser said about the text as it is.
    message: Option<String>,
    pending: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl SourceView {
    pub fn new(model: Entity<ThemeModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::multiline(window, cx));
        let subscriptions = vec![
            cx.observe_in(&model, window, |view, _, window, cx| view.show_section(window, cx)),
            cx.subscribe_in(&input, window, |view, _, _: &Edited, window, cx| {
                view.edited(window, cx)
            }),
            cx.subscribe(&input, |view, _, Committed(_), cx| view.apply(cx)),
        ];
        Self {
            model,
            section: None,
            input,
            message: None,
            pending: Task::ready(()),
            _subscriptions: subscriptions,
        }
    }

    fn open(&mut self, section: Option<&'static str>, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        self.message = None;
        self.show_text(window, cx, true);
    }

    /// Shows the file's text for the section, unless the author is typing in it.
    fn show_section(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_text(window, cx, false);
    }

    fn show_text(&mut self, window: &mut Window, cx: &mut Context<Self>, force: bool) {
        cx.notify();
        let Some(section) = self.section else { return };
        if !force && self.input.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let text = self.written(section, cx).unwrap_or_default();
        if self.input.read(cx).text() != text {
            let customized = !text.is_empty();
            self.input.update(cx, |input, cx| input.set_value(text, customized, cx));
        }
    }

    /// The section as the file has it, byte for byte.
    fn written(&self, section: &str, cx: &Context<Self>) -> Option<String> {
        let file = self.model.read(cx).file().ok()?;
        let span = patch::value_span(file.text(), &[section]).ok()??;
        Some(file.text()[span].to_owned())
    }

    fn edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(PAUSE).await;
            view.update(cx, |view, cx| view.apply(cx)).ok();
        });
        cx.notify();
    }

    /// Applies the text if it parses and the engine takes the theme with it. Empty text removes the section.
    fn apply(&mut self, cx: &mut Context<Self>) {
        let Some(section) = self.section else { return };
        let text = self.input.read(cx).text().to_owned();
        if self.written(section, cx).unwrap_or_default() == text {
            self.message = None;
            cx.notify();
            return;
        }
        let change = if text.trim().is_empty() {
            Change::remove(&[section])
        } else {
            let parsed = match serde_json::from_str::<Value>(&text) {
                Ok(value) => value,
                Err(error) => {
                    self.message = Some(crate::t!(Key::JsonInvalid, value = &error.to_string()));
                    cx.notify();
                    return;
                }
            };
            let Ok(file) = self.model.read(cx).file() else { return };
            let whole = match patch::value_span(file.text(), &[section]) {
                Ok(Some(span)) => Ok(format!(
                    "{}{}{}",
                    &file.text()[..span.start],
                    text.trim(),
                    &file.text()[span.end..]
                )),
                Ok(None) => patch::set(file.text(), &[section], &parsed),
                Err(message) => Err(message),
            };
            match whole {
                Ok(whole) => Change::Text(whole),
                Err(message) => {
                    self.message = Some(crate::t!(Key::JsonInvalid, value = &message));
                    cx.notify();
                    return;
                }
            }
        };
        self.message = match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => None,
            Err(message) => Some(crate::t!(Key::JsonInvalid, value = &message)),
        };
        cx.notify();
    }

    fn list(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let file_has = |section: &str| {
            self.model
                .read(cx)
                .file()
                .is_ok_and(|file| file.value().get(section).is_some())
        };
        let rows: Vec<_> = SECTIONS
            .iter()
            .map(|&(section, panel)| {
                let set = file_has(section);
                widgets::row(tokens, SharedString::from(format!("section-{section}")), section)
                    .child(
                        div()
                            .flex()
                            .gap(chrome::SPACE_3)
                            .child(widgets::tag(
                                tokens,
                                match panel {
                                    Some(panel) => crate::t!(panel),
                                    None => crate::t!(Key::JsonOnly),
                                },
                                panel.is_some(),
                            ))
                            .child(widgets::tag(
                                tokens,
                                if set { Key::Customized } else { Key::Inherited },
                                set,
                            )),
                    )
                    .on_click(cx.listener(move |view, _, window, cx| view.open(Some(section), window, cx)))
            })
            .collect();
        div()
            .id("section-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .px(chrome::SPACE_3)
                    .py(chrome::SPACE_2)
                    .child(widgets::note(tokens, Key::JsonNote)),
            )
            .children(rows)
    }

    fn editor(&self, section: &'static str, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let reference = self.model.read(cx).file().ok().map(|file| {
            let value = file.merged().get(section).cloned().unwrap_or(Value::Null);
            let pretty = serde_json::to_string_pretty(&value).unwrap_or_default();
            let lines: Vec<&str> = pretty.lines().collect();
            let more = lines.len() > DEFAULT_LINES;
            let shown = lines.into_iter().take(DEFAULT_LINES).collect::<Vec<_>>().join("\n");
            if more { format!("{shown}\n…") } else { shown }
        });
        let message = self.message.clone();
        let panel = SECTIONS
            .iter()
            .find(|(name, _)| *name == section)
            .and_then(|(_, panel)| *panel);
        div()
            .id("source-editor")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(chrome::SPACE_3)
            .py(chrome::SPACE_2)
            .flex()
            .flex_col()
            .gap(chrome::SPACE_2)
            .when_some(panel, |column, panel| {
                column.child(widgets::note(
                    tokens,
                    format!("{}: {}", crate::t!(Key::JsonDesigned), crate::t!(panel)),
                ))
            })
            .child(self.input.clone())
            .child(match message {
                Some(message) => widgets::error_line(tokens, message),
                None => widgets::note(tokens, Key::JsonValid),
            })
            .when_some(reference, |column, text| {
                column
                    .child(widgets::section(tokens, crate::t!(Key::DefaultWord).to_uppercase()))
                    .child(
                        div()
                            .font_family("Menlo")
                            .text_size(chrome::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .whitespace_nowrap()
                            .child(SharedString::from(text)),
                    )
            })
    }
}

impl Render for SourceView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = &Tokens::get(cx).clone();
        let column = div().flex_1().min_w_0().h_full().flex().flex_col();
        if let Err(message) = self.model.read(cx).file() {
            let message = if message.is_empty() {
                crate::t!(Key::NoTheme).to_owned()
            } else {
                message.to_owned()
            };
            return column
                .child(widgets::header(tokens, crate::t!(Key::JsonTitle).to_uppercase()))
                .child(
                    div()
                        .p(chrome::SPACE_3)
                        .text_color(tokens.text_quiet)
                        .child(SharedString::from(message)),
                )
                .into_any_element();
        }
        match self.section {
            None => column
                .child(widgets::header(tokens, crate::t!(Key::JsonTitle).to_uppercase()))
                .child(self.list(tokens, cx))
                .into_any_element(),
            Some(section) => column
                .child(
                    widgets::bar(tokens)
                        .child(
                            widgets::link(tokens, "source-back", format!("‹ {}", crate::t!(Key::Back)))
                                .on_click(cx.listener(|view, _, window, cx| view.open(None, window, cx))),
                        )
                        .child(section),
                )
                .child(self.editor(section, tokens, cx))
                .into_any_element(),
        }
    }
}
