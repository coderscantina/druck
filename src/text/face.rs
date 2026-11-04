//! Faces kept across jobs. A [`Face`] owns its font file and everything that can outlive a job: the handle the
//! PDF writer embeds and the shape plans. A job parses its own borrowed `rustybuzz::Face` from it. Which file a face
//! came from belongs to the job, since the same bytes may sit under another name in the next one.

use std::collections::HashMap;
use std::fmt;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use rustybuzz::ttf_parser::Tag;

use super::variations;
use crate::config::resolved::FaceFile;
use crate::config::theme;
use crate::resource::Bytes;

/// What decides a shape plan of a face: the script, the language, and the values of the features.
pub(super) type PlanKey = (rustybuzz::Script, &'static str, [u32; 7]);

/// One instance of a font file, shared and immutable. It is all a painter needs to draw a run's glyphs.
pub struct Face {
    data: Bytes,
    index: u32,
    /// The instance, by axis tag, in the axis's own units.
    coordinates: Vec<([u8; 4], f32)>,
    pdf: krilla::text::Font,
    /// The glyph of a space, if the font has one.
    space: Option<u16>,
    /// Whether the font's OS/2 `fsType` restricts embedding or subsetting.
    restricted: bool,
    /// Building a plan takes about as long as shaping a word with it, and a document needs few of them. A plan
    /// holds no reference to the face it was built from, so plans outlive every job.
    plans: RwLock<HashMap<PlanKey, Arc<rustybuzz::ShapePlan>>>,
}

impl fmt::Debug for Face {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Face({} bytes, index {})", self.data.len(), self.index)
    }
}

impl Face {
    /// The font file.
    pub fn data(&self) -> &Bytes {
        &self.data
    }

    /// The face's index in its file, for collections.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The variation instance, by axis tag, in the axis's own units.
    pub fn coordinates(&self) -> &[([u8; 4], f32)] {
        &self.coordinates
    }

    /// A font handle of its own, for one export. krilla clones a font once per glyph it writes, and the clones of
    /// one handle that jobs on several threads share would contend on its reference count.
    pub(crate) fn pdf_for_export(&self) -> krilla::text::Font {
        let coordinates: Vec<_> = (self.coordinates.iter())
            .map(|(tag, value)| (krilla::text::Tag::new(tag), *value))
            .collect();
        krilla::text::Font::new_variable(self.data.data(), self.index, &coordinates).expect("parsed before")
    }

    pub(super) fn pdf(&self) -> &krilla::text::Font {
        &self.pdf
    }

    /// The glyph of a space, if the font has one.
    pub(crate) fn space(&self) -> Option<u16> {
        self.space
    }

    pub(super) fn restricted(&self) -> bool {
        self.restricted
    }

    /// The face as the shaper reads it. Parsing has succeeded once, when this face was built.
    pub(super) fn shaper(&self) -> rustybuzz::Face<'_> {
        let mut buzz = rustybuzz::Face::from_slice(&self.data, self.index).expect("this face has been parsed");
        let variations: Vec<_> = (self.coordinates.iter())
            .map(|(tag, value)| rustybuzz::Variation {
                tag: Tag::from_bytes(tag),
                value: *value,
            })
            .collect();
        buzz.set_variations(&variations);
        buzz
    }

    /// The plan `rustybuzz::shape` would build for these properties, built once. `build` makes it.
    pub(super) fn plan(&self, key: PlanKey, build: impl FnOnce() -> rustybuzz::ShapePlan) -> Arc<rustybuzz::ShapePlan> {
        if let Some(plan) = self.plans.read().expect("no panic while planning").get(&key) {
            return plan.clone();
        }
        let plan = Arc::new(build());
        let mut plans = self.plans.write().expect("no panic while planning");
        plans.entry(key).or_insert(plan).clone()
    }
}

/// What decides a face: the bytes, the face in the file, and the variation instance.
type FaceKey = (u64, u32, Vec<([u8; 4], u32)>);

struct Entry {
    face: Arc<Face>,
    /// The newest job that used this face.
    used: AtomicU64,
}

/// The faces a renderer keeps between jobs. Only ever gains completed faces, so a failed job leaves it sound.
#[derive(Default)]
pub struct FaceCache {
    faces: RwLock<HashMap<FaceKey, Entry>>,
    #[cfg(test)]
    pub(crate) builds: AtomicUsize,
}

impl FaceCache {
    /// The face of `file` in `data` for the theme face `face`, built unless a job has already built it. Errors
    /// are messages naming the resource.
    pub fn face(&self, data: &Bytes, file: &FaceFile, face: theme::Face, job: u64) -> Result<Arc<Face>, String> {
        let resource = &file.resource;
        let invalid = || match file.index {
            0 => format!("{resource} is not a parsable OpenType or TrueType font"),
            index => format!("{resource} has no parsable face at index {index}"),
        };
        let mut buzz = rustybuzz::Face::from_slice(data, file.index).ok_or_else(invalid)?;
        let variations = variations::coordinates(&buzz, face).map_err(|message| format!("{resource}: {message}"))?;
        let coordinates: Vec<_> = (variations.iter()).map(|v| (v.tag.to_bytes(), v.value)).collect();
        let bits = coordinates.iter().map(|(tag, value)| (*tag, value.to_bits())).collect();
        let key = (data.hash(), file.index, bits);
        if let Some(entry) = self.faces.read().expect("no panic while caching").get(&key) {
            entry.used.store(job, Ordering::Relaxed);
            return Ok(entry.face.clone());
        }

        buzz.set_variations(&variations);
        let pdf_coordinates: Vec<_> = (coordinates.iter())
            .map(|(tag, value)| (krilla::text::Tag::new(tag), *value))
            .collect();
        let pdf = krilla::text::Font::new_variable(data.data(), file.index, &pdf_coordinates).ok_or_else(invalid)?;
        let built = Arc::new(Face {
            data: data.clone(),
            index: file.index,
            coordinates,
            pdf,
            space: buzz.glyph_index(' ').map(|glyph| glyph.0).filter(|id| *id != 0),
            restricted: super::restricts_embedding(&buzz),
            plans: RwLock::default(),
        });
        #[cfg(test)]
        self.builds.fetch_add(1, Ordering::Relaxed);
        let mut faces = self.faces.write().expect("no panic while caching");
        let entry = faces.entry(key).or_insert(Entry {
            face: built,
            used: AtomicU64::new(job),
        });
        entry.used.store(job, Ordering::Relaxed);
        Ok(entry.face.clone())
    }

    /// Forgets the faces no job since `oldest` used. A layout that holds one keeps it alive.
    pub fn sweep(&self, oldest: u64) {
        let mut faces = self.faces.write().expect("no panic while caching");
        faces.retain(|_, entry| entry.used.load(Ordering::Relaxed) >= oldest);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.faces.read().expect("no panic while caching").len()
    }
}
