//! Positioned page content produced by layout and consumed by PDF output.
//!
//! Coordinates are points from the top-left corner of the page, with y growing downward.

use crate::config::theme::NumberFormat;
use crate::config::values::{Color, Pt};
use crate::document::Link;
use crate::image::Image;
use crate::tags::{Mark, Tree};
use crate::text::ShapedRun;

/// Laid out pages with their navigation: where each anchor ended up and the heading outline.
#[derive(Debug, Clone)]
pub struct Output {
    pub pages: Vec<Page>,
    /// The position of each anchor, by its number.
    pub anchors: Vec<Position>,
    /// Headings in document order, for bookmarks.
    pub outline: Vec<Bookmark>,
    /// The text of the first top-level level 1 heading, the PDF title when the metadata has none.
    pub heading_title: Option<String>,
    /// The number each page shows, for PDF page labels: empty for a document without parts, whose
    /// pages show their physical number, and `None` for a page before the first part.
    pub page_numbers: Vec<Option<PageNumber>>,
    /// The logical structure, whose elements the pages' [`Item::Begin`] marks refer to.
    pub tags: Tree,
    /// The margin notes and sidenotes in page order, top to bottom on each page. Their items are on
    /// the page inside `rect`. The PDF finds notes through their tags, so only tests read this.
    #[cfg_attr(not(test), expect(dead_code, reason = "a layout result that tests inspect"))]
    pub asides: Vec<Aside>,
}

/// A note set in the margin beside the line that references it.
#[derive(Debug, Clone, PartialEq)]
pub struct Aside {
    /// The page index from zero.
    pub page: usize,
    /// The box the note's text is set in.
    pub rect: Rect,
    pub note: NoteKind,
    /// The start of the referencing line, on the baseline of the reference. The note follows the
    /// reference in reading order.
    pub reference: Position,
}

/// What a note in the margin is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// An unnumbered margin note, by its index in `Document::margin_notes`.
    Margin(usize),
    /// A numbered footnote set as a sidenote, by its index in `Document::footnotes`.
    Side(usize),
}

/// A page number and how it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageNumber {
    pub format: NumberFormat,
    pub value: usize,
}

/// A place on a page: the page index from zero and a point on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub page: usize,
    pub x: Pt,
    pub y: Pt,
}

/// A heading in the outline. Levels nest: a level 2 bookmark belongs to the level 1 before it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub level: u8,
    pub title: String,
    pub anchor: usize,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub width: Pt,
    pub height: Pt,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    /// A shaped run whose baseline starts at (`x`, `y`).
    Text {
        x: Pt,
        y: Pt,
        run: ShapedRun,
        color: Color,
    },
    /// A shaped run turned `angle` degrees counterclockwise about the start of its baseline at
    /// (`x`, `y`), used for watermarks.
    TurnedText {
        x: Pt,
        y: Pt,
        angle: f64,
        run: ShapedRun,
        color: Color,
    },
    /// A filled rectangle, used for underlines and rules.
    Rect {
        rect: Rect,
        color: Color,
    },
    /// A clickable area that opens a URL or goes to an anchor.
    Link {
        rect: Rect,
        link: Link,
    },
    /// An image scaled to fill `rect`.
    Image {
        rect: Rect,
        image: Image,
    },
    /// The destination of internal links to anchor `id`, at the top left of what it marks. Not drawn.
    Anchor {
        id: usize,
        x: Pt,
        y: Pt,
    },
    /// Starts what [`Mark`] marks, up to the matching [`Item::End`]: the content of a structure element.
    /// Not drawn.
    Begin(Mark),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: Pt,
    pub y: Pt,
    pub width: Pt,
    pub height: Pt,
}
