//! A book's Git repo, through gitoxide: what it holds, its history, and commits. Nothing here needs Git installed, and
//! what it writes is what `git` itself would write: a loose commit, the updated branch with its reflog, and an index
//! `git status` agrees with. Holds no gpui types.
//!
//! The repo may hold more than the book, as a folder of several books does. Changes and commits cover the book's folder;
//! branches and history are the repo's.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use gix::bstr::{BStr, ByteSlice as _};
use gix::index::entry::{Flags, Mode, Stage, Stat};
use gix::objs::tree::EntryKind;
use gix::refs::FullName;
use gix::refs::transaction::{Change as RefChange, LogChange, PreviousValue, RefEdit, RefLog};

use super::git_cli::Git;
use super::strings::Key;
use super::{history, words};
use crate::persist::atomic;
use crate::session::templates::{GITIGNORE, GITIGNORE_TEXT};

/// How far back a file's history and the list of commits look.
const LOG_LIMIT: usize = 5000;
/// Files the middle of a merge leaves in the Git folder, which a finished merge clears.
const MERGE_FILES: [&str; 4] = ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "AUTO_MERGE"];

/// Why something could not be done, in terms a view can word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    NothingToCommit,
    /// Git has no name or email to put on a commit.
    NoIdentity,
    /// A rebase or similar is running, which belongs to the tool that began it.
    Busy(String),
    /// Files outside the book are in conflict.
    UnmergedElsewhere(String),
    BranchExists(String),
    BadName(String),
    /// Anything else, in the library's words.
    Other(String),
}

impl<E: std::fmt::Display> From<E> for Problem {
    fn from(error: E) -> Self {
        Self::Other(error.to_string())
    }
}

type Result<T> = std::result::Result<T, Problem>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Removed,
    /// Both sides of a merge changed it.
    Unmerged,
    /// Moved here from [`Change::from`], as Git's rename tracking finds it.
    Moved,
    /// Moved elsewhere. The commit takes it out, and lists show only where it went.
    MovedAway,
}

/// A file that differs from the last commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    /// Its path in the book.
    pub name: String,
    pub kind: ChangeKind,
    /// Words added and removed, for a text file.
    pub words: Option<(usize, usize)>,
    /// Its path in the book before it moved, for [`ChangeKind::Moved`].
    pub from: Option<String>,
}

/// One commit, for lists and the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub summary: String,
    pub author: String,
    /// Seconds since 1970.
    pub time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub current: bool,
    /// Everything on it is in the current branch, so deleting it loses nothing.
    pub merged: bool,
}

/// Whether a file's words mean anything to an author: Markdown has words, and a manifest or a bibliography has only changes.
fn counts_words(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown"))
}

/// Where the current branch stands against its upstream, as of the last fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sync {
    /// The repo has no remote.
    #[default]
    NoRemote,
    /// There is a remote, but this branch was never pushed.
    NotPushed,
    Synced,
    Ahead(usize),
    Behind(usize),
    Diverged {
        ahead: usize,
        behind: usize,
    },
}

/// One side's text of a file in conflict. `None`: that side has no such file, or it is not text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: PathBuf,
    pub name: String,
    pub base: Option<String>,
    pub theirs: Option<String>,
    /// The book's folder holds it. A conflict elsewhere in the repo cannot be shown on the conflict line.
    pub in_book: bool,
    pub ours_exists: bool,
}

/// Everything the views show about a repo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overview {
    /// The current branch, or a short commit id on a detached head.
    pub branch: String,
    pub detached: bool,
    pub branches: Vec<Branch>,
    pub changes: Vec<Change>,
    /// Newest first.
    pub log: Vec<Commit>,
    pub sync: Sync,
    /// The message of a merge in the middle of happening.
    pub merging: Option<String>,
    /// A rebase or similar that another tool began.
    pub busy: Option<String>,
    pub has_identity: bool,
    /// The author's settings ask for signed commits.
    pub signs: bool,
    /// A remote to name when pushing a branch for the first time.
    pub remote: Option<String>,
}

impl Overview {
    /// Whether a commit would record anything: a change, or a merge to finish. Commit is in the accent only then.
    pub fn has_something_to_commit(&self) -> bool {
        !self.changes.is_empty() || self.merging.is_some()
    }

    /// The changes a list shows: a file that moved shows once, where it went.
    pub fn listed(&self) -> impl Iterator<Item = &Change> {
        (self.changes.iter()).filter(|change| change.kind != ChangeKind::MovedAway)
    }
}

pub struct Repo {
    repo: gix::Repository,
    /// The book's folder as the session knows it.
    book: PathBuf,
    /// The book's folder in the worktree, empty or ending in a slash.
    scope: String,
}

impl Repo {
    /// The repo a book's folder is in, if any.
    pub fn open(book: &Path) -> Result<Option<Self>> {
        if !book.ancestors().any(|folder| folder.join(".git").exists()) {
            return Ok(None);
        }
        Self::from_repo(gix::discover(book)?, book).map(Some)
    }

    /// Makes a book's folder a repo. Nothing is committed. Exports stay out of it, so a first commit does not take them,
    /// unless the folder already says what to leave out.
    pub fn init(book: &Path) -> Result<Self> {
        let repo = Self::from_repo(gix::init(book)?, book)?;
        let ignore = book.join(GITIGNORE);
        if !ignore.exists() {
            fs::write(ignore, GITIGNORE_TEXT)?;
        }
        Ok(repo)
    }

    fn from_repo(repo: gix::Repository, book: &Path) -> Result<Self> {
        let workdir = repo
            .workdir()
            .ok_or_else(|| Problem::Other(crate::t!(Key::NoWorktree).to_owned()))?;
        let (workdir, canonical) = (fs::canonicalize(workdir)?, fs::canonicalize(book)?);
        let inside = canonical.strip_prefix(&workdir)?.to_string_lossy().into_owned();
        let scope = if inside.is_empty() {
            inside
        } else {
            format!("{inside}/")
        };
        Ok(Self {
            repo,
            book: book.to_owned(),
            scope,
        })
    }

    pub fn git_dir(&self) -> &Path {
        self.repo.path()
    }

    fn head_id(&self) -> Option<gix::ObjectId> {
        self.repo.head_id().ok().map(gix::Id::detach)
    }

    fn head_tree(&self) -> Option<gix::Tree<'_>> {
        self.repo.head_tree().ok()
    }

    /// A file's text at the tip of a tree, by its path in the repo.
    fn text_in(&self, tree: &gix::Tree<'_>, rela: &str) -> Option<String> {
        let entry = tree.lookup_entry_by_path(rela).ok()??;
        let blob = self.repo.find_blob(entry.object_id()).ok()?;
        String::from_utf8(blob.data.clone()).ok()
    }

    fn entry_id(&self, tree: &gix::Tree<'_>, rela: &str) -> Option<gix::ObjectId> {
        Some(tree.lookup_entry_by_path(rela).ok()??.object_id())
    }

    /// What `git status` lists for the book's folder, with each file's words against the last commit.
    pub fn changes(&self) -> Result<Vec<Change>> {
        use gix::status::index_worktree::Item as Worktree;
        let unmerged = self.unmerged_paths()?;
        let mut paths = BTreeSet::new();
        // Where each moved file came from, by where it is now.
        let mut moved: HashMap<String, String> = HashMap::new();
        let status = (self.repo.status(gix::progress::Discard)?)
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_rewrites(gix::diff::Rewrites::default())
            .tree_index_track_renames(gix::status::tree_index::TrackRenames::Given(
                gix::diff::Rewrites::default(),
            ))
            .into_iter(None)?;
        for item in status {
            let item = item?;
            let location = item.location().to_str_lossy().into_owned();
            // A move not yet staged is one from the index to the worktree, and one another tool staged is one from the
            // tree to the index.
            let from = match &item {
                gix::status::Item::IndexWorktree(Worktree::Rewrite {
                    source, copy: false, ..
                }) => Some(source.rela_path().to_str_lossy().into_owned()),
                gix::status::Item::TreeIndex(gix::diff::index::ChangeRef::Rewrite {
                    source_location,
                    copy: false,
                    ..
                }) => Some(source_location.to_str_lossy().into_owned()),
                _ => None,
            };
            if let Some(from) = from {
                paths.insert(from.clone());
                moved.insert(location.clone(), from);
            }
            paths.insert(location);
        }
        let moved_away: HashSet<&String> = moved.values().collect();
        paths.extend(unmerged.iter().cloned());
        let tree = self.head_tree();
        let mut changes = Vec::new();
        for rela in paths {
            let Some(name) = rela.strip_prefix(&self.scope) else {
                continue;
            };
            let path = self.book.join(name);
            if atomic::is_temporary(&path) {
                continue;
            }
            let on_disk = fs::read(&path).ok();
            let in_head = tree.as_ref().and_then(|tree| self.entry_id(tree, &rela));
            let same = match (&on_disk, in_head) {
                (Some(bytes), Some(id)) => {
                    gix::ObjectId::from_hex(history::blob_id(bytes).as_bytes()).is_ok_and(|on_disk| on_disk == id)
                }
                (None, None) => true,
                _ => false,
            };
            if same && !unmerged.contains(&rela) {
                continue;
            }
            let from = moved.get(&rela);
            let kind = match (unmerged.contains(&rela), &on_disk, in_head) {
                (true, ..) => ChangeKind::Unmerged,
                (_, None, _) if moved_away.contains(&rela) => ChangeKind::MovedAway,
                (_, None, _) => ChangeKind::Removed,
                (_, _, None) if from.is_some() => ChangeKind::Moved,
                (_, _, None) => ChangeKind::Added,
                _ => ChangeKind::Modified,
            };
            let old = tree
                .as_ref()
                .and_then(|tree| self.text_in(tree, from.unwrap_or(&rela)))
                .unwrap_or_default();
            let words = match &on_disk {
                Some(bytes) if std::str::from_utf8(bytes).is_err() => None,
                _ if !counts_words(&path) => Some((0, 0)),
                Some(bytes) => std::str::from_utf8(bytes).ok().map(|new| words::counts(&old, new)),
                None => Some(words::counts(&old, "")),
            };
            changes.push(Change {
                path,
                name: name.to_owned(),
                kind,
                words,
                from: (kind == ChangeKind::Moved)
                    .then(|| from.and_then(|from| from.strip_prefix(&self.scope)).map(str::to_owned))
                    .flatten(),
            });
        }
        Ok(changes)
    }

    fn unmerged_paths(&self) -> Result<HashSet<String>> {
        let index = self.repo.index_or_empty()?;
        Ok((index.entries().iter())
            .filter(|entry| entry.stage() != Stage::Unconflicted)
            .map(|entry| entry.path(&index).to_str_lossy().into_owned())
            .collect())
    }

    /// Whether Git knows who is committing.
    pub fn has_identity(&self) -> bool {
        matches!(self.repo.committer(), Some(Ok(_))) && matches!(self.repo.author(), Some(Ok(_)))
    }

    /// Sets who commits, for this repo only, as `git config` would.
    pub fn set_identity(&self, name: &str, email: &str) -> Result<()> {
        let quote = |value: &str| value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ");
        let path = self.repo.common_dir().join("config");
        let mut config = fs::read_to_string(&path)?;
        if !config.is_empty() && !config.ends_with('\n') {
            config.push('\n');
        }
        config.push_str(&format!(
            "[user]\n\tname = \"{}\"\n\temail = \"{}\"\n",
            quote(name),
            quote(email)
        ));
        atomic::write(&path, config.as_bytes())?;
        Ok(())
    }

    /// Commits every change in the book's folder, as `git add <book>` and `git commit -- <book>` would. During a merge it
    /// finishes the merge, with both parents and everything the merge staged. With `git` at hand and `commit.gpgsign` on,
    /// `git` signs it.
    pub fn commit(&mut self, message: &str, git: Option<&Git>) -> Result<String> {
        if let Some(busy) = self.busy() {
            return Err(Problem::Busy(busy));
        }
        if !self.has_identity() {
            return Err(Problem::NoIdentity);
        }
        let merge_heads = self.merge_heads();
        let changes = self.changes()?;
        if changes.is_empty() && merge_heads.is_empty() {
            return Err(Problem::NothingToCommit);
        }
        let mut index = self.staged_index(&changes)?;
        let parents: Vec<gix::ObjectId> = self.head_id().into_iter().chain(merge_heads.iter().copied()).collect();
        let tree = if merge_heads.is_empty() {
            self.tree_with_changes(&changes, &index)?
        } else {
            self.tree_from_index(&index)?
        };
        let id = match (self.signs(), git) {
            (true, Some(git)) => self.write_signed(git, tree, &parents, message)?,
            _ => self.repo.commit("HEAD", message, tree, parents)?.detach(),
        };
        index.write(gix::index::write::Options::default())?;
        for name in MERGE_FILES {
            fs::remove_file(self.repo.path().join(name)).ok();
        }
        Ok(id.to_string())
    }

    fn write_signed(
        &self,
        git: &Git,
        tree: gix::ObjectId,
        parents: &[gix::ObjectId],
        message: &str,
    ) -> Result<gix::ObjectId> {
        let names: Vec<String> = parents.iter().map(ToString::to_string).collect();
        let id = git
            .signed_commit(&self.book, &tree.to_string(), &names, message)
            .map_err(Problem::Other)?;
        let id = gix::ObjectId::from_hex(id.as_bytes())?;
        let summary = message.lines().next().unwrap_or_default();
        let kind = match parents.len() {
            0 => "commit (initial)",
            1 => "commit",
            _ => "commit (merge)",
        };
        let expected = match parents.first() {
            Some(previous) => PreviousValue::MustExistAndMatch((*previous).into()),
            None => PreviousValue::MustNotExist,
        };
        self.repo.edit_reference(RefEdit {
            change: RefChange::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: format!("{kind}: {summary}").into(),
                },
                expected,
                new: id.into(),
            },
            name: FullName::try_from("HEAD")?,
            deref: true,
        })?;
        Ok(id)
    }

    fn signs(&self) -> bool {
        self.repo.config_snapshot().boolean("commit.gpgsign").unwrap_or(false)
    }

    /// The index as it should be after the commit: the changes staged, with their files' stat, so `git status` need not
    /// read them again. Entries elsewhere stay as they are.
    fn staged_index(&self, changes: &[Change]) -> Result<gix::index::File> {
        let (mut pipeline, current) = self.repo.filter_pipeline(None)?;
        let mut index = current.clone().into_owned();
        let conflicted = self.unmerged_paths()?;
        if let Some(path) = conflicted.iter().find(|rela| !rela.starts_with(&self.scope)) {
            return Err(Problem::UnmergedElsewhere(path.clone()));
        }
        let unwanted: HashSet<String> = (changes.iter())
            .map(|change| format!("{}{}", self.scope, change.name))
            .collect();
        index.remove_entries(|_, path, _| {
            let rela = path.to_str_lossy();
            unwanted.contains(rela.as_ref()) || conflicted.contains(rela.as_ref())
        });
        for change in changes {
            // A deleted file only leaves.
            let Ok(metadata) = change.path.symlink_metadata() else {
                continue;
            };
            let rela = format!("{}{}", self.scope, change.name);
            let (id, mode) = if metadata.file_type().is_symlink() {
                let target = fs::read_link(&change.path)?;
                let id = self.repo.write_blob(target.as_os_str().as_encoded_bytes())?.detach();
                (id, Mode::SYMLINK)
            } else {
                let file = fs::File::open(&change.path)?;
                let mut converted = pipeline.convert_to_git(file, Path::new(&rela), &current)?;
                let mut bytes = Vec::new();
                converted.read_to_end(&mut bytes)?;
                let id = self.repo.write_blob(&bytes)?.detach();
                (
                    id,
                    if is_executable(&metadata) {
                        Mode::FILE_EXECUTABLE
                    } else {
                        Mode::FILE
                    },
                )
            };
            let stat = gix::index::fs::Metadata::from_path_no_follow(&change.path)
                .ok()
                .and_then(|metadata| Stat::from_fs(&metadata).ok())
                .unwrap_or_default();
            index.dangerously_push_entry(stat, id, Flags::empty(), mode, BStr::new(&rela));
        }
        index.sort_entries();
        // Both describe the old entries.
        index.remove_tree();
        index.remove_resolve_undo();
        Ok(index)
    }

    /// The last commit's tree with the book's changes in it, and staged files elsewhere left out, as they stay staged.
    fn tree_with_changes(&self, changes: &[Change], index: &gix::index::File) -> Result<gix::ObjectId> {
        let base = self.head_tree().map(|tree| tree.id().detach());
        let mut editor = match base {
            Some(tree) => self.repo.edit_tree(tree)?,
            None => self.repo.empty_tree().edit()?,
        };
        for change in changes {
            let rela = format!("{}{}", self.scope, change.name);
            match index.entry_by_path(BStr::new(&rela)) {
                Some(entry) => {
                    editor.upsert(rela.as_str(), entry_kind(entry.mode)?, entry.id)?;
                }
                None => {
                    editor.remove(rela.as_str())?;
                }
            }
        }
        Ok(editor.write()?.detach())
    }

    /// The tree the whole index describes, as a merge wants it.
    fn tree_from_index(&self, index: &gix::index::File) -> Result<gix::ObjectId> {
        let mut editor = self.repo.empty_tree().edit()?;
        for entry in index.entries() {
            if !entry.flags.contains(Flags::INTENT_TO_ADD) {
                editor.upsert(entry.path(index), entry_kind(entry.mode)?, entry.id)?;
            }
        }
        Ok(editor.write()?.detach())
    }

    fn merge_heads(&self) -> Vec<gix::ObjectId> {
        let text = fs::read_to_string(self.repo.path().join("MERGE_HEAD")).unwrap_or_default();
        (text.lines())
            .filter_map(|line| gix::ObjectId::from_hex(line.trim().as_bytes()).ok())
            .collect()
    }

    /// A running operation this app does not finish.
    fn busy(&self) -> Option<String> {
        use gix::state::InProgress;
        match self.repo.state()? {
            InProgress::Merge => None,
            InProgress::Bisect => Some("bisect".to_owned()),
            InProgress::CherryPick | InProgress::CherryPickSequence => Some("cherry-pick".to_owned()),
            InProgress::Revert | InProgress::RevertSequence => Some("revert".to_owned()),
            InProgress::ApplyMailbox | InProgress::ApplyMailboxRebase => Some("am".to_owned()),
            InProgress::Rebase | InProgress::RebaseInteractive => Some("rebase".to_owned()),
        }
    }

    /// The commits of the current branch, newest first, in `git log`'s order.
    pub fn log(&self, limit: usize) -> Result<Vec<Commit>> {
        let Some(head) = self.head_id() else {
            return Ok(Vec::new());
        };
        let walk = self.repo.rev_walk([head]).sorting(by_time()).all()?;
        let mut commits = Vec::new();
        for info in walk.take(limit) {
            commits.push(self.describe(info?.id)?);
        }
        Ok(commits)
    }

    fn describe(&self, id: gix::ObjectId) -> Result<Commit> {
        let commit = self.repo.find_commit(id)?;
        let summary = commit.message()?.summary().to_str_lossy().into_owned();
        let author = commit.author()?.name.to_str_lossy().into_owned();
        let time = commit.time()?.seconds;
        Ok(Commit {
            id: id.to_string(),
            summary,
            author,
            time,
        })
    }

    /// The commits of the current branch that changed a file, oldest first, each with the file's blob id then. `path` is
    /// the file's place on disk.
    pub fn file_history(&self, path: &Path) -> Result<Vec<(Commit, String)>> {
        let (Some(head), Some(rela)) = (self.head_id(), self.rela(path)) else {
            return Ok(Vec::new());
        };
        let mut blob_of: HashMap<gix::ObjectId, Option<gix::ObjectId>> = HashMap::new();
        let mut lookup = |id: gix::ObjectId| -> Result<Option<gix::ObjectId>> {
            if let Some(found) = blob_of.get(&id) {
                return Ok(*found);
            }
            let tree = self.repo.find_commit(id)?.tree()?;
            let found = self.entry_id(&tree, &rela);
            blob_of.insert(id, found);
            Ok(found)
        };
        let mut commits = Vec::new();
        let walk = self.repo.rev_walk([head]).sorting(by_time()).all()?;
        for info in walk.take(LOG_LIMIT) {
            let info = info?;
            let blob = lookup(info.id)?;
            let before = match info.parent_ids.first() {
                Some(parent) => lookup(*parent)?,
                None => None,
            };
            if let Some(blob) = blob.filter(|_| blob != before) {
                commits.push((self.describe(info.id)?, blob.to_string()));
            }
        }
        commits.reverse();
        Ok(commits)
    }

    /// A file's exact text at a commit.
    pub fn text_at(&self, commit: &str, path: &Path) -> Result<Option<String>> {
        let Some(rela) = self.rela(path) else {
            return Ok(None);
        };
        let id = gix::ObjectId::from_hex(commit.as_bytes())?;
        let tree = self.repo.find_commit(id)?.tree()?;
        Ok(self.text_in(&tree, &rela))
    }

    fn rela(&self, path: &Path) -> Option<String> {
        let inside = path.strip_prefix(&self.book).ok()?;
        Some(format!("{}{}", self.scope, inside.to_string_lossy()))
    }

    pub fn branches(&self) -> Result<Vec<Branch>> {
        let current = self.repo.head_name()?;
        let ancestors: HashSet<gix::ObjectId> = match self.head_id() {
            Some(head) => (self.repo.rev_walk([head]).all()?)
                .filter_map(|info| info.ok().map(|info| info.id))
                .collect(),
            None => HashSet::new(),
        };
        let platform = self.repo.references()?;
        let mut branches = Vec::new();
        for reference in platform.local_branches()? {
            let mut reference = reference.map_err(|error| Problem::Other(error.to_string()))?;
            let tip = reference.peel_to_id()?.detach();
            branches.push(Branch {
                name: reference.name().shorten().to_str_lossy().into_owned(),
                current: current.as_ref().is_some_and(|name| name.as_ref() == reference.name()),
                merged: ancestors.contains(&tip),
            });
        }
        // An unborn branch has no reference yet, and is the one the author is on.
        if let Some(name) = &current
            && !branches.iter().any(|branch| branch.current)
        {
            branches.push(Branch {
                name: name.shorten().to_str_lossy().into_owned(),
                current: true,
                merged: true,
            });
        }
        branches.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(branches)
    }

    /// Makes a branch at the current commit and makes it the current one. It holds the same commit, so no file changes,
    /// and unsaved text stays as it is.
    pub fn create_branch(&self, name: &str) -> Result<()> {
        let full = branch_ref(name)?;
        let head = self
            .head_id()
            .ok_or_else(|| Problem::Other(crate::t!(Key::NoCommitYet).to_owned()))?;
        if self.repo.try_find_reference(full.as_ref())?.is_some() {
            return Err(Problem::BranchExists(name.to_owned()));
        }
        let from = self.repo.head_name()?.map(|name| name.shorten().to_string());
        self.repo.reference(
            full.clone(),
            head,
            PreviousValue::MustNotExist,
            "branch: Created from HEAD",
        )?;
        self.repo.edit_reference(RefEdit {
            change: RefChange::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: format!("checkout: moving from {} to {name}", from.unwrap_or_default()).into(),
                },
                expected: PreviousValue::Any,
                new: gix::refs::Target::Symbolic(full),
            },
            name: FullName::try_from("HEAD")?,
            deref: false,
        })?;
        Ok(())
    }

    /// Deletes a branch and its settings. The current one cannot go, and whether it is merged is for the caller to ask.
    pub fn delete_branch(&mut self, name: &str) -> Result<()> {
        let full = branch_ref(name)?;
        self.repo.delete_local_branches([full])?;
        Ok(())
    }

    /// Where the branch stands against its upstream, as of the last fetch, and the first remote's name.
    pub fn sync(&self) -> Result<(Sync, Option<String>)> {
        let remote = (self.repo.remote_names().iter().next()).map(|name| name.to_str_lossy().into_owned());
        if remote.is_none() {
            return Ok((Sync::NoRemote, None));
        }
        let (Some(name), Some(head)) = (self.repo.head_name()?, self.head_id()) else {
            return Ok((Sync::NotPushed, remote));
        };
        let upstream = self
            .repo
            .branch_remote_tracking_ref_name(name.as_ref(), gix::remote::Direction::Fetch);
        let Some(Ok(upstream)) = upstream else {
            return Ok((Sync::NotPushed, remote));
        };
        let Some(mut upstream) = self.repo.try_find_reference(upstream.as_ref())? else {
            return Ok((Sync::NotPushed, remote));
        };
        let upstream = upstream.peel_to_id()?.detach();
        let count = |from: gix::ObjectId, without: gix::ObjectId| -> Result<usize> {
            Ok(self.repo.rev_walk([from]).with_hidden([without]).all()?.count())
        };
        let (ahead, behind) = (count(head, upstream)?, count(upstream, head)?);
        let sync = match (ahead, behind) {
            (0, 0) => Sync::Synced,
            (ahead, 0) => Sync::Ahead(ahead),
            (0, behind) => Sync::Behind(behind),
            (ahead, behind) => Sync::Diverged { ahead, behind },
        };
        Ok((sync, remote))
    }

    /// The name of the current branch's upstream, to merge after a fetch.
    pub fn upstream(&self) -> Result<Option<String>> {
        let Some(name) = self.repo.head_name()? else {
            return Ok(None);
        };
        let upstream = self
            .repo
            .branch_remote_tracking_ref_name(name.as_ref(), gix::remote::Direction::Fetch);
        Ok(upstream
            .and_then(|upstream| upstream.ok())
            .map(|upstream| upstream.shorten().to_string()))
    }

    /// The files a merge stopped on, with what each side holds.
    pub fn conflicts(&self) -> Result<Vec<Conflict>> {
        let index = self.repo.index_or_empty()?;
        let mut found: Vec<Conflict> = Vec::new();
        for entry in index.entries() {
            let stage = entry.stage();
            if stage == Stage::Unconflicted {
                continue;
            }
            let rela = entry.path(&index).to_str_lossy().into_owned();
            let name = rela.strip_prefix(&self.scope);
            let conflict = match found.last_mut() {
                Some(last) if last.name == name.unwrap_or(&rela) => last,
                _ => {
                    found.push(Conflict {
                        path: self.book.join(name.unwrap_or(&rela)),
                        name: name.unwrap_or(&rela).to_owned(),
                        base: None,
                        theirs: None,
                        in_book: name.is_some(),
                        ours_exists: false,
                    });
                    let at = found.len() - 1;
                    &mut found[at]
                }
            };
            let text =
                || -> Option<String> { String::from_utf8(self.repo.find_blob(entry.id).ok()?.data.clone()).ok() };
            match stage {
                Stage::Base => conflict.base = text(),
                Stage::Ours => conflict.ours_exists = true,
                Stage::Theirs => conflict.theirs = text(),
                Stage::Unconflicted => {}
            }
        }
        Ok(found)
    }

    /// Everything the views show.
    pub fn overview(&self, log_limit: usize) -> Result<Overview> {
        let head_name = self.repo.head_name()?;
        let detached = head_name.is_none();
        let branch = match &head_name {
            Some(name) => name.shorten().to_str_lossy().into_owned(),
            None => (self.head_id().map(|id| id.to_hex_with_len(7).to_string())).unwrap_or_default(),
        };
        let (sync, remote) = self.sync()?;
        let merging = (!self.merge_heads().is_empty()).then(|| {
            let message = fs::read_to_string(self.repo.path().join("MERGE_MSG")).unwrap_or_default();
            (message.lines())
                .filter(|line| !line.starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_owned()
        });
        Ok(Overview {
            branch,
            detached,
            branches: self.branches()?,
            changes: self.changes()?,
            log: self.log(log_limit)?,
            sync,
            merging,
            busy: self.busy(),
            has_identity: self.has_identity(),
            signs: self.signs(),
            remote,
        })
    }
}

fn by_time() -> gix::revision::walk::Sorting {
    gix::revision::walk::Sorting::ByCommitTime(Default::default())
}

fn branch_ref(name: &str) -> Result<FullName> {
    FullName::try_from(format!("refs/heads/{name}")).map_err(|_| Problem::BadName(name.to_owned()))
}

fn entry_kind(mode: Mode) -> Result<EntryKind> {
    let unknown = || Problem::Other(crate::t!(Key::UnknownIndexEntry).to_owned());
    Ok(mode.to_tree_entry_mode().ok_or_else(unknown)?.kind())
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &fs::Metadata) -> bool {
    false
}

/// The sync service a folder is in, whose copying between machines can damage a repo's folder.
pub fn cloud_service(path: &Path) -> Option<&'static str> {
    const SERVICES: [(&str, &str); 5] = [
        ("Mobile Documents", "iCloud Drive"),
        ("Dropbox", "Dropbox"),
        ("Google Drive", "Google Drive"),
        ("GoogleDrive", "Google Drive"),
        ("OneDrive", "OneDrive"),
    ];
    path.components().find_map(|part| {
        let part = part.as_os_str().to_string_lossy();
        (SERVICES.iter())
            .find(|(folder, _)| part == *folder || part.starts_with(&format!("{folder}-")))
            .map(|(_, service)| *service)
    })
}

#[cfg(test)]
#[path = "repo_tests.rs"]
mod tests;
