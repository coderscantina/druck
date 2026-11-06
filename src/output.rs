//! The result of layout: an immutable display list that owns what it needs to be drawn or exported.
//!
//! # Coordinates and units
//!
//! Lengths are points, 1/72 inch. Each page has its own coordinates, with the origin at the top left corner of the
//! trim box and y growing down. [`Page::width`] and [`Page::height`] are the trim size: bleed and crop marks exist
//! only in [`crate::PdfOptions`].
//!
//! # Drawing order
//!
//! [`Page::items`] draw in order, later over earlier. [`Item::Link`] and [`Item::Anchor`] are places, not ink.
//! [`Item::Begin`] and [`Item::End`] mark the logical structure and a painter skips them.
//!
//! # Glyphs
//!
//! A [`Run`] draws from `(x, y)` on the baseline. Start with `pen = x`. For each [`Glyph`], draw the outline of
//! [`Glyph::id`] from [`Face::data`], at [`Face::index`] and the variation [`Face::coordinates`], scaled by
//! `run.size / units_per_em`, at `(pen + x_offset, y - y_offset)`, then add `x_advance` to `pen`. No shaping or font
//! lookup is needed. [`Glyph::text`] is the byte range of [`Run::text`] the glyph stands for, empty for the second
//! and later glyphs of a cluster. [`Item::TurnedText`] turns its run `angle` degrees counterclockwise about `(x, y)`.
//!
//! # Resources
//!
//! A [`Layout`] holds its fonts and images through shared, immutable storage. It stays valid after the input, the
//! resolver, and the renderer are gone, and exporting it reads no resource. [`Picture::source`] is the file as read,
//! and [`Picture::pixels`] says what a painter draws.
//!
//! # Source mapping
//!
//! With [`crate::LayoutOptions::source_map`], [`Layout::source_map`] connects text items to the written text, see
//! [`SourceMap`]. Without it, every [`Item::Text`] has no `source`.

use std::collections::BTreeSet;

use crate::cancel::Cancel;
use crate::config::front_matter::Metadata;
use crate::config::source::InputSource;
pub use crate::config::theme::NumberFormat;
use crate::config::theme::{Conformance, FontStyle, Lang, Weight};
pub use crate::config::values::{Color, Pt};
use crate::date::Date;
use crate::diagnostic::Diagnostic;
pub use crate::document::Link;
pub use crate::image::{Image as Picture, Pixels};
use crate::renderer::{Failure, PdfOptions, Rendered};
pub use crate::source_map::{
    BlockId, BlockKind, BlockSource, Chapter, ChapterId, SourceMap, SourcePos, Span, TextId, TextSource,
};
use crate::tags::Tree;
pub use crate::tags::{Id as StructureId, Mark as Structure};
use crate::text::FontSet;
pub use crate::text::face::Face;
pub use crate::text::{FaceId, Glyph, ShapedRun as Run};

/// The number of an anchor, which [`Link::Anchor`], [`Item::Anchor`], and [`Layout::anchor`] share.
pub type AnchorId = usize;

/// How far above the baseline [`Layout::rect`] starts, as a part of the size. What is left of the size goes below.
pub const EM_ABOVE_BASELINE: f64 = 0.8;
/// A point this many times farther above or below a line than beside it counts as equally far, so a click between
/// two lines finds the text on the nearer line rather than the nearer end of a word.
const LINE_WEIGHT: f64 = 4.0;

/// A laid out document: its pages, what the pages' runs refer to, its navigation, and what a PDF needs besides.
pub struct Layout {
    pub(crate) laid: Output,
    pub(crate) fonts: FontSet,
    pub(crate) metadata: Metadata,
    pub(crate) lang: Lang,
    pub(crate) conformance: Conformance,
    pub(crate) today: Date,
    pub(crate) source: InputSource,
}

/// Where a point falls in the text of a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// The index of the text item in [`Page::items`].
    pub item: usize,
    /// The byte of [`Run::text`] nearest the point, on a glyph's edge.
    pub byte: usize,
}

/// Where a source position is on a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    pub page: usize,
    /// The index of the text item in [`Page::items`].
    pub item: usize,
    /// The part of the item that shows the position, as tall as the run's size around its baseline.
    pub rect: Rect,
}

impl Layout {
    pub fn pages(&self) -> &[Page] {
        &self.laid.pages
    }

    /// The font of a run. A [`FaceId`] is valid for the layout whose run holds it.
    pub fn face(&self, id: FaceId) -> &Face {
        self.fonts.face_of(id)
    }

    /// The face a theme font family uses for text of `weight` (100 to 900) and slant, chosen as inline emphasis
    /// chooses: the nearest weight, then the slant asked for. `None` for a family the theme does not load. For
    /// clients that set their own text in the document's fonts.
    pub fn font(&self, family: &str, weight: u16, italic: bool) -> Option<&Face> {
        let style = if italic { FontStyle::Italic } else { FontStyle::Normal };
        let id = self.fonts.nearest(family, Weight::of_font(weight), style)?;
        Some(self.fonts.face_of(id))
    }

    /// Every font the document names, in [`FaceId`] order.
    pub fn faces(&self) -> impl Iterator<Item = &std::sync::Arc<Face>> {
        self.fonts.faces()
    }

    /// Where an anchor is on the pages.
    pub fn anchor(&self, id: AnchorId) -> Position {
        self.laid.anchors[id]
    }

    /// The headings in document order, for bookmarks.
    pub fn outline(&self) -> &[Bookmark] {
        &self.laid.outline
    }

    /// The number each page shows: empty for a document without parts, whose pages show their physical number,
    /// and `None` for a page before the first part.
    pub fn page_numbers(&self) -> &[Option<PageNumber>] {
        &self.laid.page_numbers
    }

    /// The typesetting rules the layout breaks, since no better layout was found, and what stands in for a missing
    /// image. A typesetting rule names its page.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.laid.warnings
    }

    /// The connection to the written text, if [`crate::LayoutOptions::source_map`] asked for it.
    pub fn source_map(&self) -> Option<&SourceMap> {
        self.laid.source_map.as_ref()
    }

    /// The text nearest a point of a page, which is what a click means. `None` for a page without text.
    pub fn hit(&self, page: usize, x: Pt, y: Pt) -> Option<Hit> {
        let items = &self.laid.pages.get(page)?.items;
        let distance = |run_x: f64, run_y: f64, run: &Run| {
            let top = run_y - EM_ABOVE_BASELINE * run.size.0;
            let dx = (run_x - x.0).max(x.0 - (run_x + run.width.0)).max(0.0);
            let dy = (top - y.0).max(y.0 - (top + run.size.0)).max(0.0);
            dx * dx + (LINE_WEIGHT * dy).powi(2)
        };
        let (item, x_start, run) = (items.iter().enumerate())
            .filter_map(|(index, item)| match item {
                Item::Text { x, y, run, .. } => Some((index, x.0, y.0, run)),
                _ => None,
            })
            .min_by(|a, b| distance(a.1, a.2, a.3).total_cmp(&distance(b.1, b.2, b.3)))
            .map(|(index, x, _, run)| (index, x, run))?;
        let byte = byte_at(run, x.0 - x_start);
        Some(Hit { item, byte })
    }

    /// The written place of the text nearest a point of a page. Text the engine made goes to where it stands, or
    /// to the start of its block. `None` without a source map, or where the nearest text has no block, such as a
    /// page number.
    pub fn position_at(&self, page: usize, x: Pt, y: Pt) -> Option<SourcePos> {
        let map = self.source_map()?;
        let hit = self.hit(page, x, y)?;
        let Item::Text { source, .. } = &self.laid.pages[page].items[hit.item] else {
            return None;
        };
        Some(map.nearest((*source)?, hit.byte))
    }

    /// Where a written place is on the pages: the text item that shows it, or the nearest one.
    pub fn locate(&self, at: SourcePos) -> Option<Located> {
        let map = self.source_map()?;
        let (page, item) = map.locate(at)?;
        let Item::Text { x, y, run, source, .. } = &self.laid.pages[page].items[item] else {
            return None;
        };
        let byte = map.byte_of((*source)?, at, run.text.len());
        let (from, to) = glyph_edges(run, byte);
        Some(Located {
            page,
            item,
            rect: Rect {
                x: Pt(x.0 + from),
                y: Pt(y.0 - EM_ABOVE_BASELINE * run.size.0),
                width: Pt((to - from).max(0.0)),
                height: run.size,
            },
        })
    }

    /// Writes the PDF from this display list alone: nothing is shaped, paginated, or read again. It is checked
    /// against the standards the document must conform to. The warnings of the result name fonts whose license
    /// restricts embedding, and `parts` is the page count. A layout outlives the job that made it, so an export has
    /// a token of its own.
    pub fn pdf(&self, options: &PdfOptions, cancel: &Cancel) -> Result<Rendered, Failure> {
        let settings = crate::pdf::Settings {
            lang: self.lang,
            print: crate::pdf::Print {
                bleed: options.bleed,
                crop_marks: options.crop_marks,
            },
            conformance: &self.conformance,
            today: self.today,
            source: &self.source,
        };
        let bytes = crate::pdf::write(&self.laid, &self.fonts, &self.metadata, &settings, cancel)?;
        let used: BTreeSet<_> = (self.laid.pages.iter())
            .flat_map(|page| &page.items)
            .filter_map(|item| match item {
                Item::Text { run, .. } | Item::TurnedText { run, .. } => Some(run.face),
                _ => None,
            })
            .collect();
        let warnings = (used.into_iter())
            .filter_map(|face| self.fonts.embedding_warning(face))
            .collect();
        Ok(Rendered {
            bytes,
            warnings,
            parts: self.laid.pages.len(),
        })
    }
}

/// The byte of `run` nearest a point `x` from its start, on a glyph's edge. The second half of a glyph means the
/// byte after it.
fn byte_at(run: &Run, x: f64) -> usize {
    let mut pen = 0.0;
    for glyph in &run.glyphs {
        let next = pen + glyph.x_advance.0;
        if x < next {
            return if x < (pen + next) / 2.0 {
                glyph.text.start
            } else {
                glyph.text.end
            };
        }
        pen = next;
    }
    run.text.len()
}

/// Where the glyph that shows `byte` starts and ends, from the start of `run`.
fn glyph_edges(run: &Run, byte: usize) -> (f64, f64) {
    let mut pen = 0.0;
    for glyph in &run.glyphs {
        let next = pen + glyph.x_advance.0;
        if glyph.text.contains(&byte) {
            return (pen, next);
        }
        pen = next;
    }
    (pen, pen)
}

/// What layout produces. [`Layout`] adds the fonts and what a PDF needs besides.
#[derive(Debug, Clone)]
pub(crate) struct Output {
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub asides: Vec<Aside>,
    /// The typesetting rules the layout breaks, since no better layout was found.
    pub warnings: Vec<Diagnostic>,
    pub source_map: Option<SourceMap>,
}

/// A note set in the margin beside the line that references it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Aside {
    /// The page index from zero.
    #[cfg_attr(not(test), allow(dead_code))]
    pub page: usize,
    /// The box the note's text is set in.
    #[cfg_attr(not(test), allow(dead_code))]
    pub rect: Rect,
    #[cfg_attr(not(test), allow(dead_code))]
    pub note: NoteKind,
    /// The start of the referencing line, on the baseline of the reference. The note follows the
    /// reference in reading order.
    #[cfg_attr(not(test), allow(dead_code))]
    pub reference: Position,
}

/// What a note in the margin is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteKind {
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
    pub anchor: AnchorId,
}

/// A page: its trim size and what is drawn on it.
#[derive(Debug, Clone)]
pub struct Page {
    pub width: Pt,
    pub height: Pt,
    pub items: Vec<Item>,
}

/// One thing on a page. See [the module](self) for how each draws.
#[derive(Debug, Clone)]
pub enum Item {
    /// A shaped run whose baseline starts at (`x`, `y`).
    Text {
        x: Pt,
        y: Pt,
        run: Run,
        color: Color,
        /// What the run is in the written text, in a layout with a source map. `None` for text outside every
        /// block, such as running heads and page numbers.
        source: Option<TextId>,
    },
    /// A shaped run turned `angle` degrees counterclockwise about the start of its baseline at
    /// (`x`, `y`), used for watermarks.
    TurnedText {
        x: Pt,
        y: Pt,
        angle: f64,
        run: Run,
        color: Color,
    },
    /// A filled rectangle, used for underlines and rules.
    Rect {
        rect: Rect,
        color: Color,
    },
    /// A clickable area that opens a URL or goes to an anchor. Not drawn.
    Link {
        rect: Rect,
        link: Link,
    },
    /// A picture scaled to fill `rect`.
    Image {
        rect: Rect,
        picture: Picture,
    },
    /// The destination of internal links to anchor `id`, at the top left of what it marks. Not drawn.
    Anchor {
        id: AnchorId,
        x: Pt,
        y: Pt,
    },
    /// Starts what [`Structure`] marks, up to the matching [`Item::End`]: the content of a structure element. Not
    /// drawn. PDF export reads these, and a painter skips them.
    Begin(Structure),
    End,
}

/// A rectangle on a page, from its top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: Pt,
    pub y: Pt,
    pub width: Pt,
    pub height: Pt,
}
