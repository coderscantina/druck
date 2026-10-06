//! Fonts and text shaping: loads the configured faces and turns text into positioned glyphs.

mod bundled;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ops::Range;
use std::str::FromStr;

use crate::config::resolved::{Config, FontFiles};
use crate::config::source::Resource;
use crate::config::theme::{FontStyle, Lang, Weight};
use crate::config::values::Pt;
use crate::diagnostic::Diagnostic;

/// Index of a loaded face in its [`Fonts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceId(pub usize);

/// A parsed face. Font bytes live for the rest of the process: bundled fonts are static and
/// user fonts are leaked once, so the shaper and the PDF writer share one copy.
struct Face {
    buzz: rustybuzz::Face<'static>,
    font: krilla::text::Font,
}

/// The faces of one family. Missing styles are `None` and fall back at lookup.
struct Family {
    regular: FaceId,
    italic: Option<FaceId>,
    bold: Option<FaceId>,
    bold_italic: Option<FaceId>,
}

/// Every face named by the configuration, loaded and parsed.
pub struct Fonts {
    faces: Vec<Face>,
    families: BTreeMap<String, Family>,
}

impl Fonts {
    /// Loads every face in `config.fonts`. Bundled faces are compiled into the binary;
    /// others are read from their resource path. Errors name the resource and property.
    pub fn load(config: &Config) -> Result<Self, Vec<Diagnostic>> {
        Self::from_files(&config.fonts)
    }

    pub(crate) fn from_files(files: &BTreeMap<String, FontFiles>) -> Result<Self, Vec<Diagnostic>> {
        let mut faces = Vec::new();
        let mut families = BTreeMap::new();
        let mut errors = Vec::new();
        for (name, family) in files {
            let mut load = |face: &str, resource: &Resource| {
                let property = format!("fonts.{name}.{face}");
                match load_face(resource) {
                    Ok(loaded) => {
                        faces.push(loaded);
                        Some(FaceId(faces.len() - 1))
                    }
                    Err(message) => {
                        errors.push(Diagnostic::new(None, message).property(Some(property)));
                        None
                    }
                }
            };
            let regular = load("regular", &family.regular);
            let italic = family.italic.as_ref().and_then(|r| load("italic", r));
            let bold = family.bold.as_ref().and_then(|r| load("bold", r));
            let bold_italic = family.bold_italic.as_ref().and_then(|r| load("bold-italic", r));
            if let Some(regular) = regular {
                let family = Family {
                    regular,
                    italic,
                    bold,
                    bold_italic,
                };
                families.insert(name.clone(), family);
            }
        }
        if errors.is_empty() {
            Ok(Self { faces, families })
        } else {
            Err(errors)
        }
    }

    /// The face for a family key of `config.fonts`. A family without the requested italic,
    /// bold, or bold-italic face uses its closest available face: bold-italic falls back
    /// to bold, then italic, then regular; italic and bold fall back to regular.
    ///
    /// Panics for a family that is not in the configuration.
    pub fn face(&self, family: &str, weight: Weight, style: FontStyle) -> FaceId {
        let family = self
            .families
            .get(family)
            .unwrap_or_else(|| panic!("font family \"{family}\" is not configured"));
        let face = match (weight, style) {
            (Weight::Bold, FontStyle::Italic) => family.bold_italic.or(family.bold).or(family.italic),
            (Weight::Bold, FontStyle::Normal) => family.bold,
            (Weight::Regular, FontStyle::Italic) => family.italic,
            (Weight::Regular, FontStyle::Normal) => None,
        };
        face.unwrap_or(family.regular)
    }

    /// Vertical metrics of a face at `size`.
    pub fn metrics(&self, face: FaceId, size: Pt) -> Metrics {
        let buzz = &self.faces[face.0].buzz;
        let upem = buzz.units_per_em();
        let scale = size.0 / f64::from(upem);
        let (position, thickness) = buzz.underline_metrics().map_or((-upem / 10, upem / 20), |line| {
            (line.position.into(), line.thickness.into())
        });
        Metrics {
            ascender: Pt(f64::from(buzz.ascender()) * scale),
            descender: Pt(f64::from(-buzz.descender()) * scale),
            underline_position: Pt(f64::from(-position) * scale),
            underline_thickness: Pt(f64::from(thickness) * scale),
        }
    }

    /// Shapes one line-free piece of text with kerning and standard ligatures enabled.
    /// A positive `y_offset` moves a glyph up.
    pub fn shape(&self, text: &str, face: FaceId, size: Pt, lang: Lang) -> ShapedRun {
        let buzz = &self.faces[face.0].buzz;
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(rustybuzz::Direction::LeftToRight);
        let language = match lang {
            Lang::En => "en",
            Lang::De => "de",
        };
        buffer.set_language(rustybuzz::Language::from_str(language).expect("language tag"));
        buffer.guess_segment_properties();
        let features =
            [b"kern", b"liga"].map(|tag| rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(tag), 1, ..));
        let shaped = rustybuzz::shape(buzz, &features, buffer);

        let scale = size.0 / f64::from(buzz.units_per_em());
        let points = |units: i32| Pt(f64::from(units) * scale);
        let infos = shaped.glyph_infos();
        let mut glyphs: Vec<Glyph> = Vec::with_capacity(infos.len());
        let mut width = 0.0;
        for (i, (info, pos)) in infos.iter().zip(shaped.glyph_positions()).enumerate() {
            let start = info.cluster as usize;
            // A glyph covers its cluster up to the next cluster start. Further glyphs of the same
            // cluster get an empty range so no byte is attributed twice.
            let end = if i > 0 && infos[i - 1].cluster == info.cluster {
                start
            } else {
                infos[i + 1..]
                    .iter()
                    .map(|next| next.cluster as usize)
                    .find(|&cluster| cluster != start)
                    .unwrap_or(text.len())
            };
            width += f64::from(pos.x_advance) * scale;
            glyphs.push(Glyph {
                id: info.glyph_id as u16,
                x_advance: points(pos.x_advance),
                x_offset: points(pos.x_offset),
                y_offset: points(pos.y_offset),
                text: start..end,
            });
        }
        ShapedRun {
            text: text.to_owned(),
            face,
            size,
            glyphs,
            width: Pt(width),
        }
    }

    /// The font handle krilla embeds and subsets for a face.
    pub(crate) fn pdf_font(&self, face: FaceId) -> &krilla::text::Font {
        &self.faces[face.0].font
    }
}

/// Reads and parses one font resource.
fn load_face(resource: &Resource) -> Result<Face, String> {
    let data: &'static [u8] = match resource.file() {
        None => bundled::font(&resource.path).ok_or_else(|| format!("unknown bundled font: {resource}"))?,
        Some(path) => {
            let bytes = std::fs::read(&path).map_err(|e| format!("cannot read font {}: {e}", path.display()))?;
            Vec::leak(bytes)
        }
    };
    let invalid = || format!("{resource} is not a parsable OpenType or TrueType font");
    let buzz = rustybuzz::Face::from_slice(data, 0).ok_or_else(invalid)?;
    let font = krilla::text::Font::new(data.into(), 0).ok_or_else(invalid)?;
    Ok(Face { buzz, font })
}

/// Vertical font metrics in points at a given size. `descender` is positive below the baseline.
/// `underline_position` is the distance from the baseline down to the top of the underline
/// stroke, also positive downward.
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
