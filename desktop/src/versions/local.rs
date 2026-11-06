//! The automatic tier: when local history keeps what each file holds. [`super::history`] is the store, and this decides
//! when, off the main thread: at each pause in typing, before a labelled moment, and around changes from outside.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{App, Context, WeakEntity, Window};

use super::history::{History, Label, Moment, Orphan, now_ms};
use super::pacing::{Due, Pacing};
use super::strings::Key;
use super::{REFRESH_PAUSE, Versions};
use crate::persist::FileChange;
use crate::persist::sync::{ChangeKind, Problem};
use crate::session::{ChapterId, Kind, Session};
use crate::workspace::Current;

/// Local history's state in the view: the store, when snapshots are due, and which file had which path.
pub(super) struct LocalHistory {
    pub store: Option<Arc<History>>,
    pub pacing: Pacing,
    pub pacing_running: bool,
    /// Each chapter's file when last looked at, to tell a rename.
    pub paths: HashMap<ChapterId, PathBuf>,
    /// The file names that may be another file's lost history, by the file with no history.
    pub unplaced: HashMap<PathBuf, Orphan>,
}

impl LocalHistory {
    pub(super) fn new(store: Option<Arc<History>>) -> Self {
        Self {
            store,
            pacing: Pacing::default(),
            pacing_running: false,
            paths: HashMap::new(),
            unplaced: HashMap::new(),
        }
    }
}

/// Records what every file of the document holds now in its local history, with a label: before an agent's turn, and
/// before anything else that may change the text. A file that then does not change adds no mark to its timeline. The
/// work happens off the main thread.
pub fn label_history(session: &Session, label: impl Into<Label>, cx: &App) {
    let Some(view) = Current::<Versions>::view(cx) else {
        return;
    };
    let Some(store) = view.read(cx).history.store.clone() else {
        return;
    };
    store_off_thread(
        store,
        files_with_text(session),
        Some(label.into()),
        view.downgrade(),
        cx,
    );
}

fn files_with_text(session: &Session) -> Vec<(PathBuf, String)> {
    (session.files().into_iter())
        .filter_map(|path| session.file_text(&path).map(|text| (path, text)))
        .collect()
}

/// Keeps what `files` hold in local history, off the main thread, and says at the window's foot when a file's history
/// cannot be kept, until it can again.
fn store_off_thread(
    store: Arc<History>,
    files: Vec<(PathBuf, String)>,
    label: Option<Label>,
    view: WeakEntity<Versions>,
    cx: &App,
) {
    let kept = cx.background_spawn(async move {
        let now = now_ms();
        (files.into_iter())
            .map(|(path, text)| {
                let result = store.snapshot(&path, &text, label.as_ref(), now);
                (path, result.err().map(|error| error.to_string()))
            })
            .collect::<Vec<_>>()
    });
    cx.spawn(async move |cx| {
        let kept = kept.await;
        view.update(cx, |view, cx| {
            view.persist.update(cx, |persist, cx| {
                for (path, error) in kept {
                    persist.report(&path, Problem::History, error, cx);
                }
            });
        })
        .ok();
    })
    .detach();
}

impl Versions {
    /// At opening: finds history that moved with the book, says so, and thins what is old.
    pub(super) fn look_after_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.history.store.clone() else {
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
                view.history.unplaced = followed.unplaced.into_iter().collect();
                if let Some((from, _)) = followed.moved.first() {
                    let text = crate::t!(Key::HistoryFollowed, from = from.display().to_string());
                    view.show_note(text, window, cx);
                } else if left > 0 {
                    let text = match left {
                        1 => crate::t!(Key::OneHistoryLeft).to_owned(),
                        left => crate::t!(Key::HistoryLeft, count = left),
                    };
                    view.show_note(text, window, cx);
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
        if self.history.pacing.edited(path, now) {
            // Before the first edit this run, keep what the file held, or the timeline would start after it.
            if let (Some(store), Some(known)) = (self.history.store.clone(), session.file_base(path)) {
                let files = vec![(path.to_owned(), known.base.to_string())];
                store_off_thread(store, files, None, cx.entity().downgrade(), cx);
            }
        }
        self.start_pacing(window, cx);
        if self.merge.is_awaiting() {
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
        if self.history.pacing_running {
            return;
        }
        self.history.pacing_running = true;
        cx.spawn_in(window, async move |view, cx| {
            loop {
                let wait = view.update(cx, |view, cx| match view.history.pacing.next(Instant::now()) {
                    Due::Idle => {
                        view.history.pacing_running = false;
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
        let Some(store) = self.history.store.clone() else {
            return;
        };
        let session = self.session(cx);
        let files: Vec<(PathBuf, String)> = (paths.iter())
            .filter_map(|path| session.file_text(path).map(|text| (path.clone(), text)))
            .collect();
        store_off_thread(store, files, None, cx.entity().downgrade(), cx);
    }

    /// The app or the document is about to go: what waits for a pause is stored now.
    pub(super) fn keep_waiting(&mut self, cx: &App) {
        let Some(store) = self.history.store.clone() else {
            return;
        };
        let session = self.session(cx);
        let now = now_ms();
        for path in self.history.pacing.drain() {
            if let Some(text) = session.file_text(&path) {
                store.snapshot(&path, &text, None, now).ok();
            }
        }
    }

    /// Labels the current state of every file with text.
    pub(super) fn label_all(&self, label: Moment, cx: &mut Context<Self>) {
        label_history(self.session(cx), label, cx);
    }

    /// A chapter's file changed name: its log goes along.
    pub(super) fn session_changed(&mut self, cx: &mut Context<Self>) {
        let now = self.chapter_paths(cx);
        if let Some(store) = self.history.store.clone() {
            let renamed: Vec<(PathBuf, PathBuf)> = (now.iter())
                .filter_map(|(id, path)| {
                    let before = self.history.paths.get(id)?;
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
        self.history.paths = now;
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
        let Some(store) = self.history.store.clone() else {
            return;
        };
        let (path, base) = (change.path.clone(), change.base.clone());
        let after = self.session(cx).file_text(&path);
        let label = Label::from(Moment::Outside);
        self.history.pacing.taken(&path, Instant::now());
        cx.background_spawn(async move {
            let now = now_ms();
            store.snapshot(&path, &base, Some(&label), now).ok();
            if let Some(after) = after {
                store.snapshot(&path, &after, None, now + 1).ok();
            }
        })
        .detach();
    }
}
