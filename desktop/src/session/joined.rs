//! The whole document as one text, as the engine reads it, with the map between chapter and joined offsets.

use std::ops::Range;
use std::path::{Path, PathBuf};

use druck::resource::{Bytes, FileResolver, InstalledFace, Origin, ResolveError, Resolver, normalize};

use super::ChapterId;

/// One chapter's place in the joined text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    id: ChapterId,
    path: PathBuf,
    range: Range<usize>,
}

/// Maps offsets in a chapter's text to offsets in the joined text. The blank lines that join two chapters belong to
/// the chapter before them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChapterMap {
    places: Vec<Place>,
}

impl ChapterMap {
    pub(super) fn push(&mut self, id: ChapterId, path: PathBuf, range: Range<usize>) {
        self.places.push(Place { id, path, range });
    }

    /// The joined offset of `offset` in chapter `id`.
    pub fn to_joined(&self, id: ChapterId, offset: usize) -> Option<usize> {
        let place = self.places.iter().find(|place| place.id == id)?;
        Some(place.range.start + offset.min(place.range.len()))
    }

    /// The chapter and the offset in it of a joined offset.
    pub fn chapter_at(&self, joined: usize) -> Option<(ChapterId, usize)> {
        let index = self.places.partition_point(|place| place.range.start <= joined);
        let place = &self.places[index.checked_sub(1)?];
        Some((place.id, (joined - place.range.start).min(place.range.len())))
    }

    /// The part of the joined text that is the chapter file at `path`.
    pub(crate) fn range_of(&self, path: &Path) -> Option<Range<usize>> {
        let place = self.places.iter().find(|place| place.path == path)?;
        Some(place.range.clone())
    }

    /// Each chapter file and its part of the joined text, in the book's order.
    pub(crate) fn files(&self) -> impl Iterator<Item = (&Path, Range<usize>)> {
        (self.places.iter()).map(|place| (place.path.as_path(), place.range.clone()))
    }
}

impl Joined {
    /// A copy of a project's text in which `change` gives some chapter files other text, for a trial that must leave
    /// the session alone. `None` keeps a file as it is.
    pub fn with_files(&self, change: impl Fn(&Path, &str) -> Option<String>) -> Self {
        let mut text = String::with_capacity(self.text.len());
        let mut map = ChapterMap::default();
        let mut copied = 0;
        for place in &self.map.places {
            // What joins two chapters stays as it is.
            text.push_str(&self.text[copied..place.range.start]);
            let start = text.len();
            match change(&place.path, &self.text[place.range.clone()]) {
                Some(new) => text.push_str(&new),
                None => text.push_str(&self.text[place.range.clone()]),
            }
            map.push(place.id, place.path.clone(), start..text.len());
            copied = place.range.end;
        }
        text.push_str(&self.text[copied..]);
        Self {
            text,
            map,
            revision: self.revision,
        }
    }

    /// The text of the chapter file at `path`, in a project.
    pub fn file(&self, path: &Path) -> Option<&str> {
        Some(&self.text[self.map.range_of(path)?])
    }
}

/// Whether a single file's section ends inside a line, as after typing past its last line break or deleting it. The
/// join puts a line break between it and the next section, so that section's first line, often its heading, stays a
/// line of its own.
pub(super) fn ends_open(section: &str) -> bool {
    !section.is_empty() && !section.ends_with('\n')
}

/// An unchanging copy of the document for a layout on another thread. A single file's text is the file. A project's is
/// its chapters with a blank line between them, the same rule as the engine's, with a chapter's byte order mark left
/// in. Its pages' source positions name a chapter file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joined {
    pub text: String,
    pub map: ChapterMap,
    /// The session's revision this is a copy of.
    pub revision: u64,
}

impl Joined {
    /// Chapters in the book's order, each with its file's path and text. A project's go one after another with a blank
    /// line between them, the engine's rule; a single file's sections go one after another as they are, with a line
    /// break after one that ends without one, see [`ends_open`].
    pub(super) fn join<'a>(
        chapters: impl IntoIterator<Item = (ChapterId, &'a Path, &'a str)>,
        project: bool,
        revision: u64,
    ) -> Self {
        let mut text = String::new();
        let mut map = ChapterMap::default();
        let mut open = false;
        for (id, path, chapter) in chapters {
            if open {
                text.push('\n');
            }
            open = !project && ends_open(chapter);
            let start = text.len();
            text.push_str(chapter);
            map.push(id, path.to_owned(), start..text.len());
            if project {
                if !text.ends_with('\n') && text.len() > start {
                    text.push('\n');
                }
                text.push('\n');
            }
        }
        Self { text, map, revision }
    }

    /// A project's chapter files with their texts, in the book's order, apart from any session, as for a copy of the
    /// book. Their ids are their places.
    pub fn of_files(files: &[(PathBuf, String)], revision: u64) -> Self {
        let chapters =
            (files.iter().enumerate()).map(|(at, (path, text))| (ChapterId(at as u32), path.as_path(), text.as_str()));
        Self::join(chapters, true, revision)
    }

    /// Text with no chapters: one file, a measurement's edit of it, or a test's.
    pub fn single(text: String, revision: u64) -> Self {
        Self {
            text,
            map: ChapterMap::default(),
            revision,
        }
    }
}

/// Reads chapters from a [`Joined`] and everything else from disk, so a layout sees text that is not saved yet and the
/// engine still reads the manifest, checks the list, and reports the real chapter files.
pub struct LiveResolver<'a> {
    joined: &'a Joined,
    files: &'a FileResolver,
}

impl<'a> LiveResolver<'a> {
    pub fn new(joined: &'a Joined, files: &'a FileResolver) -> Self {
        Self { joined, files }
    }
}

impl Resolver for LiveResolver<'_> {
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError> {
        let chapter = origin
            .file(path)
            .and_then(|file| self.joined.map.range_of(&normalize(&file)));
        match chapter {
            Some(range) => Ok(Bytes::from(self.joined.text.as_bytes()[range].to_vec())),
            None => self.files.read(origin, path),
        }
    }

    fn list(&self, origin: &Origin, dir: &str) -> Option<Vec<String>> {
        self.files.list(origin, dir)
    }

    fn installed(&self, family: &str) -> Result<Vec<InstalledFace>, ResolveError> {
        self.files.installed(family)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> ChapterMap {
        let mut map = ChapterMap::default();
        map.push(ChapterId(1), "/a.md".into(), 0..4);
        map.push(ChapterId(2), "/b.md".into(), 6..9);
        map
    }

    #[test]
    fn offsets_go_both_ways() {
        let map = map();
        assert_eq!(map.to_joined(ChapterId(2), 1), Some(7));
        assert_eq!(map.chapter_at(7), Some((ChapterId(2), 1)));
        assert_eq!(map.chapter_at(0), Some((ChapterId(1), 0)));
    }

    #[test]
    fn the_join_counts_as_the_end_of_the_chapter_before_it() {
        assert_eq!(map().chapter_at(5), Some((ChapterId(1), 4)));
    }

    #[test]
    fn an_offset_past_a_chapter_stays_in_it() {
        assert_eq!(map().to_joined(ChapterId(1), 99), Some(4));
        assert_eq!(map().to_joined(ChapterId(9), 0), None);
    }
}
