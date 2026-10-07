//! Title blocks and title pages from theme slots and document metadata.
//!
//! The title layout in use is the `title-page` groups with `document.title-page`, else the
//! `title-block` slots. A title block is set at the start of the body when the document has a value
//! for at least one of its placeholders; a title page is always set when requested. Slots are set in
//! order, each `space-before` below the previous one; slot styles add no spacing of their own. An
//! optional slot whose placeholders lack a value is omitted with its spacing. A slot in the `abstract`
//! style starts with the `abstract` label in the `abstract-heading` style and that style's
//! `space-after`. The title block keeps `title-block.space-after` from the body.
//!
//! Each title page group stacks its slots in a box as wide as the group, placed at its anchor on the
//! margin frame: its top edge `y` below the frame's top, its middle `y` below the frame's middle, or its
//! bottom edge `y` above the frame's bottom, and its left or right edge `x` inside the anchored side.
//! The box includes the space above the first slot.
//!
//! Values are inserted as text. A line break in the slot text and each entry of a list value start a
//! new line; empty lines are dropped. Within a value, blank lines separate paragraphs and other line
//! breaks are spaces.

use std::borrow::Cow;
use std::collections::HashMap;

use super::paragraph;
use super::{Break, Flow, Frame, Line, translate_line};
use crate::config::front_matter::{MetaValue, Metadata};
use crate::config::resolved::{Config, Group, SlotContent, Style, TitleSlot};
use crate::config::source::{Resource, Source};
use crate::config::template::{Placeholder, Value};
use crate::config::theme::{Align, SlotStyle, TemplateStyle, Vertical};
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
    let errors: Vec<_> = active(config)
        .into_iter()
        .flat_map(|(prefix, slots)| {
            slots
                .iter()
                .enumerate()
                .map(move |(index, slot)| (prefix.clone(), index, slot))
        })
        .filter(|(_, _, slot)| slot.required)
        .filter_map(|(prefix, index, slot)| {
            let SlotContent::Text(text) = &slot.content else {
                return None;
            };
            let missing = text.fill(|p| value(&config.metadata, p)).err()?;
            let message = format!("this required slot needs {{{missing}}}; set {missing} in the front matter");
            Some(Diagnostic::new(Some(source.clone()), message).property(Some(format!("{prefix}.slots.{index}.text"))))
        })
        .collect();
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// The property prefix and slots of each part of the title layout in use.
fn active(config: &Config) -> Vec<(String, &[TitleSlot])> {
    if config.document.title_page {
        let groups = config.title_page.iter().enumerate();
        groups
            .map(|(index, group)| (format!("title-page.groups.{index}"), group.slots.as_slice()))
            .collect()
    } else {
        block(config)
            .map(|slots| ("title-block".to_owned(), slots))
            .into_iter()
            .collect()
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
pub(super) fn value(metadata: &Metadata, placeholder: &Placeholder) -> Option<Value> {
    let text = match placeholder {
        Placeholder::Title => metadata.title.clone(),
        Placeholder::Subtitle => metadata.subtitle.clone(),
        Placeholder::Author => Some(metadata.authors.join(", ")),
        Placeholder::Date => metadata.date.clone(),
        Placeholder::Abstract => metadata.abstract_.clone(),
        Placeholder::Meta(key) => match metadata.meta.get(key)? {
            MetaValue::Text(text) => Some(text.clone()),
            MetaValue::Lines(lines) => return Some(Value::Lines(lines.clone())),
        },
        Placeholder::Section | Placeholder::Subsection | Placeholder::Page | Placeholder::Pages => None,
    };
    text.filter(|text| !text.trim().is_empty()).map(Value::Text)
}

/// The title page: each `title-page` group placed at its anchor on the margin frame.
pub(super) fn page(
    config: &Config,
    fonts: &Fonts,
    theme_images: &HashMap<Resource, Image>,
    source: &Source,
) -> Result<Page, Vec<Diagnostic>> {
    let geometry = &config.page;
    let error = |message: String| vec![Diagnostic::new(Some(source.clone()), message)];
    let (frame_width, frame_height) = (geometry.text_width().0, geometry.text_height().0);
    let mut items = Vec::new();
    for (index, group) in config.title_page.iter().enumerate() {
        let lines = slots(&group.slots, group.align, config, fonts, theme_images, group.width.0).map_err(error)?;
        let height: f64 = lines.iter().map(|(space, line)| space + line.height).sum();
        let top = match group.anchor.vertical() {
            Vertical::Top => group.y.0,
            Vertical::Middle => (frame_height - height) / 2.0 + group.y.0,
            Vertical::Bottom => frame_height - group.y.0 - height,
        };
        if top < -1e-9 || top + height > frame_height + 1e-9 {
            let message = format!(
                "this group is {height:.1}pt high and runs past the {frame_height:.1}pt text area from its anchor"
            );
            let property = format!("title-page.groups.{index}");
            return Err(vec![
                Diagnostic::new(Some(source.clone()), message).property(Some(property)),
            ]);
        }
        let left = geometry.left_margin(0).0 + group_left(group, frame_width);
        let mut y = geometry.margin_top.0 + top;
        for (space, line) in lines {
            y += space;
            items.extend(line.items.into_iter().map(|item| super::translate(item, left, y)));
            y += line.height;
        }
    }
    Ok(Page {
        width: geometry.width,
        height: geometry.height,
        items,
    })
}

/// The left edge of a group within a frame `frame_width` wide.
pub(super) fn group_left<S>(group: &Group<S>, frame_width: f64) -> f64 {
    if group.anchor.is_right() {
        frame_width - group.x.0 - group.width.0
    } else {
        group.x.0
    }
}

impl<'a> Flow<'a> {
    /// Sets the title block at the start of the body, kept on one page.
    pub(super) fn title(&mut self, slots: &[TitleSlot], frame: Frame<'a>) {
        let width = self.width(frame);
        match self::slots(slots, None, self.config, self.fonts, self.theme_images, width) {
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

/// The lines of the slots that have values, each with the space above it. Lines are set across `width`,
/// aligned by `align` if given, else by their style.
fn slots(
    slots: &[TitleSlot],
    align: Option<Align>,
    config: &Config,
    fonts: &Fonts,
    theme_images: &HashMap<Resource, Image>,
    width: f64,
) -> Result<Vec<(f64, Line)>, String> {
    let mut lines = Vec::new();
    for slot in slots {
        let style = aligned(config.slot_style(&slot.style), align);
        let start = lines.len();
        match &slot.content {
            SlotContent::Text(text) => {
                let Ok(filled) = text.fill(|p| value(&config.metadata, p)) else {
                    continue;
                };
                let mut set = Vec::new();
                for line in &filled {
                    set.extend(text_lines(line, &style, config, fonts, width)?);
                }
                if slot.style == SlotStyle::Template(TemplateStyle::Abstract) {
                    let heading = aligned(&config.styles.abstract_heading, align);
                    let label = &config.labels.abstract_;
                    lines.extend(
                        text_lines(label, &heading, config, fonts, width)?
                            .into_iter()
                            .map(|l| (0.0, l)),
                    );
                    let gap = heading.space_after.0;
                    lines.extend(
                        set.into_iter()
                            .enumerate()
                            .map(|(index, line)| (if index == 0 { gap } else { 0.0 }, line)),
                    );
                } else {
                    lines.extend(set.into_iter().map(|l| (0.0, l)));
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

/// `style` with its alignment replaced by `align`, if given.
fn aligned(style: &Style, align: Option<Align>) -> Cow<'_, Style> {
    match align {
        Some(align) => Cow::Owned(Style { align, ..style.clone() }),
        None => Cow::Borrowed(style),
    }
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
