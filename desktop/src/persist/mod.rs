//! Keeping the text safe while others write the same files: atomic autosave, a folder watcher, three-way merges, and a
//! recovery copy. The small view here runs it and shows the saving state at the window's foot when something is wrong. What it does is in the modules,
//! without gpui: [`atomic`], [`merge`], [`watch`], [`recovery`], and [`sync`].
//!
//! Other parts listen for [`FileChange`] on this view to see each file change that came from outside, and call
//! [`PersistView::scan_now`] to see all of them up to now.

pub mod atomic;
pub mod merge;
pub mod recovery;
mod strings;
pub mod sync;
pub mod watch;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Context, Entity, EventEmitter, FontWeight, Global, KeyBinding, MouseButton, SharedString,
    Subscription, Task, WeakEntity, Window, actions, div,
};

use self::atomic::Outcome;
use self::merge::Choice;
use self::recovery::{Copies, data_dir};
use self::strings::Key;
use self::sync::{ChangeKind, ConflictPassage, FileSync, Next, Pacer, Problem};
pub use self::sync::{Closing, FileChange, GitMove, Source};
use self::watch::{Ready, Signal, Waiting, Watcher};
use crate::a11y::Control as _;
use crate::editor::{EditorView, ExternalUndo, SaveRequested, SessionChanged, TextChanged};
use crate::inspector::{InspectorView, ThemeModel, ThemeSwitched};
use crate::session::disk::Outgoing;
use crate::session::{Kind, Session};
use crate::tokens::{self, Tokens};
use crate::workspace::{Parts, Slots};

actions!(persist, [KeepMine, TakeTheirs, SeeBoth]);

/// How often to look for a folder that went away, only while it is away.
const GONE_POLL: Duration = Duration::from_secs(2);
/// How soon to try again while an input method is composing or a write is running.
const BUSY_RETRY: Duration = Duration::from_millis(100);

/// The view the conflict keys go to.
struct Current {
    view: WeakEntity<PersistView>,
    window: AnyWindowHandle,
}

impl Global for Current {}

/// Binds the keys of the conflict prompt. They work wherever the focus is, as the prompt never takes it.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-cmd-m", KeepMine, None),
        KeyBinding::new("ctrl-cmd-t", TakeTheirs, None),
        KeyBinding::new("ctrl-cmd-b", SeeBoth, None),
    ]);
    cx.on_action(|_: &KeepMine, cx| choose(Choice::Mine, cx));
    cx.on_action(|_: &TakeTheirs, cx| choose(Choice::Theirs, cx));
    cx.on_action(|_: &SeeBoth, cx| choose(Choice::Both, cx));
}

fn choose(choice: Choice, cx: &mut App) {
    let Some(current) = cx.try_global::<Current>() else {
        return;
    };
    let (view, window) = (current.view.clone(), current.window);
    // An action runs inside its window's update, which cannot be entered again.
    cx.defer(move |cx| {
        window
            .update(cx, |_, window, cx| {
                view.update(cx, |view, cx| view.resolve_first(choice, window, cx)).ok();
            })
            .ok();
    });
}

/// Says on the saving-state line of the open document that something about `path` went wrong, or with `None` that it is
/// over, for parts of the app that hold no view of it.
pub fn report(path: &Path, kind: fn(String) -> Problem, error: Option<String>, cx: &mut App) {
    let Some(current) = cx.try_global::<Current>() else {
        return;
    };
    let view = current.view.clone();
    let path = path.to_owned();
    // A report may come from inside the window's own update, which cannot be entered again.
    cx.defer(move |cx| {
        view.update(cx, |view, cx| view.report(&path, kind, error, cx)).ok();
    });
}

/// Clears a file's recovery copy once nothing of it is unsaved, for code that made it clean without a write.
pub fn forget_copy(path: &Path, cx: &mut App) {
    let Some(current) = cx.try_global::<Current>() else {
        return;
    };
    let view = current.view.clone();
    let path = path.to_owned();
    cx.defer(move |cx| {
        view.update(cx, |view, cx| {
            let session = view.editor.read(cx).session();
            view.sync.forget_copy_if_clean(session, &path);
        })
        .ok();
    });
}

/// Mounts the saving state and starts everything behind it. One line in `app::mount`.
pub fn mount(parts: &Parts, slots: &mut Slots, window: &mut Window, cx: &mut App) -> Entity<PersistView> {
    let view = cx.new(|cx| PersistView::new(parts.editor.clone(), parts.inspector.clone(), window, cx));
    cx.set_global(Current {
        view: view.downgrade(),
        window: window.window_handle(),
    });
    slots.footer.push(view.clone().into());
    view
}

/// The theme the inspector has open: the one the document names, or what the author chose or forked.
fn open_theme(model: &Entity<ThemeModel>, cx: &App) -> Option<PathBuf> {
    model.read(cx).file().ok().map(|file| file.path().to_owned())
}

/// Writes everything now for a document that is about to go, and says what is left: see [`Closing`]. Without a document
/// there is nothing to write. The answer comes once any save that was running has landed.
pub fn settle(window: &mut Window, cx: &mut App) -> Option<futures::channel::oneshot::Receiver<Closing>> {
    let view = cx.try_global::<Current>()?.view.upgrade()?;
    Some(view.update(cx, |view, cx| view.settle(window, cx)))
}

/// Writes what a float's fields hold into the text, before a document closes.
pub fn commit_floats(window: &mut Window, cx: &mut App) {
    let Some(view) = cx.try_global::<Current>().and_then(|current| current.view.upgrade()) else {
        return;
    };
    let editor = view.read(cx).editor.clone();
    editor.update(cx, |editor, cx| editor.commit_floats(window, cx));
}

/// Clears what is kept for a document's unsaved text, when a person chose to discard it.
pub fn forget_unsaved(location: &Path) {
    if let Some(data) = data_dir() {
        Copies::new(&data, location).clear_all();
    }
}

/// The saving state at the foot of the window, and the owner of everything behind it. Nothing shows while all is well.
pub struct PersistView {
    editor: Entity<EditorView>,
    inspector: Entity<InspectorView>,
    sync: FileSync,
    pacer: Pacer,
    /// The timer behind [`Pacer`] is running.
    pacing: bool,
    /// Held while writing, so the blocking save on quit waits for one in the background.
    write_lock: Arc<Mutex<()>>,
    writing: Option<Task<()>>,
    /// Another save was asked for while one was running.
    write_again: bool,
    /// Callers of [`settle`] that wait for the running save to land first.
    settling: Vec<futures::channel::oneshot::Sender<Closing>>,
    waiting: Waiting,
    /// The channel the watcher reports to, kept to start it anew if the folder comes back.
    signals: futures::channel::mpsc::UnboundedSender<Signal>,
    watcher: Option<Watcher>,
    theme: Option<PathBuf>,
    note: Option<SharedString>,
    /// A kept copy the note is about, which Show in Finder shows.
    note_file: Option<PathBuf>,
    note_task: Task<()>,
    gone_task: Option<Task<()>>,
    /// A Git command is moving the files, and what they hold now is not for taking in yet.
    holding_outside: bool,
    /// The Git command that moves the files, which what comes in until it lets go is named after.
    git_move: Option<Source>,
    /// The writing partner's turn is running, so what comes in is named after it.
    agent_working: bool,
    /// What a merge stopped on is being merged in, which the conflict line names.
    merging: Option<String>,
    /// The first place the first conflict is about, as the conflict line shows it.
    passage: Option<ConflictPassage>,
    _incoming: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FileChange> for PersistView {}

/// A file that is no text of the book changed, which a layout that failed may have lacked.
pub struct ResourceChanged;

impl EventEmitter<ResourceChanged> for PersistView {}

/// A conflict key was pressed with no conflict open, for "See both"'s markers that a merge left standing.
pub struct ChoiceWithoutConflict(pub Choice);

impl EventEmitter<ChoiceWithoutConflict> for PersistView {}

impl PersistView {
    fn new(
        editor: Entity<EditorView>,
        inspector: Entity<InspectorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let location = editor.read(cx).session().location().to_owned();
        let copies = data_dir().map(|data| Copies::new(&data, &location));
        let model = inspector.read(cx).model().clone();
        let theme = open_theme(&model, cx);
        let (signals, mut receive) = futures::channel::mpsc::unbounded::<Signal>();
        let incoming = cx.spawn_in(window, async move |view, cx| {
            // Parked in `next` while nothing changes: no timer runs at idle.
            while let Some(signal) = receive.next().await {
                let mut signal = Some(signal);
                loop {
                    let mut wait = None;
                    let alive = view.update_in(cx, |view, window, cx| {
                        let now = Instant::now();
                        if let Some(signal) = signal.take() {
                            view.waiting.note(signal, now);
                        }
                        while let Ok(more) = receive.try_recv() {
                            view.waiting.note(more, now);
                        }
                        wait = view.take_ready(window, cx);
                    });
                    if alive.is_err() {
                        return;
                    }
                    let Some(wait) = wait else {
                        break;
                    };
                    cx.background_executor().timer(wait).await;
                }
            }
        });
        let subscriptions = vec![
            cx.subscribe_in(&editor, window, |view, _, _: &TextChanged, window, cx| {
                view.edited(window, cx);
                if view.passage.is_some() {
                    view.sync_passage(cx);
                }
            }),
            cx.subscribe_in(&editor, window, |view, _, _: &SessionChanged, window, cx| {
                view.update_watched_files(cx);
                if view.has_conflicts() || view.passage.is_some() {
                    view.sync_passage(cx);
                }
                if view.editor.read(cx).session().is_dirty() {
                    view.edited(window, cx);
                }
            }),
            cx.subscribe(&model, |view, _, _: &ThemeSwitched, cx| view.update_watched_files(cx)),
            cx.subscribe_in(&editor, window, |view, _, _: &SaveRequested, window, cx| {
                view.flush(window, cx);
            }),
            cx.subscribe_in(&editor, window, Self::external_undo),
            cx.on_app_quit(|view, cx| {
                view.flush_blocking(cx);
                async {}
            }),
        ];
        let mut view = Self {
            editor,
            inspector,
            sync: FileSync::new(copies),
            pacer: Pacer::default(),
            pacing: false,
            write_lock: Arc::new(Mutex::new(())),
            writing: None,
            write_again: false,
            settling: Vec::new(),
            waiting: Waiting::default(),
            signals,
            watcher: None,
            theme,
            note: None,
            note_file: None,
            note_task: Task::ready(()),
            gone_task: None,
            holding_outside: false,
            git_move: None,
            agent_working: false,
            merging: None,
            passage: None,
            _incoming: incoming,
            _subscriptions: subscriptions,
        };
        view.start_watching(cx);
        view.remove_leftovers(cx);
        view.restore(window, cx);
        // Whatever changed between reading the files and the watcher starting.
        view.signals.unbounded_send(Signal::Everything).ok();
        view
    }

    fn session<'a>(&self, cx: &'a App) -> &'a Session {
        self.editor.read(cx).session()
    }

    /// Reads every file the session has now and takes in what changed, without waiting for the watcher. Anything that
    /// wants all changes up to now calls this first. While a save is running or an input method composes, the look
    /// waits for them.
    pub fn scan_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.writing.is_some() || self.holding_outside || self.editor.read(cx).holds_outside_changes() {
            self.signals.unbounded_send(Signal::Everything).ok();
            return;
        }
        self.process(
            Ready {
                paths: Vec::new(),
                everything: true,
                resource: false,
            },
            window,
            cx,
        );
    }

    /// While a Git command moves the files: what the watcher reports waits. Whoever holds it lets go with
    /// [`Self::release_outside_changes`].
    pub fn hold_outside_changes(&mut self, git: Source) {
        self.holding_outside = true;
        self.git_move = Some(git);
    }

    /// The Git command is done: what it changed comes in now, named after it.
    pub fn release_outside_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.holding_outside = false;
        self.scan_now(window, cx);
        self.git_move = None;
    }

    /// Whether the writing partner's turn is running, so what comes in meanwhile is named after it.
    pub fn set_agent_working(&mut self, working: bool) {
        self.agent_working = working;
    }

    fn source(&self) -> Source {
        match (&self.git_move, self.agent_working) {
            (Some(git), _) => git.clone(),
            (None, true) => Source::Agent,
            (None, false) => Source::Elsewhere,
        }
    }

    /// Whether a conflict waits on the line at the window's foot.
    pub fn has_conflicts(&self) -> bool {
        self.sync.conflicts().next().is_some()
    }

    /// Whether this file's conflict waits there.
    pub fn has_conflict_in(&self, path: &Path) -> bool {
        self.sync.conflicts().any(|conflict| conflict == path)
    }

    /// Says which branch a merge's conflicts come from, for the line to name.
    pub fn merging_from(&mut self, branch: String) {
        self.merging = Some(branch);
    }

    /// Takes in one file of a Git merge that overlapped. The file gets their text, the buffer keeps ours, and the conflict
    /// goes to the line at the window's foot. Git reported it, so it does not merge by sentences. The view goes to the
    /// passage, since the author asked for this merge and is looking at it.
    pub fn take_merge(
        &mut self,
        path: &Path,
        base: &str,
        theirs: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> std::io::Result<()> {
        atomic::write(path, theirs.as_bytes())?;
        self.sync.set_source(self.source());
        let sync = &mut self.sync;
        let change = self.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| sync.take_git_conflict(session, path, base, theirs),
                window,
                cx,
            )
        });
        if let Some(change) = change {
            self.tell(change, window, cx);
        }
        self.sync_passage(cx);
        self.go_to_passage(window, cx);
        cx.notify();
        Ok(())
    }

    /// Looks again at what the first conflict is about, and marks it in the text if its chapter is open.
    fn sync_passage(&mut self, cx: &mut Context<Self>) {
        if !self.has_conflicts() {
            self.merging = None;
        }
        let session = self.session(cx);
        let passage = (self.sync.conflicts().next()).and_then(|path| {
            let passage = self.sync.conflict_passage(session, path)?;
            let (chapter, at) = session.place_in_file(path, passage.range.start)?;
            let mark = (chapter == session.active()).then(|| at..at + passage.range.len());
            Some((passage, mark))
        });
        let (passage, mark) = passage.unzip();
        if self.passage != passage {
            self.passage = passage;
            cx.notify();
        }
        let mark = mark.flatten();
        self.editor.update(cx, |editor, cx| editor.set_conflict_mark(mark, cx));
    }

    /// Opens the chapter of a merge's first conflict at its passage.
    fn go_to_passage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session(cx);
        let place = (self.sync.conflicts().next())
            .zip(self.passage.as_ref())
            .and_then(|(path, passage)| session.place_in_file(path, passage.range.start));
        if let Some((chapter, at)) = place {
            self.editor
                .update(cx, |editor, cx| editor.show_place(chapter, at, window, cx));
        }
    }

    fn start_watching(&mut self, cx: &mut Context<Self>) {
        let session = self.session(cx);
        let root = session.base_dir();
        let project = matches!(session.kind(), Kind::Project { .. });
        let signals = self.signals.clone();
        self.watcher = match Watcher::start(&root, project, move |signal| {
            signals.unbounded_send(signal).ok();
        }) {
            Ok(watcher) => Some(watcher),
            Err(error) => {
                self.sync.report(&root, Problem::Watch(error.to_string()));
                None
            }
        };
        self.update_watched_files(cx);
    }

    /// Clears what a crash in the middle of a save left beside the document's files.
    fn remove_leftovers(&self, cx: &App) {
        let folders: HashSet<PathBuf> = (self.session(cx).files().iter())
            .filter_map(|file| file.parent().map(Path::to_owned))
            .collect();
        cx.background_spawn(async move { folders.iter().for_each(|folder| atomic::remove_leftovers(folder)) })
            .detach();
    }

    fn update_watched_files(&mut self, cx: &mut Context<Self>) {
        self.theme = open_theme(self.inspector.read(cx).model(), cx);
        let mut files = self.session(cx).files();
        files.extend(self.theme.clone());
        let resources = self.session(cx).named_files();
        if let Some(watcher) = &mut self.watcher {
            watcher.set_files(&files, &resources);
        }
    }

    /// Brings back what a run that did not finish left in recovery copies. Each is one undo step, and merges with
    /// whatever the file holds by now.
    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(copies) = self.sync.copies().cloned() else {
            return;
        };
        let found = copies.load();
        if found.is_empty() {
            return;
        }
        let mut names = Vec::new();
        let sync = &mut self.sync;
        self.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    for copy in found {
                        if session.file_text(&copy.path).as_deref() == Some(copy.text.as_str()) {
                            copies.clear(&copy.path);
                            continue;
                        }
                        if let Err(problem) = session.restore_text(&copy.path, &copy.base, &copy.text) {
                            sync.report(&copy.path, Problem::Restore(problem));
                            continue;
                        }
                        names.push(file_name(&copy.path));
                        sync.on_disk(session, &copy.path, std::fs::read(&copy.path).ok());
                    }
                },
                window,
                cx,
            );
        });
        if !names.is_empty() {
            let text = format!("{} {}", crate::t!(Key::Restored), names.join(", "));
            self.show_note(text, window, cx);
            self.edited(window, cx);
        }
    }

    // Saving

    fn edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pacer.edited(Instant::now());
        self.start_pacing(window, cx);
    }

    fn start_pacing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pacing {
            return;
        }
        self.pacing = true;
        cx.spawn_in(window, async move |view, cx| {
            loop {
                let wait = view.update_in(cx, |view, window, cx| match view.pacer.next(Instant::now()) {
                    Next::Idle => {
                        view.pacing = false;
                        None
                    }
                    Next::Wait(wait) => Some(wait),
                    Next::Recover => {
                        view.store_recovery(cx);
                        Some(Duration::ZERO)
                    }
                    Next::Save => {
                        view.flush(window, cx);
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

    /// Writes what is unsaved now, in the background. Cmd+S and the pause after typing both land here.
    fn flush(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.writing.is_some() {
            self.write_again = true;
            return;
        }
        self.retry_conflicts(window, cx);
        self.sync.forget_save_problems_if_clean(self.session(cx));
        let outgoing = self.editor.update(cx, |editor, _| editor.session_quietly().outgoing());
        self.sync.forget_clean_copies(self.session(cx));
        if outgoing.is_empty() {
            cx.notify();
            return;
        }
        let lock = self.write_lock.clone();
        let copies = self.sync.copies().cloned();
        let writes = cx.background_spawn(async move {
            let _writing = lock.lock();
            (outgoing.into_iter())
                .map(|outgoing| {
                    let result = outgoing.write();
                    if let (Ok(Outcome::Written), Some(copies)) = (&result, &copies) {
                        copies.clear(&outgoing.path);
                    }
                    (outgoing, result)
                })
                .collect::<Vec<_>>()
        });
        self.writing = Some(cx.spawn_in(window, async move |view, cx| {
            let results = writes.await;
            view.update_in(cx, |view, window, cx| view.finish_writes(results, window, cx))
                .ok();
        }));
    }

    /// Writing on may have taken away what a conflict was about.
    fn retry_conflicts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sync.can_settle_a_conflict(self.session(cx)) {
            return;
        }
        let sync = &mut self.sync;
        let changes = self.editor.update(cx, |editor, cx| {
            editor.update_session(|session| sync.settle_conflicts(session), window, cx)
        });
        for change in changes {
            self.tell(change, window, cx);
        }
    }

    fn finish_writes(
        &mut self,
        results: Vec<(Outgoing, std::io::Result<Outcome>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.writing = None;
        let mut failed = false;
        let mut changed = Vec::new();
        // A write that only records its result changes nothing on screen. Anything else goes through the session's front
        // door, to reach the galley and the panels.
        let manifest = self.session(cx).manifest_path();
        let quiet = (results.iter()).all(|(outgoing, _)| manifest.as_ref() != Some(&outgoing.path));
        let sync = &mut self.sync;
        let record = |session: &mut Session| {
            for (outgoing, result) in results {
                failed |= result.is_err();
                if sync.finish_write(session, &outgoing, result) {
                    changed.push(outgoing.path);
                }
            }
        };
        if quiet {
            self.editor.update(cx, |editor, _| record(editor.session_quietly()));
        } else {
            self.editor
                .update(cx, |editor, cx| editor.update_session(record, window, cx));
        }
        let now = Instant::now();
        if failed {
            self.pacer.retry_later(now);
            self.start_pacing(window, cx);
            self.sync.check_root(self.editor.read(cx).session());
            self.watch_for_return(window, cx);
        } else {
            self.pacer.saved_all();
        }
        // Someone else wrote first. Their file gets a look once they are done, then the merge is saved.
        if !changed.is_empty() {
            for path in changed {
                self.signals.unbounded_send(Signal::Path(path)).ok();
            }
            self.edited(window, cx);
        }
        cx.notify();
        if !self.settling.is_empty() {
            self.write_again = false;
            let closing = self.settle_now(window, cx);
            for waiting in std::mem::take(&mut self.settling) {
                waiting.send(closing.clone()).ok();
            }
            return;
        }
        if std::mem::take(&mut self.write_again) {
            self.flush(window, cx);
        }
    }

    /// Writes everything now and waits for it: the app is about to go. What cannot be written stays as a recovery copy.
    fn flush_blocking(&mut self, cx: &mut Context<Self>) {
        // A footnote's text being typed counts as written.
        if let Some(handle) = cx.try_global::<Current>().map(|current| current.window) {
            let editor = self.editor.clone();
            handle
                .update(cx, |_, window, cx| {
                    editor.update(cx, |editor, cx| editor.commit_floats(window, cx));
                })
                .ok();
        }
        let lock = self.write_lock.clone();
        let _writing = lock.lock();
        let sync = &mut self.sync;
        self.editor
            .update(cx, |editor, _| sync.write_all(editor.session_quietly()));
        // Theme edits are written on a thread of their own, which a quitting app would not wait for.
        self.inspector.read(cx).model().read(cx).flush();
    }

    /// For a document that is about to close or be replaced: writes what changed and says what is left. A save that is
    /// running lands first, because it tells the session what it wrote.
    fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) -> futures::channel::oneshot::Receiver<Closing> {
        let (reply, answer) = futures::channel::oneshot::channel();
        if self.writing.is_some() {
            self.settling.push(reply);
        } else {
            reply.send(self.settle_now(window, cx)).ok();
        }
        answer
    }

    fn settle_now(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Closing {
        let lock = self.write_lock.clone();
        let _writing = lock.lock();
        self.retry_conflicts(window, cx);
        let sync = &mut self.sync;
        let closing = self.editor.update(cx, |editor, cx| {
            editor.update_session(|session| sync.write_all(session), window, cx)
        });
        cx.notify();
        closing
    }

    /// Stores a recovery copy of every file with unsaved text that changed since its last copy, on another thread.
    fn store_recovery(&mut self, cx: &mut Context<Self>) {
        let Some(copies) = self.sync.copies().cloned() else {
            return;
        };
        let mut unsaved = self.session(cx).unsaved();
        unsaved.retain(|file| self.sync.copy_is_due(&file.path, &file.text));
        if unsaved.is_empty() {
            return;
        }
        let lock = self.write_lock.clone();
        let stored = cx.background_spawn(async move {
            let _writing = lock.lock();
            (unsaved.into_iter())
                .map(|file| {
                    let result = copies.store(&file.path, &file.base, &file.text);
                    (file.path, result.map_err(|error| error.to_string()))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |view, cx| {
            let stored = stored.await;
            view.update(cx, |view, cx| {
                for (path, result) in stored {
                    match result {
                        Ok(()) => view.sync.clear(&path, Problem::Copy),
                        Err(error) => view.sync.report(&path, Problem::Copy(error)),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // Changes from outside

    /// Takes what is ready from the waiting paths, and returns how long until the next look, if anything still waits.
    fn take_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Duration> {
        let now = Instant::now();
        let busy = self.writing.is_some() || self.holding_outside || self.editor.read(cx).holds_outside_changes();
        let session = self.editor.read(cx).session();
        let theme = self.theme.as_deref();
        // An empty file may be a program halfway through writing it, so it gets the same grace as a missing one.
        let plain = |path: &Path| {
            theme == Some(path)
                || (session.knows_file(path) && std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0))
        };
        if busy {
            return self.waiting.wait(now, plain).map(|wait| wait.max(BUSY_RETRY));
        }
        let ready = self.waiting.take(now, plain);
        let wait = self.waiting.wait(now, plain);
        if ready.resource {
            cx.emit(ResourceChanged);
        }
        if !ready.paths.is_empty() || ready.everything {
            self.process(ready, window, cx);
        }
        wait
    }

    fn process(&mut self, mut ready: Ready, window: &mut Window, cx: &mut Context<Self>) {
        let theme = self.theme.as_ref();
        let theme_changed = ready.everything || ready.paths.iter().any(|path| Some(path) == theme);
        ready.paths.retain(|path| Some(path) != theme);
        // A look that finds nothing new touches nothing, so a save's own echo costs one read and no repaint.
        let session = self.session(cx);
        let anything = ready.everything
            || self.sync.root_is_gone(session)
            || !session.base_dir().is_dir()
            || ready.paths.iter().any(|path| {
                let disk = std::fs::read(path).ok();
                (!session.knows_file(path) && disk.is_some()) || self.sync.differs(session, path, disk.as_deref())
            });
        if anything {
            self.sync.set_source(self.source());
            let sync = &mut self.sync;
            let report = self.editor.update(cx, |editor, cx| {
                editor.update_session(|session| sync.process(session, &ready), window, cx)
            });
            if self.sync.root_is_gone(self.session(cx)) {
                self.watch_for_return(window, cx);
            }
            if report.files_changed {
                self.update_watched_files(cx);
            }
            for change in report.changes {
                self.tell(change, window, cx);
            }
            if self.session(cx).is_dirty() {
                self.edited(window, cx);
            }
        }
        if theme_changed {
            self.inspector.update(cx, |inspector, cx| inspector.reload_theme(cx));
        }
        cx.notify();
    }

    /// While the folder is away: looks for it now and then, and takes up the work when it is back.
    fn watch_for_return(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gone_task.is_some() || self.session(cx).base_dir().is_dir() {
            return;
        }
        let root = self.session(cx).base_dir();
        self.gone_task = Some(cx.spawn_in(window, async move |view, cx| {
            while !root.is_dir() {
                cx.background_executor().timer(GONE_POLL).await;
            }
            view.update_in(cx, |view, window, cx| {
                view.gone_task = None;
                view.start_watching(cx);
                view.scan_now(window, cx);
                view.flush(window, cx);
            })
            .ok();
        }));
    }

    fn tell(&mut self, change: FileChange, window: &mut Window, cx: &mut Context<Self>) {
        let name = change
            .chapter
            .map_or_else(|| file_name(&change.path), |chapter| self.session(cx).title_of(chapter));
        let updated = |source: &Source| match source {
            Source::Elsewhere => crate::t!(Key::Reloaded).to_owned(),
            Source::Agent => crate::t!(Key::ReloadedAgent).to_owned(),
            Source::Git(GitMove::Merge, branch) => crate::t!(Key::ReloadedMerge, branch = branch),
            Source::Git(GitMove::Pull, branch) => crate::t!(Key::ReloadedPull, branch = branch),
            Source::Git(GitMove::Switch, branch) => crate::t!(Key::ReloadedSwitch, branch = branch),
            Source::Git(GitMove::Abort, _) => crate::t!(Key::ReloadedAbort).to_owned(),
        };
        let merged = |source: &Source| match source {
            Source::Elsewhere => crate::t!(Key::Merged),
            Source::Agent => crate::t!(Key::MergedAgent),
            Source::Git(..) => crate::t!(Key::MergedGit),
        };
        match change.kind {
            ChangeKind::Merged => self.show_note(format!("{} {name}", merged(&change.source)), window, cx),
            ChangeKind::Reloaded => self.show_note(format!("{} {name}", updated(&change.source)), window, cx),
            ChangeKind::Removed => self.show_note(format!("{name} {}", crate::t!(Key::Removed)), window, cx),
            ChangeKind::Conflict | ChangeKind::Unchanged => {}
        }
        cx.emit(change);
        self.sync_passage(cx);
    }

    /// A passing note on the saving line, the window's one place for them.
    pub fn show_note(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.show_note_about(text, None, window, cx);
    }

    /// A note about a kept copy: it names the folder, and Show in Finder shows the copy in it.
    fn note_kept(&mut self, before: Option<&str>, kept: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let says = crate::t!(Key::CopyKept, folder = recovery::KEPT_FOLDER);
        let text = before.map_or_else(|| says.clone(), |before| format!("{before} {says}"));
        self.show_note_about(text, Some(kept), window, cx);
    }

    fn show_note_about(&mut self, text: String, file: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self.note_file = file;
        self.note_task = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(tokens::NOTE_TIME).await;
            view.update(cx, |view, cx| {
                view.note = None;
                view.note_file = None;
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn external_undo(
        &mut self,
        _: &Entity<EditorView>,
        event: &ExternalUndo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.done {
            self.show_note(crate::t!(Key::UndoStopped).to_owned(), window, cx);
            return;
        }
        // The file still holds what the undo just took out of the buffer.
        let session = self.session(cx);
        let path = session.active_chapter().path().to_owned();
        let kept = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| self.sync.copies()?.keep(&path, "from-outside", &text).ok());
        match kept {
            Some(kept) => self.note_kept(Some(crate::t!(Key::UndoDone)), kept, window, cx),
            None => self.show_note(crate::t!(Key::UndoDone).to_owned(), window, cx),
        }
    }

    /// Says at the foot that something about `path` went wrong outside saving, or, with `None`, that a problem of
    /// `kind` is over.
    pub fn report(&mut self, path: &Path, kind: fn(String) -> Problem, error: Option<String>, cx: &mut Context<Self>) {
        match error {
            Some(error) => self.sync.report(path, kind(error)),
            None => self.sync.clear(path, kind),
        }
        cx.notify();
    }

    /// Settles the first conflict, as a key or button of the prompt does. For scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn settle_first(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        self.resolve_first(choice, window, cx);
    }

    /// Settles the first conflict, for the keys and buttons of the prompt.
    fn resolve_first(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.sync.conflicts().next().map(Path::to_owned) else {
            cx.emit(ChoiceWithoutConflict(choice));
            return;
        };
        self.resolve(&path, choice, window, cx);
    }

    fn resolve(&mut self, path: &Path, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        let sync = &mut self.sync;
        let result = self.editor.update(cx, |editor, cx| {
            editor.update_session(|session| sync.resolve(session, path, choice), window, cx)
        });
        match result {
            Ok(Some(kept)) => self.note_kept(None, kept, window, cx),
            Ok(None) => {}
            Err(error) => {
                self.show_note(format!("{}: {error}", crate::t!(Key::CannotSave)), window, cx);
                return;
            }
        }
        self.edited(window, cx);
        self.update_watched_files(cx);
        self.sync_passage(cx);
        if self.merging.is_some() {
            self.go_to_passage(window, cx);
        }
        cx.notify();
    }

    fn button(&self, id: &'static str, choice: Choice, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        foot_button(Tokens::get(cx), id, choice)
            .on_click(cx.listener(move |view, _, window, cx| view.resolve_first(choice, window, cx)))
    }
}

/// A small raised button at the window's foot that settles both versions by `choice`.
pub fn foot_button(tokens: &Tokens, id: &'static str, choice: Choice) -> gpui::Stateful<gpui::Div> {
    let label = crate::t!(match choice {
        Choice::Mine => Key::KeepMine,
        Choice::Theirs => Key::TakeTheirs,
        Choice::Both => Key::SeeBoth,
    });
    div()
        .id(id)
        .button(tokens, label)
        .flex_none()
        .px(tokens::SPACE_2)
        .rounded(tokens::RADIUS)
        .cursor_pointer()
        .font_weight(FontWeight::MEDIUM)
        .text_color(tokens.text)
        .bg(tokens.raised)
        .hover(|button| button.bg(tokens.divider))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}

impl Render for PersistView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let session = self.session(cx);
        let root = div()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .px(tokens::SPACE_3)
            .bg(tokens.canvas)
            .text_size(tokens::TEXT_SMALL);
        if let Some(path) = self.sync.conflicts().next() {
            let name = session
                .chapter_of_file(path)
                .map_or_else(|| file_name(path), |chapter| session.title_of(chapter));
            let removal = self.sync.conflict_is_removal(path);
            let says = match (&self.merging, removal) {
                (_, true) => format!("{name} {}", crate::t!(Key::Deleted)),
                (Some(branch), false) => crate::t!(Key::OverlapBranch, name = name, branch = branch),
                (None, false) => format!("{name} {}", crate::t!(Key::Overlap)),
            };
            let theirs = self.passage.as_ref().map(|passage| {
                let font = self.editor.read(cx).body_font();
                div()
                    .id("conflict-theirs")
                    .absolute()
                    .bottom_full()
                    .left(tokens::SPACE_3)
                    .w(tokens::CONFLICT_CARD_WIDTH)
                    .max_h(tokens::SLIP_TEXT_MAX_HEIGHT)
                    .overflow_y_scroll()
                    .p(tokens::SPACE_3)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .bg(tokens.panel)
                    .border_1()
                    .border_color(tokens.divider)
                    .rounded(tokens::RADIUS)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(crate::versions::compared(
                        &passage.mine,
                        &passage.theirs,
                        &font,
                        &tokens,
                    ))
            });
            return root
                .relative()
                .flex_1()
                .text_color(tokens.accent)
                .children(theirs)
                .child(div().min_w_0().truncate().child(says))
                .child(self.button("keep-mine", Choice::Mine, cx))
                .child(self.button("take-theirs", Choice::Theirs, cx))
                .when(!removal, |root| root.child(self.button("see-both", Choice::Both, cx)))
                .into_any_element();
        }
        if let Some((path, problem)) = self.sync.problems().next() {
            let text = match problem {
                Problem::Gone => crate::t!(Key::FolderGone).to_owned(),
                Problem::Save(reason) => format!(
                    "{} {}: {reason}. {}",
                    crate::t!(Key::CannotSave),
                    file_name(path),
                    crate::t!(Key::KeptInMemory)
                ),
                Problem::Unreadable(reason) => format!("{}: {reason}", crate::t!(Key::Unreadable)),
                Problem::Watch(error) => crate::t!(Key::CannotWatch, error = error),
                Problem::Restore(error) => crate::t!(Key::CannotRestore, file = file_name(path), error = error),
                Problem::Copy(error) => crate::t!(Key::CannotKeepCopy, file = file_name(path), error = error),
                Problem::Merge(error) => crate::t!(Key::CannotMerge, file = file_name(path), error = error),
                Problem::ChapterList(error) => crate::t!(Key::CannotUpdateChapters, error = error),
                Problem::History(error) => crate::t!(Key::CannotKeepHistory, file = file_name(path), error = error),
                Problem::Repo(error) => crate::t!(Key::CannotMakeRepo, error = error),
                Problem::State(error) => crate::t!(Key::CannotSaveState, file = path.display(), error = error),
                Problem::Fonts(error) => crate::t!(Key::CannotUseFonts, error = error),
                Problem::AgentSession(error) => crate::t!(Key::CannotRememberAgent, error = error),
                Problem::AgentSessionUnread(error) => crate::t!(Key::CannotReadAgent, error = error),
            };
            return root
                .flex_1()
                .text_color(tokens.accent)
                .child(div().min_w_0().truncate().child(text))
                .into_any_element();
        }
        if let Some(note) = &self.note {
            let reveal = self.note_file.clone().map(|file| {
                div()
                    .id("show-kept")
                    .control(&tokens, gpui::Role::Link, crate::t!(Key::ShowInFinder))
                    .flex_none()
                    .cursor_pointer()
                    .text_color(tokens.text)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| cx.reveal_path(&file))
                    .child(crate::t!(Key::ShowInFinder))
            });
            return root
                .flex_1()
                .text_color(tokens.text_quiet)
                .child(div().min_w_0().truncate().child(note.clone()))
                .children(reveal)
                .into_any_element();
        }
        // All is well, so there is nothing to say.
        div().into_any_element()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}
