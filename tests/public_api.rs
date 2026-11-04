//! Behaviors of the public API that no other test reaches without the CLI: distinct document and theme origins
//! on disk, the layers of configuration, and the supported theme schema versions. The rest of the contract's
//! acceptance list is covered by the unit tests named in `docs/ACCEPTANCE.md`.

use std::fs;
use std::path::PathBuf;

use druck::cancel::Cancel;
use druck::diagnostic::Source;
use druck::input::{Date, DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::output::Item;
use druck::resource::{FileResolver, MemoryResolver, Origin};
use druck::{Failure, LayoutOptions, Renderer, THEME_SCHEMA_VERSIONS};

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

fn svg(color: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="{color}"/></svg>"#
    )
}

/// A folder of its own below the temporary folder.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("druck-api-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temporary folder");
    dir
}

fn input(text: &str, dir: PathBuf, theme: ThemeSelection, overrides: Overrides) -> Input {
    Input {
        text: DocumentText::Single {
            text: text.to_owned(),
            origin: Origin::Document(dir),
            name: "doc.md".to_owned(),
        },
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

/// The resolved configuration of `input`, as JSON.
fn config(input: &Input) -> serde_json::Value {
    on_big_stack(|| {
        let (renderer, resolver, cancel) = (Renderer::new(), MemoryResolver::new(), Cancel::default());
        let configured = (renderer.job(&resolver, input, &cancel).configure()).unwrap_or_else(|e| panic!("{e}"));
        serde_json::from_str(&configured.config_json()).expect("the configuration is JSON")
    })
}

#[test]
fn a_theme_and_a_document_read_the_same_name_from_their_own_folders() {
    let (theme_dir, document_dir) = (folder("theme"), folder("document"));
    let report = fs::read_to_string(format!("{SAMPLES}/themes/report.json")).expect("sample theme");
    let mut theme: serde_json::Value = serde_json::from_str(&report).expect("JSON");
    theme["images"]["logo"] = "mark.svg".into();
    fs::write(theme_dir.join("theme.json"), theme.to_string()).expect("theme");
    fs::write(theme_dir.join("mark.svg"), svg("#00aa00")).expect("theme image");
    fs::write(document_dir.join("mark.svg"), svg("#aa0000")).expect("document image");

    let text = "---\ntitle: Origins\nauthor: Ada Example\ntitle-page: true\n---\n\n![A square](mark.svg)\n";
    let selection = ThemeSelection::File {
        path: theme_dir.join("theme.json"),
    };
    let input = input(text, document_dir.clone(), selection, Overrides::default());
    let layout = on_big_stack(|| {
        let (renderer, resolver, cancel) = (Renderer::new(), FileResolver::new(), Cancel::default());
        let configured = (renderer.job(&resolver, &input, &cancel).configure()).unwrap_or_else(|e| panic!("{e}"));
        configured
            .layout(&LayoutOptions::default())
            .unwrap_or_else(|e| panic!("{e}"))
    });

    let sources: Vec<&[u8]> = (layout.pages().iter())
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Image { picture, .. } => Some(&**picture.source()),
            _ => None,
        })
        .collect();
    let (from_theme, from_document) = (svg("#00aa00"), svg("#aa0000"));
    assert!(
        sources.contains(&from_theme.as_bytes()),
        "the title page shows the theme's mark"
    );
    assert!(
        sources.contains(&from_document.as_bytes()),
        "the figure shows the document's mark"
    );
    for dir in [theme_dir, document_dir] {
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn explicit_overrides_beat_the_document_the_theme_and_the_defaults_in_that_order() {
    let origin = Origin::Document("/memory".into());
    let theme = |size: &str| ThemeSelection::Json {
        text: format!(r#"{{"version": 1, "styles": {{"body": {{"size": "{size}"}}}}}}"#),
        origin: origin.clone(),
    };
    let size = |input: &Input| config(input)["styles"]["body"]["size"].as_f64().expect("a size");
    let dir = PathBuf::from("/memory");
    let working = dir.as_path();

    let defaults = size(&input("Text.\n", dir.clone(), no_theme(), Overrides::default()));
    let themed = size(&input("Text.\n", dir.clone(), theme("10pt"), Overrides::default()));
    let written = size(&input(
        "---\nfont-size: 11pt\n---\n\nText.\n",
        dir.clone(),
        theme("10pt"),
        Overrides::default(),
    ));
    let parsed = Overrides::parse(&["font-size=12pt".to_owned()], working).map_err(Failure::from);
    let overridden = size(&input(
        "---\nfont-size: 11pt\n---\n\nText.\n",
        dir.clone(),
        theme("10pt"),
        parsed.expect("a valid override"),
    ));
    let from_json = Overrides::from_json(serde_json::json!({ "font-size": "13pt" }), working).map_err(Failure::from);
    let overridden_by_json = size(&input(
        "Text.\n",
        dir,
        theme("10pt"),
        from_json.expect("a valid override"),
    ));

    assert_ne!(defaults, 10.0, "the theme changes the default");
    assert_eq!(
        (themed, written, overridden, overridden_by_json),
        (10.0, 11.0, 12.0, 13.0)
    );
}

#[test]
fn only_the_listed_theme_schema_versions_are_read_and_others_fail_at_the_version() {
    let origin = Origin::Document("/memory".into());
    let with_version = |version: u64| {
        input(
            "Text.\n",
            PathBuf::from("/memory"),
            ThemeSelection::Json {
                text: format!(r#"{{"version": {version}}}"#),
                origin: origin.clone(),
            },
            Overrides::default(),
        )
    };
    for version in THEME_SCHEMA_VERSIONS {
        config(&with_version(*version));
    }
    let unsupported = THEME_SCHEMA_VERSIONS.iter().max().expect("a version") + 1;
    let input = with_version(unsupported);
    let (renderer, resolver, cancel) = (Renderer::new(), MemoryResolver::new(), Cancel::default());
    let Err(failure) = renderer.job(&resolver, &input, &cancel).configure() else {
        panic!("version {unsupported} is not supported");
    };
    let Failure::Errors(errors) = &failure else {
        panic!("nothing cancels this job");
    };
    assert_eq!(errors[0].property.as_deref(), Some("version"));
    assert!(matches!(errors[0].source, Some(Source::Theme(_))));
    assert!(
        failure
            .to_string()
            .contains(&format!("unsupported theme version {unsupported}"))
    );
}
