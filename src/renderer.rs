//! The entry points: a renderer, and the stages of one job.

use std::path::Path;
use std::sync::Arc;

use crate::config::resolved::Config;
use crate::diagnostic::Diagnostic;
use crate::input::{DocumentText, Input};
use crate::output::{Layout, Pt};
use crate::pipeline::{self, DocumentFile};
use crate::{date, epub, project};

/// Runs jobs. Keeps nothing between them yet, so it costs nothing to make.
#[derive(Default)]
pub struct Renderer(());

impl Renderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// The simple path: Markdown and configuration in, PDF bytes and every warning out.
    pub fn render(&self, input: &Input, options: &PdfOptions) -> Result<Rendered, Failure> {
        let configured = self.job(input).configure()?;
        let layout = configured.layout(&LayoutOptions::default())?;
        let mut rendered = layout.pdf(options)?;
        let mut warnings = configured.warnings().to_vec();
        warnings.extend_from_slice(layout.warnings());
        warnings.append(&mut rendered.warnings);
        rendered.warnings = warnings;
        Ok(rendered)
    }

    /// The explicit path, one stage at a time. A job is one document on one thread.
    pub fn job<'r>(&'r self, input: &'r Input) -> Job<'r> {
        Job { input }
    }
}

/// One document on its way to being configured.
pub struct Job<'r> {
    input: &'r Input,
}

impl Job<'_> {
    /// Resolves and checks the configuration. Errors here never reach layout.
    pub fn configure(self) -> Result<Configured, Failure> {
        let input = self.input;
        let (config, document) = match &input.text {
            DocumentText::Single { path, text } => {
                pipeline::configure(&input.request(path), &input.working_dir, text.clone())?
            }
            DocumentText::Project(project) => project.configure(&input.request(Path::new("")), &input.working_dir)?,
        };
        Ok(Configured {
            config: Arc::new(config),
            document: Arc::new(document),
        })
    }
}

/// A document with its configuration resolved.
pub struct Configured {
    pub(crate) config: Arc<Config>,
    pub(crate) document: Arc<DocumentFile>,
}

impl Configured {
    /// What is wrong with the configuration without stopping the job.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.config.warnings
    }

    /// The resolved configuration as pretty JSON, in schema order.
    pub fn config_json(&self) -> String {
        serde_json::to_string_pretty(&*self.config).expect("configuration serializes")
    }

    /// Fails with one error per open placeholder, at its real location, naming the final `export`.
    pub fn check_final(&self, export: &str) -> Result<(), Failure> {
        Ok(project::check_final(&self.config, &self.document, export)?)
    }

    /// Lays out the document, reporting everything a render reports before writing.
    pub fn layout(&self, _options: &LayoutOptions) -> Result<Layout, Failure> {
        let (fonts, laid) = pipeline::typeset(&self.config, &self.document)?;
        Ok(Layout {
            config: self.config.clone(),
            document: self.document.clone(),
            fonts,
            laid,
        })
    }

    /// Writes an EPUB, which needs no layout. `parts` is the number of content documents.
    pub fn epub(&self, _options: &EpubOptions) -> Result<Rendered, Failure> {
        let (config, document) = (&*self.config, &*self.document);
        let prepared = pipeline::prepare(config, document)?;
        let today = date::Date::today().map_err(|e| vec![Diagnostic::new(None, e)])?;
        // `SOURCE_DATE_EPOCH` fixes the build day; without it an ISO `date` does, so the same input gives the
        // same bytes.
        let iso = config
            .metadata
            .date
            .as_deref()
            .and_then(|text| date::Date::iso(text.trim()));
        let modified = match std::env::var_os("SOURCE_DATE_EPOCH") {
            Some(_) => today,
            None => iso.unwrap_or(today),
        };
        let written = epub::write(&epub::Book {
            document: &prepared.content,
            cited: &prepared.cited,
            config,
            source: &document.source,
            images: &prepared.images,
            dir: document.path.parent().expect("an absolute file path has a parent"),
            today,
            modified,
            fallback_title: document.name().unwrap_or("Untitled"),
        })?;
        Ok(Rendered {
            bytes: written.bytes,
            warnings: written
                .warnings
                .into_iter()
                .map(|warning| Diagnostic::warning(None, warning))
                .collect(),
            parts: written.documents,
        })
    }
}

/// Finished output.
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub warnings: Vec<Diagnostic>,
    /// The pages of a PDF, or the content documents of an EPUB.
    pub parts: usize,
}

#[derive(Default)]
pub struct LayoutOptions {}

/// Export settings. They never change the layout.
#[derive(Clone, Copy)]
pub struct PdfOptions {
    /// How far the bleed box reaches past the trim box on each side.
    pub bleed: Pt,
    /// Whether crop marks show the trim box at the corners, outside the bleed.
    pub crop_marks: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            bleed: Pt(0.0),
            crop_marks: false,
        }
    }
}

#[derive(Default)]
pub struct EpubOptions {}

/// Why a job produced no output.
#[derive(Debug)]
pub enum Failure {
    Cancelled,
    /// Every error of the stage that failed, in reading order.
    Errors(Vec<Diagnostic>),
}

impl From<Vec<Diagnostic>> for Failure {
    fn from(errors: Vec<Diagnostic>) -> Self {
        Self::Errors(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> Input {
        Input {
            text: DocumentText::Single {
                path: Path::new("/nowhere/document.md").to_owned(),
                text: text.to_owned(),
            },
            theme: None,
            overrides: Vec::new(),
            working_dir: Path::new("/nowhere").to_owned(),
        }
    }

    fn pages(rendered: Result<Rendered, Failure>) -> Rendered {
        rendered.unwrap_or_else(|failure| panic!("{failure:?}"))
    }

    #[test]
    fn the_simple_and_the_explicit_path_write_the_same_pdf() {
        let input = input("---\ntitle: Same\ndate: 2026-01-02\n---\n\nSome text.\n");
        let renderer = Renderer::new();
        let simple = pages(renderer.render(&input, &PdfOptions::default()));
        let configured = renderer.job(&input).configure().unwrap_or_else(|f| panic!("{f:?}"));
        let layout = configured
            .layout(&LayoutOptions::default())
            .unwrap_or_else(|f| panic!("{f:?}"));
        let explicit = pages(layout.pdf(&PdfOptions::default()));
        assert!(simple.bytes.starts_with(b"%PDF"));
        assert_eq!(simple.bytes, explicit.bytes);
        assert_eq!(simple.parts, 1);
    }

    #[test]
    fn errors_in_the_configuration_are_structured() {
        let input = input("---\ntitle: [unclosed\n---\n\nText.\n");
        let Err(Failure::Errors(errors)) = Renderer::new().job(&input).configure() else {
            panic!("the front matter is invalid")
        };
        assert!(!errors.is_empty());
    }
}
