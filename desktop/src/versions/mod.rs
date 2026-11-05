//! Versions: local history and first-class Git, in two tiers.
//!
//! Local history is automatic: [`history`] keeps what each file held at each pause in the app data folder, and the view
//! here decides when. Git is deliberate: the app commits only when asked. [`repo`] reads the repo and commits through
//! gitoxide, with no Git installed, and [`git_cli`] calls the installed `git` for switching, merging, pushing, and pulling.
//! See [the decisions](../../../docs/DECISIONS.md).
//!
//! `Cmd+Shift+S` opens the [commit card](commit_card), `Cmd+Y` the [timeline](timeline), and `Ctrl+Shift+G` the
//! [Git panel](panel). All three show what [`Versions`] knows, and ask it to do things.
//!
//! For other slices: [`label_history`] before something that may change the text, and [`set_unreviewed_changes`] for what
//! the commit card warns about.

mod commit_card;
mod dates;
mod git_cli;
mod history;
mod pacing;
mod panel;
mod repo;
mod shown;
mod strings;
mod timeline;
mod ui;
mod words;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Context, Entity, Global, KeyBinding, SharedString, Subscription, Task, WeakEntity, Window,
    actions, div,
};

use self::commit_card::{CardField, CommitCard};
use self::git_cli::{Git, Merged, Refused};
use self::history::{History, Orphan, now_ms};
use self::pacing::{Due, Pacing};
pub use self::panel::GitPanel;
use self::panel::Slot;
use self::repo::{Conflict, Overview, Problem, Repo};
use self::strings::Key;
use self::timeline::Timeline;
use crate::editor::{SessionChanged, TextChanged};
use crate::persist::recovery::data_dir;
use crate::persist::sync::ChangeKind;
use crate::persist::{Closing, FileChange, PersistView};
use crate::session::{ChapterId, Kind, Session};
use crate::tokens::{self, Tokens};
use crate::workspace::{Mode, Parts, Slots};

actions!(versions, [ShowCommitCard, ShowTimeline, ShowGitPanel]);

/// How long after typing stops the open panel reads the repo again.
const REFRESH_PAUSE: Duration = Duration::from_millis(1500);
/// The time a scripted step gets before the next.
const SCRIPT_STEP: Duration = Duration::from_secs(3);
/// How often the open panel looks whether another tool moved the repo.
const PANEL_LOOK: Duration = Duration::from_secs(5);
/// How long a passing note stays.
const NOTE_TIME: Duration = Duration::from_secs(8);
/// How often an open panel may fetch, as `git fetch` goes over the network.
const FETCH_GAP: Duration = Duration::from_secs(300);
/// Commits a panel lists. A long history stays usable through the terminal.
const LOG_ROWS: usize = 500;

struct Current {
    view: WeakEntity<Versions>,
    window: AnyWindowHandle,
    store: Option<Arc<History>>,
}

impl Global for Current {}

/// What the commit card warns about, from whoever knows.
struct Unreviewed(Box<dyn Fn(&App) -> usize>);

impl Global for Unreviewed {}

/// What shows the Git rail tab.
type ShowRailTab = dyn Fn(&mut Window, &mut App);

struct RailTab(Box<ShowRailTab>);

impl Global for RailTab {}

/// Binds the keys. Once, when the app starts.
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-s", ShowCommitCard, None),
        KeyBinding::new("cmd-y", ShowTimeline, None),
        KeyBinding::new("ctrl-shift-g", ShowGitPanel, None),
    ]);
    timeline::bind_keys(cx);
    commit_card::bind_keys(cx);
    cx.on_action(|_: &ShowCommitCard, cx| with_view(cx, Versions::toggle_card));
    cx.on_action(|_: &ShowTimeline, cx| with_view(cx, Versions::toggle_timeline));
    cx.on_action(|_: &ShowGitPanel, cx| with_view(cx, Versions::toggle_panel));
}

fn with_view<Run>(cx: &mut App, run: Run)
where
    Run: FnOnce(&mut Versions, &mut Window, &mut Context<Versions>) + 'static,
{
    // An action runs inside its window's update, which cannot be entered again.
    cx.defer(move |cx| {
        let Some((view, window)) = cx
            .try_global::<Current>()
            .and_then(|current| Some((current.view.upgrade()?, current.window)))
        else {
            return;
        };
        window
            .update(cx, |_, window, cx| view.update(cx, |view, cx| run(view, window, cx)))
            .ok();
    });
}

/// Mounts the three views and starts local history. One line in `app::mount`.
///
/// `DRUCK_VERSIONS=card`, `timeline`, `panel`, or `menu` opens one, for screenshots. `DRUCK_VERSIONS_RUN` runs steps from
/// outside, see [`Versions::run_scripted`], as automated mouse events do not reach the window.
pub fn mount(
    parts: &Parts,
    persist: &Entity<PersistView>,
    slots: &mut Slots,
    window: &mut Window,
    cx: &mut Context<crate::workspace::Workspace>,
) -> Entity<GitPanel> {
    let workspace = cx.entity().downgrade();
    let view = cx.new(|cx| Versions::new(parts.clone(), persist.clone(), workspace, window, cx));
    cx.set_global(Current {
        view: view.downgrade(),
        window: window.window_handle(),
        store: view.read(cx).store.clone(),
    });
    // First, so it stands at the left with the saving state and not past the word count.
    slots.footer.insert(0, view.clone().into());
    let (card, timeline, panel) = {
        let read = view.read(cx);
        (read.card.clone(), read.timeline.clone(), read.panel.clone())
    };
    card.update(cx, |_, cx| cx.observe(&view, |_, _, cx| cx.notify()).detach());
    timeline.update(cx, |_, cx| cx.observe(&view, |_, _, cx| cx.notify()).detach());
    panel.update(cx, |_, cx| cx.observe(&view, |_, _, cx| cx.notify()).detach());
    slots.overlays.push(card.into());
    slots.overlays.push(timeline.into());
    let slot = cx.new(|_| Slot::new(panel.clone(), view.downgrade()));
    slots.right.push(slot.into());
    if let Some(shown) = crate::gate::var("DRUCK_VERSIONS") {
        let view = view.clone();
        window.defer(cx, move |window, cx| {
            view.update(cx, |view, cx| match shown.as_str() {
                "card" => view.toggle_card(window, cx),
                "timeline" => view.toggle_timeline(window, cx),
                "panel" => view.toggle_panel(window, cx),
                "menu" => {
                    view.toggle_panel(window, cx);
                    view.panel.update(cx, |panel, cx| panel.show_branches(cx));
                }
                _ => {}
            });
        });
    }
    if let Some(steps) = crate::gate::var("DRUCK_VERSIONS_RUN") {
        let view = view.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            for step in steps.split(';').filter(|step| !step.is_empty()) {
                cx.background_executor().timer(SCRIPT_STEP).await;
                view.update_in(cx, |view, window, cx| view.run_scripted(step, window, cx))
                    .ok();
            }
        })
        .detach();
    }
    panel
}

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

/// Says how many agent changes are not reviewed yet, which the commit card names in the accent color.
pub fn set_unreviewed_changes(count: impl Fn(&App) -> usize + 'static, cx: &mut App) {
    cx.set_global(Unreviewed(Box::new(count)));
}

fn unreviewed_changes(cx: &App) -> usize {
    if let Some(count) = crate::gate::var("DRUCK_UNREVIEWED").and_then(|count| count.parse().ok()) {
        return count;
    }
    cx.try_global::<Unreviewed>().map_or(0, |count| (count.0)(cx))
}

/// Says how to show the Git panel's rail tab, once it holds the panel: `Ctrl+Shift+G` then shows it in Publish, and the
/// right panel stays to Write.
pub fn use_rail_tab(show: impl Fn(&mut Window, &mut App) + 'static, cx: &mut App) {
    cx.set_global(RailTab(Box::new(show)));
}

/// The project just made, to warn about once it opens.
struct NewProject(PathBuf);

impl Global for NewProject {}

/// Makes a new project's folder a repo. Nothing is committed. A folder that cannot be one still works, with local history.
pub fn start_repo(folder: &Path, cx: &mut App) {
    if let Err(problem) = Repo::init(folder) {
        eprintln!("cannot make {} a repo: {problem:?}", folder.display());
    }
    cx.set_global(NewProject(folder.to_owned()));
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

/// What is known about the book's repo.
#[derive(Clone, PartialEq, Eq)]
enum RepoState {
    /// A single Markdown file, which has local history only.
    Single,
    /// Being read for the first time.
    Reading,
    NoRepo,
    Ready(Box<Overview>),
    Broken(String),
}

/// Where a merge takes its other side from.
enum MergeFrom {
    Branch(String),
    /// A fetch, then the branch's upstream.
    Upstream,
}

enum MergeEnd {
    Done,
    Conflicts(Vec<Conflict>),
    /// One file cannot be shown on the conflict line, so the merge was undone.
    Outside(String),
}

type Done<T> = Box<dyn FnOnce(&mut Versions, Result<T, String>, &mut Window, &mut Context<Versions>)>;

pub struct Versions {
    parts: Parts,
    persist: Entity<PersistView>,
    workspace: WeakEntity<crate::workspace::Workspace>,
    store: Option<Arc<History>>,
    git: Option<Arc<Git>>,
    state: RepoState,
    card: Entity<CommitCard>,
    timeline: Entity<Timeline>,
    panel: Entity<GitPanel>,
    /// A job on another thread. Only one at a time, since they all move the repo.
    working: bool,
    /// What went wrong last, shown in the card and the panel until the next action.
    error: Option<String>,
    note: Option<SharedString>,
    note_task: Task<()>,
    pacing: Pacing,
    pacing_running: bool,
    /// Each chapter's file when last looked at, to tell a rename.
    paths: HashMap<ChapterId, PathBuf>,
    /// The file names that may be another file's lost history, by the file with no history.
    unplaced: HashMap<PathBuf, Orphan>,
    refresh_pause: Task<()>,
    panel_watch: Task<()>,
    reading: Task<()>,
    last_fetch: Option<Instant>,
    /// A merge stopped on conflicts, and the card opens when the last one is settled.
    awaiting_merge: bool,
    /// The card opens once the repo has been read again, for the merge's message.
    card_after_reading: bool,
    /// Changes coming in now are the repo's own moves, which were labelled before they began.
    quiet: bool,
    _subscriptions: Vec<Subscription>,
}

impl Versions {
    fn new(
        parts: Parts,
        persist: Entity<PersistView>,
        workspace: WeakEntity<crate::workspace::Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = data_dir().map(|data| Arc::new(History::new(&data)));
        let git = if crate::gate::var("DRUCK_NO_GIT").is_some() {
            None
        } else {
            Git::find().map(Arc::new)
        };
        let view = cx.entity().downgrade();
        let card = cx.new(|cx| CommitCard::new(view.clone(), window, cx));
        let book = match parts.editor.read(cx).session().kind() {
            Kind::Project { dir } => Some(dir.clone()),
            Kind::Single { .. } => None,
        };
        let timeline = cx.new(|cx| Timeline::new(view.clone(), &parts, store.clone(), book, cx));
        let panel = cx.new(|cx| GitPanel::new(view, window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&parts.editor, window, |view, _, _: &TextChanged, window, cx| {
                view.edited(window, cx);
            }),
            cx.subscribe_in(&parts.editor, window, |view, _, _: &SessionChanged, _, cx| {
                view.session_changed(cx);
            }),
            cx.subscribe(&persist, |view, _, change: &FileChange, cx| {
                view.outside_change(change, cx)
            }),
            cx.observe(&persist, |view, _, cx| view.conflicts_settled(cx)),
            cx.on_app_quit(|view, cx| {
                view.keep_waiting(cx);
                async {}
            }),
            cx.on_release(|view, cx| view.keep_waiting(cx)),
        ];
        let state = match parts.editor.read(cx).session().kind() {
            Kind::Single { .. } => RepoState::Single,
            Kind::Project { .. } => RepoState::Reading,
        };
        let mut view = Self {
            parts,
            persist,
            workspace,
            store,
            git,
            state,
            card,
            timeline,
            panel,
            working: false,
            error: None,
            note: None,
            note_task: Task::ready(()),
            pacing: Pacing::default(),
            pacing_running: false,
            paths: HashMap::new(),
            unplaced: HashMap::new(),
            refresh_pause: Task::ready(()),
            panel_watch: Task::ready(()),
            reading: Task::ready(()),
            last_fetch: None,
            awaiting_merge: false,
            card_after_reading: false,
            quiet: false,
            _subscriptions: subscriptions,
        };
        view.paths = view.chapter_paths(cx);
        view.look_after_history(window, cx);
        view.warn_about_cloud(window, cx);
        view.refresh(cx);
        view
    }

    /// A project just made in a folder a sync service copies says so once. The card and the panel keep saying it.
    fn warn_about_cloud(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new = cx.try_global::<NewProject>().map(|project| project.0.clone());
        let Some(book) = self.book(cx).filter(|book| new.as_ref() == Some(book)) else {
            return;
        };
        if let Some(service) = repo::cloud_service(&book) {
            self.show_note(crate::t!(Key::CloudWarning, service = service), window, cx);
        }
    }

    fn session<'a>(&self, cx: &'a App) -> &'a Session {
        self.parts.editor.read(cx).session()
    }

    /// The folder the repo is looked for from, or nothing for a single file.
    fn book(&self, cx: &App) -> Option<PathBuf> {
        match self.session(cx).kind() {
            Kind::Project { dir } => Some(dir.clone()),
            Kind::Single { .. } => None,
        }
    }

    /// The views observe this one, so a view that called in here is free again by the time it hears.
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    // Local history

    /// At opening: finds history that moved with the book, says so, and thins what is old.
    fn look_after_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn keep_waiting(&mut self, cx: &App) {
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
    fn label_all(&self, label: &str, cx: &mut Context<Self>) {
        label_history(self.session(cx), label, cx);
    }

    /// A chapter's file changed name: its log goes along.
    fn session_changed(&mut self, cx: &mut Context<Self>) {
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

    fn chapter_paths(&self, cx: &App) -> HashMap<ChapterId, PathBuf> {
        let session = self.session(cx);
        if matches!(session.kind(), Kind::Single { .. }) {
            return HashMap::new();
        }
        (session.files().into_iter())
            .filter_map(|path| Some((session.chapter_of_file(&path)?, path)))
            .collect()
    }

    /// A change from outside: what the file held before it goes into local history, with a label, and so does the result.
    fn outside_change(&mut self, change: &FileChange, cx: &mut Context<Self>) {
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

    // The repo

    /// Reads the repo again, off the main thread.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(book) = self.book(cx) else {
            return;
        };
        let work = cx.background_spawn(async move {
            match Repo::open(&book) {
                Ok(None) => RepoState::NoRepo,
                Ok(Some(repo)) => match repo.overview(LOG_ROWS) {
                    Ok(overview) => RepoState::Ready(Box::new(overview)),
                    Err(problem) => RepoState::Broken(describe(&problem)),
                },
                Err(problem) => RepoState::Broken(describe(&problem)),
            }
        });
        self.reading = cx.spawn(async move |view, cx| {
            let state = work.await;
            view.update(cx, |view, cx| {
                if view.state != state {
                    view.state = state;
                    view.changed(cx);
                    if view.card.read(cx).is_open() {
                        with_view(cx, Versions::prepare_open_card);
                    }
                }
                if view.card_after_reading {
                    view.card_after_reading = false;
                    with_view(cx, Versions::show_card);
                }
            })
            .ok();
        });
    }

    /// A fetch now and then while the panel or the timeline is open, for Behind to mean something. Quiet about failing:
    /// the author may be offline, and a push or pull says why.
    fn fetch_now_and_then(&mut self, cx: &mut Context<Self>) {
        let has_remote = matches!(&self.state, RepoState::Ready(overview) if overview.remote.is_some());
        let (Some(git), Some(book), true) = (self.git.clone(), self.book(cx), has_remote) else {
            return;
        };
        if self.last_fetch.is_some_and(|last| last.elapsed() < FETCH_GAP) {
            return;
        }
        self.last_fetch = Some(Instant::now());
        let work = cx.background_spawn(async move { git.fetch(&book) });
        cx.spawn(async move |view, cx| {
            if work.await.is_ok() {
                view.update(cx, |view, cx| view.refresh(cx)).ok();
            }
        })
        .detach();
    }

    /// Writes what is unsaved, then does `then`. A conflict or a failed save stops it, and says why.
    fn after_settling(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.error = None;
        crate::persist::commit_floats(window, cx);
        let Some(closing) = crate::persist::settle(window, cx) else {
            then(self, window, cx);
            return;
        };
        cx.spawn_in(window, async move |view, cx| {
            let Ok(closing) = closing.await else {
                return;
            };
            view.update_in(cx, |view, window, cx| match closing {
                Closing::Saved => then(view, window, cx),
                Closing::Held => view.fail(crate::t!(Key::HeldConflict).to_owned(), cx),
                Closing::Failed { file, reason } => {
                    let file = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    view.fail(crate::t!(Key::CannotSave, file = file, reason = reason), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn fail(&mut self, text: String, cx: &mut Context<Self>) {
        self.error = Some(text);
        self.changed(cx);
    }

    /// Runs a job on another thread and `done` on this one, then reads the repo again.
    fn work<T: Send + 'static>(
        &mut self,
        job: impl FnOnce() -> Result<T, String> + Send + 'static,
        done: Done<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.working = true;
        self.changed(cx);
        let job = cx.background_spawn(async move { job() });
        cx.spawn_in(window, async move |view, cx| {
            let result = job.await;
            view.update_in(cx, |view, window, cx| {
                view.working = false;
                done(view, result, window, cx);
                view.refresh(cx);
                view.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Commits everything in the book, on the author's say, and pushes after if `push`.
    fn commit(&mut self, message: String, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        let message = message.trim().to_owned();
        if self.working || message.is_empty() {
            return;
        }
        self.after_settling(window, cx, move |view, window, cx| {
            let Some(book) = view.book(cx) else {
                return;
            };
            let git = view.git.clone();
            let new_upstream = view.new_upstream();
            let job = move || {
                let mut repo = open_repo(&book)?;
                repo.commit(&message, git.as_deref())
                    .map_err(|problem| describe(&problem))?;
                if push && let Some(git) = &git {
                    let target = new_upstream
                        .as_ref()
                        .map(|(remote, branch)| (remote.as_str(), branch.as_str()));
                    if let Err(reason) = git.push(&book, target) {
                        return Ok(Some(crate::t!(Key::PushFailed, reason = reason)));
                    }
                }
                Ok(None)
            };
            let done: Done<Option<String>> = Box::new(|view, result, window, cx| match result {
                Ok(problem) => {
                    view.error = problem;
                    view.awaiting_merge = false;
                    view.card
                        .update(cx, |card, cx| card.committed(view.error.is_none(), window, cx));
                    view.panel.update(cx, |panel, cx| panel.committed(cx));
                }
                Err(reason) => view.error = Some(reason),
            });
            view.work(job, done, window, cx);
        });
    }

    /// The remote and branch of a first push, for a branch with no upstream yet.
    fn new_upstream(&self) -> Option<(String, String)> {
        let RepoState::Ready(overview) = &self.state else {
            return None;
        };
        (overview.sync == repo::Sync::NotPushed && !overview.detached)
            .then(|| Some((overview.remote.clone()?, overview.branch.clone())))
            .flatten()
    }

    fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(git), Some(book)) = (self.git.clone(), self.book(cx)) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        let target = self.new_upstream();
        let job = move || {
            let target = target
                .as_ref()
                .map(|(remote, branch)| (remote.as_str(), branch.as_str()));
            git.push(&book, target)
        };
        let done: Done<()> = Box::new(|view, result, _, _| view.error = result.err());
        self.work(job, done, window, cx);
    }

    fn pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_merge(MergeFrom::Upstream, crate::t!(Key::BeforePull).to_owned(), window, cx);
    }

    fn merge_branch(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        let label = crate::t!(Key::BeforeMerge, branch = branch);
        self.run_merge(MergeFrom::Branch(branch), label, window, cx);
    }

    /// Merges into the current branch through `git`. While it runs, the files' changes are not taken in, because a file
    /// `git` leaves in conflict holds marker lines for a moment. Then what overlaps goes to the existing conflict line.
    fn run_merge(&mut self, from: MergeFrom, label: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.working || self.git.is_none() {
            return;
        }
        self.after_settling(window, cx, move |view, window, cx| {
            let (Some(git), Some(book)) = (view.git.clone(), view.book(cx)) else {
                return;
            };
            view.label_all(&label, cx);
            let session = view.session(cx);
            let takeable: HashSet<PathBuf> = (session.files().into_iter())
                .filter(|path| session.file_text(path).is_some() && session.chapter_of_file(path).is_some())
                .collect();
            view.persist.update(cx, |persist, _| persist.hold_outside_changes(true));
            let job = move || merge_job(&git, &book, from, &takeable);
            let done: Done<MergeEnd> = Box::new(|view, result, window, cx| view.merge_finished(result, window, cx));
            view.work(job, done, window, cx);
        });
    }

    fn merge_finished(&mut self, result: Result<MergeEnd, String>, window: &mut Window, cx: &mut Context<Self>) {
        self.quiet = true;
        match result {
            Ok(MergeEnd::Done) => {}
            Ok(MergeEnd::Outside(file)) => self.error = Some(crate::t!(Key::MergeOutside, file = file)),
            Ok(MergeEnd::Conflicts(conflicts)) => {
                let persist = self.persist.clone();
                self.awaiting_merge = true;
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
        self.persist.update(cx, |persist, cx| {
            persist.hold_outside_changes(false);
            persist.scan_now(window, cx);
        });
        self.quiet = false;
        self.conflicts_settled(cx);
    }

    /// The last conflict of a merge was settled: the author commits it.
    fn conflicts_settled(&mut self, cx: &mut Context<Self>) {
        if !self.awaiting_merge || self.working || self.persist.read(cx).has_conflicts() {
            return;
        }
        self.awaiting_merge = false;
        self.card_after_reading = true;
        self.refresh(cx);
    }

    fn abort_merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(git), Some(book)) = (self.git.clone(), self.book(cx)) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        self.awaiting_merge = false;
        self.persist.update(cx, |persist, _| persist.hold_outside_changes(true));
        let job = move || git.abort_merge(&book);
        let done: Done<()> = Box::new(|view, result, window, cx| {
            view.error = result.err();
            view.quiet = true;
            view.persist.update(cx, |persist, cx| {
                persist.hold_outside_changes(false);
                persist.scan_now(window, cx);
            });
            view.quiet = false;
        });
        self.work(job, done, window, cx);
    }

    /// Switches branch through `git`. Unsaved text is written first and labelled, and changes that overlap stop it.
    fn switch(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.working || self.git.is_none() {
            return;
        }
        self.after_settling(window, cx, move |view, window, cx| {
            let (Some(git), Some(book)) = (view.git.clone(), view.book(cx)) else {
                return;
            };
            view.label_all(&crate::t!(Key::BeforeSwitch, branch = branch), cx);
            view.persist.update(cx, |persist, _| persist.hold_outside_changes(true));
            let job = move || git.switch(&book, &branch).map_err(|refused| describe_refused(&refused));
            let done: Done<()> = Box::new(|view, result, window, cx| {
                view.error = result.err();
                view.quiet = true;
                view.persist.update(cx, |persist, cx| {
                    persist.hold_outside_changes(false);
                    persist.scan_now(window, cx);
                });
                view.quiet = false;
            });
            view.work(job, done, window, cx);
        });
    }

    fn create_branch(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(book) = self.book(cx) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        let job = move || {
            open_repo(&book)?
                .create_branch(&name)
                .map_err(|problem| describe(&problem))
        };
        let done: Done<()> = Box::new(|view, result, _, _| view.error = result.err());
        self.work(job, done, window, cx);
    }

    fn delete_branch(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(book) = self.book(cx) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        let job = move || {
            open_repo(&book)?
                .delete_branch(&name)
                .map_err(|problem| describe(&problem))
        };
        let done: Done<()> = Box::new(|view, result, _, _| view.error = result.err());
        self.work(job, done, window, cx);
    }

    /// Makes the book a repo, when the author said so.
    fn make_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(book) = self.book(cx) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        let job = move || Repo::init(&book).map(|_| ()).map_err(|problem| describe(&problem));
        let done: Done<()> = Box::new(|view, result, _, _| view.error = result.err());
        self.work(job, done, window, cx);
    }

    fn set_identity(&mut self, name: String, email: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(book) = self.book(cx) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        let job = move || {
            open_repo(&book)?
                .set_identity(&name, &email)
                .map_err(|problem| describe(&problem))
        };
        let done: Done<()> = Box::new(|view, result, _, _| view.error = result.err());
        self.work(job, done, window, cx);
    }

    /// One step of `DRUCK_VERSIONS_RUN`, separated by semicolons: `commit:message`, `create:branch`, `switch:branch`,
    /// `merge:branch`, `delete:branch`, `abort`, `push`, `pull`, `repo`, `keep:mine`, `keep:theirs`, or `keep:both`. These call
    /// what the buttons call.
    fn run_scripted(&mut self, step: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (name, value) = step.split_once(':').unwrap_or((step, ""));
        let value = value.to_owned();
        match name {
            "commit" => self.commit(value, false, window, cx),
            "commit-push" => self.commit(value, true, window, cx),
            "create" => self.create_branch(value, window, cx),
            "switch" => self.switch(value, window, cx),
            "merge" => self.merge_branch(value, window, cx),
            "delete" => self.delete_branch(value, window, cx),
            "abort" => self.abort_merge(window, cx),
            "push" => self.push(window, cx),
            "pull" => self.pull(window, cx),
            "repo" => self.make_repo(window, cx),
            "keep" => {
                let choice = match value.as_str() {
                    "theirs" => crate::persist::merge::Choice::Theirs,
                    "both" => crate::persist::merge::Choice::Both,
                    _ => crate::persist::merge::Choice::Mine,
                };
                self.persist
                    .update(cx, |persist, cx| persist.settle_first(choice, window, cx));
            }
            _ => eprintln!("no such step: {step}"),
        }
        eprintln!("step: {step}: {:?}", self.error);
    }

    // What the views open

    fn toggle_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.card.read(cx).is_open() {
            self.card.update(cx, |card, cx| card.close(window, cx));
        } else {
            self.show_card(window, cx);
        }
    }

    fn show_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.card.update(cx, |card, cx| card.open(window, cx));
        self.prepare_open_card(window, cx);
        // Saved text first, so the card lists what a commit would hold.
        self.after_settling(window, cx, |view, _, cx| view.refresh(cx));
    }

    /// What the open card needs from what is known of the repo, when it opens or that changes: where the keys go, what a
    /// merge suggests as a message, and whether this is the once that a folder is asked to become a repo.
    fn prepare_open_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (message, field) = match &self.state {
            RepoState::Ready(overview) if !overview.has_identity => (None, CardField::Identity),
            RepoState::Ready(overview) => (overview.merging.clone(), CardField::Message),
            RepoState::Reading => (None, CardField::Message),
            RepoState::Single | RepoState::NoRepo | RepoState::Broken(_) => (None, CardField::None),
        };
        let asking = matches!(self.state, RepoState::NoRepo)
            && (self.book(cx).zip(self.store.clone())).is_some_and(|(book, store)| store.first_asking(&book));
        self.card
            .update(cx, |card, cx| card.prepare(message, asking, field, window, cx));
    }

    fn toggle_timeline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, cx| {
            if timeline.is_open() {
                timeline.close(window, cx);
            } else {
                timeline.open(window, cx);
            }
        });
        self.refresh(cx);
        self.fetch_now_and_then(cx);
    }

    fn has_rail_tab(&self, cx: &App) -> bool {
        cx.has_global::<RailTab>()
    }

    /// While the panel is open, other tools may move the repo.
    fn watch_repo_while_open(&mut self, cx: &mut Context<Self>) {
        self.panel_watch = cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(PANEL_LOOK).await;
                let open = view.update(cx, |view, cx| {
                    let open = view.panel.read(cx).is_open();
                    if open {
                        view.refresh(cx);
                    }
                    open
                });
                if !matches!(open, Ok(true)) {
                    return;
                }
            }
        });
    }

    fn toggle_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode(cx) == Mode::Publish && self.has_rail_tab(cx) {
            cx.update_global::<RailTab, _>(|tab, cx| (tab.0)(window, cx));
            self.refresh(cx);
            return;
        }
        let opening = !self.panel.read(cx).is_open();
        self.panel.update(cx, |panel, cx| panel.set_open(opening, window, cx));
        if opening {
            self.refresh(cx);
            self.fetch_now_and_then(cx);
            self.watch_repo_while_open(cx);
        } else if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| workspace.focus_main(window, cx));
        }
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |_, cx| cx.notify());
        }
    }

    /// What the views read.
    fn state(&self) -> &RepoState {
        &self.state
    }

    fn is_working(&self) -> bool {
        self.working
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn git_installed(&self) -> bool {
        self.git.is_some()
    }

    fn unplaced_history(&self, path: &Path) -> Option<&Orphan> {
        self.unplaced.get(path)
    }

    /// The history of a file the author said is this one's.
    fn claim_history(&mut self, path: &Path, cx: &mut Context<Self>) {
        let (Some(store), Some(orphan)) = (self.store.clone(), self.unplaced.remove(path)) else {
            return;
        };
        let to = path.to_owned();
        let work = cx.background_spawn(async move { store.claim(&orphan, &to) });
        cx.spawn(async move |view, cx| {
            let result = work.await;
            view.update(cx, |view, cx| {
                view.error = result.err().map(|error| error.to_string());
                view.timeline.update(cx, |timeline, cx| timeline.reload(cx));
                view.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Whether a file waits on the conflict line.
    fn in_conflict(&self, path: &Path, cx: &App) -> bool {
        self.persist.read(cx).has_conflict_in(path)
    }

    /// A chapter's name for a list: the book's title for it, or its path.
    fn title_of(&self, path: &Path, name: &str, cx: &App) -> String {
        let session = self.session(cx);
        (session.chapter_of_file(path)).map_or_else(|| name.to_owned(), |chapter| session.title_of(chapter))
    }

    fn book_folder(&self, cx: &App) -> Option<PathBuf> {
        self.book(cx)
    }

    fn mode(&self, cx: &App) -> Mode {
        self.workspace
            .upgrade()
            .map_or(Mode::Write, |workspace| workspace.read(cx).mode())
    }

    fn show_note(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.note = Some(text.into());
        self.note_task = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(NOTE_TIME).await;
            view.update(cx, |view, cx| {
                view.note = None;
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }
}

impl Render for Versions {
    /// A passing note at the window's foot, in the saving state's form.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(note) = &self.note else {
            return div().into_any_element();
        };
        let tokens = Tokens::get(cx);
        div()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .px(tokens::SPACE_3)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .child(div().min_w_0().truncate().child(note.clone()))
            .into_any_element()
    }
}

fn open_repo(book: &Path) -> Result<Repo, String> {
    match Repo::open(book) {
        Ok(Some(repo)) => Ok(repo),
        Ok(None) => Err(describe(&Problem::Other("this book is not a Git repo".to_owned()))),
        Err(problem) => Err(describe(&problem)),
    }
}

fn merge_job(git: &Git, book: &Path, from: MergeFrom, takeable: &HashSet<PathBuf>) -> Result<MergeEnd, String> {
    let repo = open_repo(book)?;
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
            let conflicts = open_repo(book)?.conflicts().map_err(|problem| describe(&problem))?;
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
            Ok(MergeEnd::Conflicts(conflicts))
        }
        Err(refused) => Err(describe_refused(&refused)),
    }
}

fn describe_refused(refused: &Refused) -> String {
    match refused {
        Refused::LocalChanges => crate::t!(Key::LocalChanges).to_owned(),
        Refused::Said(text) => text.clone(),
    }
}

fn describe(problem: &Problem) -> String {
    match problem {
        Problem::NothingToCommit => crate::t!(Key::NothingToCommit).to_owned(),
        Problem::NoIdentity => crate::t!(Key::NoIdentity).to_owned(),
        Problem::Busy(operation) => crate::t!(Key::BusyElsewhere, operation = operation),
        Problem::UnmergedElsewhere(file) => crate::t!(Key::MergeOutside, file = file),
        Problem::BranchExists(name) => crate::t!(Key::BranchExists, name = name),
        Problem::BadName(name) => crate::t!(Key::BadName, name = name),
        Problem::Other(text) => text.clone(),
    }
}

/// The current branch's state against its upstream, in words.
fn sync_words(sync: repo::Sync) -> String {
    use repo::Sync;
    match sync {
        Sync::NoRemote => crate::t!(Key::NoRemote).to_owned(),
        Sync::NotPushed => crate::t!(Key::NotPushed).to_owned(),
        Sync::Synced => crate::t!(Key::Synced).to_owned(),
        Sync::Ahead(count) => crate::t!(Key::Ahead, count = count),
        Sync::Behind(count) => crate::t!(Key::Behind, count = count),
        Sync::Diverged { ahead, behind } => crate::t!(Key::Diverged, ahead = ahead, behind = behind),
    }
}

/// What a changed file's row says at its end.
fn change_words(change: &repo::Change, in_conflict: bool) -> String {
    use repo::ChangeKind as Kind;
    if in_conflict {
        return crate::t!(Key::InConflict).to_owned();
    }
    if change.kind == Kind::Removed {
        return crate::t!(Key::Deleted).to_owned();
    }
    match change.words {
        None => crate::t!(Key::Binary).to_owned(),
        Some((0, 0)) => crate::t!(Key::WordsSame).to_owned(),
        Some((1, 0)) => crate::t!(Key::OneWordAdded).to_owned(),
        Some((0, 1)) => crate::t!(Key::OneWordRemoved).to_owned(),
        Some((added, 0)) => crate::t!(Key::WordsAdded, added = added),
        Some((0, removed)) => crate::t!(Key::WordsRemoved, removed = removed),
        Some((added, removed)) => crate::t!(Key::WordsBoth, added = added, removed = removed),
    }
}
