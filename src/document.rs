//! The document content model: Markdown blocks and inline text after parsing.
//!
//! Inline content is flat. Each text piece carries its complete inline style, so layout
//! never walks a nested inline tree.

/// A 1-based line and column in the Markdown file. Columns count characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: u64,
    pub column: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph {
        at: Location,
        content: Vec<Inline>,
    },
    /// `level` is 1 to 6.
    Heading {
        at: Location,
        level: u8,
        content: Vec<Inline>,
    },
    List {
        at: Location,
        /// The first number of an ordered list; `None` for bullets.
        start: Option<u64>,
        /// Each item is a sequence of blocks. Tight list items hold paragraphs too.
        items: Vec<Vec<Block>>,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Text without line breaks. Soft breaks become a single space in the surrounding text.
    Text { text: String, style: InlineStyle },
    /// A hard line break.
    LineBreak,
}

/// The inline formatting that applies to a text piece. Nesting has been flattened.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InlineStyle {
    pub emphasis: bool,
    pub strong: bool,
    pub code: bool,
    /// The target of an enclosing link.
    pub link: Option<String>,
}
