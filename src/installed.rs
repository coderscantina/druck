//! Font families looked up among the fonts installed on the machine. Only [`FileResolver`] looks there, so
//! configuration and layout never depend on the system.
//!
//! [`FileResolver`]: crate::resource::FileResolver

use std::collections::{BTreeMap, HashMap};

use fontdb::{Database, FaceInfo, Source, Stretch, Style};

use crate::config::resolved::{FaceFile, FontFiles};
use crate::config::source::{Origin, Resource};
use crate::config::theme::{Face, FontStyle, Weight};
use crate::diagnostic::Diagnostic;
use crate::resource::{Bytes, InstalledFace, Resolver};

/// The faces of installed families, with the bytes of their files.
#[derive(Default)]
pub struct Found {
    pub families: BTreeMap<String, FontFiles>,
    pub files: HashMap<Resource, Bytes>,
}

/// The faces of every family in `requests`, which maps a family name to the faces styles request
/// and the first property requesting each. Each requested face must be installed; the family's other
/// faces serve inline emphasis and strong text. Errors name the family and face searched for.
pub fn lookup(
    resolver: &dyn Resolver,
    requests: &BTreeMap<String, BTreeMap<Face, String>>,
) -> Result<Found, Vec<Diagnostic>> {
    let mut found = Found::default();
    let mut errors = Vec::new();
    for (family, requested) in requests {
        let (first, property) = requested.first_key_value().expect("a family is requested for a face");
        let installed = match resolver.installed(family) {
            Ok(installed) => installed,
            Err(e) => {
                let message = format!("cannot look up installed font family \"{family}\": {e}");
                errors.push(Diagnostic::new(None, message).property(Some(property.clone())));
                continue;
            }
        };
        let faces = supported_faces(resolver, &installed, &mut found.files);
        if faces.is_empty() {
            let message = format!(
                "font family \"{family}\" is not defined in fonts and not installed; searched the installed fonts \
                 for \"{family}\" {first}"
            );
            errors.push(Diagnostic::new(None, message).property(Some(property.clone())));
            continue;
        }
        for (face, property) in requested.iter().filter(|(face, _)| !faces.contains_key(face)) {
            let installed: Vec<_> = faces.keys().map(Face::to_string).collect();
            let message = format!(
                "installed font family \"{family}\" has no {face} face; it has {}",
                installed.join(", ")
            );
            errors.push(Diagnostic::new(None, message).property(Some(property.clone())));
        }
        found.families.insert(family.clone(), faces);
    }
    if errors.is_empty() { Ok(found) } else { Err(errors) }
}

/// The faces of `installed` with the instances a variable file supports. A file that cannot be read or
/// parsed supplies none. The first file to supply a face keeps it.
fn supported_faces(
    resolver: &dyn Resolver,
    installed: &[InstalledFace],
    files: &mut HashMap<Resource, Bytes>,
) -> FontFiles {
    let mut faces = FontFiles::new();
    for face in installed {
        let key = Face {
            weight: Weight::of_font(face.weight),
            style: if face.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
        };
        let resource = Resource {
            origin: Origin::Installed,
            path: face.path.clone(),
        };
        let data = match files.get(&resource) {
            Some(data) => data.clone(),
            None => match resolver.read(&resource.origin, &resource.path) {
                Ok(data) => {
                    files.insert(resource.clone(), data.clone());
                    data
                }
                Err(_) => continue,
            },
        };
        let Some(font) = rustybuzz::Face::from_slice(&data, face.index) else {
            continue;
        };
        let supported = if font.is_variable() {
            crate::text::variations::faces(&font, key)
        } else {
            vec![key]
        };
        for key in supported {
            faces.entry(key).or_insert_with(|| FaceFile {
                resource: resource.clone(),
                index: face.index,
                // This file is already expanded to its supported instances.
                variable: false,
            });
        }
    }
    faces
}

/// The installed faces whose family name is `family`, ignoring ASCII case. Only the width closest
/// to normal counts, so condensed faces do not stand in for regular ones. Where two files hold the
/// same face, italic beats oblique, then the first path and index win.
pub fn family_faces(database: &Database, family: &str) -> Vec<InstalledFace> {
    let mut faces: Vec<(&FaceInfo, &str)> = database
        .faces()
        .filter(|face| face.families.iter().any(|(name, _)| name.eq_ignore_ascii_case(family)))
        .filter_map(|face| match &face.source {
            Source::File(path) | Source::SharedFile(path, _) => Some((face, path.to_str()?)),
            Source::Binary(_) => None,
        })
        .collect();
    let distance = |face: &FaceInfo| face.stretch.to_number().abs_diff(Stretch::Normal.to_number());
    let width = faces.iter().map(|(face, _)| distance(face)).min();
    faces.retain(|(face, _)| Some(distance(face)) == width);
    faces.sort_by_key(|(face, path)| (face.style == Style::Oblique, *path, face.index));

    faces
        .into_iter()
        .map(|(face, path)| InstalledFace {
            path: path.to_owned(),
            index: face.index,
            weight: face.weight.0,
            italic: face.style != Style::Normal,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::FileResolver;
    use crate::text::bundled;
    use crate::text::tests::collection;

    /// The bundled Libertinus fonts stand in for installed ones.
    fn fixture() -> FileResolver {
        let mut database = Database::new();
        database.load_fonts_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts"));
        FileResolver::with_fonts(database)
    }

    fn face(name: &str) -> Face {
        name.parse().expect("face name")
    }

    fn requests(family: &str, faces: &[&str]) -> BTreeMap<String, BTreeMap<Face, String>> {
        let faces = faces.iter().map(|name| (face(name), "styles.body.font".to_owned()));
        BTreeMap::from([(family.to_owned(), faces.collect())])
    }

    #[test]
    fn finds_every_face_of_a_family_by_weight_and_italic() {
        let found = lookup(&fixture(), &requests("libertinus serif", &["bold-italic"])).expect("installed");
        let faces = &found.families["libertinus serif"];
        let names: Vec<_> = faces.keys().map(Face::to_string).collect();
        assert_eq!(names, ["regular", "italic", "bold", "bold-italic"]);
        let file = &faces[&face("bold-italic")];
        assert_eq!(file.resource.origin, Origin::Installed);
        assert!(
            file.resource.path.ends_with("LibertinusSerif-BoldItalic.otf"),
            "{file:?}"
        );
    }

    #[test]
    fn discovers_variable_weight_ranges_and_separate_italic_files() {
        let mut database = Database::new();
        database.load_fonts_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fonts"));
        let resolver = FileResolver::with_fonts(database);
        let Ok(found) = lookup(&resolver, &requests("Public Sans", &["300", "900-italic"])) else {
            panic!("installed")
        };
        let faces = &found.families["Public Sans"];
        assert_eq!(faces.len(), 18);
        assert!(faces[&face("300")].resource.path.ends_with("PublicSans.ttf"));
        assert!(
            faces[&face("900-italic")]
                .resource
                .path
                .ends_with("PublicSans-Italic.ttf")
        );
        let fonts = crate::text::Fonts::from_files(&found.families).unwrap_or_else(|errors| panic!("{errors:?}"));
        let light = fonts.face("Public Sans", Weight::of_font(300), FontStyle::Normal);
        let black = fonts.face("Public Sans", Weight::of_font(900), FontStyle::Normal);
        assert_ne!(
            fonts
                .shape(
                    "Hamburg",
                    light,
                    crate::config::values::Pt(12.0),
                    crate::config::theme::Lang::En
                )
                .width,
            fonts
                .shape(
                    "Hamburg",
                    black,
                    crate::config::values::Pt(12.0),
                    crate::config::theme::Lang::En
                )
                .width
        );
    }

    #[test]
    fn reports_a_missing_family_or_face_with_what_was_searched_for() {
        let errors = lookup(&fixture(), &requests("Nope Sans", &["500"]))
            .err()
            .expect("not installed");
        assert_eq!(errors[0].property.as_deref(), Some("styles.body.font"));
        assert_eq!(
            errors[0].message,
            "font family \"Nope Sans\" is not defined in fonts and not installed; searched the installed fonts \
             for \"Nope Sans\" 500"
        );

        let errors = lookup(&fixture(), &requests("Libertinus Serif", &["regular", "300-italic"]))
            .err()
            .expect("no such face");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].message,
            "installed font family \"Libertinus Serif\" has no 300-italic face; it has regular, italic, bold, \
             bold-italic"
        );
    }

    #[test]
    fn finds_a_face_of_a_collection_by_its_index() {
        let regular = bundled::font("fonts/LibertinusSerif-Regular.otf").unwrap();
        let bold = bundled::font("fonts/LibertinusSerif-Bold.otf").unwrap();
        let dir = std::env::temp_dir().join(format!("druck-collection-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Serif.ttc");
        std::fs::write(&path, collection(&[regular, bold])).unwrap();
        let mut database = Database::new();
        database.load_fonts_dir(&dir);
        let resolver = FileResolver::with_fonts(database);
        let found = lookup(&resolver, &requests("Libertinus Serif", &["bold"]));
        std::fs::remove_dir_all(&dir).unwrap();
        let Ok(found) = found else { panic!("installed") };
        let bold = &found.families["Libertinus Serif"][&face("bold")];
        assert_eq!(bold.index, 1);
        assert!(bold.resource.path.ends_with("Serif.ttc"), "{bold:?}");
    }
}
