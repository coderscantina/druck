//! Configuration inputs and the origins their resource paths resolve against.

use std::fmt;
use std::path::PathBuf;

use serde::Serialize;

/// Where a configuration layer came from. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Bundled,
    Theme(PathBuf),
    FrontMatter(PathBuf),
    Cli { working_dir: PathBuf },
}

impl Source {
    /// The origin for relative resource paths supplied by this layer.
    pub fn origin(&self) -> Origin {
        let parent = |path: &PathBuf| path.parent().map(PathBuf::from).unwrap_or_default();
        match self {
            Self::Bundled => Origin::Bundled,
            Self::Theme(path) => Origin::Theme(parent(path)),
            Self::FrontMatter(path) => Origin::Document(parent(path)),
            Self::Cli { working_dir } => Origin::WorkingDir(working_dir.clone()),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundled => f.write_str("bundled default theme"),
            Self::Theme(path) => write!(f, "{}", path.display()),
            Self::FrontMatter(path) => write!(f, "{}", path.display()),
            Self::Cli { .. } => f.write_str("command-line override"),
        }
    }
}

/// The base a resource path is relative to. Directories are absolute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "origin", content = "dir")]
pub enum Origin {
    Bundled,
    Theme(PathBuf),
    Document(PathBuf),
    WorkingDir(PathBuf),
}

/// A resource reference with the origin of the layer that supplied it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
            Origin::Theme(dir) | Origin::Document(dir) | Origin::WorkingDir(dir) => Some(dir.join(&self.path)),
        }
    }
}

impl fmt::Display for Resource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.origin {
            Origin::Bundled => write!(f, "bundled resource \"{}\"", self.path),
            Origin::Theme(dir) => write!(f, "\"{}\" relative to the theme in {}", self.path, dir.display()),
            Origin::Document(dir) => write!(f, "\"{}\" relative to the document in {}", self.path, dir.display()),
            Origin::WorkingDir(dir) => write!(
                f,
                "\"{}\" relative to the working directory {}",
                self.path,
                dir.display()
            ),
        }
    }
}
