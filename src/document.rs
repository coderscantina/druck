//! The document content model: Markdown blocks and inline text after parsing.
//!
//! Inline content is flat. Each text piece carries its complete inline style, so layout
//! never walks a nested inline tree.

use crate::bibliography::syntax;

/// A 1-based line and column in the Markdown file. Columns count characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: u64,
    pub column: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Footnotes in the order of their references, which is also their numbering.
    pub footnotes: Vec<Footnote>,
    /// The distinct image files referenced, in order of first use.
    pub images: Vec<ImageFile>,
    /// The citations in source order. [`Inline::Citation`] refers to them by index.
    pub citations: Vec<Citation>,
}

/// A citation as written, such as `[@a, p. 3; @b]` or `@a`. Its text depends on every citation in the
/// document, so it is formatted after parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct Citation {
    pub at: Location,
    /// The parsed citation. Its byte ranges refer to the scanned text and are not used after parsing.
    pub syntax: syntax::Citation,
    /// Where each item's key is written, in the order of the items.
    pub keys: Vec<Location>,
}

/// An image file as written in the Markdown, relative to the document, with its first reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageFile {
    pub at: Location,
    pub path: String,
}

/// A footnote definition. Its number is its index in [`Document::footnotes`] plus one.
#[derive(Debug, Clone, PartialEq)]
pub struct Footnote {
    pub at: Location,
    pub blocks: Vec<Block>,
}

/// A theme's custom style applied with `{.name}`, where the attribute is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub name: String,
    pub at: Location,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph {
        at: Location,
        content: Vec<Inline>,
        /// A custom style from `{.name}` at the end of the paragraph.
        class: Option<Class>,
    },
    /// `level` is 1 to 6.
    Heading {
        at: Location,
        level: u8,
        content: Vec<Inline>,
        /// A `sec:` label from `{#sec:name}` at the end of the heading.
        label: Option<String>,
        /// A custom style from `{.name}` at the end of the heading.
        class: Option<Class>,
    },
    List {
        at: Location,
        /// The first number of an ordered list; `None` for bullets.
        start: Option<u64>,
        /// Each item is a sequence of blocks. Tight list items hold paragraphs too.
        items: Vec<Vec<Block>>,
        /// A custom style from `{.name}` on the line before the list.
        class: Option<Class>,
    },
    Quote {
        at: Location,
        blocks: Vec<Block>,
    },
    /// A fenced or indented code block. Line `i` starts at source line `first_line + i`.
    Code {
        at: Location,
        first_line: u64,
        lines: Vec<String>,
    },
    /// An image alone in its paragraph: an index into [`Document::images`] and the caption from its
    /// description, empty for none.
    Image {
        at: Location,
        image: usize,
        caption: Vec<Inline>,
        /// A `fig:` label from `{#fig:name}` after the image. Only captioned images have one.
        label: Option<String>,
    },
    /// A pipe table. The header row and every body row have one cell per column.
    Table {
        at: Location,
        /// The alignment of each column; `None` keeps the alignment of the cell style.
        align: Vec<Option<ColumnAlign>>,
        header: Row,
        rows: Vec<Row>,
        /// The caption from a `: Caption` paragraph directly after the table, empty for none.
        caption: Vec<Inline>,
        /// A `tbl:` label from `{#tbl:name}` at the end of the caption.
        label: Option<String>,
    },
    /// `::: keep`: content that stays on one page.
    Keep {
        at: Location,
        blocks: Vec<Block>,
    },
    /// `::: columns`: content in two columns.
    Columns {
        at: Location,
        blocks: Vec<Block>,
    },
    /// `::: full-width`: a block across the text area, directly inside [`Block::Columns`].
    FullWidth {
        at: Location,
        blocks: Vec<Block>,
    },
    /// `::: page-break`.
    PageBreak {
        at: Location,
    },
    /// The bibliography: `::: bibliography`, or added at the end of a document that cites without one.
    Bibliography {
        at: Location,
    },
}

/// A table row with its cells in column order.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub at: Location,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub at: Location,
    pub content: Vec<Inline>,
}

/// A column alignment set in a table's delimiter row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Text without line breaks. Soft breaks become a single space in the surrounding text.
    Text { text: String, style: InlineStyle },
    /// A hard line break.
    LineBreak,
    /// A footnote reference: an index into [`Document::footnotes`].
    FootnoteRef(usize),
    /// A cross-reference to a labelled heading, figure, or table.
    Ref(Reference),
    /// A citation: an index into [`Document::citations`].
    Citation { index: usize, style: InlineStyle },
}

/// A cross-reference such as `@fig:chart`, or `[@fig:chart, page]` for the page it is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// The label with its kind prefix, such as `fig:chart`. The parser checks that it is defined.
    pub label: String,
    /// Whether the reference shows the target's page instead of its number.
    pub page: bool,
    pub style: InlineStyle,
}

/// What a label names, from its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    Section,
    Figure,
    Table,
}

impl LabelKind {
    /// The kind of a prefixed label such as `sec:intro`, or `None` without a known prefix.
    pub fn of(label: &str) -> Option<Self> {
        match label.split_once(':')?.0 {
            "sec" => Some(Self::Section),
            "fig" => Some(Self::Figure),
            "tbl" => Some(Self::Table),
            _ => None,
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            Self::Section => "sec",
            Self::Figure => "fig",
            Self::Table => "tbl",
        }
    }
}

/// A link target. The parser produces URLs; layout links cross-references to anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Url(String),
    /// An internal destination, numbered by layout.
    Anchor(usize),
}

/// The inline formatting that applies to a text piece. Nesting has been flattened.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InlineStyle {
    pub emphasis: bool,
    pub strong: bool,
    pub code: bool,
    /// The target of an enclosing link.
    pub link: Option<Link>,
}
