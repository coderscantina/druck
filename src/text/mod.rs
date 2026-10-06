//! Fonts and text shaping: loads the configured faces and turns text into positioned glyphs.

use std::ops::Range;

use crate::config::resolved::Config;
use crate::config::theme::{FontStyle, Lang, Weight};
use crate::config::values::Pt;
use crate::diagnostic::Diagnostic;

/// Index of a loaded face in its [`Fonts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceId(pub usize);

/// Every face named by the configuration, loaded and parsed.
pub struct Fonts {}

impl Fonts {
    /// Loads every face in `config.fonts`. Bundled faces are compiled into the binary;
    /// others are read from their resource path. Errors name the resource and property.
    pub fn load(config: &Config) -> Result<Self, Vec<Diagnostic>> {
        let _ = config;
        todo!("milestone 02 font loading")
    }

    /// The face for a family key of `config.fonts`. A family without the requested italic,
    /// bold, or bold-italic face uses its closest available face: bold-italic falls back
    /// to bold, then italic, then regular; italic and bold fall back to regular.
    pub fn face(&self, family: &str, weight: Weight, style: FontStyle) -> FaceId {
        let _ = (family, weight, style);
        todo!()
    }

    /// Vertical metrics of a face at `size`.
    pub fn metrics(&self, face: FaceId, size: Pt) -> Metrics {
        let _ = (face, size);
        todo!()
    }

    /// Shapes one line-free piece of text with kerning and standard ligatures enabled.
    pub fn shape(&self, text: &str, face: FaceId, size: Pt, lang: Lang) -> ShapedRun {
        let _ = (text, face, size, lang);
        todo!()
    }
}

/// Vertical font metrics in points at a given size. `descender` is positive below the baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub ascender: Pt,
    pub descender: Pt,
    pub underline_position: Pt,
    pub underline_thickness: Pt,
}

/// Shaped text in one face and size. Glyphs are in visual (left-to-right) order.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    pub text: String,
    pub face: FaceId,
    pub size: Pt,
    pub glyphs: Vec<Glyph>,
    /// The sum of the glyph advances.
    pub width: Pt,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub id: u16,
    pub x_advance: Pt,
    pub x_offset: Pt,
    pub y_offset: Pt,
    /// The bytes of [`ShapedRun::text`] this glyph represents. Ligatures span several characters.
    pub text: Range<usize>,
}
