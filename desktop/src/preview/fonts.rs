//! The font families a layout uses, with the styles that name them and the files of their faces. Holds no gpui
//! types.

use std::convert::Infallible;

use druck::editor::resolved_config::{Config, FontStyle};
use druck::resource::{Bytes, Origin};

/// The key `used_by` gives inline code, which is no block style.
pub const INLINE_CODE: &str = "inline-code";

/// Where a family's files come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Bundled,
    Installed,
    /// A file from a theme folder or the document's folder.
    ProjectFile,
}

/// One face of a family, as the layout loaded it.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceUse {
    /// `regular`, `italic`, `bold`, `bold-italic`, or a weight such as `500`.
    pub face: String,
    /// The path as the theme wrote it, the bundled name, or the family name of an installed font.
    pub file: String,
    pub bytes: Bytes,
    pub index: u32,
}

/// A family the styles use.
#[derive(Clone, Debug, PartialEq)]
pub struct FontUse {
    /// The name the theme gives the family.
    pub family: String,
    pub source: Source,
    pub faces: Vec<FaceUse>,
    /// Style keys such as `body` and `heading-1`, then custom style names, then [`INLINE_CODE`].
    pub used_by: Vec<String>,
}

/// The families the resolved styles use, in order of first use.
pub fn fonts_in_use(config: &Config, laid: &druck::output::Layout) -> Vec<FontUse> {
    let mut named: Vec<(String, String)> = Vec::new();
    let Ok(_) = config.styles.clone().try_map(|key, style| {
        named.push((key.to_owned(), style.font));
        Ok::<_, Infallible>(())
    });
    named.extend(
        config
            .custom_styles
            .iter()
            .map(|(name, custom)| (name.clone(), custom.style().font.clone())),
    );
    named.push((INLINE_CODE.to_owned(), config.inline.code_font.clone()));

    let mut uses: Vec<FontUse> = Vec::new();
    for (key, family) in named {
        if let Some(found) = uses.iter_mut().find(|found| found.family == family) {
            found.used_by.push(key);
        } else if let Some(mut found) = family_use(config, laid, &family) {
            found.used_by.push(key);
            uses.push(found);
        }
    }
    uses
}

fn family_use(config: &Config, laid: &druck::output::Layout, family: &str) -> Option<FontUse> {
    let face = |face: String, weight: u16, italic: bool, file: String| {
        let loaded = laid.font(family, weight, italic)?;
        Some(FaceUse {
            face,
            file,
            bytes: loaded.data().clone(),
            index: loaded.index(),
        })
    };
    let (source, faces) = if let Some(files) = config.fonts.get(family) {
        let source = match files.values().next().map(|file| &file.resource.origin) {
            Some(Origin::Bundled) => Source::Bundled,
            Some(Origin::Installed) => Source::Installed,
            _ => Source::ProjectFile,
        };
        let faces = files
            .iter()
            .filter_map(|(key, file)| {
                let italic = key.style == FontStyle::Italic;
                face(key.to_string(), key.weight.get(), italic, file.resource.path.clone())
            })
            .collect();
        (source, faces)
    } else {
        let requested = config.installed_fonts.get(family)?;
        let faces = requested
            .keys()
            .filter_map(|key| {
                let italic = key.style == FontStyle::Italic;
                face(key.to_string(), key.weight.get(), italic, family.to_owned())
            })
            .collect();
        (Source::Installed, faces)
    };
    Some(FontUse {
        family: family.to_owned(),
        source,
        faces,
        used_by: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use druck::cancel::Cancel;
    use druck::editor::resolved_config;
    use druck::input::{DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
    use druck::resource::{FileResolver, clock_date};
    use druck::{LayoutOptions, Missing, Renderer};

    #[test]
    fn the_default_theme_sets_body_text_in_bundled_libertinus_serif() {
        let dir = std::env::temp_dir();
        let input = Input {
            text: DocumentText::Single {
                text: "# Heading\n\nBody text with `code`.\n".into(),
                origin: Origin::Document(dir.clone()),
                name: "document.md".into(),
            },
            theme: ThemeSelection::Document {
                dirs: ThemeDirs {
                    project: None,
                    shared: None,
                },
            },
            overrides: Overrides::default(),
            today: clock_date().expect("a clock"),
        };
        let renderer = Renderer::new();
        let resolver = FileResolver::new();
        let cancel = Cancel::default();
        let Ok(configured) = renderer
            .job(&resolver, &input, &cancel)
            .missing(Missing::StandIn)
            .configure()
        else {
            panic!("configures")
        };
        let Ok(laid) = configured.layout(&LayoutOptions { source_map: false }) else {
            panic!("typesets")
        };
        let fonts = fonts_in_use(resolved_config::of(&configured), &laid);
        let body = fonts.first().expect("the body family comes first");
        assert_eq!(body.family, "Libertinus Serif");
        assert_eq!(body.source, Source::Bundled);
        assert_eq!(body.used_by.first().map(String::as_str), Some("body"));
        assert!(body.faces.iter().any(|face| face.face == "regular"));
        assert!(body.faces.iter().all(|face| !face.bytes.is_empty()));
    }
}
