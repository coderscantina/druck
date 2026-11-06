//! The renderer's promises: identical paths, in-memory rendering, reuse by content, isolated concurrent jobs, and
//! resolver failures that name the resource.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use crate::cancel::Cancel;
use crate::input::{Date, DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use crate::renderer::{Failure, LayoutOptions, PdfOptions, Renderer};
use crate::resource::{Bytes, FileResolver, MemoryResolver, Origin, ResolveError, Resolver};
use crate::text::bundled;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");
/// Layout in a debug build needs more stack than a test thread has by default.
const STACK: usize = 16 * 1024 * 1024;

fn on_big_stack<T: Send>(run: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(STACK)
            .spawn_scoped(scope, run)
            .expect("a thread")
            .join()
            .expect("no panic")
    })
}

fn memory_dir() -> PathBuf {
    PathBuf::from("/memory")
}

fn document(text: &str, dir: PathBuf, name: &str, theme: ThemeSelection) -> Input {
    Input {
        text: DocumentText::Single {
            text: text.to_owned(),
            origin: Origin::Document(dir),
            name: name.to_owned(),
        },
        theme,
        overrides: Overrides::default(),
        today: Date::from_unix(0),
    }
}

fn no_theme() -> ThemeSelection {
    ThemeSelection::Document {
        dirs: ThemeDirs::default(),
    }
}

fn in_memory_theme() -> ThemeSelection {
    ThemeSelection::File {
        path: memory_dir().join("theme.json"),
    }
}

fn pdf(renderer: &Renderer, resolver: &dyn Resolver, input: &Input) -> Vec<u8> {
    on_big_stack(|| {
        renderer
            .render(resolver, input, &PdfOptions::default(), &Cancel::default())
            .unwrap_or_else(|failure| panic!("{failure:?}"))
            .bytes
    })
}

fn errors(failure: Result<Vec<u8>, Failure>) -> Vec<crate::diagnostic::Diagnostic> {
    match failure {
        Err(Failure::Errors(errors)) => errors,
        Err(Failure::Cancelled) => panic!("not cancelled"),
        Ok(_) => panic!("expected a failure"),
    }
}

fn try_pdf(renderer: &Renderer, resolver: &dyn Resolver, input: &Input) -> Result<Vec<u8>, Failure> {
    on_big_stack(|| {
        renderer
            .render(resolver, input, &PdfOptions::default(), &Cancel::default())
            .map(|rendered| rendered.bytes)
    })
}

#[test]
fn the_simple_and_the_explicit_path_write_the_same_pdf() {
    let text = "---\ntitle: Same\ndate: 2026-01-02\n---\n\nSome text.\n";
    let input = document(text, memory_dir(), "same.md", no_theme());
    let resolver = MemoryResolver::new();
    let renderer = Renderer::new();
    let simple = pdf(&renderer, &resolver, &input);
    let explicit = on_big_stack(|| {
        let cancel = Cancel::default();
        let configured = renderer
            .job(&resolver, &input, &cancel)
            .configure()
            .unwrap_or_else(|f| panic!("{f:?}"));
        let layout = configured
            .layout(&LayoutOptions::default())
            .unwrap_or_else(|f| panic!("{f:?}"));
        layout
            .pdf(&PdfOptions::default(), &cancel)
            .unwrap_or_else(|f| panic!("{f:?}"))
            .bytes
    });
    assert!(simple.starts_with(b"%PDF"));
    assert_eq!(simple, explicit);
}

#[test]
fn errors_in_the_configuration_are_structured() {
    let input = document(
        "---\ntitle: [unclosed\n---\n\nText.\n",
        memory_dir(),
        "bad.md",
        no_theme(),
    );
    let resolver = MemoryResolver::new();
    let renderer = Renderer::new();
    let configured = on_big_stack(|| {
        renderer
            .job(&resolver, &input, &Cancel::default())
            .configure()
            .map(|_| ())
    });
    let Err(Failure::Errors(errors)) = configured else {
        panic!("the front matter is invalid")
    };
    assert!(!errors.is_empty());
}

/// Every file below `folder` under its path relative to `root`.
fn insert_tree(resolver: &mut MemoryResolver, origin: &Origin, root: &Path, folder: &Path) {
    for entry in fs::read_dir(folder).expect("sample folder") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            insert_tree(resolver, origin, root, &path);
        } else {
            let name = path
                .strip_prefix(root)
                .expect("below the root")
                .to_str()
                .expect("utf-8");
            resolver.insert(origin.clone(), name, fs::read(&path).expect("sample"));
        }
    }
}

#[test]
fn a_memory_resolver_renders_a_report_with_its_theme_and_images_as_files_would() {
    let samples = Path::new(SAMPLES);
    let text = fs::read_to_string(samples.join("report.md")).expect("sample");

    let memory = memory_dir().join("report");
    let theme_dir = memory_dir().join("report-theme");
    let mut resolver = MemoryResolver::new();
    let document_origin = Origin::Document(memory.clone());
    insert_tree(&mut resolver, &document_origin, samples, &samples.join("images"));
    let bibliography = fs::read(samples.join("references.bib")).expect("sample");
    resolver.insert(document_origin, "references.bib", bibliography);
    insert_tree(
        &mut resolver,
        &Origin::Theme(theme_dir.clone()),
        &samples.join("themes"),
        &samples.join("themes"),
    );
    let in_memory = document(
        &text,
        memory,
        "report.md",
        ThemeSelection::File {
            path: theme_dir.join("report.json"),
        },
    );
    let from_memory = pdf(&Renderer::new(), &resolver, &in_memory);

    let from_files = document(
        &text,
        samples.to_owned(),
        "report.md",
        ThemeSelection::File {
            path: samples.join("themes/report.json"),
        },
    );
    assert_eq!(from_memory, pdf(&Renderer::new(), &FileResolver::new(), &from_files));
}

fn font(path: &str) -> &'static [u8] {
    bundled::font(path).expect("bundled font")
}

const FONT_THEME: &str =
    r#"{"version": 1, "fonts": {"Body": {"regular": "body.otf"}}, "styles": {"body": {"font": "Body"}}}"#;

fn font_resolver(file: &'static str) -> MemoryResolver {
    let mut resolver = MemoryResolver::new();
    let origin = Origin::Theme(memory_dir());
    resolver.insert(origin.clone(), "theme.json", FONT_THEME.as_bytes());
    resolver.insert(origin, "body.otf", font(file));
    resolver
}

#[test]
fn a_replaced_font_is_seen_and_an_unchanged_one_is_not_built_again() {
    let input = document("Plain text.\n", memory_dir(), "doc.md", in_memory_theme());
    let renderer = Renderer::new();
    let mut resolver = font_resolver("fonts/LibertinusSerif-Regular.otf");
    let first = pdf(&renderer, &resolver, &input);
    let built = renderer.faces.builds.load(Ordering::Relaxed);
    assert_eq!(first, pdf(&renderer, &resolver, &input));
    assert_eq!(
        renderer.faces.builds.load(Ordering::Relaxed),
        built,
        "unchanged fonts are reused"
    );

    resolver.insert(
        Origin::Theme(memory_dir()),
        "body.otf",
        font("fonts/LibertinusSerif-Bold.otf"),
    );
    let replaced = pdf(&renderer, &resolver, &input);
    assert_eq!(
        renderer.faces.builds.load(Ordering::Relaxed),
        built + 1,
        "only the new file is built"
    );
    assert_ne!(first, replaced);
    assert_eq!(
        replaced,
        pdf(&Renderer::new(), &resolver, &input),
        "as a fresh renderer gives"
    );
}

fn svg(width: u32) -> Vec<u8> {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="20"><rect width="{width}" height="20"/></svg>"#)
        .into_bytes()
}

#[test]
fn a_replaced_image_is_seen_and_an_unchanged_one_is_not_decoded_again() {
    let input = document("![A picture](picture.svg)\n", memory_dir(), "doc.md", no_theme());
    let origin = Origin::Document(memory_dir());
    let renderer = Renderer::new();
    let mut resolver = MemoryResolver::new();
    resolver.insert(origin.clone(), "picture.svg", svg(40));
    let first = pdf(&renderer, &resolver, &input);
    assert_eq!(renderer.images.decodes.load(Ordering::Relaxed), 1);
    assert_eq!(first, pdf(&renderer, &resolver, &input));
    assert_eq!(
        renderer.images.decodes.load(Ordering::Relaxed),
        1,
        "unchanged images are reused"
    );

    resolver.insert(origin, "picture.svg", svg(80));
    let replaced = pdf(&renderer, &resolver, &input);
    assert_eq!(renderer.images.decodes.load(Ordering::Relaxed), 2);
    assert_ne!(first, replaced);
    assert_eq!(
        replaced,
        pdf(&Renderer::new(), &resolver, &input),
        "as a fresh renderer gives"
    );
}

#[test]
fn the_extension_still_decides_when_the_content_is_already_decoded() {
    let origin = Origin::Document(memory_dir());
    let png = fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/images/pixel.png")).expect("fixture");
    let renderer = Renderer::new();
    let mut resolver = MemoryResolver::new();
    resolver.insert(origin.clone(), "good.png", png.clone());
    resolver.insert(origin, "bad.jpg", png);
    let good = document("![A](good.png)\n", memory_dir(), "doc.md", no_theme());
    pdf(&renderer, &resolver, &good);
    let bad = document("![B](bad.jpg)\n", memory_dir(), "doc.md", no_theme());
    let errors = errors(try_pdf(&renderer, &resolver, &bad));
    assert!(errors[0].message.contains("contains PNG data"), "{}", errors[0].message);
}

#[test]
fn what_two_jobs_did_not_use_is_forgotten_and_a_layout_keeps_its_own() {
    let input = document("Plain text.\n", memory_dir(), "doc.md", in_memory_theme());
    let plain = document("Plain text.\n", memory_dir(), "doc.md", no_theme());
    let renderer = Renderer::new();
    let resolver = font_resolver("fonts/LibertinusSerif-Italic.otf");
    let cancel = Cancel::default();
    let kept = on_big_stack(|| {
        let configured = renderer
            .job(&resolver, &input, &cancel)
            .configure()
            .unwrap_or_else(|f| panic!("{f:?}"));
        configured
            .layout(&LayoutOptions::default())
            .unwrap_or_else(|f| panic!("{f:?}"))
    });
    let with_theme = renderer.faces.len();
    pdf(&renderer, &resolver, &plain);
    assert_eq!(renderer.faces.len(), with_theme, "the previous job's faces stay");
    pdf(&renderer, &resolver, &plain);
    pdf(&renderer, &resolver, &plain);
    assert!(renderer.faces.len() < with_theme, "the theme's font is gone");
    assert!(
        kept.pdf(&PdfOptions::default(), &Cancel::default()).is_ok(),
        "the layout holds its own face"
    );
}

#[test]
fn a_bare_theme_name_means_the_project_then_a_local_file_then_the_shared_folder() {
    let (project, base, shared) = (
        PathBuf::from("/project"),
        PathBuf::from("/base"),
        PathBuf::from("/shared"),
    );
    let dirs = ThemeDirs {
        project: Some(project.clone()),
        shared: Some(shared.clone()),
    };
    let found = |resolver: &MemoryResolver, selection: &str| {
        crate::pipeline::theme_file(resolver, Path::new(selection), &base, &dirs)
    };
    let mut resolver = MemoryResolver::new();
    assert_eq!(
        found(&resolver, "book"),
        shared.join("book.json"),
        "named in the last place looked"
    );
    assert_eq!(
        found(&resolver, "themes/book"),
        base.join("themes/book"),
        "paths are not names"
    );
    resolver.insert(Origin::Theme(shared.clone()), "book.json", b"{}".as_slice());
    assert_eq!(found(&resolver, "book"), shared.join("book.json"));
    resolver.insert(Origin::Theme(base.clone()), "book", b"{}".as_slice());
    assert_eq!(found(&resolver, "book"), base.join("book"));
    resolver.insert(
        Origin::Theme(project.join("themes/book")),
        "theme.json",
        b"{}".as_slice(),
    );
    assert_eq!(found(&resolver, "book"), project.join("themes/book/theme.json"));
}

#[test]
fn concurrent_jobs_with_different_themes_give_what_they_give_alone() {
    let samples = Path::new(SAMPLES);
    let read = |name: &str| fs::read_to_string(samples.join(name)).expect("sample");
    let (report, novel) = (read("report.md"), read("de.md"));
    let resolver = FileResolver::new();
    let report = document(
        &report,
        samples.to_owned(),
        "report.md",
        ThemeSelection::File {
            path: samples.join("themes/report.json"),
        },
    );
    let novel = document(
        &novel,
        samples.to_owned(),
        "de.md",
        ThemeSelection::File {
            path: samples.join("themes/novel.json"),
        },
    );
    let alone = [
        pdf(&Renderer::new(), &resolver, &report),
        pdf(&Renderer::new(), &resolver, &novel),
    ];
    assert_ne!(alone[0], alone[1]);

    let renderer = Renderer::new();
    let run = |input: &Input| -> Vec<Vec<u8>> { (0..3).map(|_| pdf(&renderer, &resolver, input)).collect() };
    let (reports, novels) = std::thread::scope(|scope| {
        let reports = scope.spawn(|| run(&report));
        let novels = scope.spawn(|| run(&novel));
        (reports.join().expect("no panic"), novels.join().expect("no panic"))
    });
    assert!(reports.iter().all(|bytes| *bytes == alone[0]));
    assert!(novels.iter().all(|bytes| *bytes == alone[1]));
}

/// Fails every read of one path, as a broken disk would.
struct Broken {
    inner: MemoryResolver,
    path: &'static str,
}

impl Resolver for Broken {
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError> {
        if path == self.path {
            return Err(ResolveError::Unreadable("the disk is on fire".to_owned()));
        }
        self.inner.read(origin, path)
    }
}

#[test]
fn a_resolver_failure_names_the_resource_and_its_origin_and_harms_nothing() {
    let renderer = Renderer::new();
    let input = document("Plain text.\n", memory_dir(), "doc.md", in_memory_theme());

    // A missing font file has a stand-in, see `a_missing_font_file_has_a_bundled_face_stand_in_and_says_so`, and the
    // warning still names the resource.
    let mut missing = MemoryResolver::new();
    missing.insert(Origin::Theme(memory_dir()), "theme.json", FONT_THEME.as_bytes());
    let warned = rendered(&renderer, &missing, &input);
    assert_eq!(warned.warnings[0].property.as_deref(), Some("fonts.Body.regular"));
    assert_eq!(
        warned.warnings[0].message,
        format!(
            "resource not found: \"body.otf\" relative to the theme in {}; Libertinus Serif stands in",
            memory_dir().display()
        )
    );

    let broken = Broken {
        inner: font_resolver("fonts/LibertinusSerif-Regular.otf"),
        path: "body.otf",
    };
    let errors_of_broken = errors(try_pdf(&renderer, &broken, &input));
    assert_eq!(
        errors_of_broken[0].message,
        format!(
            "cannot read \"body.otf\" relative to the theme in {}: the disk is on fire",
            memory_dir().display()
        )
    );

    let picture = document("Text.\n\n![A](gone.png)\n", memory_dir(), "doc.md", no_theme());
    let image_warning = rendered(&renderer, &missing, &picture).warnings.remove(0);
    assert_eq!(image_warning.location, Some((3, 1).into()));
    assert_eq!(
        image_warning.message,
        format!(
            "image not found: \"gone.png\" relative to the document in {}; a box stands in",
            memory_dir().display()
        )
    );

    let working = font_resolver("fonts/LibertinusSerif-Regular.otf");
    assert_eq!(
        pdf(&renderer, &working, &input),
        pdf(&Renderer::new(), &working, &input),
        "the renderer still works"
    );
}

fn rendered(renderer: &Renderer, resolver: &dyn Resolver, input: &Input) -> crate::renderer::Rendered {
    on_big_stack(|| {
        renderer
            .render(resolver, input, &PdfOptions::default(), &Cancel::default())
            .unwrap_or_else(|failure| panic!("{failure:?}"))
    })
}

fn codes(rendered: &crate::renderer::Rendered) -> Vec<crate::diagnostic::Code> {
    rendered
        .warnings
        .iter()
        .filter_map(|warning| warning.code.clone())
        .collect()
}

#[test]
fn a_missing_image_stands_in_as_a_box_and_says_so() {
    let input = document(
        "Text.\n\n![A map](assets/gone.png)\n",
        memory_dir(),
        "doc.md",
        no_theme(),
    );
    let done = rendered(&Renderer::new(), &MemoryResolver::new(), &input);
    assert!(done.parts > 0);
    let warning = done
        .warnings
        .iter()
        .find(|warning| warning.code.is_some())
        .expect("a warning");
    assert_eq!(
        warning.code,
        Some(crate::diagnostic::Code::ImageNotFound {
            file: memory_dir().join("assets/gone.png")
        })
    );
    assert_eq!(warning.location, Some((3, 1).into()));
}

#[test]
fn a_missing_bibliography_leaves_citations_as_their_keys_and_says_so() {
    let text = "---\nbibliography: gone.bib\n---\n\nAs [@smith2020] wrote.\n";
    let input = document(text, memory_dir(), "doc.md", no_theme());
    let done = rendered(&Renderer::new(), &MemoryResolver::new(), &input);
    assert_eq!(
        codes(&done),
        vec![crate::diagnostic::Code::BibliographyNotFound {
            file: memory_dir().join("gone.bib")
        }]
    );
}

#[test]
fn a_missing_font_file_has_a_bundled_face_stand_in_and_says_so() {
    let mut resolver = MemoryResolver::new();
    resolver.insert(Origin::Theme(memory_dir()), "theme.json", FONT_THEME.as_bytes());
    let input = document("Plain text.\n", memory_dir(), "doc.md", in_memory_theme());
    let done = rendered(&Renderer::new(), &resolver, &input);
    assert_eq!(
        codes(&done),
        vec![crate::diagnostic::Code::FontNotFound {
            family: "Body".to_owned(),
            file: memory_dir().join("body.otf"),
            stand_in: "Libertinus Serif".to_owned(),
        }]
    );
}

#[test]
fn a_final_export_refuses_what_is_missing() {
    let input = document("![A map](assets/gone.png)\n", memory_dir(), "doc.md", no_theme());
    let renderer = Renderer::new();
    let resolver = MemoryResolver::new();
    let cancel = Cancel::default();
    let configured = renderer
        .job(&resolver, &input, &cancel)
        .configure()
        .expect("configures");
    let Err(Failure::Errors(errors)) = configured.check_final("print") else {
        panic!("a final export refuses a missing image");
    };
    assert!(
        errors[0]
            .message
            .contains("is final, but this image is not there: assets/gone.png"),
        "{errors:?}"
    );
}
