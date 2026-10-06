use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;
use crate::config::source::Origin;

fn bundled(path: &str) -> Resource {
    Resource {
        origin: Origin::Bundled,
        path: format!("fonts/{path}"),
    }
}

fn files(family: FontFiles) -> BTreeMap<String, FontFiles> {
    BTreeMap::from([("Test".to_owned(), family)])
}

fn serif() -> FontFiles {
    FontFiles {
        regular: bundled("LibertinusSerif-Regular.otf"),
        italic: Some(bundled("LibertinusSerif-Italic.otf")),
        bold: Some(bundled("LibertinusSerif-Bold.otf")),
        bold_italic: Some(bundled("LibertinusSerif-BoldItalic.otf")),
    }
}

fn load(family: FontFiles) -> Fonts {
    Fonts::from_files(&files(family)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn shape(fonts: &Fonts, text: &str) -> ShapedRun {
    let face = fonts.face("Test", Weight::Regular, FontStyle::Normal);
    fonts.shape(text, face, Pt(10.0), Lang::En)
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
fn missing_faces_fall_back() {
    let regular = bundled("LibertinusSerif-Regular.otf");
    let bold = Some(bundled("LibertinusSerif-Bold.otf"));
    let italic = Some(bundled("LibertinusSerif-Italic.otf"));
    let none = FontFiles {
        regular: regular.clone(),
        italic: None,
        bold: bold.clone(),
        bold_italic: None,
    };
    let fonts = load(none);
    let face = |weight, style| fonts.face("Test", weight, style);
    assert_eq!(
        face(Weight::Bold, FontStyle::Italic),
        face(Weight::Bold, FontStyle::Normal)
    );
    assert_eq!(
        face(Weight::Regular, FontStyle::Italic),
        face(Weight::Regular, FontStyle::Normal)
    );

    let only_italic = FontFiles {
        regular,
        italic,
        bold: None,
        bold_italic: None,
    };
    let fonts = load(only_italic);
    assert_eq!(
        fonts.face("Test", Weight::Bold, FontStyle::Italic),
        fonts.face("Test", Weight::Regular, FontStyle::Italic)
    );
    assert_ne!(
        fonts.face("Test", Weight::Bold, FontStyle::Italic),
        fonts.face("Test", Weight::Regular, FontStyle::Normal)
    );
}

#[test]
fn bundled_faces_cover_german_and_english_text() {
    let mono = FontFiles {
        regular: bundled("LibertinusMono-Regular.otf"),
        italic: None,
        bold: None,
        bold_italic: None,
    };
    let text = "äöüÄÖÜß „“ ‚‘ – — ’ “ ” … abc XYZ 019";
    for family in [serif(), mono] {
        let run = shape(&load(family), text);
        assert!(run.glyphs.iter().all(|g| g.id != 0), "notdef glyph in {text}");
    }
}

#[test]
fn missing_font_file_names_the_property() {
    let missing = Resource {
        origin: Origin::WorkingDir(PathBuf::from("/nonexistent-kyber-dir")),
        path: "nope.otf".to_owned(),
    };
    let family = FontFiles {
        regular: bundled("LibertinusSerif-Regular.otf"),
        italic: Some(missing),
        bold: None,
        bold_italic: None,
    };
    let Err(errors) = Fonts::from_files(&files(family)) else {
        panic!("loading should fail");
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].property.as_deref(), Some("fonts.Test.italic"));
    assert!(errors[0].message.contains("nope.otf"), "{}", errors[0].message);
}

#[test]
fn unknown_bundled_path_is_an_error() {
    let family = FontFiles {
        regular: bundled("Nope.otf"),
        italic: None,
        bold: None,
        bold_italic: None,
    };
    let Err(errors) = Fonts::from_files(&files(family)) else {
        panic!("loading should fail");
    };
    assert_eq!(errors[0].property.as_deref(), Some("fonts.Test.regular"));
}
