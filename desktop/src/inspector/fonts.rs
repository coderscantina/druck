//! The Fonts tab: the families the layout uses, and a detail view for one with its specimen, files, license, and
//! axes. A font file is added through the shared [`ThemeModel`], which copies it next to the theme.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, PathPromptOptions, SharedString, Window, div};

use super::model::ThemeModel;
use super::strings::Key;
use super::style_property::style_label;
use super::widgets;
use crate::preview::fonts::{FontUse, INLINE_CODE, Source};
use crate::theme_file::editor::Change;
use crate::theme_file::font_info::{Embedding, FontInfo, Format};
use crate::tokens::{self, Tokens};

/// The faces a file can be added as, in the order the choice shows them.
const FACES: [(&str, Key); 4] = [
    ("regular", Key::Regular),
    ("italic", Key::Italic),
    ("bold", Key::Bold),
    ("bold-italic", Key::BoldItalic),
];

/// A family in use, with what its first file says about itself.
struct Family {
    font: FontUse,
    info: Option<FontInfo>,
    /// The size of its distinct files.
    total: usize,
}

impl Family {
    fn of(font: FontUse) -> Self {
        let info = font
            .faces
            .first()
            .and_then(|face| FontInfo::read(&face.bytes, face.index).ok());
        let mut files: Vec<&druck::resource::Bytes> = Vec::new();
        for face in &font.faces {
            if !files.contains(&&face.bytes) {
                files.push(&face.bytes);
            }
        }
        let total = files.iter().map(|bytes| bytes.len()).sum();
        Self { font, info, total }
    }
}

/// A chosen font file waiting for its face.
struct Adding {
    from: PathBuf,
    family: String,
    face: &'static str,
}

#[allow(dead_code, reason = "mounted by the integrator")]
pub struct FontsPanel {
    model: Entity<ThemeModel>,
    families: Vec<Family>,
    /// The family open in the detail view.
    selected: Option<String>,
    adding: Option<Adding>,
    error: Option<String>,
}

#[allow(dead_code, reason = "mounted by the integrator")]
impl FontsPanel {
    pub fn new(model: Entity<ThemeModel>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        Self {
            model,
            families: Vec::new(),
            selected: None,
            adding: None,
            error: None,
        }
    }

    /// The families of the latest layout.
    pub fn set_fonts(&mut self, fonts: Vec<FontUse>, cx: &mut Context<Self>) {
        self.families = fonts.into_iter().map(Family::of).collect();
        // `DRUCK_FONT` opens a family for screenshots.
        if self.selected.is_none() {
            self.selected = std::env::var("DRUCK_FONT")
                .ok()
                .filter(|name| self.families.iter().any(|family| &family.font.family == name));
        }
        if let Some(selected) = &self.selected
            && !self.families.iter().any(|family| &family.font.family == selected)
        {
            self.selected = None;
        }
        cx.notify();
    }

    fn select(&mut self, family: Option<String>, cx: &mut Context<Self>) {
        self.selected = family;
        self.adding = None;
        self.error = None;
        cx.notify();
    }

    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(Key::ChooseFile.into()),
        });
        cx.spawn_in(window, async move |panel, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            panel.update(cx, |panel, cx| panel.prepare(path, cx)).ok();
        })
        .detach();
    }

    /// Reads the chosen file to suggest its family and face. A file that is no font goes no further.
    fn prepare(&mut self, from: PathBuf, cx: &mut Context<Self>) {
        let info = std::fs::read(&from)
            .map_err(|error| format!("{}: {error}", from.display()))
            .and_then(|bytes| FontInfo::read(&bytes, 0).map_err(|_| crate::t!(Key::NotAFont).to_owned()));
        match info {
            Ok(info) => {
                let family = self.selected.clone().unwrap_or_else(|| {
                    if info.family.is_empty() {
                        from.file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    } else {
                        info.family.clone()
                    }
                });
                self.adding = Some(Adding {
                    from,
                    family,
                    face: suggested_face(&info.style),
                });
                self.error = None;
            }
            Err(message) => {
                self.adding = None;
                self.error = Some(message);
            }
        }
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(adding) = self.adding.take() else {
            return;
        };
        let change = Change::AddFont {
            family: adding.family.clone(),
            face: adding.face.to_owned(),
            from: adding.from.clone(),
        };
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(_) => self.error = None,
            Err(message) => {
                self.error = Some(if message.is_empty() {
                    crate::t!(Key::NoTheme).to_owned()
                } else {
                    message
                });
                self.adding = Some(adding);
            }
        }
        cx.notify();
    }

    // Rendering

    fn list(&self, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let mut rows: Vec<AnyElement> = self
            .families
            .iter()
            .map(|family| {
                let name = family.font.family.clone();
                widgets::row(tokens, SharedString::from(format!("font-{name}")), name.clone())
                    .child(widgets::tag(tokens, source_word(family.font.source), false))
                    .on_click(cx.listener(move |panel, _, _, cx| panel.select(Some(name.clone()), cx)))
                    .into_any_element()
            })
            .collect();
        if rows.is_empty() {
            rows.push(padded(widgets::note(tokens, Key::NoFonts)).into_any_element());
        }
        rows.push(self.add(tokens, cx));
        div()
            .id("fonts-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }

    fn detail(&self, family: &Family, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let font = &family.font;
        let mut parts: Vec<AnyElement> = Vec::new();
        if let Some(info) = &family.info {
            parts.push(specimen(info, tokens).into_any_element());
            if info.embedding == Embedding::Restricted {
                parts.push(warning(tokens, Key::EmbeddingWarning).into_any_element());
            }
        }
        let typeface = family
            .info
            .as_ref()
            .map(|info| format!("{} {}", info.family, info.style).trim().to_owned())
            .unwrap_or_else(|| font.family.clone());
        parts.push(value_row(tokens, Key::Typeface, typeface, None));
        parts.push(value_row(tokens, Key::TotalSize, size(family.total), None));
        if let Some(info) = &family.info {
            let format = match info.format {
                Format::TrueType => Key::Truetype,
                Format::OpenType => Key::Opentype,
            };
            let collection =
                (info.faces > 1).then(|| crate::t!(Key::CollectionFormat, value = &info.faces.to_string()));
            parts.push(value_row(tokens, Key::Format, crate::t!(format).to_owned(), collection));
        }
        let path = (font.source == Source::ProjectFile)
            .then(|| font.faces.first().map(|face| face.file.clone()))
            .flatten();
        parts.push(value_row(
            tokens,
            Key::Source,
            crate::t!(source_word(font.source)).to_owned(),
            path,
        ));
        let used_by = font
            .used_by
            .iter()
            .map(|key| {
                if key == INLINE_CODE {
                    crate::t!(Key::InlineCode)
                } else {
                    style_label(key)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(value_row(tokens, Key::UsedBy, used_by, None));

        parts.push(widgets::section(tokens, crate::t!(Key::Faces).to_uppercase()).into_any_element());
        for face in &font.faces {
            let label = FACES
                .iter()
                .find(|(key, _)| *key == face.face)
                .map_or(face.face.as_str(), |&(_, label)| crate::t!(label));
            parts.push(labelled_row(tokens, label, face_file(tokens, file_name_of(&face.file))).into_any_element());
        }

        if let Some(info) = &family.info {
            parts.push(widgets::section(tokens, crate::t!(Key::License).to_uppercase()).into_any_element());
            let license = match &info.license {
                Some(license) => div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text)
                    .child(license.clone()),
                None => widgets::note(tokens, Key::NoLicense),
            };
            parts.push(padded(license).into_any_element());
            if let Some(url) = &info.license_url {
                parts.push(padded(widgets::note(tokens, url.clone())).into_any_element());
            }
            if info.is_variable() {
                parts.push(widgets::section(tokens, crate::t!(Key::Axes).to_uppercase()).into_any_element());
                parts.push(padded(widgets::note(tokens, Key::AxesNote)).into_any_element());
                for axis in &info.axes {
                    let range = format!("{} / {} / {}", number(axis.min), number(axis.default), number(axis.max));
                    parts.push(labelled_row(tokens, &axis.tag, div().child(range)).into_any_element());
                }
            }
        }
        parts.push(self.add(tokens, cx));
        div()
            .id("font-detail")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(parts)
            .into_any_element()
    }

    /// The add button, then the face choice for a chosen file, then why the model refused it.
    fn add(&self, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let mut foot = div()
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .child(
                widgets::button(tokens, "font-add", Key::AddFont)
                    .on_click(cx.listener(|panel, _, window, cx| panel.choose_file(window, cx))),
            );
        if let Some(adding) = &self.adding {
            let choices = FACES
                .iter()
                .map(|&(face, label)| {
                    let item = div()
                        .id(SharedString::from(format!("font-face-{face}")))
                        .child(label)
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            if let Some(adding) = &mut panel.adding {
                                adding.face = face;
                            }
                            cx.notify();
                        }));
                    (item, adding.face == face)
                })
                .collect();
            let note = if self.selected.is_some() {
                crate::t!(Key::AddToFamily, value = &adding.family)
            } else {
                crate::t!(Key::AddAsFamily, value = &adding.family)
            };
            let file = file_name_of(&adding.from.to_string_lossy());
            foot = foot
                .child(div().text_color(tokens.text).min_w_0().truncate().child(file))
                .child(div().text_color(tokens.text_quiet).child(crate::t!(Key::FaceName)))
                .child(widgets::segmented(tokens, choices))
                .child(widgets::note(tokens, note))
                .child(
                    div()
                        .flex()
                        .gap(tokens::SPACE_2)
                        .child(
                            widgets::button(tokens, "font-add-confirm", Key::Add)
                                .on_click(cx.listener(|panel, _, _, cx| panel.confirm(cx))),
                        )
                        .child(
                            widgets::button(tokens, "font-add-cancel", Key::Cancel).on_click(cx.listener(
                                |panel, _, _, cx| {
                                    panel.adding = None;
                                    panel.error = None;
                                    cx.notify();
                                },
                            )),
                        ),
                );
        }
        foot.when_some(self.error.clone(), |foot, message| {
            foot.child(widgets::error_line(tokens, message))
        })
        .into_any_element()
    }

    fn top(&self, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let Some(selected) = &self.selected else {
            return widgets::header(tokens, crate::t!(Key::FontsTitle).to_uppercase()).into_any_element();
        };
        widgets::bar(tokens)
            .child(
                div()
                    .id("font-back")
                    .flex_none()
                    .size(tokens::ICON_SIZE + tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(tokens.divider)
                    .text_color(tokens.text)
                    .cursor_pointer()
                    .hover(|back| back.bg(tokens.raised))
                    .child("‹")
                    .on_click(cx.listener(|panel, _, _, cx| panel.select(None, cx))),
            )
            .child(div().min_w_0().truncate().child(selected.to_uppercase()))
            .into_any_element()
    }
}

impl Render for FontsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = &Tokens::get(cx).clone();
        let open = self
            .selected
            .as_ref()
            .and_then(|selected| self.families.iter().position(|family| &family.font.family == selected));
        let body = match open {
            Some(index) => self.detail(&self.families[index], tokens, cx),
            None => self.list(tokens, cx),
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .text_size(tokens::TEXT_BODY)
            .child(self.top(tokens, cx))
            .child(body)
    }
}

/// The face a file's style name suggests.
fn suggested_face(style: &str) -> &'static str {
    let style = style.to_lowercase();
    let italic = style.contains("italic") || style.contains("oblique");
    match (style.contains("bold"), italic) {
        (true, true) => "bold-italic",
        (true, false) => "bold",
        (false, true) => "italic",
        (false, false) => "regular",
    }
}

fn source_word(source: Source) -> Key {
    match source {
        Source::Bundled => Key::SourceBundled,
        Source::Installed => Key::SourceInstalled,
        Source::ProjectFile => Key::SourceFile,
    }
}

/// A file size in kB below a megabyte, else in MB with one decimal.
fn size(bytes: usize) -> String {
    if bytes < 1_000_000 {
        crate::t!(Key::SizeKb, value = &bytes.div_ceil(1000).to_string())
    } else {
        let megabytes = format!("{:.1}", bytes as f64 / 1_000_000.);
        crate::t!(
            Key::SizeMb,
            value = &megabytes.replace('.', crate::t!(Key::DecimalPoint))
        )
    }
}

/// An axis value without a trailing `.0`.
fn number(value: f32) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.replace('.', crate::t!(Key::DecimalPoint))
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

fn padded(child: impl IntoElement) -> gpui::Div {
    div().px(tokens::SPACE_3).py(tokens::SPACE_1).child(child)
}

/// The large "Aa" in the family, on a card.
fn specimen(info: &FontInfo, tokens: &Tokens) -> impl IntoElement {
    div().px(tokens::SPACE_3).py(tokens::SPACE_3).child(
        div()
            .py(tokens::SPACE_4)
            .flex()
            .justify_center()
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .bg(tokens.raised)
            .font_family(SharedString::from(info.family.clone()))
            .text_size(tokens::TEXT_TITLE * 2.)
            .text_color(tokens.text)
            .child(crate::t!(Key::FontSpecimen)),
    )
}

fn warning(tokens: &Tokens, message: Key) -> impl IntoElement {
    div().px(tokens::SPACE_3).pb(tokens::SPACE_2).child(
        div()
            .p(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.accent)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.accent)
            .child(message),
    )
}

/// A quiet label at the start, `value` at the end.
fn labelled_row(tokens: &Tokens, label: &str, value: impl IntoElement) -> gpui::Div {
    div()
        .min_h(tokens::LIST_ROW_HEIGHT)
        .px(tokens::SPACE_3)
        .flex()
        .items_center()
        .justify_between()
        .gap(tokens::SPACE_3)
        .child(div().flex_none().text_color(tokens.text_quiet).child(label.to_owned()))
        .child(value)
}

fn face_file(tokens: &Tokens, file: String) -> impl IntoElement {
    div().min_w_0().truncate().text_color(tokens.text_quiet).child(file)
}

/// A label and its value, with a quiet line below the value when there is one.
fn value_row(tokens: &Tokens, label: Key, value: String, below: Option<String>) -> AnyElement {
    let value = div()
        .min_w_0()
        .py(tokens::SPACE_1)
        .flex()
        .flex_col()
        .items_end()
        .text_color(tokens.text)
        .child(div().text_right().child(value))
        .when_some(below, |value, below| {
            value.child(widgets::note(tokens, below).text_right())
        });
    labelled_row(tokens, crate::t!(label), value).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_name_suggests_a_face() {
        assert_eq!(suggested_face("Regular"), "regular");
        assert_eq!(suggested_face("Italic"), "italic");
        assert_eq!(suggested_face("Bold"), "bold");
        assert_eq!(suggested_face("Bold Italic"), "bold-italic");
    }
}
