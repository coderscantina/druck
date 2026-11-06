//! The stable meanings of diagnostics, one case of each kind of reference, and how a terminal shows them.

use std::path::{Path, PathBuf};

use crate::cancel::Cancel;
use crate::diagnostic::{Code, Diagnostic, Origin, Resource, Severity, Source};
use crate::input::{Date, DocumentText, Input, Overrides, Project, ThemeDirs, ThemeSelection};
use crate::renderer::{Failure, LayoutOptions, Renderer};
use crate::resource::{MemoryResolver, Resolver};

fn book() -> PathBuf {
    PathBuf::from("/work/book")
}

fn input(text: DocumentText, theme: ThemeSelection, overrides: Overrides) -> Input {
    Input {
        text,
        theme,
        overrides,
        today: Date::from_unix(0),
    }
}

fn no_theme() -> ThemeSelection {
    ThemeSelection::Document {
        dirs: ThemeDirs::default(),
    }
}

fn single(text: &str) -> DocumentText {
    DocumentText::Single {
        text: text.to_owned(),
        origin: Origin::Document(book()),
        name: "doc.md".to_owned(),
    }
}

/// The errors of laying out `input`, on a stack that suits a debug build.
fn errors(resolver: &dyn Resolver, input: &Input) -> Vec<Diagnostic> {
    let cancel = Cancel::default();
    let renderer = Renderer::new();
    let run = || {
        renderer
            .job(resolver, input, &cancel)
            .configure()?
            .layout(&LayoutOptions::default())
    };
    let result = std::thread::scope(|scope| {
        let thread = std::thread::Builder::new().stack_size(16 * 1024 * 1024);
        thread
            .spawn_scoped(scope, run)
            .expect("thread")
            .join()
            .expect("no panic")
    });
    match result {
        Err(Failure::Errors(errors)) => errors,
        Err(Failure::Cancelled) => panic!("nothing cancels this job"),
        Ok(_) => panic!("expected errors"),
    }
}

#[test]
fn an_error_in_a_project_names_the_chapter_file_and_its_line() {
    let mut resolver = MemoryResolver::new();
    let origin = Origin::Document(book());
    let manifest = "chapters:\n  - manuscript/one.md\n  - manuscript/two.md\n";
    resolver.insert(origin.clone(), "druck.yaml", manifest.as_bytes());
    resolver.insert(origin.clone(), "manuscript/one.md", b"# One\n\nText.\n".as_slice());
    resolver.insert(origin, "manuscript/two.md", b"# Two\n\nSee [@gone].\n".as_slice());
    let project = Project::open(&resolver, &book()).expect("the project opens");
    let (text, _) = project.export(None).expect("no export asked for");

    let errors = errors(&resolver, &input(text, no_theme(), Overrides::default()));

    let chapter = book().join("manuscript/two.md");
    assert_eq!(errors[0].severity, Severity::Error);
    assert_eq!(errors[0].source, Some(Source::Document(chapter)));
    assert_eq!(errors[0].location, Some((3, 5).into()));
    assert!(
        (errors[0].display_from(Path::new("/work/book")).to_string())
            .starts_with("error: manuscript/two.md:3:5: a citation needs a bibliography"),
        "{}",
        errors[0]
    );
}

#[test]
fn a_message_naming_another_chapter_shortens_it_like_the_source() {
    let mut resolver = MemoryResolver::new();
    let origin = Origin::Document(book());
    let manifest = "chapters:\n  - one.md\n  - two.md\n";
    resolver.insert(origin.clone(), "druck.yaml", manifest.as_bytes());
    resolver.insert(origin.clone(), "one.md", b"# One {#sec:same}\n".as_slice());
    resolver.insert(origin, "two.md", b"# Two {#sec:same}\n".as_slice());
    let project = Project::open(&resolver, &book()).expect("the project opens");
    let (text, _) = project.export(None).expect("no export asked for");

    let errors = errors(&resolver, &input(text, no_theme(), Overrides::default()));

    let absolute = errors[0].message.clone();
    assert!(absolute.contains("line 1 of /work/book/one.md"), "{absolute}");
    let shown = errors[0].display_from(Path::new("/work/book")).to_string();
    assert!(shown.contains("line 1 of one.md"), "{shown}");
    assert!(shown.starts_with("error: two.md:1:"), "{shown}");
}

#[test]
fn a_bad_theme_value_names_the_theme_and_the_property() {
    let mut resolver = MemoryResolver::new();
    let theme = r#"{"version": 1, "styles": {"body": {"size": "huge"}}}"#;
    resolver.insert(Origin::Theme(book()), "theme.json", theme.as_bytes());
    let selection = ThemeSelection::File {
        path: book().join("theme.json"),
    };

    let errors = errors(&resolver, &input(single("Text.\n"), selection, Overrides::default()));

    assert_eq!(errors[0].source, Some(Source::Theme(book().join("theme.json"))));
    assert_eq!(errors[0].property.as_deref(), Some("styles.body.size"));
    assert_eq!(errors[0].resource, None);
}

#[test]
fn a_missing_theme_image_names_the_resource_with_its_origin() {
    let mut resolver = MemoryResolver::new();
    let theme = r#"{"version": 1, "images": {"logo": "logo.png"}}"#;
    resolver.insert(Origin::Theme(book()), "theme.json", theme.as_bytes());
    let selection = ThemeSelection::File {
        path: book().join("theme.json"),
    };

    let errors = errors(&resolver, &input(single("Text.\n"), selection, Overrides::default()));

    let resource = Resource {
        origin: Origin::Theme(book()),
        path: "logo.png".to_owned(),
    };
    assert_eq!(errors[0].resource.as_ref(), Some(&resource));
    assert_eq!(errors[0].property.as_deref(), Some("images.logo"));
    assert_eq!(
        errors[0].code,
        Some(Code::ResourceNotFound {
            property: "images.logo".to_owned(),
            file: book().join("logo.png"),
        })
    );
    let from = |base: &str| errors[0].display_from(Path::new(base)).to_string();
    let expected = "error: images.logo: resource not found: \"logo.png\" relative to the theme in";
    assert_eq!(from("/work/book"), format!("{expected} ."));
    assert_eq!(from("/work"), format!("{expected} book"));
    assert_eq!(from("/elsewhere"), format!("{expected} /work/book"));
    assert_eq!(errors[0].to_string(), format!("{expected} /work/book"));
}

#[test]
fn a_missing_image_has_a_resource_and_a_place_in_the_document() {
    let resolver = MemoryResolver::new();
    let input = input(single("Text.\n\n![A](gone.png)\n"), no_theme(), Overrides::default());
    let warnings = std::thread::scope(|scope| {
        let thread = std::thread::Builder::new().stack_size(16 * 1024 * 1024);
        let run = || {
            let (cancel, renderer) = (Cancel::default(), Renderer::new());
            let configured = renderer
                .job(&resolver, &input, &cancel)
                .configure()
                .expect("configures");
            let layout = configured
                .layout(&LayoutOptions::default())
                .expect("pages, with a box standing in");
            layout.warnings().to_vec()
        };
        thread
            .spawn_scoped(scope, run)
            .expect("thread")
            .join()
            .expect("no panic")
    });

    assert_eq!(warnings[0].severity, Severity::Warning);
    assert_eq!(warnings[0].source, Some(Source::Document(book().join("doc.md"))));
    assert_eq!(warnings[0].location, Some((3, 1).into()));
    let resource = warnings[0].resource.as_ref().expect("the image");
    assert_eq!(
        (&resource.origin, resource.path.as_str()),
        (&Origin::Document(book()), "gone.png")
    );
}

#[test]
fn a_bad_override_names_the_overrides() {
    let pairs = ["no-such-setting=1".to_owned()];
    let Err(errors) = Overrides::parse(&pairs, &book()) else {
        panic!("the setting does not exist")
    };
    assert_eq!(errors[0].source, Some(Source::Overrides));
}

#[test]
fn warnings_sit_beside_success() {
    let mut resolver = MemoryResolver::new();
    let origin = Origin::Document(book());
    resolver.insert(origin.clone(), "druck.yaml", b"chapters:\n  - one.md\n".as_slice());
    resolver.insert(origin, "one.md", b"Text.\n".as_slice());
    let project = Project::open(&resolver, &book()).expect("the project opens");
    assert!(project.warnings().is_empty());

    let warning = Diagnostic::warning(None, "something to know");
    assert!(warning.is_warning());
    assert_eq!(warning.to_string(), "warning: something to know");
}
