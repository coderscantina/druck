//! Positioned page content produced by layout and consumed by PDF output.
//!
//! Coordinates are points from the top-left corner of the page, with y growing downward.

use crate::config::values::{Color, Pt};
use crate::document::Link;
use crate::image::Image;
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
    Text { x: Pt, y: Pt, run: ShapedRun, color: Color },
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
    Rect { rect: Rect, color: Color },
    /// A clickable area that opens a URL or goes to an anchor.
    Link { rect: Rect, link: Link },
    /// An image scaled to fill `rect`.
    Image { rect: Rect, image: Image },
    /// The destination of internal links to anchor `id`, at the top left of what it marks. Not drawn.
    Anchor { id: usize, x: Pt, y: Pt },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: Pt,
    pub y: Pt,
    pub width: Pt,
    pub height: Pt,
}
