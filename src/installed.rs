//! Font families looked up among the fonts installed on the machine. Only the CLI looks there, so
//! configuration and layout never depend on the system.

use std::collections::BTreeMap;

use fontdb::{Database, FaceInfo, Source, Stretch, Style};

use crate::config::resolved::{FaceFile, FontFiles};
use crate::config::source::{Origin, Resource};
use crate::config::theme::{Face, FontStyle, Weight};
use crate::diagnostic::Diagnostic;

/// The fonts installed in the system's font directories.
pub fn system() -> Database {
    let mut database = Database::new();
    database.load_system_fonts();
    database
}

/// The faces of every family in `requests`, which maps a family name to the faces styles request
/// and the first property requesting each. Each requested face must be installed; the family's other
/// faces serve inline emphasis and strong text. Errors name the family and face searched for.
pub fn lookup(
    database: &Database,
    requests: &BTreeMap<String, BTreeMap<Face, String>>,
) -> Result<BTreeMap<String, FontFiles>, Vec<Diagnostic>> {
    let mut found = BTreeMap::new();
    let mut errors = Vec::new();
    for (family, requested) in requests {
        let faces = family_faces(database, family);
        if faces.is_empty() {
            let (face, property) = requested.first_key_value().expect("a family is requested for a face");
            let message = format!(
                "font family \"{family}\" is not defined in fonts and not installed; searched the installed fonts \
                 for \"{family}\" {face}"
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
        found.insert(family.clone(), faces);
    }
    if errors.is_empty() { Ok(found) } else { Err(errors) }
}

/// The installed faces whose family name is `family`, ignoring ASCII case. Only the width closest
/// to normal counts, so condensed faces do not stand in for regular ones. Where two files hold the
/// same face, italic beats oblique, then the first path and index win.
fn family_faces(database: &Database, family: &str) -> FontFiles {
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

    let mut files = FontFiles::new();
    for (face, path) in faces {
        let key = Face {
            weight: Weight::of_font(face.weight.0),
            style: match face.style {
                Style::Normal => FontStyle::Normal,
                Style::Italic | Style::Oblique => FontStyle::Italic,
            },
        };
        let supported = database
            .with_face_data(face.id, |data, index| {
                let font = rustybuzz::Face::from_slice(data, index)?;
                Some(if font.is_variable() {
                    crate::text::variations::faces(&font, key)
                } else {
                    vec![key]
                })
            })
            .flatten()
            .unwrap_or_default();
        for key in supported {
            files.entry(key).or_insert_with(|| FaceFile {
                resource: Resource {
                    origin: Origin::Installed,
                    path: path.to_owned(),
                },
                index: face.index,
                // Lookup already expanded this file to its supported instances.
                variable: false,
            });
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::bundled;
    use crate::text::tests::collection;

    /// The bundled Libertinus fonts stand in for installed ones.
    fn fixture() -> Database {
        let mut database = Database::new();
        database.load_fonts_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts"));
        database
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
        let faces = &found["libertinus serif"];
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
        let found = lookup(&database, &requests("Public Sans", &["300", "900-italic"])).unwrap();
        let faces = &found["Public Sans"];
        assert_eq!(faces.len(), 18);
        assert!(faces[&face("300")].resource.path.ends_with("PublicSans.ttf"));
        assert!(
            faces[&face("900-italic")]
                .resource
                .path
                .ends_with("PublicSans-Italic.ttf")
        );
        let fonts = crate::text::Fonts::from_files(&found).unwrap_or_else(|errors| panic!("{errors:?}"));
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
        let errors = lookup(&fixture(), &requests("Nope Sans", &["500"])).unwrap_err();
        assert_eq!(errors[0].property.as_deref(), Some("styles.body.font"));
        assert_eq!(
            errors[0].message,
            "font family \"Nope Sans\" is not defined in fonts and not installed; searched the installed fonts \
             for \"Nope Sans\" 500"
        );

        let errors = lookup(&fixture(), &requests("Libertinus Serif", &["regular", "300-italic"])).unwrap_err();
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
        let found = lookup(&database, &requests("Libertinus Serif", &["bold"])).expect("installed");
        std::fs::remove_dir_all(&dir).unwrap();
        let bold = &found["Libertinus Serif"][&face("bold")];
        assert_eq!(bold.index, 1);
        assert!(bold.resource.path.ends_with("Serif.ttc"), "{bold:?}");
    }
}
