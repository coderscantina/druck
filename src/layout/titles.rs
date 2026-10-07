//! Title blocks and title pages from theme slots and document metadata.
//!
//! The title layout in use is the `title-page` slots with `document.title-page`, else the
//! `title-block` slots. A title block is set at the start of the body when the document has a value
//! for at least one of its placeholders; a title page is always set when requested. Slots are set in
//! order, each `space-before` below the previous one; slot styles add no spacing of their own. An
//! optional slot whose placeholders lack a value is omitted with its spacing. A slot in the `abstract`
//! style starts with the `abstract` label in the `abstract-heading` style and that style's
//! `space-after`. The title block keeps `title-block.space-after` from the body.
//!
//! Values are inserted as text. Blank lines in a value separate paragraphs; other line breaks are spaces.

use std::collections::HashMap;

use super::paragraph;
use super::{Break, Flow, Frame, Line, translate_line};
use crate::config::front_matter::Metadata;
use crate::config::resolved::{Config, SlotContent, Style, TitleSlot};
use crate::config::source::{Resource, Source};
use crate::config::template::Placeholder;
use crate::config::theme::{Align, TemplateStyle};
use crate::config::values::Pt;
use crate::diagnostic::Diagnostic;
use crate::document::{Inline, InlineStyle, Location};
use crate::image::Image;
use crate::page::{Item, Page, Rect};
use crate::text::Fonts;

/// Where title content is reported: the front matter at the top of the document.
pub(super) const FRONT_MATTER: Location = Location { line: 1, column: 1 };

/// Reports required slots of the title layout in use whose placeholders have no value.
pub fn check(config: &Config, source: &Source) -> Result<(), Vec<Diagnostic>> {
    let Some((layout, slots)) = active(config) else {
        return Ok(());
    };
    let errors: Vec<_> = slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.required)
        .filter_map(|(index, slot)| {
            let SlotContent::Text(text) = &slot.content else {
                return None;
            };
            let missing = text.fill(|p| value(&config.metadata, p)).err()?;
            let name = missing.name();
            let message = format!("this required slot needs {{{name}}}; set {name} in the front matter");
            Some(Diagnostic::new(Some(source.clone()), message).property(Some(format!("{layout}.slots.{index}.text"))))
        })
        .collect();
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// The name and slots of the title layout in use, if one is set.
fn active(config: &Config) -> Option<(&'static str, &[TitleSlot])> {
    if config.document.title_page {
        Some(("title-page", &config.title_page))
    } else {
        block(config).map(|slots| ("title-block", slots))
    }
}

/// The title block's slots, if the document has a value for one of its placeholders.
pub(super) fn block(config: &Config) -> Option<&[TitleSlot]> {
    let slots = &config.title_block.slots;
    let shown = slots.iter().any(|slot| match &slot.content {
        SlotContent::Text(text) => text.placeholders().any(|p| value(&config.metadata, p).is_some()),
        SlotContent::Image { .. } => false,
    });
    shown.then_some(slots.as_slice())
}

/// The metadata value of a placeholder. Authors are joined with commas. Blank values count as missing.
pub(super) fn value(metadata: &Metadata, placeholder: Placeholder) -> Option<String> {
    let value = match placeholder {
        Placeholder::Title => metadata.title.clone(),
        Placeholder::Subtitle => metadata.subtitle.clone(),
        Placeholder::Author => Some(metadata.authors.join(", ")),
        Placeholder::Date => metadata.date.clone(),
        Placeholder::Abstract => metadata.abstract_.clone(),
        Placeholder::Section | Placeholder::Subsection | Placeholder::Page => None,
    };
    value.filter(|value| !value.trim().is_empty())
}

/// The title page: the `title-page` slots stacked from the top of the text area, keeping the space
/// above the first slot.
pub(super) fn page(
    config: &Config,
    fonts: &Fonts,
    theme_images: &HashMap<Resource, Image>,
    source: &Source,
) -> Result<Page, Vec<Diagnostic>> {
    let geometry = &config.page;
    let error = |message: String| vec![Diagnostic::new(Some(source.clone()), message)];
    let lines = slots(&config.title_page, config, fonts, theme_images, geometry.text_width().0).map_err(error)?;
    let height: f64 = lines.iter().map(|(space, line)| space + line.height).sum();
    if height > geometry.text_height().0 {
        return Err(error(format!(
            "the title page content is {height:.1}pt high, more than the {:.1}pt text area",
            geometry.text_height().0
        )));
    }
    let (left, mut y) = (geometry.margin_inner.0, geometry.margin_top.0);
    let mut items = Vec::new();
    for (space, line) in lines {
        y += space;
        items.extend(line.items.into_iter().map(|item| super::translate(item, left, y)));
        y += line.height;
    }
    Ok(Page {
        width: geometry.width,
        height: geometry.height,
        items,
    })
}

impl<'a> Flow<'a> {
    /// Sets the title block at the start of the body, kept on one page.
    pub(super) fn title(&mut self, slots: &[TitleSlot], frame: Frame<'a>) {
        let width = self.width(frame);
        match self::slots(slots, self.config, self.fonts, self.theme_images, width) {
            Ok(lines) => {
                let count = lines.len();
                for (index, (space, line)) in lines.into_iter().enumerate() {
                    self.space(space);
                    let after = if index + 1 == count {
                        Break::Allowed(0.0)
                    } else {
                        Break::Never
                    };
                    self.push(translate_line(line, frame.left), FRONT_MATTER, after);
                }
                self.space(self.config.title_block.space_after.0);
            }
            Err(problem) => self.errors.push(self.error(FRONT_MATTER, problem)),
        }
    }
}

/// The lines of the slots that have values, each with the space above it. Lines are set across `width`.
fn slots(
    slots: &[TitleSlot],
    config: &Config,
    fonts: &Fonts,
    theme_images: &HashMap<Resource, Image>,
    width: f64,
) -> Result<Vec<(f64, Line)>, String> {
    let mut lines = Vec::new();
    for slot in slots {
        let style = config.styles.get(slot.style);
        let start = lines.len();
        match &slot.content {
            SlotContent::Text(text) => {
                let Ok(text) = text.fill(|p| value(&config.metadata, p)) else {
                    continue;
                };
                if slot.style == TemplateStyle::Abstract {
                    let heading = &config.styles.abstract_heading;
                    let label = &config.labels.abstract_;
                    lines.extend(
                        text_lines(label, heading, config, fonts, width)?
                            .into_iter()
                            .map(|l| (0.0, l)),
                    );
                    let gap = heading.space_after.0;
                    lines.extend(
                        text_lines(&text, style, config, fonts, width)?
                            .into_iter()
                            .enumerate()
                            .map(|(index, line)| (if index == 0 { gap } else { 0.0 }, line)),
                    );
                } else {
                    lines.extend(
                        text_lines(&text, style, config, fonts, width)?
                            .into_iter()
                            .map(|l| (0.0, l)),
                    );
                }
            }
            SlotContent::Image {
                image,
                width: image_width,
            } => {
                let decoded = theme_images
                    .get(image)
                    .ok_or_else(|| format!("theme image {image} is not loaded"))?;
                lines.push((0.0, image_line(decoded, *image_width, style.align, width)));
            }
        }
        if let Some((first, _)) = lines.get_mut(start) {
            *first = slot.space_before.0;
        }
    }
    Ok(lines)
}

/// Plain text in the slot's style. Each paragraph after the first gets the style's first-line indent.
fn text_lines(text: &str, style: &Style, config: &Config, fonts: &Fonts, width: f64) -> Result<Vec<Line>, String> {
    let lang = config.document.lang;
    let measure = width - 2.0 * style.indent.0;
    let mut lines = Vec::new();
    for (index, paragraph) in text.split("\n\n").filter(|p| !p.trim().is_empty()).enumerate() {
        let content = plain(&paragraph.replace('\n', " "));
        let indent = if index == 0 { 0.0 } else { style.first_line_indent.0 };
        let set = paragraph::lines(&content, style, &config.inline, fonts, lang, measure, indent)?;
        lines.extend(set.into_iter().map(|line| translate_line(line, style.indent.0)));
    }
    Ok(lines)
}

fn plain(text: &str) -> Vec<Inline> {
    vec![Inline::Text {
        text: text.to_owned(),
        style: InlineStyle::default(),
    }]
}

/// A theme image `width` wide, placed in `frame_width` by the slot style's alignment.
fn image_line(image: &Image, width: Pt, align: Align, frame_width: f64) -> Line {
    let (natural_width, natural_height) = image.size();
    let height = width.0 * natural_height.0 / natural_width.0;
    let x = match align {
        Align::Left | Align::Justify => 0.0,
        Align::Center => (frame_width - width.0) / 2.0,
        Align::Right => frame_width - width.0,
    };
    let rect = Rect {
        x: Pt(x),
        y: Pt(0.0),
        width,
        height: Pt(height),
    };
    Line {
        height,
        baseline: height,
        items: vec![Item::Image {
            rect,
            image: image.clone(),
        }],
        notes: Vec::new(),
    }
}
