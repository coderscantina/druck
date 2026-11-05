//! What the session knows about its files on disk: the text each one last agreed with, and the changes that come and go
//! between the buffers and the files. [`crate::persist`] decides when these happen.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use druck::input::MANIFEST;

use super::outline::outline;
use super::{Chapter, ChapterId, Group, Kind, MANUSCRIPT, NOTES, RESEARCH, Read, Session};
use crate::persist::atomic::{self, Outcome, content_hash};
use crate::persist::merge::{edits_between, map_offset};
use crate::source::buffer::EditError;
use crate::source::{Blocks, Buffer, Edit, EditKind, Selection};

/// How many earlier versions of a file to keep, to recognize a write that started from one of them. A few minutes of
/// autosaves at most, so a program that reads, thinks, and writes back does not take back what was typed meanwhile.
const EARLIER_VERSIONS: usize = 32;

/// The last text a file and the session agreed on.
pub(super) struct Synced {
    base: Arc<str>,
    hash: u64,
    earlier: VecDeque<Arc<str>>,
    on_disk: bool,
}

impl Synced {
    pub(super) fn new(text: &str, on_disk: bool) -> Self {
        Self {
            base: text.into(),
            hash: content_hash(text.as_bytes()),
            earlier: VecDeque::new(),
            on_disk,
        }
    }

    fn update(&mut self, text: Arc<str>, on_disk: bool) {
        if text != self.base {
            let old = std::mem::replace(&mut self.base, text);
            self.hash = content_hash(self.base.as_bytes());
            self.earlier.push_back(old);
            if self.earlier.len() > EARLIER_VERSIONS {
                self.earlier.pop_front();
            }
        }
        self.on_disk = on_disk;
    }
}

/// A file's sync state, for a merge.
#[derive(Clone)]
pub struct FileBase {
    pub base: Arc<str>,
    pub hash: u64,
    /// Older versions of the file, oldest first.
    pub earlier: Vec<Arc<str>>,
    pub on_disk: bool,
}

/// A file's text on its way to the disk. Writing it needs no session, so it can happen on another thread.
pub struct Outgoing {
    pub path: PathBuf,
    pub text: Arc<str>,
    marks: Vec<(ChapterId, u64)>,
    expected: Option<u64>,
    /// A folder a missing parent may be created under. If this is gone too, the document is gone, and a save fails.
    root: PathBuf,
}

impl Outgoing {
    /// Writes the file unless it no longer holds what the session last knew.
    pub fn write(&self) -> io::Result<Outcome> {
        if self.expected.is_none()
            && self.root.is_dir()
            && let Some(parent) = self.path.parent()
        {
            fs::create_dir_all(parent)?;
        }
        atomic::write_if(&self.path, self.text.as_bytes(), self.expected)
    }
}

/// A file with text not on disk, for the recovery copy.
pub struct Unsaved {
    pub path: PathBuf,
    pub base: Arc<str>,
    pub text: String,
}

impl Session {
    pub fn manifest_path(&self) -> Option<PathBuf> {
        match &self.kind {
            Kind::Project { dir } => Some(dir.join(MANIFEST)),
            Kind::Single { .. } => None,
        }
    }

    /// The chapters that are this file: one in a project, every section of a single file.
    fn ids_of(&self, path: &Path) -> Vec<ChapterId> {
        match &self.kind {
            Kind::Single { path: file } if file == path => self.book.clone(),
            Kind::Single { .. } => Vec::new(),
            Kind::Project { .. } => (self.chapters.iter())
                .filter(|(_, chapter)| chapter.path == path)
                .map(|(id, _)| *id)
                .collect(),
        }
    }

    /// Every file the session has a chapter or settings from.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.synced.keys().cloned().collect();
        files.sort();
        files
    }

    /// Whether a file is one the session knows.
    pub fn knows_file(&self, path: &Path) -> bool {
        self.synced.contains_key(path)
    }

    /// The chapter to name for a file: in a single file, the first section.
    pub fn chapter_of_file(&self, path: &Path) -> Option<ChapterId> {
        self.ids_of(path).first().copied()
    }

    /// The buffers' text of a file, if they have been read.
    pub fn file_text(&self, path: &Path) -> Option<String> {
        let ids = self.ids_of(path);
        let first = self.chapters.get(ids.first()?)?;
        if first.read == Read::Later {
            return None;
        }
        Some(match &self.kind {
            Kind::Single { .. } => self.joined().text,
            Kind::Project { .. } => first.text().to_owned(),
        })
    }

    /// The chapter holding a place in a file's text, and the place in that chapter. A single file's sections each hold
    /// part of its text.
    pub fn place_in_file(&self, path: &Path, at: usize) -> Option<(ChapterId, usize)> {
        let ids = self.ids_of(path);
        let mut start = 0;
        for (index, id) in ids.iter().enumerate() {
            let length = self.known(*id).buffer.len();
            if at < start + length || index + 1 == ids.len() {
                return Some((*id, at - start));
            }
            start += length;
        }
        None
    }

    pub fn file_base(&self, path: &Path) -> Option<FileBase> {
        let synced = self.synced.get(path)?;
        Some(FileBase {
            base: synced.base.clone(),
            hash: synced.hash,
            earlier: synced.earlier.iter().cloned().collect(),
            on_disk: synced.on_disk,
        })
    }

    /// Records what a file holds now.
    pub fn set_synced(&mut self, path: &Path, text: Arc<str>, on_disk: bool) {
        match self.synced.get_mut(path) {
            Some(synced) => synced.update(text, on_disk),
            None => {
                self.synced.insert(path.to_owned(), Synced::new(&text, on_disk));
            }
        }
    }

    /// Starts a file's record over from `base`, as what the file and the session last agreed on, with no earlier versions
    /// to match against. For a merge whose base is known exactly. The buffers count as unsaved from here, since the file
    /// is about to hold the other side: a conflict then holds the text as any other, with a recovery copy.
    pub fn restart_sync(&mut self, path: &Path, base: &str) {
        self.synced.insert(path.to_owned(), Synced::new(base, true));
        for id in self.ids_of(path) {
            let chapter = self.known_mut(id);
            chapter.saved = chapter.buffer.revision().wrapping_sub(1);
        }
    }

    /// Holds a file's text back from the disk, or lets it go again.
    pub fn hold_file(&mut self, path: &Path, held: bool) {
        if held {
            self.held.insert(path.to_owned());
        } else {
            self.held.remove(path);
        }
    }

    /// The file holds what its buffers hold, so nothing needs to write them.
    pub fn mark_clean(&mut self, path: &Path) {
        for id in self.ids_of(path) {
            let chapter = self.known_mut(id);
            chapter.saved = chapter.buffer.revision();
            let returned = chapter.read == Read::Missing;
            if chapter.read != Read::Later {
                chapter.read = Read::Done;
            }
            if returned && chapter.group == Group::Book {
                self.bump_revision();
            }
        }
        if matches!(self.kind, Kind::Single { .. }) {
            self.resplit_unsaved = false;
        }
    }

    /// The files whose text differs from the disk, ready to write. Skips the ones held back. A buffer that came back to
    /// what the file holds, as after an undo, is marked saved and needs no write.
    pub fn outgoing(&mut self) -> Vec<Outgoing> {
        let root = self.base_dir();
        let mut paths: Vec<PathBuf> = match &self.kind {
            Kind::Single { path } if self.is_dirty() => vec![path.clone()],
            Kind::Single { .. } => Vec::new(),
            Kind::Project { .. } => (self.chapters.values())
                .filter(|chapter| chapter.is_dirty() && !matches!(chapter.read, Read::Failed(_)))
                .map(|chapter| chapter.path.clone())
                .collect(),
        };
        paths.retain(|path| !self.held.contains(path));
        let mut outgoing = Vec::with_capacity(paths.len());
        for path in paths {
            let Some(text) = self.file_text(&path) else {
                continue;
            };
            let hash = content_hash(text.as_bytes());
            let synced = self.synced.get(&path);
            if synced.is_some_and(|synced| synced.on_disk && synced.hash == hash) {
                self.mark_clean(&path);
                continue;
            }
            let expected = synced.filter(|synced| synced.on_disk).map(|synced| synced.hash);
            let marks = (self.ids_of(&path).into_iter())
                .map(|id| (id, self.known(id).buffer.revision()))
                .collect();
            outgoing.push(Outgoing {
                path,
                text: text.into(),
                marks,
                expected,
                root: root.clone(),
            });
        }
        outgoing
    }

    /// A write finished. Edits made since it started stay unsaved.
    pub fn written(&mut self, outgoing: &Outgoing) {
        self.set_synced(&outgoing.path, outgoing.text.clone(), true);
        for (id, revision) in &outgoing.marks {
            if let Some(chapter) = self.chapters.get_mut(id) {
                chapter.saved = *revision;
                chapter.read = Read::Done;
            }
        }
        if matches!(self.kind, Kind::Single { .. }) {
            self.resplit_unsaved = false;
        }
        if self.manifest_path().is_some_and(|path| path == outgoing.path) {
            self.read_manifest();
        }
    }

    /// The files with text not on disk, also those held back.
    pub fn unsaved(&self) -> Vec<Unsaved> {
        let paths: Vec<PathBuf> = match &self.kind {
            Kind::Single { path } if self.is_dirty() => vec![path.clone()],
            Kind::Single { .. } => Vec::new(),
            Kind::Project { .. } => (self.chapters.values())
                .filter(|chapter| chapter.is_dirty())
                .map(|chapter| chapter.path.clone())
                .collect(),
        };
        (paths.into_iter())
            .filter_map(|path| {
                let text = self.file_text(&path)?;
                let base = self.synced.get(&path)?.base.clone();
                Some(Unsaved { path, base, text })
            })
            .collect()
    }

    /// Applies edits to a file's text, moving carets and scroll places with the text. The edits refer to the file's text
    /// before them. In a project they are one undo step. In a single file each section takes the part in its range, as
    /// one step in that section's history, which is the limit of per-section undo.
    pub fn apply_to_file(&mut self, path: &Path, edits: &[Edit], kind: EditKind) -> Result<(), EditError> {
        let ids = self.ids_of(path);
        if ids.len() == 1 {
            return self.apply_to_chapter(ids[0], edits.to_vec(), kind);
        }
        let mut start = 0;
        let count = ids.len();
        for (index, id) in ids.into_iter().enumerate() {
            let end = start + self.known(id).buffer.len();
            let last = index + 1 == count;
            let mut local = Vec::new();
            for edit in edits {
                let insert_here = edit.range.is_empty()
                    && edit.range.start >= start
                    && (edit.range.start < end || (last && edit.range.start == end));
                let overlaps = !edit.range.is_empty() && edit.range.start < end && start < edit.range.end;
                if insert_here || overlaps {
                    // A change across sections writes its text in the first one and clears the rest.
                    let text = if edit.range.start >= start {
                        edit.text.as_str()
                    } else {
                        ""
                    };
                    let range = edit.range.start.max(start) - start..edit.range.end.min(end) - start;
                    local.push(Edit::new(range, text));
                }
            }
            if !local.is_empty() {
                self.apply_to_chapter(id, local, kind)?;
            }
            start = end;
        }
        self.resplit();
        self.refresh_details();
        Ok(())
    }

    fn apply_to_chapter(&mut self, id: ChapterId, edits: Vec<Edit>, kind: EditKind) -> Result<(), EditError> {
        let chapter = self.known_mut(id);
        let scroll = chapter.view.scroll.and_then(|(block, offset)| {
            let start = Blocks::parse(chapter.buffer.text()).as_slice().get(block)?.range.start;
            Some((map_offset(start, &edits), offset))
        });
        let before = chapter.view.selection;
        let after = Selection {
            anchor: map_offset(before.anchor, &edits),
            head: map_offset(before.head, &edits),
        };
        chapter.buffer.transact(kind, edits, before, after, Instant::now())?;
        chapter.view.selection = after;
        chapter.view.scroll = scroll.map(|(start, offset)| {
            let blocks = Blocks::parse(chapter.buffer.text());
            (blocks.index_at(start), offset)
        });
        chapter.outline = outline(chapter.buffer.text());
        if chapter.read != Read::Later {
            chapter.read = Read::Done;
        }
        Ok(())
    }

    /// A file cannot be used as text now. Its buffer stays as it is, and saving leaves the file alone.
    pub fn mark_unreadable(&mut self, path: &Path, problem: String) {
        for id in self.ids_of(path) {
            self.known_mut(id).read = Read::Failed(problem.clone());
        }
    }

    /// A clean file is gone. A listed chapter stays as a broken row, with its text, so the same text comes back if the
    /// file does. A section of a single file stays too. Anything else leaves the session.
    pub fn file_removed(&mut self, path: &Path) {
        if let Some(synced) = self.synced.get_mut(path) {
            synced.on_disk = false;
        }
        if matches!(self.kind, Kind::Single { .. }) {
            return;
        }
        for id in self.ids_of(path) {
            match self.known(id).group {
                Group::Book => {
                    self.known_mut(id).read = Read::Missing;
                    self.bump_revision();
                }
                Group::Manifest => self.known_mut(id).read = Read::Missing,
                Group::NotInBook | Group::Notes | Group::Research => self.forget_chapter(id),
            }
        }
    }

    /// Takes a chapter out of the session altogether. There is always another to show: the manifest's row if nothing
    /// else.
    pub(super) fn forget_chapter(&mut self, id: ChapterId) {
        let Some(chapter) = self.chapters.remove(&id) else {
            return;
        };
        self.synced.remove(&chapter.path);
        self.held.remove(&chapter.path);
        if self.active == id {
            let next = (self.book.first().copied()).or_else(|| self.chapters.keys().next().copied());
            match next {
                Some(next) => self.set_active(next),
                None => self.read_manifest(),
            }
        }
    }

    /// A file moved. The chapter follows with its text and undo history. Returns whether it is in the book, so the
    /// manifest needs to name its new path.
    pub fn file_moved(&mut self, from: &Path, to: &Path) -> bool {
        let Kind::Project { dir } = &self.kind else {
            return false;
        };
        let dir = dir.clone();
        let Some(id) = self.chapter_of_file(from) else {
            return false;
        };
        let Ok(relative) = to.strip_prefix(&dir) else {
            return false;
        };
        let listed = relative.to_string_lossy().into_owned();
        self.move_synced(from, to);
        let chapter = self.known_mut(id);
        chapter.path = to.to_owned();
        chapter.listed = listed;
        chapter.group == Group::Book
    }

    /// Writes the chapter list again, after a file in the book moved.
    pub fn sync_chapter_list(&mut self) -> Result<(), String> {
        self.write_chapter_list()
    }

    /// Looks for files that came in the project's folders. Returns whether any did.
    pub fn discover_files(&mut self) -> bool {
        let before = self.chapters.len();
        self.find_files(MANUSCRIPT, Group::NotInBook);
        self.find_files(NOTES, Group::Notes);
        self.find_files(RESEARCH, Group::Research);
        self.chapters.len() != before
    }

    /// Gives a file the text of a recovery copy, as one undoable step. `base` is what that text started from. A file the
    /// session no longer has comes back as a file not in the book.
    pub fn restore_text(&mut self, path: &Path, base: &str, text: &str) -> Result<(), String> {
        if !self.knows_file(path) {
            let Kind::Project { dir } = &self.kind else {
                return Ok(());
            };
            let listed = (path.strip_prefix(dir))
                .map_err(|_| format!("{} is outside the project.", path.display()))?
                .to_string_lossy()
                .into_owned();
            let group = match listed.split('/').next() {
                Some(NOTES) => Group::Notes,
                Some(RESEARCH) => Group::Research,
                _ => Group::NotInBook,
            };
            self.add(Chapter::new(
                path.to_owned(),
                listed,
                group,
                Buffer::new(""),
                Read::Missing,
            ));
            self.synced.insert(path.to_owned(), Synced::new("", false));
        }
        for id in self.ids_of(path) {
            self.set_read(id);
        }
        let current = self.file_text(path).unwrap_or_default();
        let edits = edits_between(&current, text);
        if !edits.is_empty() {
            self.apply_to_file(path, &edits, EditKind::Discrete)
                .map_err(|error| error.to_string())?;
        }
        let on_disk = self.synced.get(path).is_some_and(|synced| synced.on_disk);
        self.set_synced(path, base.into(), on_disk);
        Ok(())
    }
}
