//! Typed theme structures, matching `schema/theme.v1.schema.json`.
//!
//! These types describe a complete theme. Partial themes are merged onto the bundled
//! default before deserialization, so every layer is checked against the same types.
//! Fields typed `Option` are the only places where `null` is accepted.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::non_null;
use super::template::Template;
use super::values::{Color, FontName, LineHeight, Size, Spacing, Spec, TokenName};

pub const THEME_VERSION: u64 = 1;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Theme {
    /// Editor hint for the schema location. Ignored by the renderer.
    #[allow(dead_code, reason = "accepted so editors can validate themes")]
    #[serde(rename = "$schema", default, deserialize_with = "non_null", skip_serializing)]
    pub schema: Option<String>,
    pub version: u64,
    pub document: DocumentDefaults,
    pub fonts: BTreeMap<String, FontFamily>,
    pub images: BTreeMap<String, ResourcePath>,
    pub tokens: Tokens,
    pub page: Page,
    pub styles: Styles<BlockStyle>,
    pub inline: InlineStyles,
    pub lists: Lists,
    pub tables: Tables,
    pub footnotes: Footnotes,
    pub captions: Captions,
    pub bibliography: Bibliography,
    pub toc: Toc,
    pub title_block: TitleBlock,
    pub title_page: TitleLayout,
    pub pages: PageVariants,
    pub labels: Labels,
}

/// Document structure defaults. Front matter and CLI overrides may replace each field.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DocumentDefaults {
    pub lang: Lang,
    pub title_page: bool,
    pub toc: bool,
    pub numbered_headings: bool,
    pub numbering_depth: HeadingDepth,
    pub toc_depth: HeadingDepth,
    pub citation_style: CitationStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lang {
    En,
    De,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CitationStyle {
    AuthorDate,
    Numeric,
}

/// A heading level from 1 to 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HeadingDepth(u8);

impl HeadingDepth {
    pub fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for HeadingDepth {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let depth = u8::deserialize(deserializer)?;
        if !(1..=6).contains(&depth) {
            return Err(serde::de::Error::custom(format!(
                "heading depth {depth} must be from 1 to 6"
            )));
        }
        Ok(Self(depth))
    }
}

/// Font files for one family. Missing faces are errors only when a style requests them.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FontFamily {
    pub regular: ResourcePath,
    pub italic: Option<ResourcePath>,
    pub bold: Option<ResourcePath>,
    pub bold_italic: Option<ResourcePath>,
}

/// A local file path relative to the origin of the layer that supplied it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourcePath(pub String);

impl<'de> Deserialize<'de> for ResourcePath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let path = String::deserialize(deserializer)?;
        if path.trim().is_empty() {
            return Err(serde::de::Error::custom("resource path must not be empty"));
        }
        if path.contains("://") {
            return Err(serde::de::Error::custom(format!(
                "resource \"{path}\" must be a local file path; remote resources are not fetched"
            )));
        }
        Ok(Self(path))
    }
}

/// Named design tokens. Tokens may reference other tokens of the same group.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Tokens {
    pub fonts: BTreeMap<TokenName, Spec<FontName>>,
    pub sizes: BTreeMap<TokenName, Spec<Size>>,
    pub spacing: BTreeMap<TokenName, Spec<Spacing>>,
    pub colors: BTreeMap<TokenName, Spec<Color>>,
}

/// Page geometry. `em` lengths here refer to the body font size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Page {
    pub size: PageSize,
    pub margins: Margins,
    /// Prose width from the inner edge of the margin frame. `null` uses the frame width.
    pub text_width: Option<Spec<Spacing>>,
    /// Block kinds set across the whole frame when prose is narrower.
    pub wide: Vec<WideBlock>,
    pub column_gap: Spec<Spacing>,
    /// Distance from the top of the text area to the header baseline.
    pub header_offset: Spec<Spacing>,
    /// Distance from the bottom of the text area to the footer baseline.
    pub footer_offset: Spec<Spacing>,
}

/// A named paper size, or `width` and `height`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum PageSize {
    Named(PaperSize),
    Custom(CustomPageSize),
}

impl<'de> Deserialize<'de> for PageSize {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{self, value};

        struct PageSizeVisitor;

        impl<'de> de::Visitor<'de> for PageSizeVisitor {
            type Value = PageSize;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a paper size (a4, a5, b5, letter, legal) or a map of width and height")
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<PageSize, E> {
                PaperSize::deserialize(value::StrDeserializer::new(name)).map(PageSize::Named)
            }

            fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<PageSize, A::Error> {
                CustomPageSize::deserialize(value::MapAccessDeserializer::new(map)).map(PageSize::Custom)
            }
        }

        deserializer.deserialize_any(PageSizeVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaperSize {
    A4,
    A5,
    B5,
    Letter,
    Legal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CustomPageSize {
    pub width: Spec<Spacing>,
    pub height: Spec<Spacing>,
}

/// Inner is the left margin on odd pages. With `mirror` it is the right one on even pages.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Margins {
    pub top: Spec<Spacing>,
    pub bottom: Spec<Spacing>,
    pub inner: Spec<Spacing>,
    pub outer: Spec<Spacing>,
    pub mirror: bool,
}

/// A block kind that may use the whole frame width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WideBlock {
    Table,
    Figure,
    CodeBlock,
}

/// Block element styles. Generic so raw and resolved configuration share one element list.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Styles<B> {
    pub body: B,
    pub heading_1: B,
    pub heading_2: B,
    pub heading_3: B,
    pub heading_4: B,
    pub heading_5: B,
    pub heading_6: B,
    pub title: B,
    pub subtitle: B,
    pub author: B,
    pub date: B,
    pub abstract_heading: B,
    #[serde(rename = "abstract")]
    pub abstract_: B,
    pub quote: B,
    pub list: B,
    pub code_block: B,
    pub caption: B,
    pub table_cell: B,
    pub table_header: B,
    pub footnote: B,
    pub bibliography: B,
    pub toc_heading: B,
    pub toc_entry: B,
    pub header: B,
    pub footer: B,
}

impl<B> Styles<B> {
    /// Applies `f` to every element in a fixed order, keeping the element name for diagnostics.
    pub fn try_map<C, E>(self, mut f: impl FnMut(&'static str, B) -> Result<C, E>) -> Result<Styles<C>, E> {
        Ok(Styles {
            body: f("body", self.body)?,
            heading_1: f("heading-1", self.heading_1)?,
            heading_2: f("heading-2", self.heading_2)?,
            heading_3: f("heading-3", self.heading_3)?,
            heading_4: f("heading-4", self.heading_4)?,
            heading_5: f("heading-5", self.heading_5)?,
            heading_6: f("heading-6", self.heading_6)?,
            title: f("title", self.title)?,
            subtitle: f("subtitle", self.subtitle)?,
            author: f("author", self.author)?,
            date: f("date", self.date)?,
            abstract_heading: f("abstract-heading", self.abstract_heading)?,
            abstract_: f("abstract", self.abstract_)?,
            quote: f("quote", self.quote)?,
            list: f("list", self.list)?,
            code_block: f("code-block", self.code_block)?,
            caption: f("caption", self.caption)?,
            table_cell: f("table-cell", self.table_cell)?,
            table_header: f("table-header", self.table_header)?,
            footnote: f("footnote", self.footnote)?,
            bibliography: f("bibliography", self.bibliography)?,
            toc_heading: f("toc-heading", self.toc_heading)?,
            toc_entry: f("toc-entry", self.toc_entry)?,
            header: f("header", self.header)?,
            footer: f("footer", self.footer)?,
        })
    }

    pub fn get(&self, element: TemplateStyle) -> &B {
        match element {
            TemplateStyle::Title => &self.title,
            TemplateStyle::Subtitle => &self.subtitle,
            TemplateStyle::Author => &self.author,
            TemplateStyle::Date => &self.date,
            TemplateStyle::AbstractHeading => &self.abstract_heading,
            TemplateStyle::Abstract => &self.abstract_,
            TemplateStyle::Body => &self.body,
        }
    }
}

/// Style of a block element. `size` em refers to the body size; other em lengths to `size`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct BlockStyle {
    pub font: Spec<FontName>,
    pub size: Spec<Size>,
    pub weight: Weight,
    pub style: FontStyle,
    pub color: Spec<Color>,
    pub line_height: LineHeight,
    pub align: Align,
    pub hyphenate: bool,
    pub space_before: Spec<Spacing>,
    pub space_after: Spec<Spacing>,
    pub indent: Spec<Spacing>,
    pub first_line_indent: Spec<Spacing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Weight {
    Regular,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontStyle {
    Normal,
    Italic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    Justify,
    Left,
    Center,
    Right,
}

/// Inline styles. Their `em` lengths refer to the surrounding text size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct InlineStyles {
    pub code: InlineCode,
    pub link: Link,
    pub footnote_marker: FootnoteMarker,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct InlineCode {
    pub font: Spec<FontName>,
    pub size: Spec<Size>,
    pub color: Spec<Color>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Link {
    pub color: Spec<Color>,
    pub underline: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FootnoteMarker {
    pub size: Spec<Size>,
    pub raise: Spec<Spacing>,
}

/// List layout. `em` lengths refer to the list style size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Lists {
    pub indent: Spec<Spacing>,
    pub item_spacing: Spec<Spacing>,
    /// Bullet per nesting level; deeper levels repeat the last entry.
    pub bullets: Vec<String>,
}

/// Table layout. `em` lengths refer to the table cell style size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Tables {
    pub cell_padding: Spec<Spacing>,
    pub rule_thickness: Spec<Spacing>,
    pub rule_color: Spec<Color>,
}

/// Footnote area. `em` lengths refer to the footnote style size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Footnotes {
    /// Minimum space between the body text and the footnote separator.
    pub gap: Spec<Spacing>,
    pub separator_width: Spec<Spacing>,
    pub separator_thickness: Spec<Spacing>,
    pub separator_color: Spec<Color>,
    pub spacing: Spec<Spacing>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Captions {
    /// Text between the numbered label and the caption, as in "Figure 1: ".
    pub separator: String,
}

/// Bibliography layout. `em` lengths refer to the bibliography style size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Bibliography {
    pub hanging_indent: Spec<Spacing>,
    pub entry_spacing: Spec<Spacing>,
}

/// Table of contents layout. `em` lengths refer to the toc entry style size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Toc {
    pub level_indent: Spec<Spacing>,
    pub leader: bool,
}

/// Ordered content slots for the title block at the start of the body, and the space below it. `em`
/// refers to the body size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TitleBlock {
    pub slots: Vec<TitleSlot>,
    pub space_after: Spec<Spacing>,
}

/// Anchored slot groups for the separate title page.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TitleLayout {
    pub groups: Vec<Group<TitleSlot>>,
}

/// Slots stacked in a box anchored to the margin frame. `em` lengths refer to the body size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Group<S> {
    pub anchor: Anchor,
    #[serde(default)]
    pub offset: Offset,
    /// Defaults to the frame width less the horizontal offset.
    #[serde(default, deserialize_with = "non_null")]
    pub width: Option<Spec<Spacing>>,
    /// Defaults to the alignment of each slot's style.
    #[serde(default, deserialize_with = "non_null")]
    pub align: Option<Align>,
    pub slots: Vec<S>,
}

/// A point on the margin frame: the top, middle, or bottom of its left or right edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    TopRight,
    MiddleLeft,
    MiddleRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vertical {
    Top,
    Middle,
    Bottom,
}

impl Anchor {
    pub fn vertical(self) -> Vertical {
        match self {
            Self::TopLeft | Self::TopRight => Vertical::Top,
            Self::MiddleLeft | Self::MiddleRight => Vertical::Middle,
            Self::BottomLeft | Self::BottomRight => Vertical::Bottom,
        }
    }

    pub fn is_right(self) -> bool {
        matches!(self, Self::TopRight | Self::MiddleRight | Self::BottomRight)
    }
}

/// Distance from the anchor into the frame: `x` from the anchored edge, `y` down from the top or
/// middle, or up from the bottom.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Offset {
    #[serde(default = "Spec::zero")]
    pub x: Spec<Spacing>,
    #[serde(default = "Spec::zero")]
    pub y: Spec<Spacing>,
}

impl Default for Offset {
    fn default() -> Self {
        Self {
            x: Spec::zero(),
            y: Spec::zero(),
        }
    }
}

/// One title slot: either text or a theme image. `em` lengths refer to the slot style size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TitleSlot {
    #[serde(default, deserialize_with = "non_null")]
    pub text: Option<Template>,
    /// Name of an entry in the theme `images` map.
    #[serde(default, deserialize_with = "non_null")]
    pub image: Option<String>,
    /// Image width. Required for image slots, rejected for text slots.
    #[serde(default, deserialize_with = "non_null")]
    pub width: Option<Spec<Spacing>>,
    #[serde(default = "TemplateStyle::body")]
    pub style: TemplateStyle,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "Spec::zero")]
    pub space_before: Spec<Spacing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateStyle {
    Title,
    Subtitle,
    Author,
    Date,
    AbstractHeading,
    Abstract,
    Body,
}

impl TemplateStyle {
    fn body() -> Self {
        Self::Body
    }
}

impl Spec<Spacing> {
    fn zero() -> Self {
        Self::Literal(Spacing(super::values::Length::Pt(0.0)))
    }
}

/// Page variants. `null` removes a variant so the fallback order applies.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PageVariants {
    pub title: Option<PageVariant>,
    pub first: Option<PageVariant>,
    pub odd: Option<PageVariant>,
    pub even: Option<PageVariant>,
    pub body: PageVariant,
}

/// Header and footer of one page variant. `null` means no header or footer.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PageVariant {
    pub header: Option<Band>,
    pub footer: Option<Band>,
}

/// The slot groups of a header or footer.
pub type Band = Vec<Group<BandSlot>>;

/// One header or footer slot. `em` lengths refer to the slot style size.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct BandSlot {
    pub text: Template,
    /// Defaults to the `header` or `footer` style.
    #[serde(default, deserialize_with = "non_null")]
    pub style: Option<TemplateStyle>,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "Spec::zero")]
    pub space_before: Spec<Spacing>,
}

/// Generated text per document language.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Labels {
    pub en: LabelSet,
    pub de: LabelSet,
}

impl Labels {
    pub fn get(&self, lang: Lang) -> &LabelSet {
        match lang {
            Lang::En => &self.en,
            Lang::De => &self.de,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct LabelSet {
    pub figure: String,
    pub table: String,
    pub section: String,
    pub page: String,
    pub contents: String,
    #[serde(rename = "abstract")]
    pub abstract_: String,
    pub references: String,
    pub continued: String,
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::En => "en",
            Self::De => "de",
        })
    }
}
