//! Fonts and text shaping: finds the configured faces and turns text into positioned glyphs.

pub mod bundled;
pub mod face;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod variations;

use std::collections::BTreeMap;
use std::ops::Range;
use std::str::FromStr;
use std::sync::Arc;

use self::face::FaceCache;
use crate::cancel::{Cancel, CheckPoint};
use crate::config::resolved::{Config, FontFiles};
use crate::config::source::Resource;
use crate::config::theme::{Face, FigureSpacing, FigureStyle, Figures, FontStyle, Lang, Weight};
use crate::config::values::Pt;
use crate::diagnostic::Diagnostic;
use crate::renderer::Failure;
use crate::resource::Files;

/// The number of a face in its layout: [`crate::output::Layout::face`] finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceId(pub usize);

/// A face as a document uses it.
#[derive(Clone)]
struct Named {
    face: Arc<face::Face>,
    /// The face's file in this job, as in diagnostics. The same bytes may sit under another name in another job.
    file: String,
    resource: Resource,
}

/// The features every run sets, in the order of a plan key's values.
const FEATURES: [&[u8; 4]; 7] = [b"kern", b"liga", b"smcp", b"lnum", b"onum", b"pnum", b"tnum"];

/// Every face named by the configuration, with each family's faces in face order. Owned, so it can outlive the
/// job that found it.
#[derive(Clone)]
pub struct FontSet {
    faces: Vec<Named>,
    families: BTreeMap<String, Vec<(Face, FaceId)>>,
}

impl FontSet {
    /// Finds every face of `config.fonts` and of the `installed` families found for
    /// `config.installed_fonts`, from `files` and the faces `cache` keeps for `job`. Errors name the resource
    /// and property.
    pub(crate) fn load(
        config: &Config,
        installed: &BTreeMap<String, FontFiles>,
        files: &Files,
        cache: &FaceCache,
        job: u64,
        cancel: &Cancel,
    ) -> Result<Self, Failure> {
        let mut fonts = Self::from_files(config.fonts.iter().chain(installed), files, cache, job, cancel)?;
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
        if errors.is_empty() {
            Ok(fonts)
        } else {
            Err(errors.into())
        }
    }

    pub(crate) fn from_files<'a>(
        families: impl IntoIterator<Item = (&'a String, &'a FontFiles)>,
        files: &Files,
        cache: &FaceCache,
        job: u64,
        cancel: &Cancel,
    ) -> Result<Self, Failure> {
        let mut fonts = Self {
            faces: Vec::new(),
            families: BTreeMap::new(),
        };
        let mut errors = Vec::new();
        for (name, family) in families {
            let mut instances = BTreeMap::new();
            for (face, file) in family {
                let result = files.get(&file.resource).and_then(|data| {
                    if !file.variable {
                        instances.insert(*face, (data, file));
                        return Ok(());
                    }
                    let font = rustybuzz::Face::from_slice(&data, file.index)
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
                            instances.insert(instance, (data.clone(), file));
                        } else {
                            instances.entry(instance).or_insert((data.clone(), file));
                        }
                    }
                    Ok(())
                });
                if let Err(message) = result {
                    let property = Some(format!("fonts.{name}.{face}"));
                    errors.push(
                        Diagnostic::new(None, message)
                            .property(property)
                            .resource(&file.resource),
                    );
                }
            }
            let mut loaded = Vec::with_capacity(instances.len());
            for (face, (data, file)) in instances {
                cancel.check(CheckPoint::Face)?;
                match cache.face(&data, file, face, job) {
                    Ok(built) => {
                        fonts.faces.push(Named {
                            face: built,
                            file: match file.index {
                                0 => file.resource.to_string(),
                                index => format!("face {index} of {}", file.resource),
                            },
                            resource: file.resource.clone(),
                        });
                        loaded.push((face, FaceId(fonts.faces.len() - 1)));
                    }
                    Err(message) => {
                        let property = Some(format!("fonts.{name}.{face}"));
                        errors.push(
                            Diagnostic::new(None, message)
                                .property(property)
                                .resource(&file.resource),
                        );
                    }
                }
            }
            fonts.families.insert(name.clone(), loaded);
        }
        if errors.is_empty() {
            Ok(fonts)
        } else {
            Err(errors.into())
        }
    }

    /// These faces as the shaper reads them, for one job on one thread.
    pub fn fonts(&self) -> Fonts<'_> {
        Fonts {
            set: self,
            buzz: self.faces.iter().map(|named| named.face.shaper()).collect(),
        }
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
    pub fn embedding_warning(&self, face: FaceId) -> Option<Diagnostic> {
        let named = &self.faces[face.0];
        named.face.restricted().then(|| {
            let message = format!(
                "{} restricts embedding in its license (OS/2 fsType); it is embedded anyway, so check that \
                 its license allows this",
                named.file
            );
            Diagnostic::warning(None, message).resource(&named.resource)
        })
    }

    /// The resource a face is read from.
    pub(crate) fn resource(&self, face: FaceId) -> &Resource {
        &self.faces[face.0].resource
    }

    /// The file a face is read from, as shown in diagnostics.
    pub fn file(&self, face: FaceId) -> &str {
        &self.faces[face.0].file
    }

    /// A face by its number.
    pub(crate) fn face_of(&self, face: FaceId) -> &face::Face {
        &self.faces[face.0].face
    }

    /// Every face, in [`FaceId`] order.
    pub(crate) fn faces(&self) -> impl Iterator<Item = &Arc<face::Face>> {
        self.faces.iter().map(|named| &named.face)
    }

    /// The font handle krilla embeds and subsets for a face.
    pub(crate) fn pdf_font(&self, face: FaceId) -> &krilla::text::Font {
        self.faces[face.0].face.pdf()
    }
}

/// The faces of a [`FontSet`] parsed for shaping. Parsing borrows the font files, so this lives as long as the
/// set it reads and costs a few table lookups per face to make.
pub struct Fonts<'j> {
    set: &'j FontSet,
    buzz: Vec<rustybuzz::Face<'j>>,
}

impl<'j> Fonts<'j> {
    /// See [`FontSet::face`].
    pub fn face(&self, family: &str, weight: Weight, style: FontStyle) -> FaceId {
        self.set.face(family, weight, style)
    }

    /// The faces these were parsed from.
    #[cfg(test)]
    pub(crate) fn set(&self) -> &'j FontSet {
        self.set
    }

    /// Vertical metrics of a face at `size`.
    pub fn metrics(&self, face: FaceId, size: Pt) -> Metrics {
        let buzz = &self.buzz[face.0];
        let upem = buzz.units_per_em();
        let scale = size.0 / f64::from(upem);
        let (position, thickness) = buzz.underline_metrics().map_or((-upem / 10, upem / 20), |line| {
            (line.position.into(), line.thickness.into())
        });
        let strikeout = buzz.strikeout_metrics().map_or(upem / 4, |line| line.position.into());
        Metrics {
            ascender: Pt(f64::from(buzz.ascender()) * scale),
            descender: Pt(f64::from(-buzz.descender()) * scale),
            underline_position: Pt(f64::from(-position) * scale),
            underline_thickness: Pt(f64::from(thickness) * scale),
            strikeout_position: Pt(f64::from(strikeout) * scale),
        }
    }

    /// The height of capital letters of a face at `size`, or 0.7 em if the font does not say.
    pub fn cap_height(&self, face: FaceId, size: Pt) -> Pt {
        let buzz = &self.buzz[face.0];
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
        self.shape_with(text, face, size, lang, tracking, false, Figures::default())
    }

    /// Shapes text like [`Fonts::shape`] with the `figures` of a style.
    pub fn shape_figures(&self, text: &str, face: FaceId, size: Pt, lang: Lang, figures: Figures) -> ShapedRun {
        self.shape_with(text, face, size, lang, 0.0, false, figures)
    }

    /// Whether a face has small capitals of its own, the OpenType `smcp` feature.
    pub fn has_small_caps(&self, face: FaceId) -> bool {
        let tag = rustybuzz::ttf_parser::Tag::from_bytes(b"smcp");
        let gsub = self.buzz[face.0].tables().gsub;
        gsub.is_some_and(|gsub| gsub.features.into_iter().any(|feature| feature.tag == tag))
    }

    /// Shapes text like [`Fonts::shape_tracked`], with the face's small capitals for lowercase letters if
    /// `small_caps` is set, and the OpenType features for `figures`. A font without them keeps its
    /// default figures.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_with(
        &self,
        text: &str,
        face: FaceId,
        size: Pt,
        lang: Lang,
        tracking: f64,
        small_caps: bool,
        figures: Figures,
    ) -> ShapedRun {
        #[cfg(test)]
        SHAPED.with(|shaped| shaped.set(shaped.get() + 1));
        let buzz = &self.buzz[face.0];
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(rustybuzz::Direction::LeftToRight);
        let language = match lang {
            Lang::En => "en",
            Lang::De => "de",
        };
        buffer.set_language(rustybuzz::Language::from_str(language).expect("language tag"));
        buffer.guess_segment_properties();
        let on = u32::from;
        let values = [
            1,
            on(tracking == 0.0),
            on(small_caps),
            on(figures.style == FigureStyle::Lining),
            on(figures.style == FigureStyle::Oldstyle),
            on(figures.spacing == FigureSpacing::Proportional),
            on(figures.spacing == FigureSpacing::Tabular),
        ];
        let script = buffer.script();
        let plan = self.set.faces[face.0].face.plan((script, language, values), || {
            let features = std::iter::zip(FEATURES, values)
                .map(|(tag, value)| rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(tag), value, ..))
                .collect::<Vec<_>>();
            rustybuzz::ShapePlan::new(
                buzz,
                rustybuzz::Direction::LeftToRight,
                // A run with no letters has no script, which the buffer reports as unknown.
                (script != rustybuzz::script::UNKNOWN).then_some(script),
                buffer.language().as_ref(),
                &features,
            )
        });
        let shaped = rustybuzz::shape_with_plan(buzz, &plan, buffer);

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
}

/// Whether a font's license, its OS/2 `fsType`, restricts embedding or subsetting.
pub(crate) fn restricts_embedding(face: &rustybuzz::Face<'_>) -> bool {
    face.permissions() == Some(rustybuzz::ttf_parser::Permissions::Restricted)
        || !face.is_subsetting_allowed()
        || !face.is_outline_embedding_allowed()
}

/// How well a face of weight `have` serves a request for `wanted`; lower is better. See [`FontSet::face`].
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

#[cfg(test)]
std::thread_local! {
    /// How many runs this thread has shaped, for tests that show something does not shape.
    pub(crate) static SHAPED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Fonts<'static> {
    /// Faces for a test that needs no renderer, read from files. The set lives as long as the test process.
    pub(crate) fn load(config: &Config, installed: &BTreeMap<String, FontFiles>) -> Result<Self, Vec<Diagnostic>> {
        let files = files_of(config.fonts.values().chain(installed.values()));
        let set = FontSet::load(config, installed, &files, &FaceCache::default(), 0, &Cancel::default())
            .map_err(Failure::into_errors)?;
        Ok(Box::leak(Box::new(set)).fonts())
    }

    /// Like [`Fonts::load`] for families alone.
    pub(crate) fn from_files<'a>(
        families: impl IntoIterator<Item = (&'a String, &'a FontFiles)>,
    ) -> Result<Self, Vec<Diagnostic>> {
        let families: Vec<_> = families.into_iter().collect();
        let files = files_of(families.iter().map(|(_, files)| *files));
        let set = FontSet::from_files(families, &files, &FaceCache::default(), 0, &Cancel::default())
            .map_err(Failure::into_errors)?;
        Ok(Box::leak(Box::new(set)).fonts())
    }
}

/// The files of `families` that can be read.
#[cfg(test)]
fn files_of<'a>(families: impl Iterator<Item = &'a FontFiles>) -> Files {
    use crate::resource::{FileResolver, Resolver};
    let mut files = Files::default();
    for file in families.flat_map(|family| family.values()) {
        let read = FileResolver::new().read(&file.resource.origin, &file.resource.path);
        if let Ok(data) = read {
            files.insert(file.resource.clone(), data);
        }
    }
    files
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
    /// The distance from the baseline up to the top of the strikethrough stroke, which has the thickness of
    /// the underline.
    pub strikeout_position: Pt,
}

/// Shaped text in one face and size: a word or part of one, or a line of code. Glyphs are in visual (left-to-right)
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    pub text: String,
    pub face: FaceId,
    pub size: Pt,
    pub glyphs: Vec<Glyph>,
    /// The sum of the glyph advances.
    pub width: Pt,
    /// Tracking after each character, included in the advances.
    pub(crate) spacing: Pt,
}

impl ShapedRun {
    /// The width up to the last glyph, without the tracking after it. Alignment measures this, so
    /// tracked text ends at its edge.
    pub(crate) fn set_width(&self) -> Pt {
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
