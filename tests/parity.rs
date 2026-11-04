//! Parity: for the same Markdown, theme, settings, and files, `Renderer::render`, configure then layout then
//! `Layout::pdf`, and the CLI binary write the same bytes, which is stricter than the same layout. The samples
//! and their themes run everywhere. A book runs where `DRUCK_BOOK` names a project folder (a `druck.yaml` with
//! chapters and a `print` export) and says so when it does not.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use druck::cancel::Cancel;
use druck::input::{Date, DocumentText, Input, Overrides, Project, ThemeDirs, ThemeSelection};
use druck::resource::FileResolver;
use druck::{EpubOptions, LayoutOptions, PdfOptions, Renderer};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");
/// Layout in a debug build needs more stack than a test thread has by default.
const STACK: usize = 16 * 1024 * 1024;

/// One document as the three paths read it.
struct Case {
    name: String,
    /// A Markdown file, or a project folder.
    path: PathBuf,
    theme: Option<PathBuf>,
    /// The export of a project folder.
    export: Option<&'static str>,
    epub: bool,
    /// Whether themes may come from the user's folder. Samples use none, so the machine decides nothing.
    shared_themes: bool,
}

fn cases() -> Vec<Case> {
    let mut documents: Vec<PathBuf> = fs::read_dir(SAMPLES)
        .expect("samples folder")
        .map(|entry| entry.expect("sample entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    documents.sort();
    let mut cases = Vec::new();
    for path in documents {
        let name = path.file_stem().expect("file name").to_string_lossy().into_owned();
        let theme = Path::new(SAMPLES).join("themes").join(format!("{name}.json"));
        if theme.is_file() {
            cases.push(Case {
                name: format!("{name} with its theme"),
                path: path.clone(),
                theme: Some(theme),
                export: None,
                epub: true,
                shared_themes: false,
            });
        }
        cases.push(Case {
            name,
            path,
            theme: None,
            export: None,
            epub: true,
            shared_themes: false,
        });
    }
    let project = Path::new(SAMPLES).join("project");
    for (name, export, epub) in [("project", None, true), ("project print export", Some("print"), false)] {
        cases.push(Case {
            name: name.to_owned(),
            path: project.clone(),
            theme: None,
            export,
            epub,
            shared_themes: false,
        });
    }
    match std::env::var_os("DRUCK_BOOK").map(PathBuf::from) {
        Some(book) if book.join("druck.yaml").is_file() => cases.push(Case {
            name: "the book, print export".to_owned(),
            path: book,
            theme: None,
            export: Some("print"),
            epub: false,
            shared_themes: true,
        }),
        _ => println!("skipping the book: DRUCK_BOOK does not name a project folder with a druck.yaml"),
    }
    cases
}

/// What the three paths wrote for one format.
struct Written {
    /// What `Renderer::render` or the first job wrote.
    api: Vec<u8>,
    /// What the second route through the API wrote: the stages one by one for a PDF, a second job for an EPUB.
    second: Vec<u8>,
    second_route: &'static str,
    cli: Vec<u8>,
}

/// The day of every run: the CLI gets it as `SOURCE_DATE_EPOCH`, the API as `Input::today`.
fn today() -> Date {
    Date::from_unix(0)
}

fn input(case: &Case, resolver: &FileResolver) -> (Input, PdfOptions) {
    let dirs = ThemeDirs {
        project: case.path.is_dir().then(|| case.path.clone()),
        shared: case.shared_themes.then(FileResolver::shared_themes).flatten(),
    };
    let theme = match &case.theme {
        Some(path) => ThemeSelection::File { path: path.clone() },
        None => ThemeSelection::Document { dirs },
    };
    let (text, options) = if case.path.is_dir() {
        let project = Project::open(resolver, &case.path).expect("a project");
        let (text, settings) = project.export(case.export).expect("an export");
        (text, settings.pdf)
    } else {
        (
            DocumentText::read(resolver, &case.path).expect("a sample"),
            PdfOptions::default(),
        )
    };
    let input = Input {
        text,
        theme,
        overrides: Overrides::default(),
        today: today(),
    };
    (input, options)
}

fn cli(case: &Case, cwd: &Path, output: &Path) -> Vec<u8> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_druck"));
    command.arg("render").arg(&case.path).arg("-o").arg(output);
    if let Some(theme) = &case.theme {
        command.arg("--theme").arg(theme);
    }
    if let Some(export) = case.export {
        command.args(["--export", export]);
    }
    command.env("SOURCE_DATE_EPOCH", "0").current_dir(cwd);
    if !case.shared_themes {
        command.env("HOME", cwd).env("XDG_CONFIG_HOME", cwd).env("APPDATA", cwd);
    }
    let run = command.output().expect("run druck");
    assert!(
        run.status.success(),
        "{}: {}",
        case.name,
        String::from_utf8_lossy(&run.stderr)
    );
    fs::read(output).expect("the output")
}

fn pdfs(case: &Case, cwd: &Path) -> Written {
    let (renderer, resolver, cancel) = (Renderer::new(), FileResolver::new(), Cancel::default());
    let (input, options) = input(case, &resolver);
    let api = (renderer.render(&resolver, &input, &options, &cancel)).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    let configured = (renderer.job(&resolver, &input, &cancel).configure()).unwrap_or_else(|e| panic!("{e}"));
    let laid = configured
        .layout(&LayoutOptions::default())
        .unwrap_or_else(|e| panic!("{e}"));
    let in_steps = laid.pdf(&options, &cancel).unwrap_or_else(|e| panic!("{e}"));
    // A map is for inspection only: it must not change what the pages say.
    let mapped = (configured.layout(&LayoutOptions { source_map: true })).unwrap_or_else(|e| panic!("{e}"));
    let mapped = mapped.pdf(&options, &cancel).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        mapped.bytes == in_steps.bytes,
        "{}: a source map changes the PDF",
        case.name
    );
    Written {
        api: api.bytes,
        second: in_steps.bytes,
        second_route: "configure, layout, pdf",
        cli: cli(case, cwd, &cwd.join("out.pdf")),
    }
}

fn epubs(case: &Case, cwd: &Path) -> Written {
    let (renderer, resolver, cancel) = (Renderer::new(), FileResolver::new(), Cancel::default());
    let (input, _) = input(case, &resolver);
    let options = EpubOptions {
        modified: Some(today()),
    };
    let configured = (renderer.job(&resolver, &input, &cancel).configure()).unwrap_or_else(|e| panic!("{e}"));
    let first = configured.epub(&options).unwrap_or_else(|e| panic!("{e}")).bytes;
    let second = (renderer.job(&resolver, &input, &cancel).configure()).unwrap_or_else(|e| panic!("{e}"));
    let second = second.epub(&options).unwrap_or_else(|e| panic!("{e}")).bytes;
    Written {
        api: first,
        second,
        second_route: "a second job",
        cli: cli(case, cwd, &cwd.join("out.epub")),
    }
}

fn compare(case: &Case, format: &str, written: &Written) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, bytes) in [(written.second_route, &written.second), ("the CLI", &written.cli)] {
        if *bytes != written.api {
            let at = (bytes.iter().zip(&written.api)).position(|(a, b)| a != b);
            problems.push(format!(
                "{} ({format}): {path} differs from the first API route: {} against {} bytes, first difference at {at:?}",
                case.name,
                bytes.len(),
                written.api.len()
            ));
        }
    }
    problems
}

fn scratch() -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "druck-parity-{}-{}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

#[test]
fn the_three_paths_write_the_same_bytes_for_every_sample_and_the_book() {
    let problems: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = cases()
            .into_iter()
            .map(|case| {
                std::thread::Builder::new()
                    .stack_size(STACK)
                    .spawn_scoped(scope, move || {
                        let cwd = scratch();
                        let mut problems = compare(&case, "PDF", &pdfs(&case, &cwd));
                        if case.epub {
                            problems.extend(compare(&case, "EPUB", &epubs(&case, &cwd)));
                        }
                        let _ = fs::remove_dir_all(&cwd);
                        problems
                    })
                    .expect("thread")
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("no case panics"))
            .collect()
    });
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
