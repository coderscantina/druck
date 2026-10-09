//! The fonts an EPUB embeds: every face of the bundled and file families its stylesheet names. Installed
//! families are only named, since their licenses rarely allow passing them on, and so are faces in font
//! collections, which readers cannot load.
//!
//! A variable file is embedded whole and declared for the weights of its `wght` axis.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};

use rustybuzz::ttf_parser::Tag;

use super::Asset;
use crate::config::resolved::{Config, FaceFile};
use crate::config::source::Resource;
use crate::config::theme::Face;
use crate::diagnostic::Diagnostic;
use crate::text::{bundled, restricts_embedding};

/// A face as `@font-face` declares it.
#[derive(Debug, PartialEq)]
pub struct FontFace {
    pub family: String,
    pub face: Face,
    /// The `font-weight` descriptor: a weight, or a range for a variable file.
    pub weight: String,
    /// The file's path in the EPUB, relative to the stylesheet.
    pub path: String,
}

#[derive(Default)]
pub struct Embedded {
    pub faces: Vec<FontFace>,
    pub files: Vec<Asset>,
    pub warnings: Vec<String>,
}

/// The faces and files of `families`, as `config` defines them. A file shared by faces is embedded once.
pub fn embed(config: &Config, families: &BTreeSet<String>) -> Result<Embedded, Vec<Diagnostic>> {
    let mut embedded = Embedded::default();
    let mut paths: HashMap<&Resource, String> = HashMap::new();
    let mut errors = Vec::new();
    for family in families {
        let Some(files) = config.fonts.get(family) else {
            embedded.warnings.push(format!(
                "font family \"{family}\" has no font files in the theme, so the EPUB names it without embedding \
                 it; readers that lack it show a fallback font"
            ));
            continue;
        };
        for (face, file) in files {
            let property = || Some(format!("fonts.{family}.{face}"));
            let data = match read(file) {
                Ok(data) => data,
                Err(message) => {
                    errors.push(Diagnostic::new(None, message).property(property()));
                    continue;
                }
            };
            let Some(kind) = Kind::of(&data).filter(|_| file.index == 0) else {
                embedded.warnings.push(format!(
                    "{} is not embedded in the EPUB: readers load only single-face OpenType, TrueType, and WOFF files",
                    file.resource
                ));
                continue;
            };
            let Some(parsed) = rustybuzz::Face::from_slice(&data, 0) else {
                let message = format!("{} is not a parsable OpenType or TrueType font", file.resource);
                errors.push(Diagnostic::new(None, message).property(property()));
                continue;
            };
            if restricts_embedding(&parsed) {
                embedded.warnings.push(format!(
                    "{} restricts embedding in its license (OS/2 fsType); it is embedded anyway, so check that its \
                     license allows this",
                    file.resource
                ));
            }
            let weight = match weight_range(&parsed).filter(|_| file.variable) {
                Some((min, max)) => format!("{min} {max}"),
                None => face.weight.get().to_string(),
            };
            let path = match paths.get(&file.resource) {
                Some(path) => path.clone(),
                None => {
                    let path = format!("fonts/font-{}.{}", paths.len() + 1, kind.extension);
                    paths.insert(&file.resource, path.clone());
                    embedded.files.push(Asset {
                        path: path.clone(),
                        media_type: kind.media_type,
                        data,
                    });
                    path
                }
            };
            embedded.faces.push(FontFace {
                family: family.clone(),
                face: *face,
                weight,
                path,
            });
        }
    }
    if errors.is_empty() { Ok(embedded) } else { Err(errors) }
}

fn read(file: &FaceFile) -> Result<Cow<'static, [u8]>, String> {
    let resource = &file.resource;
    match resource.file() {
        None => bundled::font(&resource.path)
            .map(Cow::Borrowed)
            .ok_or_else(|| format!("unknown bundled font: {resource}")),
        Some(path) => std::fs::read(&path)
            .map(Cow::Owned)
            .map_err(|e| format!("cannot read font {resource}: {e}")),
    }
}

/// The weights of a variable font's `wght` axis, within the CSS range of 1 to 1000.
fn weight_range(font: &rustybuzz::Face<'_>) -> Option<(u16, u16)> {
    let axis = font
        .variation_axes()
        .into_iter()
        .find(|axis| axis.tag == Tag::from_bytes(b"wght"))?;
    let clamp = |value: f32| value.round().clamp(1.0, 1000.0) as u16;
    Some((clamp(axis.min_value), clamp(axis.max_value)))
}

/// A font file format EPUB readers load.
struct Kind {
    media_type: &'static str,
    extension: &'static str,
}

impl Kind {
    /// The format of a single-face font file from its signature; `None` for collections and other files.
    fn of(data: &[u8]) -> Option<Self> {
        let (media_type, extension) = match data.get(..4)? {
            b"OTTO" => ("font/otf", "otf"),
            [0, 1, 0, 0] | b"true" => ("font/ttf", "ttf"),
            b"wOFF" => ("font/woff", "woff"),
            b"wOF2" => ("font/woff2", "woff2"),
            _ => return None,
        };
        Some(Self { media_type, extension })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::source::Origin;
    use crate::config::theme::{FontStyle, Weight};
    use crate::text::tests::{collection, variable_file};

    fn config(fonts: Vec<(&str, Face, FaceFile)>) -> Config {
        let mut config = super::super::tests::config("", serde_json::json!({"version": 1}));
        config.fonts.clear();
        for (family, face, file) in fonts {
            config.fonts.entry(family.to_owned()).or_default().insert(face, file);
        }
        config
    }

    fn families(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn bundled(path: &str) -> FaceFile {
        FaceFile {
            resource: Resource {
                origin: Origin::Bundled,
                path: path.to_owned(),
            },
            index: 0,
            variable: false,
        }
    }

    #[test]
    fn embeds_file_faces_once_and_only_names_installed_families() {
        let regular = Face::REGULAR;
        let italic = Face {
            weight: Weight::REGULAR,
            style: FontStyle::Italic,
        };
        let config = config(vec![
            ("Serif", regular, bundled("fonts/LibertinusSerif-Regular.otf")),
            ("Serif", italic, bundled("fonts/LibertinusSerif-Italic.otf")),
            ("Mono", regular, bundled("fonts/LibertinusSerif-Regular.otf")),
        ]);
        let embedded = embed(&config, &families(&["Mono", "Serif", "Avenir Next"])).unwrap();

        let declared: Vec<_> = (embedded.faces.iter())
            .map(|face| (face.family.as_str(), face.weight.as_str(), face.path.as_str()))
            .collect();
        assert_eq!(
            declared,
            [
                ("Mono", "400", "fonts/font-1.otf"),
                ("Serif", "400", "fonts/font-1.otf"),
                ("Serif", "400", "fonts/font-2.otf"),
            ]
        );
        assert_eq!(embedded.files.len(), 2);
        assert_eq!(embedded.files[0].media_type, "font/otf");
        assert_eq!(embedded.warnings.len(), 1);
        assert!(embedded.warnings[0].contains("\"Avenir Next\" has no font files"));
    }

    #[test]
    fn declares_a_variable_file_for_its_weight_axis_and_skips_collections() {
        let dir = std::env::temp_dir().join(format!("druck-epub-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let regular = bundled::font("fonts/LibertinusSerif-Regular.otf").unwrap();
        std::fs::write(dir.join("set.ttc"), collection(&[regular])).unwrap();
        let in_collection = FaceFile {
            resource: Resource {
                origin: Origin::Theme(dir.clone()),
                path: "set.ttc".to_owned(),
            },
            index: 0,
            variable: false,
        };
        let config = config(vec![
            ("Variable", Face::REGULAR, variable_file(false)),
            ("Collected", Face::REGULAR, in_collection),
        ]);
        let embedded = embed(&config, &families(&["Collected", "Variable"])).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(embedded.faces.len(), 1);
        assert_eq!(embedded.faces[0].family, "Variable");
        assert_eq!(embedded.faces[0].weight, "100 900");
        assert!(embedded.warnings[0].contains("set.ttc"), "{:?}", embedded.warnings);
    }
}
