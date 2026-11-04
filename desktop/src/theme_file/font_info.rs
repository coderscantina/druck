//! What a font file says about itself: its names, license text, format, embedding permission, and variable axes. Read
//! from the file's `name`, `OS/2`, and `fvar` tables. Holds no gpui types.

use skrifa::raw::{FileRef, TableProvider};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider, Tag};

/// A variable axis, shown and never edited.
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub tag: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    TrueType,
    /// OpenType with PostScript outlines.
    OpenType,
}

/// How the font's `fsType` limits embedding in documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Embedding {
    Allowed,
    /// Installable embedding is not allowed: restricted license, or no subsetting, or bitmaps only.
    Restricted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FontInfo {
    pub family: String,
    pub style: String,
    pub format: Format,
    /// The size of the whole file, which may hold several faces.
    pub bytes: usize,
    /// Faces in the file: more than one for a collection.
    pub faces: u32,
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub copyright: Option<String>,
    pub embedding: Embedding,
    pub axes: Vec<Axis>,
}

impl FontInfo {
    /// Reads the face at `index` of a font file. An error is a message for a file that is no font.
    pub fn read(data: &[u8], index: u32) -> Result<Self, String> {
        let faces = match FileRef::new(data).map_err(|error| error.to_string())? {
            FileRef::Font(_) => 1,
            FileRef::Collection(collection) => collection.len(),
        };
        let font = FontRef::from_index(data, index).map_err(|error| error.to_string())?;
        let name = |id: StringId| {
            font.localized_strings(id)
                .english_or_first()
                .map(|text| text.chars().collect::<String>())
                .filter(|text| !text.trim().is_empty())
        };
        let family = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
            .into_iter()
            .find_map(name)
            .unwrap_or_default();
        let style = [StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]
            .into_iter()
            .find_map(name)
            .unwrap_or_default();
        let format = if font.data_for_tag(Tag::new(b"CFF ")).is_some() || font.data_for_tag(Tag::new(b"CFF2")).is_some() {
            Format::OpenType
        } else {
            Format::TrueType
        };
        // Bit 1 forbids embedding. Bit 8 forbids subsetting, which a PDF needs to do. Bit 9 allows bitmaps only.
        let embedding = match font.os2() {
            Ok(os2) if os2.fs_type() & (0x0002 | 0x0100 | 0x0200) != 0 => Embedding::Restricted,
            _ => Embedding::Allowed,
        };
        let axes = font
            .axes()
            .iter()
            .map(|axis| Axis {
                tag: axis.tag().to_string(),
                min: axis.min_value(),
                default: axis.default_value(),
                max: axis.max_value(),
            })
            .collect();
        Ok(Self {
            family,
            style,
            format,
            bytes: data.len(),
            faces,
            license: name(StringId::LICENSE_DESCRIPTION),
            license_url: name(StringId::LICENSE_URL),
            copyright: name(StringId::COPYRIGHT_NOTICE),
            embedding,
            axes,
        })
    }

    pub fn is_variable(&self) -> bool {
        !self.axes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled(name: &str) -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fonts").join(name)).unwrap()
    }

    #[test]
    fn a_font_reports_its_names_license_and_format() {
        let info = FontInfo::read(&bundled("LibertinusSerif-Regular.otf"), 0).unwrap();
        assert_eq!(info.family, "Libertinus Serif");
        assert_eq!(info.style, "Regular");
        assert_eq!(info.format, Format::OpenType);
        assert_eq!(info.faces, 1);
        assert_eq!(info.embedding, Embedding::Allowed);
        assert!(info.license.as_deref().is_some_and(|license| license.contains("SIL Open Font License")));
        assert!(!info.is_variable());
    }

    #[test]
    fn a_file_that_is_no_font_is_an_error() {
        assert!(FontInfo::read(b"not a font", 0).is_err());
    }
}
