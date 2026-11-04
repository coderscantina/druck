//! The settings card: the appearance and the language. A choice applies at once and is kept by the shell.

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FontWeight, Window, div, px, relative, svg};

use super::state::Appearance;
use super::strings::LanguageSetting;
use super::text::Key;
use super::ui;
use crate::tokens::{self, Tokens};

pub enum SettingsEvent {
    Appearance(Appearance),
    Language(LanguageSetting),
}

impl EventEmitter<SettingsEvent> for SettingsPanel {}

pub struct SettingsPanel {
    open: bool,
    appearance: Appearance,
    language: LanguageSetting,
}

const CARD_WIDTH: gpui::Pixels = px(360.);

impl SettingsPanel {
    pub fn new(appearance: Appearance, language: LanguageSetting) -> Self {
        Self {
            open: false,
            appearance,
            language,
        }
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    pub fn set(&mut self, appearance: Appearance, language: LanguageSetting, cx: &mut Context<Self>) {
        self.appearance = appearance;
        self.language = language;
        cx.notify();
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let appearances = [
            ("appearance-system", Appearance::System, Key::AppearanceSystem),
            ("appearance-dark", Appearance::Dark, Key::AppearanceDark),
            ("appearance-light", Appearance::Light, Key::AppearanceLight),
        ]
        .map(|(id, choice, label)| {
            ui::chip(&tokens, id, crate::t!(label), self.appearance == choice)
                .flex_1()
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(SettingsEvent::Appearance(choice))))
        });
        let languages = [
            ("language-system", LanguageSetting::System, Key::LanguageSystem),
            ("language-english", LanguageSetting::English, Key::LanguageEnglish),
            ("language-german", LanguageSetting::German, Key::LanguageGerman),
        ]
        .map(|(id, choice, label)| {
            ui::chip(&tokens, id, crate::t!(label), self.language == choice)
                .flex_1()
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(SettingsEvent::Language(choice))))
        });
        let group = |label: &'static str, choices: Vec<gpui::Stateful<gpui::Div>>| {
            div()
                .flex()
                .flex_col()
                .gap(tokens::SPACE_2)
                .child(div().text_color(tokens.text_quiet).child(label))
                .child(div().flex().gap(tokens::SPACE_1).children(choices))
        };
        let card = div()
            .id("settings-card")
            .occlude()
            .absolute()
            .top(tokens::SPACE_4 * 3.)
            .left(relative(0.5))
            .ml(CARD_WIDTH / -2.)
            .w(CARD_WIDTH)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow_lg()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(tokens::SPACE_3)
                    .h(tokens::SECTION_HEIGHT + tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .justify_between()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crate::t!(Key::SettingsTitle))
                    .child(
                        div()
                            .id("settings-close")
                            .cursor_pointer()
                            .on_click(cx.listener(|panel, _, _, cx| panel.close(cx)))
                            .child(
                                svg()
                                    .path("icons/close.svg")
                                    .size(tokens::ICON_SIZE)
                                    .text_color(tokens.text_quiet),
                            ),
                    ),
            )
            .child(
                div()
                    .p(tokens::SPACE_3)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_4)
                    .child(group(crate::t!(Key::Appearance), appearances.into()))
                    .child(group(crate::t!(Key::Language), languages.into())),
            );
        div()
            .id("settings-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_key_down(cx.listener(|panel, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    panel.close(cx);
                }
            }))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|panel, _, _, cx| panel.close(cx)))
            .child(card)
            .into_any_element()
    }
}
