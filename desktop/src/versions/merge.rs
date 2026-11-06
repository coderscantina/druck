//! Merging through `git`: running a merge or a pull, taking what Git stopped on to the conflict line, and the time after
//! it, while "See both"'s markers stand, until the commit that finishes it. [`PendingMerge`] holds where that stands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{App, Context, Window};

use super::git_cli::{Git, Merged};
use super::history::Moment;
use super::repo::Conflict;
use super::strings::Key;
use super::{Done, RepoState, Versions, describe, describe_refused, open_repo};
use crate::persist::merge::{Choice, edits_between, has_markers, keep_one};
use crate::persist::{GitMove, Source};
use crate::source::EditKind;

/// Where a merge Git stopped on stands: its conflicts, then "See both"'s markers, then the commit that finishes it.
#[derive(Default)]
pub(super) struct PendingMerge {
    /// A merge stopped on conflicts, and the card opens when the last one is settled.
    awaiting: bool,
    /// The file that still holds both versions of a conflict, which holds the card back, and its chapter's name.
    both_stand_in: Option<(PathBuf, String)>,
    /// The card opens once the repo has been read again, for the merge's message.
    card_after_reading: bool,
}

impl PendingMerge {
    /// Git stopped on conflicts, or the app opened in the middle of such a merge.
    pub(super) fn stopped(&mut self) {
        self.awaiting = true;
    }

    /// A commit finished the merge, or it was called off.
    pub(super) fn over(&mut self) {
        self.awaiting = false;
        self.both_stand_in = None;
    }

    pub(super) fn is_awaiting(&self) -> bool {
        self.awaiting
    }

    pub(super) fn both_stand_in(&self) -> Option<&(PathBuf, String)> {
        self.both_stand_in.as_ref()
    }

    /// Takes in which file still has "See both"'s markers, once no conflict is open. With none left the merge waits only
    /// for its commit, and the card opens. Returns whether what the foot says changed.
    pub(super) fn settle(&mut self, marked: Option<(PathBuf, String)>) -> bool {
        let changed = marked != self.both_stand_in;
        self.both_stand_in = marked;
        if self.both_stand_in.is_none() {
            self.awaiting = false;
            self.card_after_reading = true;
        }
        changed
    }

    /// Whether the card is to open now that the repo has been read again, which it does once.
    pub(super) fn take_card_after_reading(&mut self) -> bool {
        std::mem::take(&mut self.card_after_reading)
    }
}

/// Where a merge takes its other side from.
pub(super) enum MergeFrom {
    Branch(String),
    /// A fetch, then the branch's upstream.
    Upstream,
}

pub(super) enum MergeEnd {
    Done,
    /// And what was merged, which is what changed the same words.
    Conflicts(Vec<Conflict>, String),
    /// One file cannot be shown on the conflict line, so the merge was undone.
    Outside(String),
}

pub(super) fn merge_job(
    git: &Git,
    book: &Path,
    from: MergeFrom,
    takeable: &HashSet<PathBuf>,
) -> Result<MergeEnd, String> {
    let repo = open_repo(book).map_err(|problem| describe(&problem))?;
    let target = match from {
        MergeFrom::Branch(name) => name,
        MergeFrom::Upstream => {
            git.fetch(book)?;
            (repo.upstream().map_err(|problem| describe(&problem))?)
                .ok_or_else(|| crate::t!(Key::NoUpstream).to_owned())?
        }
    };
    match git.merge(book, repo.git_dir(), &target) {
        Ok(Merged::Done) => Ok(MergeEnd::Done),
        Ok(Merged::Conflicts) => {
            let conflicts =
                (open_repo(book).and_then(|repo| repo.conflicts())).map_err(|problem| describe(&problem))?;
            let stuck = conflicts.iter().find(|conflict| {
                !(conflict.in_book
                    && conflict.ours_exists
                    && conflict.theirs.is_some()
                    && takeable.contains(&conflict.path))
            });
            if let Some(stuck) = stuck {
                let name = stuck.name.clone();
                git.abort_merge(book)?;
                return Ok(MergeEnd::Outside(name));
            }
            Ok(MergeEnd::Conflicts(conflicts, target))
        }
        Err(refused) => Err(describe_refused(&refused)),
    }
}

impl Versions {
    pub(super) fn pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_merge(MergeFrom::Upstream, Moment::Pull, window, cx);
    }

    pub(super) fn merge_branch(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        self.run_merge(MergeFrom::Branch(branch.clone()), Moment::Merge(branch), window, cx);
    }

    /// Merges into the current branch through `git`. While it runs, the files' changes are not taken in, because a file
    /// `git` leaves in conflict holds marker lines for a moment. Then what overlaps goes to the existing conflict line.
    pub(super) fn run_merge(&mut self, from: MergeFrom, label: Moment, window: &mut Window, cx: &mut Context<Self>) {
        if self.working || self.git.is_none() {
            return;
        }
        self.after_settling(window, cx, move |view, window, cx| {
            let (Some(git), Some(book)) = (view.git.clone(), view.book(cx)) else {
                return;
            };
            view.label_all(label, cx);
            let session = view.session(cx);
            let takeable: HashSet<PathBuf> = (session.files().into_iter())
                .filter(|path| session.file_text(path).is_some() && session.chapter_of_file(path).is_some())
                .collect();
            let source = match &from {
                MergeFrom::Branch(branch) => Source::Git(GitMove::Merge, branch.clone()),
                MergeFrom::Upstream => Source::Git(GitMove::Pull, view.upstream_name()),
            };
            view.persist
                .update(cx, |persist, _| persist.hold_outside_changes(source));
            let job = move || merge_job(&git, &book, from, &takeable);
            let done: Done<MergeEnd> = Box::new(|view, result, window, cx| view.merge_finished(result, window, cx));
            view.work(job, done, window, cx);
        });
    }

    /// `git` is done moving the files. What it changed comes in as the repo's own move, which was labelled before it began.
    pub(super) fn release_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quiet = true;
        self.persist
            .update(cx, |persist, cx| persist.release_outside_changes(window, cx));
        self.quiet = false;
    }

    /// The branch a pull brings text from, as the remote names it.
    pub(super) fn upstream_name(&self) -> String {
        match &self.state {
            RepoState::Ready(overview) => match &overview.remote {
                Some(remote) => format!("{remote}/{}", overview.branch),
                None => overview.branch.clone(),
            },
            _ => String::new(),
        }
    }

    pub(super) fn merge_finished(
        &mut self,
        result: Result<MergeEnd, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.quiet = true;
        match result {
            Ok(MergeEnd::Done) => {}
            Ok(MergeEnd::Outside(file)) => self.error = Some(crate::t!(Key::MergeOutside, file = file)),
            Ok(MergeEnd::Conflicts(conflicts, target)) => {
                let persist = self.persist.clone();
                self.merge.stopped();
                persist.update(cx, |persist, _| persist.merging_from(target));
                for conflict in conflicts {
                    let base = conflict.base.unwrap_or_default();
                    let theirs = conflict.theirs.unwrap_or_default();
                    let taken = persist.update(cx, |persist, cx| {
                        persist.take_merge(&conflict.path, &base, &theirs, window, cx)
                    });
                    if let Err(error) = taken {
                        self.error = Some(error.to_string());
                    }
                }
            }
            Err(reason) => self.error = Some(reason),
        }
        self.release_files(window, cx);
        self.conflicts_settled(cx);
    }

    /// The last conflict of a merge was settled: the author commits it. "See both" leaves both versions in the text between
    /// marker comments, which is not settled until those are gone, so the foot says so and the card waits.
    pub(super) fn conflicts_settled(&mut self, cx: &mut Context<Self>) {
        if !self.merge.is_awaiting() || self.working || self.persist.read(cx).has_conflicts() {
            return;
        }
        let marked = self.chapter_with_markers(cx);
        if self.merge.settle(marked) {
            cx.notify();
        }
        if !self.merge.is_awaiting() {
            self.refresh(cx);
        }
    }

    /// The app opened in the middle of a merge whose "See both" markers stand, as a quit left it: the foot asks again.
    pub(super) fn resume_merge(&mut self, cx: &mut Context<Self>) {
        let merging = matches!(&self.state, RepoState::Ready(overview) if overview.merging.is_some());
        if merging && !self.merge.is_awaiting() && !self.working && self.chapter_with_markers(cx).is_some() {
            self.merge.stopped();
            self.conflicts_settled(cx);
        }
    }

    /// A file that still has "See both"'s marker comments, and its chapter's name.
    pub(super) fn chapter_with_markers(&self, cx: &App) -> Option<(PathBuf, String)> {
        let session = self.session(cx);
        let path = (session.files().into_iter())
            .find(|path| session.file_text(path).is_some_and(|text| has_markers(&text)))?;
        let name = session.chapter_of_file(&path).map_or_else(
            || path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            |chapter| session.title_of(chapter),
        );
        Some((path, name))
    }

    /// Settles the versions "See both" left in a file by one side, as one undo step, which takes the markers out too.
    pub(super) fn keep_one_side(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, _)) = self.merge.both_stand_in().cloned() else {
            return;
        };
        let result = self.parts.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    let text = session.file_text(&path).unwrap_or_default();
                    let edits = edits_between(&text, &keep_one(&text, choice));
                    session.apply_to_file(&path, &edits, EditKind::Discrete)
                },
                window,
                cx,
            )
        });
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
        self.conflicts_settled(cx);
    }

    pub(super) fn abort_merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(git), Some(book)) = (self.git.clone(), self.book(cx)) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        self.merge.over();
        let branch = match &self.state {
            RepoState::Ready(overview) => overview.branch.clone(),
            _ => String::new(),
        };
        let source = Source::Git(GitMove::Abort, branch);
        self.persist
            .update(cx, |persist, _| persist.hold_outside_changes(source));
        let job = move || git.abort_merge(&book);
        let done: Done<()> = Box::new(|view, result, window, cx| {
            view.error = result.err();
            view.release_files(window, cx);
        });
        self.work(job, done, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_waits_for_see_boths_markers_then_opens_the_card_once() {
        let mut merge = PendingMerge::default();
        merge.stopped();
        let marked = Some((PathBuf::from("manuscript/one.md"), "One".to_owned()));
        assert!(merge.settle(marked.clone()));
        assert!(merge.is_awaiting());
        assert!(!merge.settle(marked));
        assert!(!merge.take_card_after_reading());
        assert!(merge.settle(None));
        assert!(!merge.is_awaiting());
        assert!(merge.take_card_after_reading());
        assert!(!merge.take_card_after_reading());
    }
}
