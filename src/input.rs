//! What to typeset.

use std::path::{Path, PathBuf};

use crate::config::source::Source;
use crate::diagnostic::Diagnostic;
use crate::pipeline::{self, Request};
use crate::renderer::PdfOptions;

pub use crate::config::front_matter::Export;
pub use crate::project::{MANIFEST, Project};

/// A document, its theme, and setting overrides.
pub struct Input {
    pub text: DocumentText,
    /// Theme name or JSON file, overriding the document's `theme` setting.
    pub theme: Option<PathBuf>,
    /// `KEY=VALUE` pairs in `druck render --set` syntax.
    pub overrides: Vec<String>,
    /// What a relative theme path and paths in the overrides start from.
    pub working_dir: PathBuf,
}

impl Input {
    pub(crate) fn request<'a>(&'a self, path: &'a Path) -> Request<'a> {
        Request {
            input: path,
            theme: self.theme.as_deref(),
            overrides: &self.overrides,
        }
    }
}

pub enum DocumentText {
    /// One Markdown file with front matter. Its relative paths start from the folder of `path`, which is
    /// absolute. `text` may be newer than the file.
    Single { path: PathBuf, text: String },
    /// A project: joined chapters without front matter, settings from the manifest.
    Project(Box<Project>),
}

impl DocumentText {
    /// Reads the Markdown file at `path`, which is absolute.
    pub fn read(path: &Path) -> Result<Self, Vec<Diagnostic>> {
        let text = pipeline::read(path, &Source::Document(path.to_owned()))?;
        Ok(Self::Single {
            path: path.to_owned(),
            text,
        })
    }
}

/// What a project's export decides besides the document.
pub struct ExportSettings {
    pub pdf: PdfOptions,
    /// Whether open placeholders fail the build, see `Configured::check_final`.
    pub is_final: bool,
    /// Where the export writes, absolute. `None` leaves the choice to the caller.
    pub output: Option<PathBuf>,
    /// Why the export's print settings do not suit an EPUB, if it has any.
    pub print_conflict: Option<Diagnostic>,
}
