//! Fonts and text shaping: loads the configured faces and turns text into positioned glyphs.

pub mod bundled;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod variations;

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::PathBuf;
use std::str::FromStr;

use crate::config::resolved::{Config, FaceFile, FontFiles};
use crate::config::theme::{Face, FontStyle, Lang, Weight};
use crate::config::values::Pt;
use crate::diagnostic::Diagnostic;

/// Index of a loaded face in its [`Fonts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceId(pub usize);

/// A parsed face. Font bytes live for the rest of the process: bundled fonts are static and
/// user fonts are leaked once per file, so the shaper and the PDF writer share one copy.
struct Loaded {
    buzz: rustybuzz::Face<'static>,
    font: krilla::text::Font,
    /// The face's file, as in diagnostics.
    file: String,
    /// Whether the font's OS/2 `fsType` restricts embedding or subsetting.
    restricted: bool,
}

/// Every face named by the configuration, loaded and parsed, with each family's faces in face order.
pub struct Fonts {
    faces: Vec<Loaded>,
    families: BTreeMap<String, Vec<(Face, FaceId)>>,
}

impl Fonts {
    /// Loads every face of `config.fonts` and of the `installed` families found for
    /// `config.installed_fonts`. Bundled faces are compiled into the binary; others are read from
    /// their files. Errors name the resource and property.
    pub fn load(config: &Config, installed: &BTreeMap<String, FontFiles>) -> Result<Self, Vec<Diagnostic>> {
        let mut fonts = Self::from_files(config.fonts.iter().chain(installed))?;
        for (family, excluded) in &config.excluded_fonts {
            fonts
                .families
                .get_mut(family)
                .expect("configured family")
                .retain(|(face, _)| !excluded.contains(face));
        }
        let mut errors = Vec::new();
        for (family, requests) in &config.requested_fonts {
            for (face, property) in requests {
                if !fonts.families[family].iter().any(|(loaded, _)| loaded == face) {
                    errors.push(
                        Diagnostic::new(
                            None,
                            format!("font family \"{family}\" has no {face} face in its files or variable axis ranges"),
                        )
                        .property(Some(property.clone())),
                    );
                }
            }
        }
        if errors.is_empty() { Ok(fonts) } else { Err(errors) }
    }

    pub(crate) fn from_files<'a>(
        families: impl IntoIterator<Item = (&'a String, &'a FontFiles)>,
    ) -> Result<Self, Vec<Diagnostic>> {
        let mut fonts = Self {
            faces: Vec::new(),
            families: BTreeMap::new(),
        };
        let mut files: HashMap<PathBuf, &'static [u8]> = HashMap::new();
        let mut errors = Vec::new();
        for (name, family) in families {
            let mut instances = BTreeMap::new();
            for (face, file) in family {
                let result = read(file, &mut files).and_then(|data| {
                    if !file.variable {
                        instances.insert(*face, (data, file));
                        return Ok(());
                    }
                    let font = rustybuzz::Face::from_slice(data, file.index)
                        .ok_or_else(|| format!("{} is not a parsable font", file.resource))?;
                    if !font.is_variable() {
                        return Err(format!(
                            "{} is marked variable but has no variation axes",
                            file.resource
                        ));
                    }
                    let supported = variations::faces(&font, *face);
                    if supported.is_empty() {
                        return Err(format!("{} has no supported weight or italic instances", file.resource));
                    }
                    for instance in supported {
                        if instance.style == face.style && family.get(&instance).is_none_or(|entry| entry.variable) {
                            instances.insert(instance, (data, file));
                        } else {
                            instances.entry(instance).or_insert((data, file));
                        }
                    }
                    Ok(())
                });
                if let Err(message) = result {
                    errors.push(Diagnostic::new(None, message).property(Some(format!("fonts.{name}.{face}"))));
                }
            }
            let mut loaded = Vec::with_capacity(instances.len());
            for (face, (data, file)) in instances {
                match parse(data, file, face) {
                    Ok(parsed) => {
                        fonts.faces.push(parsed);
                        loaded.push((face, FaceId(fonts.faces.len() - 1)));
                    }
                    Err(message) => {
                        let property = format!("fonts.{name}.{face}");
                        errors.push(Diagnostic::new(None, message).property(Some(property)));
                    }
                }
            }
            fonts.families.insert(name.clone(), loaded);
        }
        if errors.is_empty() { Ok(fonts) } else { Err(errors) }
    }

    /// The face of a family closest to `weight` and `style`. Block styles always find their exact
    /// face, which configuration and font lookup check; inline emphasis and strong text may not.
    /// The nearest weight wins, as in CSS: from 400 to 500 the weights up to 500 first, then lighter
    /// ones, then heavier ones; below 400 lighter ones first; above 500 heavier ones first. Among
    /// faces of that weight the requested style wins.
    ///
    /// Panics for a family that is not loaded.
    pub fn face(&self, family: &str, weight: Weight, style: FontStyle) -> FaceId {
        let faces = self
            .families
            .get(family)
            .unwrap_or_else(|| panic!("font family \"{family}\" is not configured"));
        let (_, id) = faces
            .iter()
            .min_by_key(|(face, _)| (weight_rank(weight, face.weight), face.style != style))
            .expect("every family has a face");
        *id
    }

    /// A warning for a face whose font restricts embedding, naming its file.
    pub fn embedding_warning(&self, face: FaceId) -> Option<String> {
        let face = &self.faces[face.0];
        face.restricted.then(|| {
            format!(
                "{} restricts embedding in its license (OS/2 fsType); it is embedded anyway, so check that \
                 its license allows this",
                face.file
            )
        })
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

    /// The height of capital letters of a face at `size`, or 0.7 em if the font does not say.
    pub fn cap_height(&self, face: FaceId, size: Pt) -> Pt {
        let buzz = &self.faces[face.0].buzz;
        let units = buzz
            .capital_height()
            .map_or(0.7 * f64::from(buzz.units_per_em()), f64::from);
        Pt(units * size.0 / f64::from(buzz.units_per_em()))
    }

    /// Shapes one line-free piece of text with kerning and standard ligatures enabled.
    /// A positive `y_offset` moves a glyph up.
    pub fn shape(&self, text: &str, face: FaceId, size: Pt, lang: Lang) -> ShapedRun {
        self.shape_tracked(text, face, size, lang, 0.0)
    }

    /// Shapes text like [`Fonts::shape`], with `tracking` em added after every character. Tracked text
    /// sets no ligatures, whose letters could not be spaced.
    pub fn shape_tracked(&self, text: &str, face: FaceId, size: Pt, lang: Lang, tracking: f64) -> ShapedRun {
        self.shape_with(text, face, size, lang, tracking, false)
    }

    /// Whether a face has small capitals of its own, the OpenType `smcp` feature.
    pub fn has_small_caps(&self, face: FaceId) -> bool {
        let tag = rustybuzz::ttf_parser::Tag::from_bytes(b"smcp");
        let gsub = self.faces[face.0].buzz.tables().gsub;
        gsub.is_some_and(|gsub| gsub.features.into_iter().any(|feature| feature.tag == tag))
    }

    /// Shapes text like [`Fonts::shape_tracked`], with the face's small capitals for lowercase letters if
    /// `small_caps` is set.
    pub fn shape_with(
        &self,
        text: &str,
        face: FaceId,
        size: Pt,
        lang: Lang,
        tracking: f64,
        small_caps: bool,
    ) -> ShapedRun {
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
        let ligatures = u32::from(tracking == 0.0);
        let features = [(b"kern", 1), (b"liga", ligatures), (b"smcp", u32::from(small_caps))]
            .map(|(tag, value)| rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(tag), value, ..));
        let shaped = rustybuzz::shape(buzz, &features, buffer);

        let scale = size.0 / f64::from(buzz.units_per_em());
        let points = |units: i32| Pt(f64::from(units) * scale);
        let spacing = tracking * size.0;
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
            let mut x_advance = points(pos.x_advance);
            // Spacing follows each character once, so marks and later glyphs of a cluster get none.
            if spacing != 0.0 && start < end {
                x_advance.0 += spacing;
                width += spacing;
            }
            glyphs.push(Glyph {
                id: info.glyph_id as u16,
                x_advance,
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
            spacing: Pt(spacing),
        }
    }

    /// The font handle krilla embeds and subsets for a face.
    pub(crate) fn pdf_font(&self, face: FaceId) -> &krilla::text::Font {
        &self.faces[face.0].font
    }
}

/// The bytes of a face's file, read once per file.
fn read(file: &FaceFile, files: &mut HashMap<PathBuf, &'static [u8]>) -> Result<&'static [u8], String> {
    let resource = &file.resource;
    let Some(path) = resource.file() else {
        return bundled::font(&resource.path).ok_or_else(|| format!("unknown bundled font: {resource}"));
    };
    if let Some(data) = files.get(&path) {
        return Ok(data);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read font {}: {e}", path.display()))?;
    let data: &'static [u8] = Vec::leak(bytes);
    files.insert(path, data);
    Ok(data)
}

/// Parses the face of `file` in `data`.
fn parse(data: &'static [u8], file: &FaceFile, face: Face) -> Result<Loaded, String> {
    let resource = &file.resource;
    let invalid = || match file.index {
        0 => format!("{resource} is not a parsable OpenType or TrueType font"),
        index => format!("{resource} has no parsable face at index {index}"),
    };
    let mut buzz = rustybuzz::Face::from_slice(data, file.index).ok_or_else(invalid)?;
    let coordinates = variations::coordinates(&buzz, face).map_err(|message| format!("{resource}: {message}"))?;
    buzz.set_variations(&coordinates);
    let pdf_coordinates: Vec<_> = coordinates
        .iter()
        .map(|variation| (krilla::text::Tag::new(&variation.tag.to_bytes()), variation.value))
        .collect();
    let font = krilla::text::Font::new_variable(data.into(), file.index, &pdf_coordinates).ok_or_else(invalid)?;
    let restricted = restricts_embedding(&buzz);
    let file = match file.index {
        0 => resource.to_string(),
        index => format!("face {index} of {resource}"),
    };
    Ok(Loaded {
        buzz,
        font,
        file,
        restricted,
    })
}

/// Whether a font's license, its OS/2 `fsType`, restricts embedding or subsetting.
pub(crate) fn restricts_embedding(face: &rustybuzz::Face<'_>) -> bool {
    face.permissions() == Some(rustybuzz::ttf_parser::Permissions::Restricted)
        || !face.is_subsetting_allowed()
        || !face.is_outline_embedding_allowed()
}

/// How well a face of weight `have` serves a request for `wanted`; lower is better. See [`Fonts::face`].
fn weight_rank(wanted: Weight, have: Weight) -> (u8, u16) {
    let (wanted, have) = (wanted.get(), have.get());
    let tier = if (400..=500).contains(&wanted) {
        match have {
            _ if (wanted..=500).contains(&have) => 0,
            _ if have < wanted => 1,
            _ => 2,
        }
    } else if wanted < 400 {
        u8::from(have > wanted)
    } else {
        u8::from(have < wanted)
    };
    (tier, wanted.abs_diff(have))
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
    /// Tracking after each character, included in the advances.
    pub spacing: Pt,
}

impl ShapedRun {
    /// The width up to the last glyph, without the tracking after it. Alignment measures this, so
    /// tracked text ends at its edge.
    pub fn set_width(&self) -> Pt {
        if self.text.is_empty() {
            self.width
        } else {
            Pt(self.width.0 - self.spacing.0)
        }
    }
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
