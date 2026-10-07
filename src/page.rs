//! Positioned page content produced by layout and consumed by PDF output.
//!
//! Coordinates are points from the top-left corner of the page, with y growing downward.

use crate::config::values::{Color, Pt};
use crate::image::Image;
use crate::text::ShapedRun;

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
    /// A filled rectangle, used for underlines and rules.
    Rect { rect: Rect, color: Color },
    /// A clickable area that opens an external URL.
    Link { rect: Rect, url: String },
    /// An image scaled to fill `rect`.
    Image { rect: Rect, image: Image },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: Pt,
    pub y: Pt,
    pub width: Pt,
    pub height: Pt,
}
