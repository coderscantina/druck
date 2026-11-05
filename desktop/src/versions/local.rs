//! The automatic tier: when local history keeps what each file holds. [`super::history`] is the store, and this decides
//! when, off the main thread: at each pause in typing, before a labelled moment, and around changes from outside.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{App, Context, Window};

use super::history::{History, now_ms};
use super::pacing::Due;
use super::strings::Key;
use super::{Current, REFRESH_PAUSE, Versions};
use crate::persist::FileChange;
use crate::persist::sync::ChangeKind;
use crate::session::{ChapterId, Kind, Session};

/// Records what every file of the document holds now in its local history, with a label: before an agent's turn, and
/// before anything else that may change the text. A file that then does not change adds no mark to its timeline. The
/// work happens off the main thread.
pub fn label_history(session: &Session, label: &str, cx: &App) {
    let Some(store) = cx.try_global::<Current>().and_then(|current| current.store.clone()) else {
        return;
    };
    let files = files_with_text(session);
    let label = label.to_owned();
    cx.background_spawn(async move { keep_labelled(&store, &files, &label) })
        .detach();
}

fn files_with_text(session: &Session) -> Vec<(PathBuf, String)> {
    (session.files().into_iter())
        .filter_map(|path| session.file_text(&path).map(|text| (path, text)))
        .collect()
}

fn keep_labelled(store: &History, files: &[(PathBuf, String)], label: &str) {
    let now = now_ms();
    for (path, text) in files {
        if let Err(error) = store.snapshot(path, text, Some(label), now) {
            eprintln!("cannot keep local history of {}: {error}", path.display());
        }
    }
}

impl Versions {
    /// At opening: finds history that moved with the book, says so, and thins what is old.
    pub(super) fn look_after_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let files = self.session(cx).files();
        let work = cx.background_spawn(async move {
            let followed = store.follow(&files);
            store.thin_daily(now_ms());
            followed
        });
        cx.spawn_in(window, async move |view, cx| {
            let followed = work.await;
            view.update_in(cx, |view, window, cx| {
                let left = followed.unplaced.len();
                view.unplaced = followed.unplaced.into_iter().collect();
                if let Some((from, _)) = followed.moved.first() {
                    let text = crate::t!(Key::HistoryFollowed, from = from.display().to_string());
                    view.show_note(text, window, cx);
                } else if left > 0 {
                    view.show_note(crate::t!(Key::HistoryLeft, count = left), window, cx);
                }
                view.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// A key was typed. Costs a lookup of the file in a small map.
    pub(super) fn edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let path = session.active_chapter().path();
        let now = Instant::now();
        if self.pacing.edited(path, now) {
            // Before the first edit this run, keep what the file held, or the timeline would start after it.
            if let (Some(store), Some(known)) = (self.store.clone(), session.file_base(path)) {
                let path = path.to_owned();
                cx.background_spawn(async move {
                    if let Err(error) = store.snapshot(&path, &known.base, None, now_ms()) {
                        eprintln!("cannot keep local history of {}: {error}", path.display());
                    }
                })
                .detach();
            }
        }
        self.start_pacing(window, cx);
        if self.awaiting_merge {
            self.conflicts_settled(cx);
        }
        if self.panel.read(cx).is_open() {
            self.refresh_pause = cx.spawn(async move |view, cx| {
                cx.background_executor().timer(REFRESH_PAUSE).await;
                view.update(cx, |view, cx| view.refresh(cx)).ok();
            });
        }
    }

    fn start_pacing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pacing_running {
            return;
        }
        self.pacing_running = true;
        cx.spawn_in(window, async move |view, cx| {
            loop {
                let wait = view.update(cx, |view, cx| match view.pacing.next(Instant::now()) {
                    Due::Idle => {
                        view.pacing_running = false;
                        None
                    }
                    Due::Wait(wait) => Some(wait),
                    Due::Take(paths) => {
                        view.keep(&paths, cx);
                        Some(Duration::ZERO)
                    }
                });
                match wait {
                    Ok(Some(wait)) => cx.background_executor().timer(wait).await,
                    _ => return,
                }
            }
        })
        .detach();
    }

    /// Stores what these files hold now, off the main thread.
    fn keep(&self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let session = self.session(cx);
        let files: Vec<(PathBuf, String)> = (paths.iter())
            .filter_map(|path| session.file_text(path).map(|text| (path.clone(), text)))
            .collect();
        cx.background_spawn(async move {
            let now = now_ms();
            for (path, text) in files {
                if let Err(error) = store.snapshot(&path, &text, None, now) {
                    eprintln!("cannot keep local history of {}: {error}", path.display());
                }
            }
        })
        .detach();
    }

    /// The app or the document is about to go: what waits for a pause is stored now.
    pub(super) fn keep_waiting(&mut self, cx: &App) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let session = self.session(cx);
        let now = now_ms();
        for path in self.pacing.drain() {
            if let Some(text) = session.file_text(&path) {
                store.snapshot(&path, &text, None, now).ok();
            }
        }
    }

    /// Labels the current state of every file with text.
    pub(super) fn label_all(&self, label: &str, cx: &mut Context<Self>) {
        label_history(self.session(cx), label, cx);
    }

    /// A chapter's file changed name: its log goes along.
    pub(super) fn session_changed(&mut self, cx: &mut Context<Self>) {
        let now = self.chapter_paths(cx);
        if let Some(store) = self.store.clone() {
            let renamed: Vec<(PathBuf, PathBuf)> = (now.iter())
                .filter_map(|(id, path)| {
                    let before = self.paths.get(id)?;
                    (before != path).then(|| (before.clone(), path.clone()))
                })
                .collect();
            if !renamed.is_empty() {
                cx.background_spawn(async move {
                    for (from, to) in renamed {
                        store.moved(&from, &to).ok();
                    }
                })
                .detach();
            }
        }
        self.paths = now;
        self.timeline.update(cx, |timeline, cx| timeline.follow_active(cx));
    }

    pub(super) fn chapter_paths(&self, cx: &App) -> HashMap<ChapterId, PathBuf> {
        let session = self.session(cx);
        if matches!(session.kind(), Kind::Single { .. }) {
            return HashMap::new();
        }
        (session.files().into_iter())
            .filter_map(|path| Some((session.chapter_of_file(&path)?, path)))
            .collect()
    }

    /// A change from outside: what the file held before it goes into local history, with a label, and so does the result.
    pub(super) fn outside_change(&mut self, change: &FileChange, cx: &mut Context<Self>) {
        self.refresh(cx);
        if self.quiet
            || !matches!(
                change.kind,
                ChangeKind::Reloaded | ChangeKind::Merged | ChangeKind::Conflict
            )
        {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        let (path, base) = (change.path.clone(), change.base.clone());
        let after = self.session(cx).file_text(&path);
        let label = crate::t!(Key::BeforeOutside);
        self.pacing.taken(&path, Instant::now());
        cx.background_spawn(async move {
            let now = now_ms();
            store.snapshot(&path, &base, Some(label), now).ok();
            if let Some(after) = after {
                store.snapshot(&path, &after, None, now + 1).ok();
            }
        })
        .detach();
    }
}
