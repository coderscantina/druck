//! Resolved configuration: tokens substituted, lengths in points, resources with origins.
//!
//! This is what layout consumes. Inline lengths stay relative where `em` refers to the
//! surrounding text, which is only known during layout.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use super::front_matter::Metadata;
use super::source::Resource;
use super::template::Template;
use super::theme::{
    Align, Anchor, DocumentDefaults, Face, FontStyle, LabelSet, RuleBelow, SlotStyle, Styles, Weight, WideBlock,
};
use super::values::{Color, Length, Pt};
use crate::diagnostic::Diagnostic;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub metadata: Metadata,
    pub document: DocumentDefaults,
    pub bibliography_file: Option<Resource>,
    pub page: PageGeometry,
    pub fonts: BTreeMap<String, FontFiles>,
    /// Font families that `fonts` does not define, to be looked up among installed fonts: each face a
    /// style requests, with the first property that requests it.
    pub installed_fonts: BTreeMap<String, BTreeMap<Face, String>>,
    pub images: BTreeMap<String, Resource>,
    pub styles: Styles<Style>,
    pub custom_styles: BTreeMap<String, CustomStyle>,
    pub inline: InlineStyles,
    pub lists: Lists,
    pub tables: Tables,
    pub footnotes: Footnotes,
    pub caption_separator: String,
    pub bibliography: Bibliography,
    pub toc: Toc,
    pub title_block: TitleBlock,
    pub title_page: Vec<Group<TitleSlot>>,
    pub pages: PageVariants,
    pub labels: LabelSet,
    /// Problems that do not stop the run.
    #[serde(skip)]
    pub warnings: Vec<Diagnostic>,
}

impl Config {
    /// Every resource reference with the property that names it.
    pub fn resources(&self) -> Vec<(String, &Resource)> {
        let fonts = self.fonts.iter().flat_map(|(family, files)| {
            files
                .iter()
                .map(move |(face, file)| (format!("fonts.{family}.{face}"), &file.resource))
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

    /// The style a title or band slot names. Resolution has checked that custom names exist.
    pub fn slot_style(&self, style: &SlotStyle) -> &Style {
        match style {
            SlotStyle::Template(style) => self.styles.get(*style),
            SlotStyle::Custom(name) => self.custom_styles[name].style(),
        }
    }
}

/// Page dimensions and the text area, the margin frame. Odd pages put the inner margin on the left,
/// and so do even pages unless margins mirror.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PageGeometry {
    pub width: Pt,
    pub height: Pt,
    pub margin_top: Pt,
    pub margin_bottom: Pt,
    pub margin_inner: Pt,
    pub margin_outer: Pt,
    pub mirror: bool,
    /// The width of prose, from the inner edge of the frame.
    pub prose_width: Pt,
    /// Block kinds set across the frame instead of the prose width.
    pub wide: Vec<WideBlock>,
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

    /// The left margin of the page at physical `index`, counted from 0.
    pub fn left_margin(&self, index: usize) -> Pt {
        if index.is_multiple_of(2) || !self.mirror {
            self.margin_inner
        } else {
            self.margin_outer
        }
    }

    /// How far prose moves right on the page at `index` to start at the inner edge of the frame.
    pub fn prose_shift(&self, index: usize) -> f64 {
        if index.is_multiple_of(2) || !self.mirror {
            0.0
        } else {
            self.text_width().0 - self.prose_width.0
        }
    }
}

/// Slots stacked in a box anchored to the margin frame. Offsets point into the frame.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Group<S> {
    pub anchor: Anchor,
    pub x: Pt,
    pub y: Pt,
    pub width: Pt,
    /// Overrides the alignment of the slot styles.
    pub align: Option<Align>,
    pub slots: Vec<S>,
}

/// Header and footer slot groups per page variant.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PageVariants {
    pub title: Option<PageVariant>,
    pub first: Option<PageVariant>,
    pub odd: Option<PageVariant>,
    pub even: Option<PageVariant>,
    pub body: PageVariant,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PageVariant {
    pub header: Option<Vec<Group<BandSlot>>>,
    pub footer: Option<Vec<Group<BandSlot>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BandSlot {
    pub text: Template,
    /// `None` uses the band's own style.
    pub style: Option<SlotStyle>,
    pub required: bool,
    pub space_before: Pt,
}

/// The files of a family's faces.
pub type FontFiles = BTreeMap<Face, FaceFile>;

/// A font file and the index of the face in it, which is 0 unless the file is a collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FaceFile {
    #[serde(flatten)]
    pub resource: Resource,
    pub index: u32,
}

/// A resolved block style. `font` is a key of [`Config::fonts`] or [`Config::installed_fonts`].
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
    /// Letter spacing in em.
    pub tracking: f64,
    pub uppercase: bool,
    pub keep_with_next: bool,
    /// Space after an automatic heading number; `None` is one space. Only heading styles have it.
    pub number_gap: Option<Pt>,
}

/// A custom style by the kind of block it applies to, which its built-in base decides.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum CustomStyle {
    /// `rule_below` applies where the style styles a table row.
    Paragraph {
        style: Style,
        rule_below: Option<RuleBelow>,
    },
    Heading {
        style: Style,
        number_gap: Option<Pt>,
    },
    List {
        style: Style,
        bullets: Option<Vec<String>>,
    },
}

impl CustomStyle {
    pub fn style(&self) -> &Style {
        match self {
            Self::Paragraph { style, .. } | Self::Heading { style, .. } | Self::List { style, .. } => style,
        }
    }

    pub fn kind(&self) -> BlockKind {
        match self {
            Self::Paragraph { .. } => BlockKind::Paragraph,
            Self::Heading { .. } => BlockKind::Heading,
            Self::List { .. } => BlockKind::List,
        }
    }
}

/// The kind of block a custom style applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading,
    List,
}

impl fmt::Display for BlockKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Paragraph => "a paragraph",
            Self::Heading => "a heading",
            Self::List => "a list",
        })
    }
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
    pub top_rule: Option<Rule>,
    pub header_rule: Option<Rule>,
    pub row_rule: Option<Rule>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Rule {
    pub thickness: Pt,
    pub color: Color,
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
    /// The entry style per heading level, the last repeating for deeper levels; empty for `toc-entry` throughout.
    pub level_styles: Vec<Style>,
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
    pub style: SlotStyle,
    pub required: bool,
    pub space_before: Pt,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotContent {
    Text(Template),
    Image { image: Resource, width: Pt },
}
