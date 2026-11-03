//! What is wrong with a document, in a form a client can present without parsing text.
//!
//! A [`Diagnostic`] has a [`Severity`], a message, and, where they apply, the [`Source`] it concerns, a
//! [`Location`] in it, the dotted property of a theme or setting, and the [`Resource`] that could not be used. These
//! meanings are stable; the words of a message are not. Paths are absolute. A terminal shortens them with
//! [`Diagnostic::display_from`].

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::source::{ChapterMap, InputSource};

pub use crate::config::source::{Origin, Resource};
pub use crate::document::Location;

/// Whether a run can go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The stage that found it produces nothing, and the [`crate::Failure`] holds the diagnostic.
    Error,
    /// Output is produced, and the diagnostic sits beside it.
    Warning,
}

/// The input a diagnostic concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The bundled default theme.
    Bundled,
    /// A theme file.
    Theme(PathBuf),
    /// A Markdown file, which in a project is the chapter holding the location, or a project's manifest.
    Document(PathBuf),
    /// A BibTeX file.
    Bibliography(PathBuf),
    /// Settings the caller passed as [`crate::input::Overrides`].
    Overrides,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// The input responsible, if a single one is.
    pub source: Option<Source>,
    /// Where in `source`.
    pub location: Option<Location>,
    /// Dotted property path, such as `styles.body.size`.
    pub property: Option<String>,
    /// The font, image, or other file concerned, with the origin its path starts from.
    pub resource: Option<Resource>,
    /// The line and column as parsed, for sorting in reading order across a project's chapters. Zero without
    /// a location, which sorts first.
    pub(crate) position: (u32, u32),
    /// A project's chapters, which turn a joined line into a real one and which a message may name.
    chapters: Option<Arc<ChapterMap>>,
    /// A further file the message names by its absolute path.
    named: Option<PathBuf>,
}

impl Diagnostic {
    /// An error about nothing a [`Source`] can name, such as the command line of a program.
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(None, message)
    }

    pub(crate) fn new(source: Option<InputSource>, message: impl Into<String>) -> Self {
        let chapters = match &source {
            Some(InputSource::Chapters(map)) => Some(map.clone()),
            _ => None,
        };
        Self {
            severity: Severity::Error,
            message: message.into(),
            source: source.map(Source::from),
            location: None,
            property: None,
            resource: None,
            position: (0, 0),
            chapters,
            named: None,
        }
    }

    pub(crate) fn warning(source: Option<InputSource>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::new(source, message)
        }
    }

    /// Places the diagnostic at a line and column of its source. A line of a project's joined chapters
    /// becomes the file and line it came from, so every diagnostic made this way names the real chapter.
    pub(crate) fn at(mut self, line: u64, column: u64) -> Self {
        self.position = (line as u32, column as u32);
        self.location = Some(Location { line, column });
        if let Some((chapter, real)) = self.chapters.as_ref().and_then(|map| map.locate(line)) {
            self.source = Some(Source::Document(chapter.to_owned()));
            self.location = Some(Location { line: real, column });
        }
        self
    }

    pub(crate) fn property(mut self, property: Option<String>) -> Self {
        self.property = property;
        self
    }

    pub(crate) fn resource(mut self, resource: &Resource) -> Self {
        self.resource = Some(resource.clone());
        self
    }

    /// Records a file the message names, so that a terminal can shorten it.
    pub(crate) fn naming(mut self, path: &Path) -> Self {
        self.named = Some(path.to_owned());
        self
    }

    pub fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }

    /// For terminals: `error: file:line:column: property: message`, with paths under `base` shown relative to it
    /// and others absolute. Displaying the diagnostic itself shows every path absolute.
    pub fn display_from<'a>(&'a self, base: &'a Path) -> impl fmt::Display + 'a {
        Shown {
            diagnostic: self,
            base: Some(base),
        }
    }

    /// The message with the paths it names shortened.
    fn message_from(&self, base: &Path) -> String {
        let mut message = self.message.clone();
        if let Some(resource) = &self.resource {
            message = message.replace(&resource.to_string(), &resource.display_from(base));
        }
        let chapters = self.chapters.iter().flat_map(|map| map.paths());
        for path in chapters.chain(&self.named) {
            message = message.replace(&path.display().to_string(), &path_from(path, base));
        }
        message
    }
}

/// A path for a terminal: relative to `base` when inside it, else as it is.
pub fn path_from(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

impl Source {
    fn name(&self, base: Option<&Path>) -> String {
        let path = |path: &Path| base.map_or_else(|| path.display().to_string(), |base| path_from(path, base));
        match self {
            Self::Bundled => "bundled default theme".to_owned(),
            Self::Theme(file) | Self::Document(file) | Self::Bibliography(file) => path(file),
            Self::Overrides => "command-line override".to_owned(),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name(None))
    }
}

impl From<InputSource> for Source {
    fn from(source: InputSource) -> Self {
        match source {
            InputSource::Bundled => Self::Bundled,
            InputSource::Theme(path) => Self::Theme(path),
            InputSource::Document(path) => Self::Document(path),
            InputSource::Chapters(map) => Self::Document(map.manifest().to_owned()),
            InputSource::Bibliography(path) => Self::Bibliography(path),
            InputSource::Overrides { .. } => Self::Overrides,
        }
    }
}

struct Shown<'a> {
    diagnostic: &'a Diagnostic,
    base: Option<&'a Path>,
}

impl fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let diagnostic = self.diagnostic;
        f.write_str(if diagnostic.is_warning() {
            "warning: "
        } else {
            "error: "
        })?;
        if let Some(source) = &diagnostic.source {
            f.write_str(&source.name(self.base))?;
            if let Some(Location { line, column }) = diagnostic.location {
                write!(f, ":{line}:{column}")?;
            }
            f.write_str(": ")?;
        }
        if let Some(property) = &diagnostic.property {
            write!(f, "{property}: ")?;
        }
        match self.base {
            Some(base) => f.write_str(&diagnostic.message_from(base)),
            None => f.write_str(&diagnostic.message),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Shown {
            diagnostic: self,
            base: None,
        }
        .fmt(f)
    }
}

#[cfg(test)]
impl From<(u64, u64)> for Location {
    fn from((line, column): (u64, u64)) -> Self {
        Self { line, column }
    }
}
