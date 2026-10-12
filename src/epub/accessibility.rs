//! EPUB Accessibility 1.1 metadata: how the book can be read, and whether it meets WCAG 2.1 Level AA as far
//! as the document and the theme show. Markup, language, navigation, and resizable text hold for every book
//! Druck writes; the descriptions of images and the contrast of text colors depend on the input, so the
//! book claims conformance only when both pass.

use crate::config::resolved::Config;
use crate::config::theme::Lang;
use crate::config::values::Color;
use crate::document::{Block, Document};
use crate::layout::structure::plain;

/// The conformance a book claims when it qualifies.
const CONFORMANCE: &str = "EPUB Accessibility 1.1 - WCAG 2.1 Level AA";
/// The least contrast WCAG 2.1 Level AA asks of normal text.
const CONTRAST: f64 = 4.5;
const WHITE: Color = Color([255, 255, 255]);

/// The package metadata as (property, value) pairs, in order. `cover` is whether the book has a cover
/// image, which is described by the title.
pub fn metadata(document: &Document, config: &Config, cover: bool) -> Vec<(&'static str, String)> {
    let mut images = Vec::new();
    collect(&document.blocks, &mut images);
    for note in &document.footnotes {
        collect(&note.blocks, &mut images);
    }
    let described = images.iter().filter(|described| **described).count() + usize::from(cover);
    let undescribed = images.len() + usize::from(cover) - described;
    let visual = described + undescribed > 0;
    let headings =
        has_heading(&document.blocks) || document.blocks.iter().any(|b| matches!(b, Block::Bibliography { .. }));
    let contrast = legible(config);

    let mut properties = vec![("schema:accessMode", "textual".to_owned())];
    if visual {
        properties.push(("schema:accessMode", "visual".to_owned()));
    }
    if undescribed == 0 {
        properties.push(("schema:accessModeSufficient", "textual".to_owned()));
    }
    if visual {
        properties.push(("schema:accessModeSufficient", "textual,visual".to_owned()));
    }
    let mut features = vec!["tableOfContents", "readingOrder", "displayTransformability", "ARIA"];
    if headings {
        features.push("structuralNavigation");
    }
    if described > 0 {
        features.push("alternativeText");
    }
    properties.extend(
        features
            .into_iter()
            .map(|feature| ("schema:accessibilityFeature", feature.to_owned())),
    );
    properties.push(("schema:accessibilityHazard", "none".to_owned()));
    let conforms = undescribed == 0 && contrast;
    properties.push((
        "schema:accessibilitySummary",
        summary(config.document.lang, visual, undescribed, contrast),
    ));
    if conforms {
        properties.push(("dcterms:conformsTo", CONFORMANCE.to_owned()));
        properties.push(("a11y:certifiedBy", "Druck".to_owned()));
    }
    properties
}

/// Whether each image in `blocks` has a description, in document order.
fn collect(blocks: &[Block], images: &mut Vec<bool>) {
    for block in blocks {
        match block {
            Block::Image { caption, .. } => images.push(!plain(caption).trim().is_empty()),
            Block::List { items, .. } => items.iter().for_each(|item| collect(item, images)),
            Block::Quote { blocks, .. }
            | Block::Keep { blocks, .. }
            | Block::Bottom { blocks, .. }
            | Block::Columns { blocks, .. }
            | Block::FullWidth { blocks, .. } => collect(blocks, images),
            Block::Table { header, rows, .. } => {
                for row in std::iter::once(header).chain(rows) {
                    row.cells.iter().for_each(|cell| collect(&cell.blocks, images));
                }
            }
            _ => {}
        }
    }
}

fn has_heading(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Heading { .. } => true,
        Block::Keep { blocks, .. }
        | Block::Bottom { blocks, .. }
        | Block::Columns { blocks, .. }
        | Block::FullWidth { blocks, .. } => has_heading(blocks),
        _ => false,
    })
}

/// Whether every text color the book's stylesheet uses has enough contrast on a white page, and inline code
/// on its background. Running heads and the watermark are not in the book.
fn legible(config: &Config) -> bool {
    let mut colors = Vec::new();
    let styles = config.styles.clone().try_map(|name, style| {
        if !matches!(name, "header" | "footer" | "watermark") {
            colors.push(style.color);
        }
        Ok::<_, ()>(())
    });
    debug_assert!(styles.is_ok());
    colors.extend(config.custom_styles.values().map(|style| style.style().color));
    colors.push(config.inline.link_color);
    let code = config.inline.code_background.unwrap_or(WHITE);
    colors.iter().all(|color| contrast(*color, WHITE) >= CONTRAST)
        && contrast(config.inline.code_color, code) >= CONTRAST
}

/// The WCAG contrast ratio of two colors, from 1 to 21.
fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The relative luminance of an sRGB color, as WCAG defines it.
fn luminance(Color(rgb): Color) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

/// A summary in the book's language of what it offers and why it claims conformance or not.
fn summary(lang: Lang, visual: bool, undescribed: usize, contrast: bool) -> String {
    let (base, all, some, meets, no_images, no_contrast) = match lang {
        Lang::En => (
            "Headings, lists, tables, notes, and the table of contents are marked up for navigation, and the \
             text can be resized.",
            "Every image has a text description.",
            "Some images have no text description.",
            "The publication meets WCAG 2.1 Level AA as far as Druck checks it.",
            "It does not claim WCAG conformance, since images without a description fail it.",
            "It does not claim WCAG conformance, since some text colors have too little contrast.",
        ),
        Lang::De => (
            "Überschriften, Listen, Tabellen, Anmerkungen und das Inhaltsverzeichnis sind für die Navigation \
             ausgezeichnet, und die Schriftgröße lässt sich ändern.",
            "Jedes Bild hat eine Textbeschreibung.",
            "Einige Bilder haben keine Textbeschreibung.",
            "Die Publikation erfüllt WCAG 2.1 Stufe AA, soweit Druck es prüft.",
            "Sie beansprucht keine WCAG-Konformität, da Bilder ohne Beschreibung sie verletzen.",
            "Sie beansprucht keine WCAG-Konformität, da einige Textfarben zu wenig Kontrast haben.",
        ),
    };
    let mut text = base.to_owned();
    if visual {
        text.push(' ');
        text.push_str(if undescribed == 0 { all } else { some });
    }
    text.push(' ');
    text.push_str(match (undescribed == 0, contrast) {
        (true, true) => meets,
        (false, _) => no_images,
        (true, false) => no_contrast,
    });
    text
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::tests::config;
    use super::*;
    use crate::config::source::Source;

    fn properties(theme: Value, body: &str) -> Vec<(&'static str, String)> {
        let document =
            crate::markdown::parse(body, 1, &Source::Document("/fake/book.md".into()), Lang::En).expect("parses");
        metadata(&document, &config("", theme), false)
    }

    fn values<'a>(properties: &'a [(&'static str, String)], property: &str) -> Vec<&'a str> {
        let matching = properties.iter().filter(|(name, _)| *name == property);
        matching.map(|(_, value)| value.as_str()).collect()
    }

    #[test]
    fn claims_wcag_only_with_described_images_and_legible_text_colors() {
        let theme = json!({"version": 1});
        let described = properties(theme.clone(), "# One\n\n![Sales by month](chart.png)\n");
        assert_eq!(values(&described, "schema:accessMode"), ["textual", "visual"]);
        assert_eq!(
            values(&described, "schema:accessModeSufficient"),
            ["textual", "textual,visual"]
        );
        let features = values(&described, "schema:accessibilityFeature");
        assert!(features.contains(&"alternativeText") && features.contains(&"structuralNavigation"));
        assert_eq!(values(&described, "schema:accessibilityHazard"), ["none"]);
        assert_eq!(values(&described, "dcterms:conformsTo"), [CONFORMANCE]);

        let undescribed = properties(theme, "# One\n\n![](chart.png)\n");
        assert_eq!(values(&undescribed, "schema:accessModeSufficient"), ["textual,visual"]);
        assert!(values(&undescribed, "dcterms:conformsTo").is_empty());
        assert!(values(&undescribed, "schema:accessibilitySummary")[0].contains("images without a description"));

        // #777777 on white has a contrast of 4.48, below the 4.5 that Level AA asks of text.
        let pale = properties(
            json!({"version": 1, "inline": {"link": {"color": "#777777"}}}),
            "Text.\n",
        );
        assert_eq!(values(&pale, "schema:accessMode"), ["textual"]);
        assert!(values(&pale, "dcterms:conformsTo").is_empty());
    }
}
