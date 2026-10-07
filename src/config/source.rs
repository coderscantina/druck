//! Configuration inputs and the origins their resource paths resolve against.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;

static DISPLAY_BASE: OnceLock<PathBuf> = OnceLock::new();

/// Shows paths inside `dir` relative to it from now on, as the CLI does for the working directory.
pub fn show_relative_to(dir: PathBuf) {
    DISPLAY_BASE.set(dir).ok();
}

/// Resolves `.` and `..` components without touching the filesystem, so symlinks stay as written.
pub fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(normalized.components().next_back(), Some(Component::Normal(_))) => {
                normalized.pop();
            }
            Component::ParentDir if normalized.has_root() => {}
            other => normalized.push(other),
        }
    }
    normalized
}

/// A path for messages: relative to the directory given to [`show_relative_to`] when inside it.
pub fn shown(path: &Path) -> String {
    let inside = DISPLAY_BASE.get().and_then(|base| path.strip_prefix(base).ok());
    match inside {
        Some(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Some(relative) => relative.display().to_string(),
        None => path.display().to_string(),
    }
}

/// Where a configuration layer came from. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Bundled,
    Theme(PathBuf),
    Document(PathBuf),
    /// A BibTeX file. It supplies no resources; its origin is its directory, like the document's.
    Bibliography(PathBuf),
    Cli {
        working_dir: PathBuf,
    },
}

impl Source {
    /// The origin for relative resource paths supplied by this layer.
    pub fn origin(&self) -> Origin {
        let parent = |path: &PathBuf| path.parent().map(PathBuf::from).unwrap_or_default();
        match self {
            Self::Bundled => Origin::Bundled,
            Self::Theme(path) => Origin::Theme(parent(path)),
            Self::Document(path) | Self::Bibliography(path) => Origin::Document(parent(path)),
            Self::Cli { working_dir } => Origin::WorkingDir(working_dir.clone()),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundled => f.write_str("bundled default theme"),
            Self::Theme(path) | Self::Document(path) | Self::Bibliography(path) => f.write_str(&shown(path)),
            Self::Cli { .. } => f.write_str("command-line override"),
        }
    }
}

/// The base a resource path is relative to. Directories are absolute.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case", tag = "origin", content = "dir")]
pub enum Origin {
    Bundled,
    Theme(PathBuf),
    Document(PathBuf),
    WorkingDir(PathBuf),
    /// An installed font, whose path is absolute.
    Installed,
}

/// A resource reference with the origin of the layer that supplied it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Resource {
    #[serde(flatten)]
    pub origin: Origin,
    pub path: String,
}

impl Resource {
    /// The file to read, or `None` for bundled resources.
    pub fn file(&self) -> Option<PathBuf> {
        match &self.origin {
            Origin::Bundled => None,
            Origin::Theme(dir) | Origin::Document(dir) | Origin::WorkingDir(dir) => {
                Some(normalize(&dir.join(&self.path)))
            }
            Origin::Installed => Some(PathBuf::from(&self.path)),
        }
    }
}

impl fmt::Display for Resource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.origin {
            Origin::Bundled => write!(f, "bundled resource \"{}\"", self.path),
            Origin::Theme(dir) => write!(f, "\"{}\" relative to the theme in {}", self.path, shown(dir)),
            Origin::Document(dir) => write!(f, "\"{}\" relative to the document in {}", self.path, shown(dir)),
            Origin::WorkingDir(dir) => write!(f, "\"{}\" relative to the working directory {}", self.path, shown(dir)),
            Origin::Installed => write!(f, "installed font \"{}\"", self.path),
        }
    }
}
