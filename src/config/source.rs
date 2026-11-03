//! Configuration inputs and the origins their resource paths resolve against.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

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

/// Where a project's chapters sit in their joined text, to name the real file and line of a joined line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterMap {
    manifest: PathBuf,
    chapters: Vec<MappedChapter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MappedChapter {
    path: PathBuf,
    /// The joined line this chapter's first line became.
    first_line: u64,
    lines: u64,
}

impl ChapterMap {
    pub fn new(manifest: PathBuf) -> Self {
        Self {
            manifest,
            chapters: Vec::new(),
        }
    }

    /// Records the next chapter, of `lines` lines, starting at joined line `first_line`.
    pub fn push(&mut self, path: PathBuf, first_line: u64, lines: u64) {
        self.chapters.push(MappedChapter {
            path,
            first_line,
            lines,
        });
    }

    /// The chapter file and line behind a joined line. The blank lines that join two chapters, and any line
    /// past the end, count as the last line of the chapter before them.
    fn locate(&self, line: u64) -> Option<(&Path, u64)> {
        let index = self.chapters.partition_point(|chapter| chapter.first_line <= line);
        let chapter = &self.chapters[index.checked_sub(1)?];
        let local = (line - chapter.first_line + 1).min(chapter.lines.max(1));
        Some((&chapter.path, local))
    }
}

/// Where a configuration layer came from. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Bundled,
    Theme(PathBuf),
    Document(PathBuf),
    /// The joined text of a project's chapters. A location in it is reported at its chapter.
    Chapters(Arc<ChapterMap>),
    /// A BibTeX file. It supplies no resources; its origin is its directory, like the document's.
    Bibliography(PathBuf),
    Cli {
        working_dir: PathBuf,
    },
}

impl Source {
    /// The real file and line of a line in this input, when it differs from the input and line given.
    pub fn chapter_line(&self, line: u64) -> Option<(Self, u64)> {
        let Self::Chapters(map) = self else {
            return None;
        };
        let (path, line) = map.locate(line)?;
        Some((Self::Document(path.to_owned()), line))
    }

    /// A line of this input as a message names it, such as `line 3` or `line 3 of manuscript/one.md`.
    pub fn line_name(&self, line: u64) -> String {
        match self.chapter_line(line) {
            Some((chapter, line)) => format!("line {line} of {chapter}"),
            None => format!("line {line}"),
        }
    }

    /// The origin for relative resource paths supplied by this layer.
    pub fn origin(&self) -> Origin {
        let parent = |path: &PathBuf| path.parent().map(PathBuf::from).unwrap_or_default();
        match self {
            Self::Bundled => Origin::Bundled,
            Self::Theme(path) => Origin::Theme(parent(path)),
            Self::Document(path) | Self::Bibliography(path) => Origin::Document(parent(path)),
            Self::Chapters(map) => Origin::Document(parent(&map.manifest)),
            Self::Cli { working_dir } => Origin::WorkingDir(working_dir.clone()),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundled => f.write_str("bundled default theme"),
            Self::Theme(path) | Self::Document(path) | Self::Bibliography(path) => f.write_str(&shown(path)),
            Self::Chapters(map) => f.write_str(&shown(&map.manifest)),
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

impl Origin {
    /// The file `path` means from this origin, or `None` for bundled resources.
    pub fn file(&self, path: &str) -> Option<PathBuf> {
        match self {
            Self::Bundled => None,
            Self::Theme(dir) | Self::Document(dir) | Self::WorkingDir(dir) => Some(normalize(&dir.join(path))),
            Self::Installed => Some(PathBuf::from(path)),
        }
    }
}

impl Resource {
    /// The file to read, or `None` for bundled resources.
    pub fn file(&self) -> Option<PathBuf> {
        self.origin.file(&self.path)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_joined_line_maps_to_its_chapter_and_joining_lines_to_the_chapter_before() {
        let mut map = ChapterMap::new("/book/druck.yaml".into());
        map.push("/book/one.md".into(), 1, 3);
        map.push("/book/empty.md".into(), 5, 0);
        map.push("/book/two.md".into(), 6, 2);
        let source = Source::Chapters(Arc::new(map));
        let at = |line| source.chapter_line(line).expect("mapped");
        let chapter = |path: &str| Source::Document(path.into());
        assert_eq!(at(3), (chapter("/book/one.md"), 3));
        assert_eq!(at(4), (chapter("/book/one.md"), 3));
        assert_eq!(at(5), (chapter("/book/empty.md"), 1));
        assert_eq!(at(6), (chapter("/book/two.md"), 1));
        assert_eq!(at(99), (chapter("/book/two.md"), 2));
        assert_eq!(Source::Document("/d.md".into()).chapter_line(3), None);
    }
}
