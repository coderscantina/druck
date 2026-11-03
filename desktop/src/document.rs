//! The document the window shows, read through the engine. Holds no gpui types.

use std::path::{Path, PathBuf};

use druck::pipeline::{self, Request};

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
        let request = Request {
            input: path,
            theme: None,
            overrides: &[],
        };
        let (metadata, theme) = match pipeline::load(&request) {
            Ok((config, file)) => (Some(config.metadata), pipeline::document_theme(&file.path, &file.text)),
            Err(diagnostics) => {
                for diagnostic in &diagnostics {
                    eprintln!("{diagnostic}");
                }
                (None, None)
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use druck::pdf::Print;

    fn sample(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples").join(name)
    }

    #[test]
    fn engine_goes_from_markdown_to_pages_and_pdf() {
        let path = sample("report.md");
        let request = Request {
            input: &path,
            theme: None,
            overrides: &[],
        };
        let (config, file) = pipeline::load(&request).unwrap_or_else(|_| panic!("the sample loads"));
        let (fonts, laid) = pipeline::typeset(&config, &file).unwrap_or_else(|_| panic!("the sample typesets"));
        assert!(!laid.pages.is_empty());
        let print = Print {
            bleed: druck::config::values::Pt(0.0),
            crop_marks: false,
        };
        let pdf =
            pipeline::write_pdf(&config, &file, &fonts, &laid, print).unwrap_or_else(|_| panic!("the sample writes"));
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn identity_comes_from_the_document() {
        let opened = Opened::read(&sample("report.md")).unwrap_or_else(|message| panic!("{message}"));
        assert!(!opened.title.is_empty());
    }

    #[test]
    fn the_theme_comes_from_the_front_matter() {
        let path = Path::new("/project/book.md");
        let theme = pipeline::document_theme(path, "---\ntheme: themes/print.json\n---\n\nText\n");
        assert_eq!(theme, Some(PathBuf::from("/project/themes/print.json")));
        assert_eq!(pipeline::document_theme(path, "Text\n"), None);
    }

    #[test]
    fn a_missing_path_is_an_error() {
        assert!(Opened::read(Path::new("no/such/document.md")).is_err());
    }
}
