//! The document the window shows, read through the engine. Holds no gpui types.

use std::path::{Path, PathBuf};

use druck::diagnostic::{Diagnostic, show_relative_to};
use druck::editor::{self, engine, resolved_config};
use druck::input::{DocumentText, Input};
use druck::resource::normalize;
use druck::{Configured, Failure, Renderer};

pub struct Opened {
    pub path: PathBuf,
    pub title: String,
    pub authors: String,
    /// The theme file the document's front matter names.
    pub theme: Option<PathBuf>,
}

impl Opened {
    /// Reads a document's identity. A document the engine rejects still opens, under its file name, because
    /// the editor is where its problems get fixed. Only a path that does not exist is an error.
    pub fn read(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("cannot open {}: no such file or folder", path.display()));
        }
        let (metadata, theme) = match configure(path) {
            Ok(configured) => {
                let (file, text) = editor::document_text(&configured);
                let theme = engine::document_theme(file, text);
                (Some(resolved_config::of(&configured).metadata.clone()), theme)
            }
            Err(Failure::Errors(diagnostics)) => {
                for diagnostic in &diagnostics {
                    eprintln!("{diagnostic}");
                }
                (None, None)
            }
            Err(Failure::Cancelled) => (None, None),
        };
        let file_name = || path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
        Ok(Self {
            path: path.to_owned(),
            title: metadata
                .as_ref()
                .and_then(|metadata| metadata.title.clone())
                .or_else(file_name)
                .unwrap_or_default(),
            authors: metadata.map(|metadata| metadata.authors.join(", ")).unwrap_or_default(),
            theme,
        })
    }
}

/// The file's configuration, with its warnings printed. Paths in messages become relative to the working
/// directory.
fn configure(path: &Path) -> Result<Configured, Failure> {
    let working_dir = std::env::current_dir().map_err(|e| {
        vec![Diagnostic::new(
            None,
            format!("cannot determine the working directory: {e}"),
        )]
    })?;
    show_relative_to(working_dir.clone());
    let input = Input {
        text: DocumentText::read(&normalize(&working_dir.join(path)))?,
        theme: None,
        overrides: Vec::new(),
        working_dir,
    };
    let configured = Renderer::new().job(&input).configure()?;
    for warning in configured.warnings() {
        eprintln!("{warning}");
    }
    Ok(configured)
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
        let configured = configure(&sample("report.md")).unwrap_or_else(|_| panic!("the sample loads"));
        let laid = configured
            .layout(&LayoutOptions::default())
            .unwrap_or_else(|_| panic!("the sample typesets"));
        let pdf = laid
            .pdf(&PdfOptions::default())
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
        let theme = engine::document_theme(path, "---\ntheme: themes/print.json\n---\n\nText\n");
        assert_eq!(theme, Some(PathBuf::from("/project/themes/print.json")));
        assert_eq!(engine::document_theme(path, "Text\n"), None);
    }

    #[test]
    fn a_missing_path_is_an_error() {
        assert!(Opened::read(Path::new("no/such/document.md")).is_err());
    }
}
