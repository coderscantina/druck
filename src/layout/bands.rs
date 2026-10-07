//! Headers and footers, drawn on the final pages in the top and bottom margins.
//!
//! Each page uses the first variant present in its chain. The title page: `title`, `body`. The first
//! body page: `first`, then `odd` or `even`, then `body`. Other pages: `odd` or `even`, then `body`.
//! Parity, `{page}`, and `{pages}` follow the physical pages, counted from 1 and including the title
//! page and blank pages, which have no bands.
//!
//! `{section}` is the first level 1 heading that starts on the page, otherwise the last one on an
//! earlier page. `{subsection}` is the first level 2 heading that starts on the page after the
//! section shown there, otherwise the last level 2 heading before the page if no level 1 heading
//! came after it. Both show the heading's number and text.
//!
//! A band is a list of slot groups. A group's slots are stacked as title slots are, one line per line
//! of their text, which is never wrapped. Groups are anchored to the band baseline across the margin
//! frame: a top anchor puts the group's first baseline `y` below it, a bottom anchor its last baseline
//! `y` above it, and a middle anchor centers its baselines `y` below it. Horizontally a group starts
//! `x` inside the anchored side of the frame.
//!
//! A slot without a value is left out, unless it is required, which is an error naming the page. A
//! line wider than its group and lines of different groups that overlap are errors, since nothing is
//! clipped.

use super::structure::Structure;
use super::{paragraph, text_item, titles};
use crate::config::resolved::{BandSlot, Config, Group, PageVariant, Style};
use crate::config::template::{Placeholder, Value};
use crate::config::theme::{Align, Vertical};
use crate::config::values::Color;
use crate::diagnostic::Diagnostic;
use crate::page::{Item, Page, Position};
use crate::text::{Fonts, ShapedRun};

/// Draws the header and footer of every page but the `blanks`. `first` is the index of the first body
/// page and `anchors` are the final anchor positions.
pub(super) fn draw(
    pages: &mut [Page],
    first: usize,
    blanks: &[usize],
    structure: &Structure,
    anchors: &[Position],
    config: &Config,
    fonts: &Fonts,
) -> Result<(), Vec<Diagnostic>> {
    let geometry = &config.page;
    let headings: Vec<(usize, u8, String)> = structure
        .headings
        .iter()
        .map(|heading| (anchors[heading.anchor].page, heading.level, heading.title()))
        .collect();
    let marks = marks(&headings, pages.len());
    let title_page = config.document.title_page;
    let total = pages.len().to_string();
    let mut errors = Vec::new();
    for (index, page) in pages.iter_mut().enumerate() {
        if blanks.contains(&index) {
            continue;
        }
        let (name, variant) = variant(config, index, title_page, first);
        let number = (index + 1).to_string();
        let (section, subsection) = &marks[index];
        let value = |placeholder: &Placeholder| match placeholder {
            Placeholder::Section => section.clone().map(Value::Text),
            Placeholder::Subsection => subsection.clone().map(Value::Text),
            Placeholder::Page => Some(Value::Text(number.clone())),
            Placeholder::Pages => Some(Value::Text(total.clone())),
            other => titles::value(&config.metadata, other),
        };
        let bands = [
            (
                "header",
                &variant.header,
                &config.styles.header,
                geometry.margin_top.0 - geometry.header_offset.0,
            ),
            (
                "footer",
                &variant.footer,
                &config.styles.footer,
                geometry.margin_top.0 + geometry.text_height().0 + geometry.footer_offset.0,
            ),
        ];
        for (band_name, groups, style, baseline) in bands {
            let Some(groups) = groups else { continue };
            let band = BandLines {
                property: format!("pages.{name}.{band_name}"),
                style,
                page: index + 1,
                left: geometry.left_margin(index).0,
                baseline,
            };
            if let Err(error) = band.draw(groups, &mut page.items, &value, config, fonts) {
                errors.push(error);
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// The variant of the page at `index`, counted from 0, and its name. The body starts at `first`.
fn variant(config: &Config, index: usize, title_page: bool, first: usize) -> (&'static str, &PageVariant) {
    let variants = &config.pages;
    let parity = if index.is_multiple_of(2) {
        ("odd", variants.odd.as_ref())
    } else {
        ("even", variants.even.as_ref())
    };
    let body = ("body", Some(&variants.body));
    let chain = if title_page && index == 0 {
        vec![("title", variants.title.as_ref()), body]
    } else if index == first {
        vec![("first", variants.first.as_ref()), parity, body]
    } else {
        vec![parity, body]
    };
    chain
        .into_iter()
        .find_map(|(name, variant)| Some((name, variant?)))
        .expect("the body variant always exists")
}

/// The `{section}` and `{subsection}` values of each page, from the headings in document order with
/// their page index, level, and title.
fn marks(headings: &[(usize, u8, String)], pages: usize) -> Vec<(Option<String>, Option<String>)> {
    let mut marks = Vec::with_capacity(pages);
    let (mut section, mut subsection): (Option<&str>, Option<&str>) = (None, None);
    let mut next = 0;
    for page in 0..pages {
        let end = next + headings[next..].iter().take_while(|heading| heading.0 == page).count();
        let on_page = &headings[next..end];
        let first_section = on_page.iter().position(|heading| heading.1 == 1);
        let shown_section = first_section.map(|i| on_page[i].2.as_str()).or(section);
        let shown_subsection = match first_section {
            Some(i) => on_page[i + 1..]
                .iter()
                .take_while(|heading| heading.1 != 1)
                .find(|heading| heading.1 == 2)
                .map(|heading| heading.2.as_str()),
            None => on_page
                .iter()
                .find(|heading| heading.1 == 2)
                .map(|heading| heading.2.as_str())
                .or(subsection),
        };
        marks.push((shown_section.map(str::to_owned), shown_subsection.map(str::to_owned)));
        for (_, level, title) in on_page {
            match level {
                1 => (section, subsection) = (Some(title.as_str()), None),
                2 => subsection = Some(title.as_str()),
                _ => {}
            }
        }
        next = end;
    }
    marks
}

/// One header or footer on one page.
struct BandLines<'a> {
    property: String,
    /// The band's own style, for slots that name none.
    style: &'a Style,
    /// The physical page number.
    page: usize,
    /// The left edge of the margin frame.
    left: f64,
    baseline: f64,
}

/// A shaped line of a group: its run and color, its x in the group, and its box from the group's
/// first baseline.
struct BandLine {
    run: ShapedRun,
    color: Color,
    x: f64,
    top: f64,
    baseline: f64,
    bottom: f64,
}

impl BandLines<'_> {
    fn draw(
        &self,
        groups: &[Group<BandSlot>],
        items: &mut Vec<Item>,
        value: &dyn Fn(&Placeholder) -> Option<Value>,
        config: &Config,
        fonts: &Fonts,
    ) -> Result<(), Diagnostic> {
        let error = |property: String, message: String| Diagnostic::new(None, message).property(Some(property));
        let frame = config.page.text_width().0;
        // Line boxes of all groups on the page, to find overlaps.
        let mut boxes: Vec<(f64, f64, f64, f64)> = Vec::new();
        for (index, group) in groups.iter().enumerate() {
            let property = format!("{}.{index}", self.property);
            let lines = self.lines(group, &property, value, config, fonts)?;
            let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
                continue;
            };
            let dy = match group.anchor.vertical() {
                Vertical::Top => group.y.0,
                Vertical::Middle => group.y.0 - (last.baseline - first.baseline) / 2.0,
                Vertical::Bottom => -group.y.0 - (last.baseline - first.baseline),
            };
            let left = titles::group_left(group, frame);
            for line in lines {
                let x = left + line.x;
                let y = self.baseline + dy;
                let placed = (x, x + line.run.set_width().0, y + line.top, y + line.bottom);
                if boxes
                    .iter()
                    .any(|b| b.0 < placed.1 && placed.0 < b.1 && b.2 < placed.3 && placed.2 < b.3)
                {
                    let message = format!(
                        "two slot groups overlap on page {}; shorten their text or move the groups",
                        self.page
                    );
                    return Err(error(self.property.clone(), message));
                }
                boxes.push(placed);
                items.push(text_item(self.left + x, y + line.baseline, line.run, line.color));
            }
        }
        Ok(())
    }

    /// The lines of a group's slots, each a line of its slot text, stacked from the first baseline at 0.
    fn lines(
        &self,
        group: &Group<BandSlot>,
        property: &str,
        value: &dyn Fn(&Placeholder) -> Option<Value>,
        config: &Config,
        fonts: &Fonts,
    ) -> Result<Vec<BandLine>, Diagnostic> {
        let error = |property: String, message: String| Diagnostic::new(None, message).property(Some(property));
        let width = group.width.0;
        let mut lines: Vec<BandLine> = Vec::new();
        for (index, slot) in group.slots.iter().enumerate() {
            let property = format!("{property}.slots.{index}");
            let style = slot.style.as_ref().map_or(self.style, |style| config.slot_style(style));
            let texts = match slot.text.fill(value) {
                Ok(texts) => texts,
                Err(missing) if slot.required => {
                    let message = format!(
                        "this required slot has no value for {{{missing}}} on page {}",
                        self.page
                    );
                    return Err(error(property, message));
                }
                Err(_) => continue,
            };
            let face = fonts.face(&style.font, style.weight, style.style);
            let height = style.size.0 * style.line_height;
            let offset = paragraph::baseline(height, &fonts.metrics(face, style.size));
            let mut space = slot.space_before.0;
            for text in texts.iter().filter(|text| !text.trim().is_empty()) {
                let mut text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if style.uppercase {
                    text = text.to_uppercase();
                }
                let run = fonts.shape_tracked(&text, face, style.size, config.document.lang, style.tracking);
                if let Some(problem) = paragraph::missing_glyph(&run) {
                    return Err(error(property, format!("{problem}, on page {}", self.page)));
                }
                let set = run.set_width().0;
                if set > width {
                    let message = format!(
                        "\"{text}\" is {set:.1}pt wide, more than the {width:.1}pt of its group, on page {}; shorten the text",
                        self.page
                    );
                    return Err(error(property, message));
                }
                let x = match group.align.unwrap_or(style.align) {
                    Align::Left | Align::Justify => 0.0,
                    Align::Center => (width - set) / 2.0,
                    Align::Right => width - set,
                };
                let top = match lines.last() {
                    Some(previous) => previous.bottom + space,
                    None => -offset,
                };
                lines.push(BandLine {
                    run,
                    color: style.color,
                    x,
                    top,
                    baseline: top + offset,
                    bottom: top + height,
                });
                space = 0.0;
            }
        }
        Ok(lines)
    }
}
