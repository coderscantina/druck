//! Resolved configuration: tokens substituted, lengths in points, resources with origins.
//!
//! This is what layout consumes. Inline lengths stay relative where `em` refers to the
//! surrounding text, which is only known during layout.

use std::collections::BTreeMap;

use serde::Serialize;

use super::front_matter::Metadata;
use super::source::Resource;
use super::template::Template;
use super::theme::{Align, DocumentDefaults, FontStyle, LabelSet, PageVariants, Styles, TemplateStyle, Weight};
use super::values::{Color, Length, Pt};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub metadata: Metadata,
    pub document: DocumentDefaults,
    pub bibliography_file: Option<Resource>,
    pub page: PageGeometry,
    pub fonts: BTreeMap<String, FontFiles>,
    pub images: BTreeMap<String, Resource>,
    pub styles: Styles<Style>,
    pub inline: InlineStyles,
    pub lists: Lists,
    pub tables: Tables,
    pub footnotes: Footnotes,
    pub caption_separator: String,
    pub bibliography: Bibliography,
    pub toc: Toc,
    pub title_block: TitleBlock,
    pub title_page: Vec<TitleSlot>,
    pub pages: PageVariants,
    pub labels: LabelSet,
}

impl Config {
    /// Every resource reference with the property that names it.
    pub fn resources(&self) -> Vec<(String, &Resource)> {
        let fonts = self.fonts.iter().flat_map(|(family, files)| {
            let faces = [
                ("regular", Some(&files.regular)),
                ("italic", files.italic.as_ref()),
                ("bold", files.bold.as_ref()),
                ("bold-italic", files.bold_italic.as_ref()),
            ];
            faces
                .into_iter()
                .filter_map(move |(face, file)| Some((format!("fonts.{family}.{face}"), file?)))
        });
        let images = self
            .images
            .iter()
            .map(|(name, image)| (format!("images.{name}"), image));
        let bibliography = self
            .bibliography_file
            .iter()
            .map(|file| ("bibliography".to_owned(), file));
        fonts.chain(images).chain(bibliography).collect()
    }
}

/// Page dimensions and the text area. Odd pages put the inner margin on the left.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PageGeometry {
    pub width: Pt,
    pub height: Pt,
    pub margin_top: Pt,
    pub margin_bottom: Pt,
    pub margin_inner: Pt,
    pub margin_outer: Pt,
    pub column_gap: Pt,
    pub header_offset: Pt,
    pub footer_offset: Pt,
}

impl PageGeometry {
    pub fn text_width(&self) -> Pt {
        Pt(self.width.0 - self.margin_inner.0 - self.margin_outer.0)
    }

    pub fn text_height(&self) -> Pt {
        Pt(self.height.0 - self.margin_top.0 - self.margin_bottom.0)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct FontFiles {
    pub regular: Resource,
    pub italic: Option<Resource>,
    pub bold: Option<Resource>,
    pub bold_italic: Option<Resource>,
}

/// A resolved block style. `font` is a key of [`Config::fonts`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Style {
    pub font: String,
    pub size: Pt,
    pub weight: Weight,
    pub style: FontStyle,
    pub color: Color,
    pub line_height: f64,
    pub align: Align,
    pub hyphenate: bool,
    pub space_before: Pt,
    pub space_after: Pt,
    pub indent: Pt,
    pub first_line_indent: Pt,
}

/// Inline styles. Their `em` lengths refer to the surrounding text size.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct InlineStyles {
    pub code_font: String,
    pub code_size: Length,
    pub code_color: Color,
    pub link_color: Color,
    pub link_underline: bool,
    pub footnote_marker_size: Length,
    pub footnote_marker_raise: Length,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Lists {
    pub indent: Pt,
    pub item_spacing: Pt,
    pub bullets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Tables {
    pub cell_padding: Pt,
    pub rule_thickness: Pt,
    pub rule_color: Color,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Footnotes {
    pub gap: Pt,
    pub separator_width: Pt,
    pub separator_thickness: Pt,
    pub separator_color: Color,
    pub spacing: Pt,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Bibliography {
    pub hanging_indent: Pt,
    pub entry_spacing: Pt,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Toc {
    pub level_indent: Pt,
    pub leader: bool,
}

/// The title block's slots and the space below it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TitleBlock {
    pub slots: Vec<TitleSlot>,
    pub space_after: Pt,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TitleSlot {
    pub content: SlotContent,
    pub style: TemplateStyle,
    pub required: bool,
    pub space_before: Pt,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotContent {
    Text(Template),
    Image { image: Resource, width: Pt },
}
