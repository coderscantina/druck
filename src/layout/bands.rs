//! Headers and footers, drawn on the final pages in the top and bottom margins.
//!
//! Each page uses the first variant present in its chain. The title page: `title`, `body`. The first
//! body page: `first`, then `odd` or `even`, then `body`. Other pages: `odd` or `even`, then `body`.
//! Parity and `{page}` both follow the physical page number, counted from 1 and including the title
//! page.
//!
//! `{section}` is the first level 1 heading that starts on the page, otherwise the last one on an
//! earlier page. `{subsection}` is the first level 2 heading that starts on the page after the
//! section shown there, otherwise the last level 2 heading before the page if no level 1 heading
//! came after it. Both show the heading's number and text.
//!
//! A slot without a value is left empty, unless it is required, which is an error naming the page.
//! Slot text is set on one line. Slots that overlap or run past the text area are an error, since
//! nothing is clipped.

use super::structure::Structure;
use super::{text_item, titles};
use crate::config::resolved::{Config, Style};
use crate::config::template::Placeholder;
use crate::config::theme::{Band, PageVariant};
use crate::diagnostic::Diagnostic;
use crate::page::{Item, Page, Position};
use crate::text::Fonts;

/// Draws the header and footer of every page. `anchors` are the final anchor positions.
pub(super) fn draw(
    pages: &mut [Page],
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
    let mut errors = Vec::new();
    for (index, page) in pages.iter_mut().enumerate() {
        let (name, variant) = variant(config, index, title_page);
        let number = (index + 1).to_string();
        let (section, subsection) = &marks[index];
        let value = |placeholder| match placeholder {
            Placeholder::Section => section.clone(),
            Placeholder::Subsection => subsection.clone(),
            Placeholder::Page => Some(number.clone()),
            other => titles::value(&config.metadata, other),
        };
        let left = if index.is_multiple_of(2) {
            geometry.margin_inner.0
        } else {
            geometry.margin_outer.0
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
        for (band_name, band, style, baseline) in bands {
            let Some(band) = band else { continue };
            let property = format!("pages.{name}.{band_name}");
            let slots = BandSlots {
                band,
                style,
                property: &property,
                page: index + 1,
                left,
                baseline,
            };
            if let Err(error) = slots.draw(&mut page.items, &value, config, fonts) {
                errors.push(error);
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// The variant of the page at `index`, counted from 0, and its name.
fn variant(config: &Config, index: usize, title_page: bool) -> (&'static str, &PageVariant) {
    let variants = &config.pages;
    let parity = if index.is_multiple_of(2) {
        ("odd", variants.odd.as_ref())
    } else {
        ("even", variants.even.as_ref())
    };
    let body = ("body", Some(&variants.body));
    let chain = if title_page && index == 0 {
        vec![("title", variants.title.as_ref()), body]
    } else if index == usize::from(title_page) {
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
struct BandSlots<'a> {
    band: &'a Band,
    style: &'a Style,
    property: &'a str,
    /// The physical page number.
    page: usize,
    /// The left edge of the text area.
    left: f64,
    baseline: f64,
}

impl BandSlots<'_> {
    fn draw(
        &self,
        items: &mut Vec<Item>,
        value: &dyn Fn(Placeholder) -> Option<String>,
        config: &Config,
        fonts: &Fonts,
    ) -> Result<(), Diagnostic> {
        let style = self.style;
        let face = fonts.face(&style.font, style.weight, style.style);
        let width = config.page.text_width().0;
        let error = |property: String, message: String| Diagnostic::new(None, message).property(Some(property));
        let mut extents = Vec::new();
        for (position, slot) in [
            ("left", &self.band.left),
            ("center", &self.band.center),
            ("right", &self.band.right),
        ] {
            let Some(slot) = slot else { continue };
            let property = format!("{}.{position}", self.property);
            let text = match slot.text.fill(value) {
                Ok(text) => text.split_whitespace().collect::<Vec<_>>().join(" "),
                Err(missing) if slot.required => {
                    let message = format!(
                        "this required slot has no value for {{{}}} on page {}",
                        missing.name(),
                        self.page
                    );
                    return Err(error(property, message));
                }
                Err(_) => continue,
            };
            let run = fonts.shape(&text, face, style.size, config.document.lang);
            if let Some(problem) = super::paragraph::missing_glyph(&run) {
                return Err(error(property, format!("{problem}, on page {}", self.page)));
            }
            let x = match position {
                "left" => 0.0,
                "center" => (width - run.width.0) / 2.0,
                _ => width - run.width.0,
            };
            extents.push((x, x + run.width.0));
            items.push(text_item(self.left + x, self.baseline, run, style.color));
        }
        let overlaps = extents.windows(2).any(|pair| pair[0].1 > pair[1].0);
        let outside = extents.iter().any(|&(start, end)| start < 0.0 || end > width);
        if overlaps || outside {
            let message = format!(
                "the slots overlap or run past the {width:.1}pt text width on page {}; shorten their text",
                self.page
            );
            return Err(error(self.property.to_owned(), message));
        }
        Ok(())
    }
}
