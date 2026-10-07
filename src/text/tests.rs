use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;
use crate::config::source::{Origin, Resource};

fn bundled(path: &str) -> FaceFile {
    FaceFile {
        resource: Resource {
            origin: Origin::Bundled,
            path: format!("fonts/{path}"),
        },
        index: 0,
        variable: false,
    }
}

/// A family of bundled files by face name, such as `("bold", "LibertinusSerif-Bold.otf")`.
fn family(faces: &[(&str, &str)]) -> FontFiles {
    faces
        .iter()
        .map(|(face, path)| (face.parse().expect("face name"), bundled(path)))
        .collect()
}

fn serif() -> FontFiles {
    family(&[
        ("regular", "LibertinusSerif-Regular.otf"),
        ("italic", "LibertinusSerif-Italic.otf"),
        ("bold", "LibertinusSerif-Bold.otf"),
        ("bold-italic", "LibertinusSerif-BoldItalic.otf"),
    ])
}

fn load(family: FontFiles) -> Fonts {
    Fonts::from_files(&BTreeMap::from([("Test".to_owned(), family)])).unwrap_or_else(|e| panic!("{e:?}"))
}

fn shape(fonts: &Fonts, text: &str) -> ShapedRun {
    let face = fonts.face("Test", Weight::REGULAR, FontStyle::Normal);
    fonts.shape(text, face, Pt(10.0), Lang::En)
}

/// A font collection holding `fonts`, each a complete OpenType file, in order.
pub(crate) fn collection(fonts: &[&[u8]]) -> Vec<u8> {
    let mut offsets = Vec::new();
    let mut end = 12 + 4 * fonts.len();
    for font in fonts {
        offsets.push(end);
        end += font.len().next_multiple_of(4);
    }
    let mut data = b"ttcf".to_vec();
    data.extend(0x0001_0000u32.to_be_bytes());
    data.extend((fonts.len() as u32).to_be_bytes());
    for offset in &offsets {
        data.extend((*offset as u32).to_be_bytes());
    }
    // Table offsets count from the start of the collection.
    for (font, base) in fonts.iter().zip(offsets) {
        let mut font = font.to_vec();
        for record in table_records(&font).collect::<Vec<_>>() {
            let old = u32::from_be_bytes(font[record + 8..record + 12].try_into().unwrap());
            font[record + 8..record + 12].copy_from_slice(&(old + base as u32).to_be_bytes());
        }
        font.resize(font.len().next_multiple_of(4), 0);
        data.extend(font);
    }
    data
}

/// The byte offsets of the table records of an OpenType file.
fn table_records(font: &[u8]) -> impl Iterator<Item = usize> {
    let count = u16::from_be_bytes([font[4], font[5]]) as usize;
    (0..count).map(|table| 12 + 16 * table)
}

fn leak(data: Vec<u8>) -> &'static [u8] {
    Vec::leak(data)
}

#[test]
fn fi_ligature_is_one_glyph_over_both_bytes() {
    let run = shape(&load(serif()), "fi");
    assert_eq!(run.glyphs.len(), 1);
    assert_eq!(run.glyphs[0].text, 0..2);
}

#[test]
fn glyph_ranges_cover_every_byte_once() {
    let run = shape(&load(serif()), "Effizienz für Straße");
    let mut next = 0;
    for glyph in &run.glyphs {
        assert_eq!(glyph.text.start, next);
        next = glyph.text.end;
    }
    assert_eq!(next, run.text.len());
    let total: f64 = run.glyphs.iter().map(|g| g.x_advance.0).sum();
    assert!((total - run.width.0).abs() < 1e-9);
}

#[test]
fn kerning_narrows_a_pair() {
    let fonts = load(serif());
    let pair = shape(&fonts, "AV").width.0;
    let apart = shape(&fonts, "A").width.0 + shape(&fonts, "V").width.0;
    assert!(pair < apart, "{pair} should be less than {apart}");
}

#[test]
fn tracking_spaces_every_character_and_sets_no_ligatures() {
    let fonts = load(serif());
    let face = fonts.face("Test", Weight::REGULAR, FontStyle::Normal);
    let tracked = |tracking| fonts.shape_tracked("fi", face, Pt(10.0), Lang::En, tracking);
    let (narrow, wide) = (tracked(0.1), tracked(0.2));
    assert_eq!(narrow.glyphs.len(), 2);
    // One more point after each of the two letters at 10pt.
    assert!((wide.width.0 - narrow.width.0 - 2.0).abs() < 1e-9);
    assert!((wide.glyphs[0].x_advance.0 - narrow.glyphs[0].x_advance.0 - 1.0).abs() < 1e-9);
}

#[test]
fn missing_faces_fall_back() {
    let fonts = load(family(&[
        ("regular", "LibertinusSerif-Regular.otf"),
        ("bold", "LibertinusSerif-Bold.otf"),
    ]));
    let face = |weight, style| fonts.face("Test", weight, style);
    assert_eq!(
        face(Weight::BOLD, FontStyle::Italic),
        face(Weight::BOLD, FontStyle::Normal)
    );
    assert_eq!(
        face(Weight::REGULAR, FontStyle::Italic),
        face(Weight::REGULAR, FontStyle::Normal)
    );

    let fonts = load(family(&[
        ("regular", "LibertinusSerif-Regular.otf"),
        ("italic", "LibertinusSerif-Italic.otf"),
    ]));
    assert_eq!(
        fonts.face("Test", Weight::BOLD, FontStyle::Italic),
        fonts.face("Test", Weight::REGULAR, FontStyle::Italic)
    );
    assert_ne!(
        fonts.face("Test", Weight::BOLD, FontStyle::Italic),
        fonts.face("Test", Weight::REGULAR, FontStyle::Normal)
    );
}

#[test]
fn matches_the_nearest_weight_and_then_the_style() {
    // The files do not matter here, only the faces they are listed for.
    let fonts = load(family(&[
        ("300", "LibertinusSerif-Regular.otf"),
        ("500", "LibertinusSerif-Italic.otf"),
        ("bold", "LibertinusSerif-Bold.otf"),
        ("300-italic", "LibertinusSerif-BoldItalic.otf"),
    ]));
    let face = |weight: u16, style| fonts.face("Test", Weight::of_font(weight), style);
    let exact = |weight: u16, style| {
        fonts.families["Test"]
            .iter()
            .find(|(f, _)| {
                *f == Face {
                    weight: Weight::of_font(weight),
                    style,
                }
            })
            .unwrap()
            .1
    };
    let (normal, italic) = (FontStyle::Normal, FontStyle::Italic);
    assert_eq!(face(400, normal), exact(500, normal), "400 tries up to 500 first");
    assert_eq!(face(600, normal), exact(700, normal), "above 500 heavier first");
    assert_eq!(
        face(200, normal),
        exact(300, normal),
        "below 400 lighter first, then heavier"
    );
    assert_eq!(face(900, normal), exact(700, normal));
    assert_eq!(face(500, italic), exact(500, normal), "the weight before the style");
    assert_eq!(face(300, italic), exact(300, italic));
}

#[test]
fn loads_a_face_of_a_collection_by_its_index() {
    let regular = bundled::font("fonts/LibertinusSerif-Regular.otf").unwrap();
    let bold = bundled::font("fonts/LibertinusSerif-Bold.otf").unwrap();
    let data = leak(collection(&[regular, bold]));
    let file = FaceFile {
        index: 1,
        ..bundled("Serif.ttc")
    };
    let face = parse(data, &file, Face::REGULAR).expect("second face parses");
    assert_eq!(face.buzz.weight().to_number(), 700);
    assert!(face.file.starts_with("face 1 of"), "{}", face.file);
    assert!(parse(data, &FaceFile { index: 2, ..file }, Face::REGULAR).is_err());
}

#[test]
fn restricted_embedding_is_a_warning_naming_the_file() {
    let mut data = bundled::font("fonts/LibertinusMono-Regular.otf").unwrap().to_vec();
    let os2 = table_records(&data)
        .find(|&record| &data[record..record + 4] == b"OS/2")
        .expect("an OS/2 table");
    let table = u32::from_be_bytes(data[os2 + 8..os2 + 12].try_into().unwrap()) as usize;
    // fsType, at offset 8 of the OS/2 table: restricted license embedding.
    data[table + 8..table + 10].copy_from_slice(&2u16.to_be_bytes());
    let file = bundled("Restricted.otf");
    let fonts = Fonts {
        faces: vec![parse(leak(data), &file, Face::REGULAR).unwrap()],
        families: BTreeMap::new(),
    };
    let warning = fonts.embedding_warning(FaceId(0)).expect("a warning");
    assert!(
        warning.contains("Restricted.otf") && warning.contains("fsType"),
        "{warning}"
    );
    assert_eq!(load(serif()).embedding_warning(FaceId(0)), None);
}

#[test]
fn bundled_faces_cover_german_and_english_text() {
    let mono = family(&[("regular", "LibertinusMono-Regular.otf")]);
    let text = "äöüÄÖÜß „“ ‚‘ – — ’ “ ” … abc XYZ 019";
    for family in [serif(), mono] {
        let run = shape(&load(family), text);
        assert!(run.glyphs.iter().all(|g| g.id != 0), "notdef glyph in {text}");
    }
}

#[test]
fn missing_font_file_names_the_property() {
    let missing = FaceFile {
        resource: Resource {
            origin: Origin::WorkingDir(PathBuf::from("/nonexistent-druck-dir")),
            path: "nope.otf".to_owned(),
        },
        index: 0,
        variable: false,
    };
    let mut files = family(&[("regular", "LibertinusSerif-Regular.otf")]);
    files.insert("italic".parse().unwrap(), missing);
    let Err(errors) = Fonts::from_files(&BTreeMap::from([("Test".to_owned(), files)])) else {
        panic!("loading should fail");
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].property.as_deref(), Some("fonts.Test.italic"));
    assert!(errors[0].message.contains("nope.otf"), "{}", errors[0].message);
}

#[test]
fn unknown_bundled_path_is_an_error() {
    let files = family(&[("regular", "Nope.otf")]);
    let Err(errors) = Fonts::from_files(&BTreeMap::from([("Test".to_owned(), files)])) else {
        panic!("loading should fail");
    };
    assert_eq!(errors[0].property.as_deref(), Some("fonts.Test.regular"));
}

pub(crate) fn variable_file(italic: bool) -> FaceFile {
    FaceFile {
        resource: Resource {
            origin: Origin::WorkingDir(PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fonts"
            ))),
            path: if italic {
                "PublicSans-Italic.ttf"
            } else {
                "PublicSans.ttf"
            }
            .to_owned(),
        },
        index: 0,
        variable: true,
    }
}

#[test]
fn variable_files_supply_weights_and_separate_italics_with_matching_pdf_instances() {
    let fonts = load(BTreeMap::from([
        (Face::REGULAR, variable_file(false)),
        ("italic".parse().unwrap(), variable_file(true)),
    ]));
    assert_eq!(fonts.families["Test"].len(), 18);
    let mut widths = Vec::new();
    for weight in [100, 400, 700, 900] {
        for style in [FontStyle::Normal, FontStyle::Italic] {
            let id = fonts.face("Test", Weight::of_font(weight), style);
            let run = fonts.shape("Hamburg", id, Pt(12.0), Lang::En);
            assert!(run.glyphs.iter().all(|glyph| glyph.id != 0));
            widths.push(run.width);
            let path = variable_file(style == FontStyle::Italic).resource.file().unwrap();
            let expected = krilla::text::Font::new_variable(
                std::fs::read(path).unwrap().into(),
                0,
                &[(krilla::text::Tag::new(b"wght"), weight as f32)],
            )
            .unwrap();
            assert_eq!(fonts.pdf_font(id), &expected);
        }
    }
    assert_ne!(widths[0], widths[6], "weight changes measured advances");

    let mut files = BTreeMap::from([(Face::REGULAR, variable_file(false))]);
    files.insert("bold".parse().unwrap(), bundled("LibertinusSerif-Bold.otf"));
    let fonts = load(files);
    let id = fonts.face("Test", Weight::BOLD, FontStyle::Normal);
    assert!(fonts.faces[id.0].file.contains("LibertinusSerif-Bold"));
}

/// Replaces the fixture's weight axis in memory to exercise italic and bounded ranges.
fn with_axis(tag: &[u8; 4], min: f32, default: f32, max: f32) -> &'static [u8] {
    let mut data = include_bytes!("../../tests/fixtures/fonts/PublicSans.ttf").to_vec();
    let record = table_records(&data)
        .find(|&record| &data[record..record + 4] == b"fvar")
        .unwrap();
    let table = u32::from_be_bytes(data[record + 8..record + 12].try_into().unwrap()) as usize;
    let axis = table + u16::from_be_bytes(data[table + 4..table + 6].try_into().unwrap()) as usize;
    data[axis..axis + 4].copy_from_slice(tag);
    for (offset, value) in [(4, min), (8, default), (12, max)] {
        data[axis + offset..axis + offset + 4].copy_from_slice(&((value * 65536.0) as i32).to_be_bytes());
    }
    leak(data)
}

#[test]
fn italic_axis_supplies_both_styles_and_sets_the_same_coordinates_for_pdf() {
    let data = with_axis(b"ital", 0.0, 0.0, 1.0);
    let font = rustybuzz::Face::from_slice(data, 0).unwrap();
    let faces = variations::faces(&font, Face::REGULAR);
    assert_eq!(faces.len(), 2);
    let normal = parse(data, &variable_file(false), faces[0]).unwrap();
    let italic = parse(data, &variable_file(false), faces[1]).unwrap();
    assert_ne!(normal.buzz.variation_coordinates(), italic.buzz.variation_coordinates());
    let expected = krilla::text::Font::new_variable(data.into(), 0, &[(krilla::text::Tag::new(b"ital"), 1.0)]).unwrap();
    assert_eq!(italic.font, expected);
}

#[test]
fn variable_ranges_are_respected_instead_of_clamped() {
    let data = with_axis(b"wght", 300.0, 400.0, 700.0);
    let font = rustybuzz::Face::from_slice(data, 0).unwrap();
    let faces = variations::faces(&font, Face::REGULAR);
    let weights: Vec<_> = faces.iter().map(|face| face.weight.get()).collect();
    assert_eq!(weights, [300, 400, 500, 600, 700]);
    let result = parse(
        data,
        &variable_file(false),
        Face {
            weight: Weight::of_font(900),
            style: FontStyle::Normal,
        },
    );
    assert!(result.err().unwrap().contains("outside 300 to 700"));
}

#[test]
fn variable_collection_uses_the_selected_face() {
    let normal = include_bytes!("../../tests/fixtures/fonts/PublicSans.ttf");
    let italic = include_bytes!("../../tests/fixtures/fonts/PublicSans-Italic.ttf");
    let data = leak(collection(&[normal, italic]));
    let file = FaceFile {
        index: 1,
        ..variable_file(true)
    };
    let face = "900-italic".parse().unwrap();
    let loaded = parse(data, &file, face).unwrap();
    assert!(loaded.buzz.is_italic());
    assert_eq!(variations::faces(&loaded.buzz, face).len(), 9);
    let expected =
        krilla::text::Font::new_variable(data.into(), 1, &[(krilla::text::Tag::new(b"wght"), 900.0)]).unwrap();
    assert_eq!(loaded.font, expected);
}
