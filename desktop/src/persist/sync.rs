//! What happens between a session and its files, without gpui: what to do with a file that changed, what is pending a
//! person's decision, and when to write. The view runs this and shows the result.
//!
//! A file on disk is compared by content with the last text the session and the file agreed on. If it differs, someone
//! else wrote it, and:
//! - a buffer with nothing unsaved takes the new text, as one undo step;
//! - a buffer with unsaved text merges, line by line and then word by word, and stays unsaved until the merge is written;
//! - where both changed the same text, a person decides. Until then the buffer keeps its text and the file keeps its
//!   text, and nothing is written over it.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::atomic::{Outcome, content_hash};
use super::merge::{Choice, edits_between, starting_point, three_way};
use super::recovery::Copies;
use super::watch::Ready;
use crate::session::disk::Outgoing;
use crate::session::{ChapterId, Session};
use crate::source::EditKind;

/// How long typing must stop before unsaved text goes to the recovery copy, cheap as that is.
const RECOVERY_PAUSE: Duration = Duration::from_millis(200);
/// Long typing without a pause still reaches the recovery copy this often.
const RECOVERY_MOST: Duration = Duration::from_secs(2);
/// How long typing must stop before an autosave.
const SAVE_PAUSE: Duration = Duration::from_millis(800);
/// Long typing without a pause still reaches the disk this often.
const SAVE_MOST: Duration = Duration::from_secs(10);
/// How soon a failed save tries again.
const RETRY: Duration = Duration::from_secs(5);

/// What the timer should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Nothing is left to do: the timer can stop.
    Idle,
    Wait(Duration),
    Recover,
    Save,
}

/// When recovery copies and saves are due. Each edit only notes the time, so a key costs nothing here.
#[derive(Debug, Default)]
pub struct Pacer {
    edited: Option<Instant>,
    unsaved_since: Option<Instant>,
    unrecovered_since: Option<Instant>,
    retry: Option<Instant>,
}

impl Pacer {
    pub fn edited(&mut self, now: Instant) {
        self.edited = Some(now);
        self.unsaved_since.get_or_insert(now);
        self.unrecovered_since.get_or_insert(now);
    }

    pub fn retry_later(&mut self, now: Instant) {
        self.retry = Some(now + RETRY);
    }

    pub fn saved_all(&mut self) {
        self.retry = None;
    }

    /// Says what to do now, or how long to wait for it. Assumes the caller does it.
    pub fn next(&mut self, now: Instant) -> Next {
        let edited = self.edited.unwrap_or(now);
        let recover = (self.unrecovered_since).map(|since| (edited + RECOVERY_PAUSE).min(since + RECOVERY_MOST));
        let save = (self.unsaved_since).map(|since| (edited + SAVE_PAUSE).min(since + SAVE_MOST));
        let earliest = [recover, save, self.retry].into_iter().flatten().min();
        let Some(earliest) = earliest else {
            return Next::Idle;
        };
        if earliest > now {
            return Next::Wait(earliest - now);
        }
        if recover.is_some_and(|due| due <= now) {
            self.unrecovered_since = None;
            return Next::Recover;
        }
        self.unsaved_since = None;
        self.retry = None;
        Next::Save
    }
}

/// How a file's change came out, for the view and for whoever wants a diff of what changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// The buffer had nothing unsaved and took the new text.
    Reloaded,
    /// The buffer had unsaved text and both sets of changes are in it now.
    Merged,
    /// Both changed the same text, and a person decides.
    Conflict,
    /// The file is gone.
    Removed,
    /// The file came back with text the buffer already had, or moved.
    Unchanged,
}

/// A file changed outside the app. `base` and `theirs` are what the other program started from and wrote, which is what
/// a per-turn diff needs.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub chapter: Option<ChapterId>,
    pub kind: ChangeKind,
    pub base: Arc<str>,
    pub theirs: Option<Arc<str>>,
}

/// A conflict waiting for a person.
struct Pending {
    base: Arc<str>,
    /// `None`: the file was deleted.
    theirs: Option<Arc<str>>,
}

/// A problem to show, by the file it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The save failed, for this reason.
    Save(String),
    /// The text cannot be read, for this reason.
    Unreadable(String),
    /// The whole document's folder is gone.
    Gone,
}

/// What is left to say about unsaved text after everything that can be written has been, for a document that is about to
/// go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closing {
    /// Nothing is unsaved.
    Saved,
    /// Text waits for a decision about a conflict, which the footer holds, and a recovery copy keeps it with the file's
    /// other version still on disk. Nothing is lost by going on.
    Held,
    /// Text could not be written, so going on would lose it.
    Failed { file: PathBuf, reason: String },
}

/// A batch's result, for the view.
#[derive(Debug, Default)]
pub struct Report {
    pub changes: Vec<FileChange>,
    /// The manifest was read again.
    pub manifest: bool,
    /// The set of files changed, so the watcher's list needs an update.
    pub files_changed: bool,
    /// Something the session shows changed.
    pub session_changed: bool,
}

pub struct FileSync {
    pending: BTreeMap<PathBuf, Pending>,
    problems: BTreeMap<PathBuf, Problem>,
    copies: Option<Copies>,
}

impl FileSync {
    /// `copies` is where recovery and kept copies go. Without an app data folder, resolving a conflict keeps no copies.
    pub fn new(copies: Option<Copies>) -> Self {
        Self {
            pending: BTreeMap::new(),
            problems: BTreeMap::new(),
            copies,
        }
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &Path> {
        self.pending.keys().map(PathBuf::as_path)
    }

    /// Whether a file with a conflict was deleted, rather than changed.
    pub fn conflict_is_removal(&self, path: &Path) -> bool {
        self.pending.get(path).is_some_and(|pending| pending.theirs.is_none())
    }

    pub fn problems(&self) -> impl Iterator<Item = (&Path, &Problem)> {
        self.problems.iter().map(|(path, problem)| (path.as_path(), problem))
    }

    pub fn copies(&self) -> Option<&Copies> {
        self.copies.as_ref()
    }

    /// Whether a file's bytes tell the session anything new. A save's own echo does not, so it needs no session change.
    pub fn differs(&self, session: &Session, path: &Path, disk: Option<&[u8]>) -> bool {
        let Some(known) = session.file_base(path) else {
            return false;
        };
        match disk {
            None => known.on_disk,
            Some(bytes) => !known.on_disk || content_hash(bytes) != known.hash || self.pending.contains_key(path),
        }
    }

    /// Takes in what a file holds now. `disk` is `None` for a missing file.
    pub fn on_disk(&mut self, session: &mut Session, path: &Path, disk: Option<Vec<u8>>) -> Option<FileChange> {
        let known = session.file_base(path)?;
        let Some(bytes) = disk else {
            // With the whole folder gone there is no decision to ask for: the text waits for the folder.
            if !self.check_root(session) {
                return None;
            }
            return self.on_removed(session, path, known.base);
        };
        let Ok(theirs) = String::from_utf8(bytes) else {
            let problem = format!("{} is not UTF-8 text.", path.display());
            session.mark_unreadable(path, problem.clone());
            self.problems.insert(path.to_owned(), Problem::Unreadable(problem));
            return None;
        };
        if matches!(self.problems.get(path), Some(Problem::Unreadable(_))) {
            self.problems.remove(path);
        }
        if known.on_disk && content_hash(theirs.as_bytes()) == known.hash {
            self.release(session, path);
            return None;
        }
        let mine = session.file_text(path)?;
        let theirs: Arc<str> = theirs.into();
        let change = |kind, base: Arc<str>| FileChange {
            path: path.to_owned(),
            chapter: session.chapter_of_file(path),
            kind,
            base,
            theirs: Some(theirs.clone()),
        };
        if mine == *theirs {
            let change = change(ChangeKind::Unchanged, known.base.clone());
            session.set_synced(path, theirs.clone(), true);
            session.mark_clean(path);
            self.release(session, path);
            return Some(change);
        }
        let base: Arc<str> = starting_point(&known.base, &known.earlier, &theirs).into();
        let merged = three_way(&base, &mine, &theirs);
        if !merged.is_clean() {
            let change = change(ChangeKind::Conflict, base.clone());
            self.pending.insert(
                path.to_owned(),
                Pending {
                    base,
                    theirs: Some(theirs),
                },
            );
            session.hold_file(path, true);
            return Some(change);
        }
        let text = merged.text(Choice::Mine);
        let kind = if text == *theirs {
            ChangeKind::Reloaded
        } else {
            ChangeKind::Merged
        };
        let change = change(kind, base);
        self.take_text(session, path, &mine, &text, &theirs);
        Some(change)
    }

    /// Gives the buffers `text` as one undo step, now that `theirs` is what the file holds.
    fn take_text(&mut self, session: &mut Session, path: &Path, mine: &str, text: &str, theirs: &Arc<str>) {
        let edits = edits_between(mine, text);
        if !edits.is_empty()
            && let Err(error) = session.apply_to_file(path, &edits, EditKind::External)
        {
            // Edits from this diff are sorted and on character boundaries, so this would be a bug in the diff. The buffer
            // is unchanged and stays unsaved, and the file is left as it is.
            eprintln!("cannot merge into {}: {error}", path.display());
            return;
        }
        session.set_synced(path, theirs.clone(), true);
        if text == &**theirs {
            session.mark_clean(path);
        }
        self.release(session, path);
    }

    fn release(&mut self, session: &mut Session, path: &Path) {
        self.pending.remove(path);
        session.hold_file(path, false);
    }

    fn on_removed(&mut self, session: &mut Session, path: &Path, base: Arc<str>) -> Option<FileChange> {
        let chapter = session.chapter_of_file(path)?;
        let dirty = session.chapter(chapter).is_some_and(|chapter| chapter.is_dirty()) || session_dirty_single(session);
        let change = FileChange {
            path: path.to_owned(),
            chapter: Some(chapter),
            kind: if dirty {
                ChangeKind::Conflict
            } else {
                ChangeKind::Removed
            },
            base: base.clone(),
            theirs: None,
        };
        if dirty {
            self.pending.insert(path.to_owned(), Pending { base, theirs: None });
            session.hold_file(path, true);
        } else {
            session.file_removed(path);
        }
        Some(change)
    }

    /// Settles a conflict in a file. A version the choice leaves out is kept as a copy first, and the choice is one undo
    /// step. Returns the copy's path, if one was made. The kept copy not being writable stops the choice, since the
    /// text would be gone.
    pub fn resolve(&mut self, session: &mut Session, path: &Path, choice: Choice) -> io::Result<Option<PathBuf>> {
        let Some(pending) = self.pending.get(path) else {
            return Ok(None);
        };
        let mine = session.file_text(path).unwrap_or_default();
        let Some(theirs) = pending.theirs.clone() else {
            return self.resolve_removal(session, path, choice, &mine);
        };
        let merged = three_way(&pending.base, &mine, &theirs);
        let kept = match choice {
            Choice::Mine => self.keep(path, "theirs", &theirs)?,
            Choice::Theirs => self.keep(path, "mine", &mine)?,
            Choice::Both => None,
        };
        let text = merged.text(choice);
        self.take_text(session, path, &mine, &text, &theirs);
        Ok(kept)
    }

    fn resolve_removal(
        &mut self,
        session: &mut Session,
        path: &Path,
        choice: Choice,
        mine: &str,
    ) -> io::Result<Option<PathBuf>> {
        match choice {
            // The file comes back with the text in the buffer, at the next save.
            Choice::Mine | Choice::Both => {
                if let Some(known) = session.file_base(path) {
                    session.set_synced(path, known.base, false);
                }
                self.release(session, path);
                Ok(None)
            }
            Choice::Theirs => {
                let kept = self.keep(path, "mine", mine)?;
                self.release(session, path);
                session.mark_clean(path);
                session.file_removed(path);
                Ok(kept)
            }
        }
    }

    fn keep(&self, path: &Path, label: &str, text: &str) -> io::Result<Option<PathBuf>> {
        self.copies
            .as_ref()
            .map(|copies| copies.keep(path, label, text))
            .transpose()
    }

    /// Takes in a finished write. Returns whether the file turned out to have changed instead, so it needs a look once
    /// whoever wrote it is done.
    pub fn finish_write(&mut self, session: &mut Session, outgoing: &Outgoing, result: io::Result<Outcome>) -> bool {
        match result {
            Ok(Outcome::Written) => {
                self.problems.remove(&outgoing.path);
                session.written(outgoing);
                false
            }
            Ok(Outcome::Changed(_)) => {
                self.problems.remove(&outgoing.path);
                true
            }
            Err(error) => {
                self.problems
                    .insert(outgoing.path.clone(), Problem::Save(error.to_string()));
                false
            }
        }
    }

    /// Writes every file that changed, now, and keeps recovery copies of what is left. A file someone else wrote in the
    /// meantime is taken in first, which merges it or makes it a conflict, and the merge is written. For a document that
    /// is about to close, and for the app quitting.
    pub fn write_all(&mut self, session: &mut Session) -> Closing {
        for _ in 0..2 {
            let mut changed = false;
            for outgoing in session.outgoing() {
                let result = outgoing.write();
                if let (Ok(Outcome::Written), Some(copies)) = (&result, &self.copies) {
                    copies.clear(&outgoing.path);
                }
                if self.finish_write(session, &outgoing, result) {
                    changed = true;
                    let disk = fs::read(&outgoing.path).ok();
                    self.on_disk(session, &outgoing.path, disk);
                }
            }
            if !changed {
                break;
            }
        }
        let unsaved = session.unsaved();
        if let Some(copies) = &self.copies {
            for file in &unsaved {
                if let Err(error) = copies.store(&file.path, &file.base, &file.text) {
                    eprintln!("cannot keep a copy of {}: {error}", file.path.display());
                }
            }
        }
        match unsaved.iter().find(|file| !self.pending.contains_key(&file.path)) {
            None if unsaved.is_empty() => Closing::Saved,
            None => Closing::Held,
            Some(file) => Closing::Failed {
                file: file.path.clone(),
                reason: match self.problems.get(&file.path) {
                    Some(Problem::Save(reason) | Problem::Unreadable(reason)) => reason.clone(),
                    _ => "the file is not as the app last saw it".to_owned(),
                },
            },
        }
    }

    /// Whether typing since then has made a waiting conflict merge cleanly.
    pub fn can_settle_a_conflict(&self, session: &Session) -> bool {
        self.pending.iter().any(|(path, pending)| {
            let (Some(theirs), Some(mine)) = (&pending.theirs, session.file_text(path)) else {
                return false;
            };
            three_way(&pending.base, &mine, theirs).is_clean()
        })
    }

    /// Merges every conflict that no longer is one.
    pub fn settle_conflicts(&mut self, session: &mut Session) -> Vec<FileChange> {
        let paths: Vec<PathBuf> = self.pending.keys().cloned().collect();
        (paths.iter())
            .filter_map(|path| self.on_disk(session, path, fs::read(path).ok()))
            .collect()
    }

    /// A save that failed does not matter any more once nothing is left to save.
    pub fn forget_save_problems_if_clean(&mut self, session: &Session) {
        if !session.is_dirty() {
            self.problems.retain(|_, problem| !matches!(problem, Problem::Save(_)));
        }
    }

    /// Whether the document's folder is there. If not, this is the one problem to show and the session stays as it is.
    pub fn check_root(&mut self, session: &Session) -> bool {
        let root = session.base_dir();
        let there = root.is_dir();
        if there {
            self.problems.remove(&root);
        } else {
            self.problems.insert(root, Problem::Gone);
        }
        there
    }

    /// Whether the folder was reported gone.
    pub fn root_is_gone(&self, session: &Session) -> bool {
        matches!(self.problems.get(&session.base_dir()), Some(Problem::Gone))
    }

    /// Takes in a batch of paths from the watcher. The session changes only for what really differs.
    pub fn process(&mut self, session: &mut Session, ready: &Ready) -> Report {
        let mut report = Report::default();
        if !self.check_root(session) {
            return report;
        }
        let mut paths = ready.paths.clone();
        if ready.everything {
            paths.extend(session.files());
            paths.sort();
            paths.dedup();
        }
        let manifest = session.manifest_path();
        let mut manifest_changed = false;
        let mut gone: Vec<(PathBuf, u64)> = Vec::new();
        let mut added: Vec<(PathBuf, u64)> = Vec::new();
        let mut present: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        for path in paths {
            let disk = fs::read(&path).ok();
            match (&disk, session.file_base(&path)) {
                (Some(bytes), None) => added.push((path, content_hash(bytes))),
                (None, Some(known)) if Some(&path) != manifest.as_ref() => {
                    if known.on_disk {
                        gone.push((path, known.hash));
                    }
                }
                (None, None) => {}
                (None, Some(_)) => manifest_changed = true,
                (Some(bytes), Some(_)) => {
                    if self.differs(session, &path, Some(bytes)) {
                        if Some(&path) == manifest.as_ref() && session.chapter_of_file(&path).is_none() {
                            manifest_changed = true;
                        } else if let Some(bytes) = disk {
                            present.push((path, bytes));
                        }
                    }
                }
            }
        }
        // A file gone and one new with the same text are a rename.
        let mut book_moved = false;
        for (old, hash) in std::mem::take(&mut gone) {
            match added.iter().position(|(_, new)| *new == hash) {
                Some(at) => {
                    let (new, _) = added.remove(at);
                    book_moved |= session.file_moved(&old, &new);
                    report.session_changed = true;
                    report.files_changed = true;
                }
                None => gone.push((old, hash)),
            }
        }
        for (path, bytes) in present {
            let is_manifest = Some(&path) == manifest.as_ref();
            let change = self.on_disk(session, &path, Some(bytes));
            manifest_changed |= is_manifest;
            if let Some(change) = change {
                report.changes.push(change);
                report.session_changed = true;
            }
        }
        if manifest_changed {
            session.read_manifest();
            report.manifest = true;
            report.session_changed = true;
            report.files_changed = true;
        } else if book_moved && let Err(problem) = session.sync_chapter_list() {
            eprintln!("cannot update the chapter list: {problem}");
        }
        for (path, _) in gone {
            if let Some(change) = self.on_disk(session, &path, None) {
                report.changes.push(change);
            }
            report.session_changed = true;
            report.files_changed = true;
        }
        if (!added.is_empty() || ready.everything) && session.discover_files() {
            report.session_changed = true;
            report.files_changed = true;
        }
        report
    }
}

/// A single file's sections share one file: unsaved text in any of them counts.
fn session_dirty_single(session: &Session) -> bool {
    matches!(session.kind(), crate::session::Kind::Single { .. }) && session.is_dirty()
}

#[cfg(test)]
mod tests;
