//! The Details tab: title, subtitle, authors, date, language, cover, and free `meta` rows, for every kind of document.
//! It holds no document: the workspace gives it what the session says and hears what the author changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, Focusable, FontWeight, ObjectFit, SharedString, Subscription, Window, div, img, svg,
};

use super::input::{Committed, TextInput};
use super::strings::Key;
use crate::session::{Details, Field, LANGUAGES, MetaRow};
use crate::tokens::{self, Tokens};

/// `meta` keys offered before they are set.
const SUGGESTED: &[(&str, Key)] = &[("isbn", Key::Isbn), ("publisher", Key::Publisher)];

/// What the author asked to change.
pub enum DetailsEdit {
    Field(Field, String),
    Meta(String, String),
    RemoveMeta(String),
    ChooseCover,
}

impl EventEmitter<DetailsEdit> for DetailsPanel {}

pub struct DetailsPanel {
    details: Details,
    cover_file: Option<PathBuf>,
    /// The cover's pixel size, read when the cover changes.
    cover_size: Option<(u32, u32)>,
    fields: Vec<(Field, Entity<TextInput>)>,
    meta_inputs: HashMap<String, Entity<TextInput>>,
    /// The field for a new row's key, while one is being added.
    new_key: Option<Entity<TextInput>>,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl DetailsPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        let mut fields = Vec::new();
        for field in [Field::Title, Field::Subtitle, Field::Authors, Field::Date] {
            let input = cx.new(|cx| TextInput::new(window, cx));
            subscriptions.push(cx.subscribe(&input, move |_, _, Committed(text), cx| {
                cx.emit(DetailsEdit::Field(field, text.clone()));
            }));
            fields.push((field, input));
        }
        Self {
            details: Details::default(),
            cover_file: None,
            cover_size: None,
            fields,
            meta_inputs: HashMap::new(),
            new_key: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Shows what the session says now.
    pub fn set_details(
        &mut self,
        details: Details,
        cover_file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.cover_file != cover_file {
            self.cover_size = cover_file.as_deref().and_then(pixel_size);
            self.cover_file = cover_file;
            cx.notify();
        }
        if details == self.details {
            return;
        }
        for (field, input) in &self.fields {
            let text = match field {
                Field::Title => &details.title,
                Field::Subtitle => &details.subtitle,
                Field::Authors => &details.authors,
                Field::Date => &details.date,
                Field::Language | Field::Cover => continue,
            };
            set_unless_editing(input, text, window, cx);
        }
        self.meta_inputs
            .retain(|key, _| details.meta.iter().any(|row| &row.key == key));
        for row in &details.meta {
            let input = self.meta_input(&row.key, window, cx);
            set_unless_editing(&input, &row.value, window, cx);
        }
        self.details = details;
        cx.notify();
    }

    /// Shows why a change was not made.
    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.error = error.map(SharedString::from);
        cx.notify();
    }

    fn meta_input(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextInput> {
        if let Some(input) = self.meta_inputs.get(key) {
            return input.clone();
        }
        let input = cx.new(|cx| TextInput::new(window, cx));
        let owned = key.to_owned();
        self._subscriptions
            .push(cx.subscribe(&input, move |_, _, Committed(text), cx| {
                cx.emit(DetailsEdit::Meta(owned.clone(), text.clone()));
            }));
        self.meta_inputs.insert(key.to_owned(), input.clone());
        input
    }

    /// Opens a field for a new row's key. The row is written once the key is entered.
    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new(window, cx));
        self._subscriptions
            .push(cx.subscribe(&input, |panel, _, Committed(key), cx| {
                panel.new_key = None;
                let key = key.trim();
                if !key.is_empty() {
                    cx.emit(DetailsEdit::Meta(key.to_owned(), String::new()));
                }
                cx.notify();
            }));
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.new_key = Some(input);
        cx.notify();
    }

    fn section_label(tokens: &Tokens, label: &'static str) -> gpui::Div {
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
            .child(label)
    }

    fn field(tokens: &Tokens, label: &'static str, input: Entity<TextInput>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .child(div().text_color(tokens.text_quiet).child(label))
            .child(input)
    }

    fn languages(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let buttons = LANGUAGES.iter().map(|&(code, name)| {
            let active = self.details.language == code;
            div()
                .id(code)
                .flex_1()
                .h(tokens::FIELD_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::RADIUS)
                .border_1()
                .border_color(if active { tokens.accent } else { tokens.divider })
                .text_color(if active { tokens.text } else { tokens.text_quiet })
                .cursor_pointer()
                .hover(|button| button.bg(tokens.raised))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    // A second click on the language in use leaves it to the theme.
                    let code = if panel.details.language == code { "" } else { code };
                    cx.emit(DetailsEdit::Field(Field::Language, code.to_owned()));
                }))
                .child(name)
        });
        div()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .child(div().text_color(tokens.text_quiet).child(crate::t!(Key::FieldLanguage)))
            .child(div().flex().gap(tokens::SPACE_1).children(buttons.collect::<Vec<_>>()))
    }

    fn cover(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = (!self.details.cover.is_empty()).then(|| {
            Path::new(&self.details.cover)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let size = self
            .cover_size
            .map(|(width, height)| format!("{width} \u{d7} {height} px"));
        let thumb = self.cover_file.clone().map(|file| {
            img(file)
                .h(tokens::COVER_PREVIEW_HEIGHT)
                .w(tokens::COVER_PREVIEW_WIDTH)
                .object_fit(ObjectFit::Contain)
        });
        div()
            .p(tokens::SPACE_3)
            .flex()
            .gap(tokens::SPACE_3)
            .items_center()
            .border_b_1()
            .border_color(tokens.divider)
            .when_some(thumb, |row, thumb| row.child(thumb))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(
                        div()
                            .truncate()
                            .child(name.clone().unwrap_or_else(|| crate::t!(Key::NoCover).to_owned())),
                    )
                    .when_some(size, |column, size| {
                        column.child(
                            div()
                                .text_color(tokens.text_quiet)
                                .text_size(tokens::TEXT_SMALL)
                                .child(size),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap(tokens::SPACE_2)
                            .child(
                                link(tokens, "cover-choose", crate::t!(Key::Choose)).on_click(cx.listener(
                                    |_, _, _, cx| {
                                        cx.emit(DetailsEdit::ChooseCover);
                                    },
                                )),
                            )
                            .when(name.is_some(), |row| {
                                row.child(
                                    link(tokens, "cover-remove", crate::t!(Key::Remove)).on_click(cx.listener(
                                        |_, _, _, cx| {
                                            cx.emit(DetailsEdit::Field(Field::Cover, String::new()));
                                        },
                                    )),
                                )
                            }),
                    ),
            )
    }

    fn meta_row(&self, row: &MetaRow, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let key = row.key.clone();
        let label = SUGGESTED
            .iter()
            .find(|(suggested, _)| *suggested == row.key)
            .map_or(row.key.clone(), |&(_, label)| crate::t!(label).to_owned());
        let remove = div()
            .id(SharedString::from(format!("meta-remove-{key}")))
            .flex_none()
            .cursor_pointer()
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(DetailsEdit::RemoveMeta(key.clone()))))
            .child(
                svg()
                    .path("icons/close.svg")
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.text_faint),
            );
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(div().truncate().text_color(tokens.text_quiet).child(label))
            .child(remove);
        let value = match self.meta_inputs.get(&row.key) {
            Some(input) if !row.lines => input.clone().into_any_element(),
            _ => div()
                .text_color(tokens.text_faint)
                .child(crate::t!(Key::SeveralLines, value = row.value))
                .into_any_element(),
        };
        div().flex().flex_col().gap(tokens::SPACE_1).child(header).child(value)
    }
}

fn set_unless_editing(input: &Entity<TextInput>, text: &str, window: &Window, cx: &mut Context<DetailsPanel>) {
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    if !focused {
        input.update(cx, |input, cx| input.set_value(text.to_owned(), true, cx));
    }
}

fn link(tokens: &Tokens, id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .text_color(tokens.text_quiet)
        .cursor_pointer()
        .hover(|link| link.text_color(tokens.text))
        .child(label)
}

/// An image file's size in pixels, read from its header.
fn pixel_size(path: &Path) -> Option<(u32, u32)> {
    image::image_dimensions(path).ok()
}

impl Render for DetailsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let fields: Vec<_> = self
            .fields
            .iter()
            .map(|(field, input)| {
                let label = match field {
                    Field::Title => crate::t!(Key::FieldTitle),
                    Field::Subtitle => crate::t!(Key::FieldSubtitle),
                    Field::Authors => crate::t!(Key::FieldAuthors),
                    Field::Date => crate::t!(Key::FieldDate),
                    Field::Language | Field::Cover => "",
                };
                Self::field(&tokens, label, input.clone())
            })
            .collect();
        let languages = self.languages(&tokens, cx);
        let cover = self.cover(&tokens, cx);
        let rows: Vec<_> = self
            .details
            .meta
            .clone()
            .iter()
            .map(|row| self.meta_row(row, &tokens, cx).into_any_element())
            .collect();
        let suggestions: Vec<_> = SUGGESTED
            .iter()
            .filter(|(key, _)| !self.details.meta.iter().any(|row| row.key == *key))
            .map(|&(key, label)| {
                let label = crate::t!(label);
                div()
                    .id(key)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_1)
                    .text_color(tokens.text_quiet)
                    .cursor_pointer()
                    .hover(|chip| chip.text_color(tokens.text))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(DetailsEdit::Meta(key.to_owned(), String::new()))))
                    .child(
                        svg()
                            .path("icons/plus.svg")
                            .size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint),
                    )
                    .child(label)
            })
            .collect();
        let add = div()
            .id("meta-add")
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .text_color(tokens.text_quiet)
            .cursor_pointer()
            .hover(|chip| chip.text_color(tokens.text))
            .on_click(cx.listener(|panel, _, window, cx| panel.add_row(window, cx)))
            .child(
                svg()
                    .path("icons/plus.svg")
                    .size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint),
            )
            .child(crate::t!(Key::Other));
        let problem = self
            .details
            .problem
            .clone()
            .map(SharedString::from)
            .or_else(|| self.error.clone());
        div()
            .id("details")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(Self::section_label(&tokens, crate::t!(Key::DetailsTitle)))
            .when_some(problem, |panel, problem| {
                panel.child(div().p(tokens::SPACE_3).text_color(tokens.accent).child(problem))
            })
            .child(
                div()
                    .p(tokens::SPACE_3)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_3)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .children(fields)
                    .child(languages),
            )
            .child(Self::section_label(&tokens, crate::t!(Key::CoverImageTitle)))
            .child(cover)
            .child(Self::section_label(&tokens, crate::t!(Key::MetaTitle)))
            .child(
                div()
                    .p(tokens::SPACE_3)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_3)
                    .children(rows)
                    .when_some(self.new_key.clone(), |column, key| {
                        column.child(Self::field(&tokens, crate::t!(Key::NewKey), key))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(tokens::SPACE_3)
                            .children(suggestions)
                            .child(add),
                    ),
            )
    }
}
