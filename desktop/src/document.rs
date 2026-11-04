//! The document the window shows, read through the engine. Holds no gpui types.

use std::path::{Path, PathBuf};

use druck::cancel::Cancel;
use druck::diagnostic::Diagnostic;
use druck::editor::{engine, resolved_config};
use druck::input::{DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::resource::{FileResolver, clock_date, normalize, source_date};
use druck::{Failure, Renderer};

pub struct Opened {
    pub path: PathBuf,
    pub title: String,
    pub authors: String,
    /// The theme file the document's front matter names.
    pub theme: Option<PathBuf>,
}

/// What a document says about itself.
#[derive(Default)]
struct Identity {
    title: Option<String>,
    authors: Vec<String>,
    theme: Option<PathBuf>,
}

impl Opened {
    /// Reads a document's identity. A document the engine rejects still opens, under its file name, because
    /// the editor is where its problems get fixed. Only a path that does not exist is an error.
    pub fn read(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("cannot open {}: no such file or folder", path.display()));
        }
        let identity = match identity(path) {
            Ok(identity) => identity,
            Err(Failure::Errors(diagnostics)) => {
                print(&diagnostics);
                Identity::default()
            }
            Err(Failure::Cancelled) => Identity::default(),
        };
        let file_name = || path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
        Ok(Self {
            path: path.to_owned(),
            title: identity.title.or_else(file_name).unwrap_or_default(),
            authors: identity.authors.join(", "),
            theme: identity.theme,
        })
    }
}

/// The file's title, authors, and theme, with the warnings of its configuration printed.
fn identity(path: &Path) -> Result<Identity, Failure> {
    let resolver = FileResolver::new();
    let renderer = Renderer::new();
    let input = input(&resolver, path)?;
    let cancel = Cancel::default();
    let configured = renderer.job(&resolver, &input, &cancel).configure()?;
    print(configured.warnings());
    let theme = match &input.text {
        DocumentText::Single { text, origin, name } => origin
            .file(name)
            .and_then(|file| engine::document_theme(&resolver, &file, text, &theme_dirs())),
        DocumentText::Project(_) => None,
    };
    let metadata = &resolved_config::of(&configured).metadata;
    Ok(Identity {
        title: metadata.title.clone(),
        authors: metadata.authors.clone(),
        theme,
    })
}

fn theme_dirs() -> ThemeDirs {
    ThemeDirs {
        project: None,
        shared: FileResolver::shared_themes(),
    }
}

/// Prints diagnostics on standard error, with paths relative to the working directory.
fn print(diagnostics: &[Diagnostic]) {
    let working_dir = std::env::current_dir().unwrap_or_default();
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.display_from(&working_dir));
    }
}

/// The file as an input to the engine.
fn input(resolver: &FileResolver, path: &Path) -> Result<Input, Failure> {
    let error = |message: String| vec![Diagnostic::error(message)];
    let working_dir =
        std::env::current_dir().map_err(|e| error(format!("cannot determine the working directory: {e}")))?;
    let today = match source_date().map_err(error)? {
        Some(day) => day,
        None => clock_date().map_err(error)?,
    };
    Ok(Input {
        text: DocumentText::read(resolver, &normalize(&working_dir.join(path)))?,
        theme: ThemeSelection::Document { dirs: theme_dirs() },
        overrides: Overrides::default(),
        today,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use druck::{LayoutOptions, PdfOptions};

    fn sample(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples").join(name)
    }

    #[test]
    fn engine_goes_from_markdown_to_pages_and_pdf() {
        let resolver = FileResolver::new();
        let renderer = Renderer::new();
        let input = input(&resolver, &sample("report.md")).unwrap_or_else(|_| panic!("the sample reads"));
        let cancel = Cancel::default();
        let configured = renderer
            .job(&resolver, &input, &cancel)
            .configure()
            .unwrap_or_else(|_| panic!("the sample loads"));
        let laid = configured
            .layout(&LayoutOptions::default())
            .unwrap_or_else(|_| panic!("the sample typesets"));
        let pdf = laid
            .pdf(&PdfOptions::default(), &cancel)
            .unwrap_or_else(|_| panic!("the sample writes"));
        assert!(pdf.parts > 0);
        assert!(pdf.bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn identity_comes_from_the_document() {
        let opened = Opened::read(&sample("report.md")).unwrap_or_else(|message| panic!("{message}"));
        assert!(!opened.title.is_empty());
    }

    #[test]
    fn the_theme_comes_from_the_front_matter() {
        let path = Path::new("/project/book.md");
        let (resolver, dirs) = (FileResolver::new(), ThemeDirs::default());
        let theme = engine::document_theme(&resolver, path, "---\ntheme: themes/print.json\n---\n\nText\n", &dirs);
        assert_eq!(theme, Some(PathBuf::from("/project/themes/print.json")));
        assert_eq!(engine::document_theme(&resolver, path, "Text\n", &dirs), None);
    }

    #[test]
    fn a_missing_path_is_an_error() {
        assert!(Opened::read(Path::new("no/such/document.md")).is_err());
    }
}
