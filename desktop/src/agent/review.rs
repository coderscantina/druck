//! What the agent does with changes and notes: where every hunk stands in the text now, what the galley shows of it, and
//! what Accept, Reject, Try again, Apply, Reply, and Dismiss do. Nothing stays in the text's way: a reject is one undo
//! step that merges with what the author typed since, and a hunk whose changed words the author rewrote is theirs and
//! leaves review.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui::{App, Context, Entity, Window};
use serde_json::Value;

use super::changes::{FileChanges, Presence};
use super::strings::{self, Key};
use super::{AgentView, Standing, Unreviewed, file_in_session, relative};
use crate::editor::SaveRequested;
use crate::editor::review::{Advice, NoteAction, Passage, ReviewAction};
use crate::persist::merge::edits_between;
use crate::persist::{FileChange, PersistView, atomic};
use crate::session::Session as Document;
use crate::source::{Edit, EditKind};

/// How far before a turn began a new file's clock may stand, for file systems with coarse times.
const CLOCK_SLACK: Duration = Duration::from_secs(2);

/// A file's text as the author has it: the session's buffers if it has them, otherwise the disk.
fn current_text(session: &Document, path: &Path) -> Option<String> {
    session.file_text(path).or_else(|| std::fs::read_to_string(path).ok())
}

impl AgentView {
    // Taking changes in

    /// Takes in a file that changed on disk during a turn. The buffer has it already, or Persistence asks about it.
    pub(super) fn file_changed(
        &mut self,
        _: &Entity<PersistView>,
        change: &FileChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(changes) = self.collecting.and_then(|turn| self.conversation.changes_mut(turn)) else {
            return;
        };
        let name = relative(&self.base, &change.path);
        match &change.theirs {
            Some(theirs) => {
                let news = changes.observe(change.path.clone(), change.chapter, change.base.clone(), theirs.clone());
                if news && change.kind == crate::persist::sync::ChangeKind::Conflict {
                    self.conversation.note(strings::merge_conflict(&name));
                }
            }
            None => self.conversation.note(strings::deleted_file(&name)),
        }
        self.take_in_new_files(cx);
        self.refresh_changes(window, cx);
    }

    /// Looks for what a turn made that Persistence has no change for: new files, and the manifest, which it only reads.
    pub(super) fn take_in_new_files(&mut self, cx: &mut Context<Self>) {
        let Some(turn) = self.collecting else {
            return;
        };
        let session = self.editor.read(cx).session();
        let Some(start) = &mut self.turn_start else {
            return;
        };
        let Some(changes) = self.conversation.changes_mut(turn) else {
            return;
        };
        if let Some(path) = session.manifest_path()
            && let Ok(now) = std::fs::read_to_string(&path)
            && start.manifest.as_deref() != Some(now.as_str())
        {
            let before: Arc<str> = start.manifest.take().unwrap_or_default().into();
            changes.observe(path, None, before, now.as_str().into());
            start.manifest = Some(now);
        }
        let mut new_files: Vec<PathBuf> = (session.files().into_iter())
            .filter(|file| !start.files.contains(file) && made_since(file, start.at))
            .collect();
        new_files.extend(
            (changes.created.iter())
                .map(|path| file_in_session(session, path).unwrap_or_else(|| path.clone()))
                .filter(|path| !start.files.contains(path)),
        );
        for file in new_files {
            if let Ok(text) = std::fs::read_to_string(&file) {
                changes.observe_created(file.clone(), session.chapter_of_file(&file), text.into());
                start.files.insert(file);
            }
        }
        cx.notify();
    }

    // Where everything stands

    /// Works out where every hunk is in the text now, for the galley, the foot, the warnings, and the turn's rows.
    pub(super) fn refresh_changes(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            self.unreviewed.clear();
            self.editor.update(cx, |editor, cx| {
                editor.set_review(Vec::new(), cx);
                editor.set_advice(Vec::new(), cx);
            });
            cx.notify();
            return;
        }
        let editor = self.editor.read(cx);
        let session = editor.session();
        let active = session.active();
        let mut standing = std::collections::HashMap::new();
        let mut passages: Vec<Passage> = Vec::new();
        let mut unreviewed: Vec<Unreviewed> = Vec::new();
        for turn in 0..=self.conversation.current_turn().unwrap_or(0) {
            let Some(changes) = self.conversation.changes_mut(turn) else {
                continue;
            };
            for file in &mut changes.files {
                // The chapter's path now, since a rename keeps the chapter.
                file.chapter = file.chapter.filter(|id| session.chapter(*id).is_some());
                file.chapter = file.chapter.or_else(|| session.chapter_of_file(&file.path));
                if let Some(chapter) = file.chapter.and_then(|id| session.chapter(id)) {
                    file.path = chapter.path().to_owned();
                }
                let text = current_text(session, &file.path);
                let mut count = 0;
                for tracked in &mut file.hunks {
                    let state = match &text {
                        None => Standing::NotOpen,
                        Some(text) => match tracked.presence(text) {
                            Presence::Applied(place) => {
                                count += 1;
                                if let Some((id, at)) = session.place_in_file(&file.path, place.range.start)
                                    && id == active
                                {
                                    let shift = at as isize - place.range.start as isize;
                                    passages.push(tracked.passage(text, &place, shift));
                                }
                                Standing::Applied
                            }
                            Presence::Absent(_) => Standing::Reverted,
                            Presence::Moved => Standing::Moved,
                        },
                    };
                    standing.insert(tracked.id, state);
                }
                if count > 0 {
                    add_unreviewed(&mut unreviewed, session, file, count);
                }
            }
        }
        passages.sort_by_key(|passage| passage.end);
        self.standing = standing;
        self.unreviewed = unreviewed;
        let advice = self.advice_for_active(cx);
        self.editor.update(cx, |editor, cx| {
            editor.set_review(passages, cx);
            editor.set_advice(advice, cx);
        });
        cx.notify();
    }

    /// Typing moves the text under what review shows, and the galley keeps up by itself. When the text rests, this looks
    /// again for what the typing changed.
    pub(super) fn text_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.standing.is_empty() && self.notes.all().is_empty() {
            return;
        }
        self.settle = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(super::SETTLE).await;
            view.update_in(cx, |view, window, cx| view.refresh_changes(window, cx))
                .ok();
        });
    }

    /// How many changes the open chapter has and how many stand elsewhere.
    pub(super) fn review_counts(&self, cx: &App) -> (usize, usize) {
        let active = self.editor.read(cx).session().active();
        let here = (self.unreviewed.iter())
            .filter(|entry| entry.chapter == Some(active))
            .map(|entry| entry.count)
            .sum();
        let all: usize = self.unreviewed.iter().map(|entry| entry.count).sum();
        (here, all - here)
    }

    /// Whether another chapter has changes to go to.
    pub(super) fn has_next_change(&self, cx: &App) -> bool {
        let active = self.editor.read(cx).session().active();
        (self.unreviewed.iter()).any(|entry| entry.chapter.is_some_and(|chapter| chapter != active))
    }

    pub(super) fn is_working(&self) -> bool {
        !self.conversation.is_idle()
    }

    pub(super) fn is_looking_for_cli(&self) -> bool {
        self.cli.is_none()
    }

    pub(super) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn chat_is_on_screen(&self) -> bool {
        self.enabled && self.chat_open && self.mode == crate::workspace::Mode::Write
    }

    pub(super) fn stop_turn(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
    }

    // The buttons

    pub(super) fn review_clicked(
        &mut self,
        id: u64,
        action: ReviewAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ReviewAction::Accept => self.accept(id, window, cx),
            ReviewAction::Reject => self.reject(id, window, cx),
            ReviewAction::TryAgain => self.try_again(id, window, cx),
        }
    }

    fn file_of(&mut self, id: u64) -> Option<&mut FileChanges> {
        let last = self.conversation.current_turn()?;
        let turn = (0..=last).rev().find(|turn| {
            (self.conversation.changes(*turn)).is_some_and(|changes| changes.files.iter().any(|file| file.holds(id)))
        })?;
        let changes = self.conversation.changes_mut(turn)?;
        changes.files.iter_mut().find(|file| file.holds(id))
    }

    pub(super) fn accept(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(file) = self.file_of(id) {
            file.accept(id);
        }
        self.refresh_changes(window, cx);
    }

    /// Takes a hunk back as one undo step in the editor: a merge with the author's text, so nothing else in it moves.
    pub(super) fn reject(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.editor.read(cx).session();
        let session_text = |path: &Path| current_text(session, path);
        let Some(file) = self.file_of(id) else {
            return;
        };
        let path = file.path.clone();
        let created = file.created;
        let Some(mine) = session_text(&path) else {
            return;
        };
        let reverted = (file.hunks.iter_mut())
            .find(|tracked| tracked.id == id)
            .and_then(|tracked| tracked.revert(&mine));
        let Some(reverted) = reverted else {
            self.conversation.note(crate::t!(Key::HunkConflict).to_owned());
            cx.notify();
            return;
        };
        if created && reverted.is_empty() {
            self.remove_created_file(&path, window, cx);
        } else {
            self.write_text(&path, &mine, &reverted, window, cx);
        }
        self.after_rejecting(window, cx);
    }

    /// Takes every change under review back. With nothing typed since, each file returns to its exact bytes, and a file
    /// the agent made goes away.
    pub(super) fn reject_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut gone = Vec::new();
        for turn in (0..=self.conversation.current_turn().unwrap_or(0)).rev() {
            let files = self.conversation.changes(turn).map_or(0, |changes| changes.files.len());
            for at in 0..files {
                let session = self.editor.read(cx).session();
                let Some(file) = (self.conversation.changes_mut(turn)).and_then(|changes| changes.files.get_mut(at))
                else {
                    continue;
                };
                let Some(mine) = current_text(session, &file.path) else {
                    continue;
                };
                let new = file.reject_all(&mine);
                let (path, created) = (file.path.clone(), file.created);
                if created && new.is_empty() {
                    gone.push(path);
                } else if new != mine {
                    self.write_text(&path, &mine, &new, window, cx);
                }
            }
        }
        for path in gone {
            self.remove_created_file(&path, window, cx);
        }
        self.after_rejecting(window, cx);
    }

    pub(super) fn accept_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for turn in 0..=self.conversation.current_turn().unwrap_or(0) {
            if let Some(changes) = self.conversation.changes_mut(turn) {
                changes.files.iter_mut().for_each(FileChanges::accept_all);
            }
        }
        self.refresh_changes(window, cx);
    }

    /// The rejected text goes to disk now, so what is on disk is never older than what the foot says.
    fn after_rejecting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |_, cx| cx.emit(SaveRequested));
        self.persist.update(cx, |persist, cx| persist.scan_now(window, cx));
        self.refresh_changes(window, cx);
    }

    /// Gives a file new text: its buffers as one undo step, or the disk for a file the session has no buffer of.
    fn write_text(&mut self, path: &Path, mine: &str, new: &str, window: &mut Window, cx: &mut Context<Self>) {
        let has_buffer = self.editor.read(cx).session().file_text(path).is_some();
        if !has_buffer {
            if let Err(error) = atomic::write(path, new.as_bytes()) {
                self.conversation.note(error.to_string());
            }
            return;
        }
        let edits = edits_between(mine, new);
        let result = self.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    session.disown_versions(path);
                    session.apply_to_file(path, &edits, EditKind::Discrete)
                },
                window,
                cx,
            )
        });
        if let Err(error) = result {
            self.conversation.note(error.to_string());
        }
    }

    /// Everything of a new file was rejected, so it goes: first from the manifest, if a change of this review put it
    /// there, then from the disk. Nothing of the author's is in it, since it has no text left.
    fn remove_created_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let listed = relative(&self.base, path);
        let manifest = self.editor.read(cx).session().manifest_path();
        if let Some(manifest) = manifest
            && let Ok(mine) = std::fs::read_to_string(&manifest)
        {
            let mut text = mine.clone();
            for turn in (0..=self.conversation.current_turn().unwrap_or(0)).rev() {
                let Some(changes) = self.conversation.changes_mut(turn) else {
                    continue;
                };
                let Some(file) = changes.files.iter_mut().find(|file| file.path == manifest) else {
                    continue;
                };
                for tracked in file.hunks.iter_mut().rev() {
                    if tracked.new_core().contains(&listed)
                        && let Some(reverted) = tracked.revert(&text)
                    {
                        text = reverted;
                    }
                }
            }
            if text != mine
                && let Err(error) = atomic::write(&manifest, text.as_bytes())
            {
                self.conversation.note(error.to_string());
            }
        }
        if let Err(error) = std::fs::remove_file(path) {
            self.conversation.note(error.to_string());
        }
        self.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    session.read_manifest();
                    session.mark_clean(path);
                    session.file_removed(path);
                },
                window,
                cx,
            );
        });
    }

    /// Rejects a passage and asks the agent for another go at it.
    fn try_again(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.conversation.is_idle() {
            self.conversation.note(crate::t!(Key::TurnBusy).to_owned());
            self.chat_open = true;
            cx.notify();
            return;
        }
        let base = self.base.clone();
        let Some(file) = self.file_of(id) else {
            return;
        };
        let path = relative(&base, &file.path);
        let Some((old, new)) = (file.hunks.iter())
            .find(|tracked| tracked.id == id)
            .map(|tracked| (tracked.old_core(), tracked.new_core()))
        else {
            return;
        };
        self.reject(id, window, cx);
        let shown = crate::t!(Key::TryAgainShown, place = path.clone());
        self.start_turn(shown, strings::try_again(&path, &old, &new), None, window, cx);
    }

    /// Opens the chapter a hunk is in and puts the caret on it.
    pub(super) fn reveal(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.editor.read(cx).session();
        let session_text = |path: &Path| session.file_text(path);
        let Some(file) = self.file_of(id) else {
            return;
        };
        let path = file.path.clone();
        let Some(text) = session_text(&path) else {
            return;
        };
        let at = match (file.hunks.iter_mut())
            .find(|tracked| tracked.id == id)
            .map(|tracked| tracked.presence(&text))
        {
            Some(Presence::Applied(place) | Presence::Absent(place)) => place.range.start,
            _ => return,
        };
        let Some((chapter, at)) = self.editor.read(cx).session().place_in_file(&path, at) else {
            return;
        };
        self.editor
            .update(cx, |editor, cx| editor.show_place(chapter, at, window, cx));
        self.refresh_changes(window, cx);
    }

    /// Goes to the next chapter with changes to review, at its first.
    pub(super) fn next_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.editor.read(cx).session().active();
        let Some(next) = (self.unreviewed.iter())
            .filter_map(|entry| entry.chapter)
            .find(|chapter| *chapter != active)
        else {
            return;
        };
        let first = (0..=self.conversation.current_turn().unwrap_or(0)).find_map(|turn| {
            let changes = self.conversation.changes(turn)?;
            let file = changes.files.iter().find(|file| file.chapter == Some(next))?;
            file.hunks
                .iter()
                .find(|tracked| self.standing.get(&tracked.id) == Some(&Standing::Applied))
                .map(|tracked| tracked.id)
        });
        match first {
            Some(id) => self.reveal(id, window, cx),
            None => self
                .editor
                .update(cx, |editor, cx| editor.show_chapter(next, window, cx)),
        }
    }

    // Notes

    /// The notes of the open chapter, each found again by its quote.
    fn advice_for_active(&self, cx: &App) -> Vec<Advice> {
        let editor = self.editor.read(cx);
        let session = editor.session();
        let chapter = relative(&self.base, session.active_chapter().path());
        let text = editor.text();
        (self.notes.all().iter())
            .filter(|note| note.chapter == chapter)
            .map(|note| Advice {
                id: note.id,
                title: note.title.clone(),
                text: note.text.clone(),
                suggestion: note.suggestion.clone(),
                anchor: note.anchor(text),
            })
            .collect()
    }

    /// The note tool took a note, whose call is in the CLI's stream.
    pub(super) fn note_left(&mut self, input: &Value, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.notes.add(input) {
            self.conversation.note(error.to_string());
        }
        self.refresh_changes(window, cx);
    }

    pub(super) fn note_clicked(&mut self, id: u64, action: NoteAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            NoteAction::Apply => self.apply_note(id, window, cx),
            NoteAction::Reply => {
                self.replying = Some(id);
                self.open_chat(window, cx);
            }
            NoteAction::Dismiss => self.dismiss_note(id, window, cx),
        }
    }

    /// Writes a note's suggestion over its quote, as one undo step.
    fn apply_note(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.notes.get(id).cloned() else {
            return;
        };
        let Some(suggestion) = note.suggestion.clone() else {
            return;
        };
        let path = self.base.join(&note.chapter);
        let session = self.editor.read(cx).session();
        let path = file_in_session(session, &path).unwrap_or(path);
        let Some(text) = session.file_text(&path) else {
            return;
        };
        let Some(anchor) = note.anchor(&text) else {
            return;
        };
        let edits = vec![Edit::new(anchor, &suggestion)];
        let result = self.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| session.apply_to_file(&path, &edits, EditKind::Discrete),
                window,
                cx,
            )
        });
        match result {
            Ok(()) => self.dismiss_note(id, window, cx),
            Err(error) => self.conversation.note(error.to_string()),
        }
    }

    pub(super) fn dismiss_note(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.replying == Some(id) {
            self.replying = None;
        }
        if let Err(error) = self.notes.remove(id) {
            self.conversation.note(error.to_string());
        }
        self.refresh_changes(window, cx);
    }
}

/// Whether a file came into being after a turn began.
fn made_since(path: &Path, start: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.created())
        .is_ok_and(|made| made + CLOCK_SLACK >= start)
}

fn add_unreviewed(unreviewed: &mut Vec<Unreviewed>, session: &Document, file: &FileChanges, count: usize) {
    if let Some(entry) = unreviewed.iter_mut().find(|entry| entry.path == file.path) {
        entry.count += count;
        return;
    }
    let name = file.chapter.map_or_else(
        || file.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        |chapter| session.title_of(chapter),
    );
    unreviewed.push(Unreviewed {
        chapter: file.chapter,
        path: file.path.clone(),
        name,
        count,
    });
}
