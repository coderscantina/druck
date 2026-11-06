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
//! For other slices: [`label_history`] before something that may change the text.

mod commit_card;
mod dates;
mod git_cli;
mod history;
mod local;
mod merge;
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
    AnyWindowHandle, App, Context, Entity, Global, KeyBinding, Subscription, Task, WeakEntity, Window, actions, div,
};

use self::commit_card::{CardField, CommitCard};
use self::git_cli::{Git, Merged, Refused};
pub use self::history::Moment;
use self::history::{History, Orphan};
pub use self::local::label_history;
use self::merge::PendingMerge;
use self::pacing::Pacing;
pub use self::panel::GitPanel;
use self::panel::Slot;
use self::repo::{Conflict, Overview, Problem, Repo};
use self::strings::Key;
use self::timeline::Timeline;
pub use self::ui::compared;
use crate::editor::{SessionChanged, TextChanged};
use crate::inspector::{Tab, TabShown};
use crate::persist::merge::{Choice, edits_between, has_markers, keep_one};
use crate::persist::recovery::data_dir;
use crate::persist::{ChoiceWithoutConflict, Closing, FileChange, GitMove, PersistView, Source};
use crate::session::{ChapterId, Kind, Session};
use crate::source::EditKind;
use crate::tokens::{self, Tokens};
use crate::workspace::{Mode, Parts, Slots};

actions!(versions, [ShowCommitCard, ShowTimeline, ShowGitPanel]);

/// How long after typing stops the open panel reads the repo again.
const REFRESH_PAUSE: Duration = Duration::from_millis(1500);
/// The time a scripted step gets before the next.
#[cfg(any(debug_assertions, feature = "measure"))]
const SCRIPT_STEP: Duration = Duration::from_secs(3);
/// How often the open panel looks whether another tool moved the repo.
const PANEL_LOOK: Duration = Duration::from_secs(5);
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
    let shown = slot.downgrade();
    slots.right.push(crate::workspace::SidePanel {
        view: slot.into(),
        width: std::rc::Rc::new(move |_, cx| {
            shown
                .upgrade()
                .map_or(gpui::px(0.), |slot| slot.read(cx).width_in_write(cx))
        }),
    });
    panel
}

fn unreviewed_changes(cx: &App) -> usize {
    #[cfg(any(debug_assertions, feature = "measure"))]
    if let Some(count) = crate::gate::var("DRUCK_UNREVIEWED").and_then(|count| count.parse().ok()) {
        return count;
    }
    crate::agent::unreviewed_changes(cx).iter().map(|file| file.count).sum()
}

/// Says how to show the Git panel's rail tab, once it holds the panel: `Ctrl+Shift+G` then shows it in Publish, and the
/// right panel stays to Write.
pub fn use_rail_tab(show: impl Fn(&mut Window, &mut App) + 'static, cx: &mut App) {
    cx.set_global(RailTab(Box::new(show)));
}

/// The project just made, to warn about once it opens, and why it could not become a repo.
#[derive(Clone)]
struct NewProject(PathBuf, Option<String>);

impl Global for NewProject {}

/// Makes a new project's folder a repo. Nothing is committed. A folder that cannot be one still works, with local
/// history, and the window's foot says so once it opens.
pub fn start_repo(folder: &Path, cx: &mut App) {
    let problem = Repo::init(folder).err().map(|problem| describe(&problem));
    cx.set_global(NewProject(folder.to_owned(), problem));
}

/// What to say at the window's foot when the book in this folder opens.
struct AtOpening(PathBuf, String);

impl Global for AtOpening {}

/// Says `text` at the window's foot once the book in `folder` opens.
pub fn say_at_opening(folder: &Path, text: String, cx: &mut App) {
    cx.set_global(AtOpening(folder.to_owned(), text));
}

/// A commit's hash as Git shortens it.
fn short_hash(hash: &str) -> &str {
    hash.get(..7).unwrap_or(hash)
}

/// `DRUCK_VERSIONS=card`, `timeline`, `panel`, or `menu` opens one, for screenshots, `DRUCK_NO_GIT` runs as with no Git
/// installed, `DRUCK_COMMIT_MESSAGE` fills the card's field, and `DRUCK_VERSIONS_RUN` runs steps from outside, see
/// [`Versions::run_scripted`], as automated mouse events do not reach the window.
#[cfg(any(debug_assertions, feature = "measure"))]
pub fn apply_gate(window: &mut Window, cx: &mut App) {
    let Some(view) = cx.try_global::<Current>().and_then(|current| current.view.upgrade()) else {
        return;
    };
    if crate::gate::var("DRUCK_NO_GIT").is_some() {
        view.update(cx, |view, _| view.git = None);
    }
    if let Some(message) = crate::gate::var("DRUCK_COMMIT_MESSAGE") {
        let card = view.read(cx).card.clone();
        card.update(cx, |card, cx| card.preset_message(message, cx));
    }
    if let Some(shown) = crate::gate::var("DRUCK_VERSIONS") {
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
    }
    if let Some(steps) = crate::gate::var("DRUCK_VERSIONS_RUN") {
        let view = view.downgrade();
        window
            .spawn(cx, async move |cx| {
                for step in steps.split(';').filter(|step| !step.is_empty()) {
                    cx.background_executor().timer(SCRIPT_STEP).await;
                    view.update_in(cx, |view, window, cx| view.run_scripted(step, window, cx))
                        .ok();
                }
            })
            .detach();
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
    /// A view asked for a fetch before the repo had been read, and it runs once it has.
    fetch_wanted: bool,
    /// A fetch came back this run, so what the repo says about its remote is worth showing.
    fetched: bool,
    /// A merge Git stopped on, until the commit that finishes it.
    merge: PendingMerge,
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
        let git = Git::find().map(Arc::new);
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
            cx.subscribe_in(
                &persist,
                window,
                |view, _, ChoiceWithoutConflict(choice), window, cx| {
                    if *choice != Choice::Both {
                        view.keep_one_side(*choice, window, cx);
                    }
                },
            ),
            cx.subscribe(&parts.inspector, |view, _, TabShown(tab), cx| {
                if *tab == Tab::Git {
                    view.refresh(cx);
                    view.fetch_now_and_then(cx);
                }
            }),
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
            pacing: Pacing::default(),
            pacing_running: false,
            paths: HashMap::new(),
            unplaced: HashMap::new(),
            refresh_pause: Task::ready(()),
            panel_watch: Task::ready(()),
            reading: Task::ready(()),
            last_fetch: None,
            fetch_wanted: false,
            fetched: false,
            merge: PendingMerge::default(),
            quiet: false,
            _subscriptions: subscriptions,
        };
        view.paths = view.chapter_paths(cx);
        view.look_after_history(window, cx);
        view.warn_about_cloud(window, cx);
        view.say_what_opening_did(window, cx);
        view.refresh(cx);
        view
    }

    /// A project just made in a folder a sync service copies says so once. The card and the panel keep saying it.
    fn warn_about_cloud(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(NewProject(new, problem)) = cx.try_global::<NewProject>().cloned() else {
            return;
        };
        let Some(book) = self.book(cx).filter(|book| *book == new) else {
            return;
        };
        if problem.is_some() {
            self.persist.update(cx, |persist, cx| {
                persist.report(&book, crate::persist::sync::Problem::Repo, problem, cx)
            });
        }
        if let Some(service) = repo::cloud_service(&book) {
            self.show_note(crate::t!(Key::CloudWarning, service = service), window, cx);
        }
    }

    fn say_what_opening_did(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(AtOpening(folder, text)) = cx.try_global::<AtOpening>() else {
            return;
        };
        if self.book(cx).as_ref() == Some(folder) {
            self.show_note(text.clone(), window, cx);
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
                    view.resume_merge(cx);
                }
                if std::mem::take(&mut view.fetch_wanted) {
                    view.fetch_now_and_then(cx);
                }
                if view.merge.take_card_after_reading() {
                    with_view(cx, Versions::show_card);
                }
            })
            .ok();
        });
    }

    /// A fetch now and then while the panel or the timeline is open, for Behind to mean something. Quiet about failing:
    /// the author may be offline, and a push or pull says why.
    fn fetch_now_and_then(&mut self, cx: &mut Context<Self>) {
        if self.state == RepoState::Reading {
            self.fetch_wanted = true;
            return;
        }
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
            let fetched = work.await;
            view.update(cx, |view, cx| {
                view.fetched = true;
                if fetched.is_ok() {
                    view.refresh(cx);
                }
                view.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Where the branch stands against its upstream, once that means something: until the first fetch returns, a remote's
    /// state is only as new as the last run's. With no Git to fetch with, it stays what the repo knows.
    fn sync_shown(&self, overview: &Overview) -> Option<String> {
        use repo::Sync;
        let waiting = self.git.is_some() && !self.fetched && !matches!(overview.sync, Sync::NoRemote | Sync::NotPushed);
        (!waiting).then(|| sync_words(overview.sync))
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
                let mut repo = open_repo(&book).map_err(|problem| describe(&problem))?;
                let hash = repo
                    .commit(&message, git.as_deref())
                    .map_err(|problem| describe(&problem))?;
                if push && let Some(git) = &git {
                    let target = new_upstream
                        .as_ref()
                        .map(|(remote, branch)| (remote.as_str(), branch.as_str()));
                    if let Err(reason) = git.push(&book, target) {
                        return Ok((hash, Some(crate::t!(Key::PushFailed, reason = reason))));
                    }
                }
                Ok((hash, None))
            };
            let done: Done<(String, Option<String>)> = Box::new(|view, result, window, cx| match result {
                Ok((hash, problem)) => {
                    view.error = problem;
                    view.merge.over();
                    view.show_note(crate::t!(Key::Committed, hash = short_hash(&hash)), window, cx);
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

    /// Switches branch through `git`. Unsaved text is written first and labelled, and changes that overlap stop it.
    fn switch(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.working || self.git.is_none() {
            return;
        }
        self.after_settling(window, cx, move |view, window, cx| {
            let (Some(git), Some(book)) = (view.git.clone(), view.book(cx)) else {
                return;
            };
            view.label_all(Moment::Switch(branch.clone()), cx);
            let source = Source::Git(GitMove::Switch, branch.clone());
            view.persist
                .update(cx, |persist, _| persist.hold_outside_changes(source));
            let job = move || git.switch(&book, &branch).map_err(|refused| describe_refused(&refused));
            let done: Done<()> = Box::new(|view, result, window, cx| {
                view.error = result.err();
                view.release_files(window, cx);
            });
            view.work(job, done, window, cx);
        });
    }

    /// Runs a change to the repo off the main thread, unless one is running. What goes wrong shows until the next action.
    fn change_repo(
        &mut self,
        job: impl FnOnce(&Path) -> Result<(), Problem> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.book(cx) else {
            return;
        };
        if self.working {
            return;
        }
        self.error = None;
        let job = move || job(&book).map_err(|problem| describe(&problem));
        let done: Done<()> = Box::new(|view, result, _, _| view.error = result.err());
        self.work(job, done, window, cx);
    }

    fn create_branch(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.change_repo(move |book| open_repo(book)?.create_branch(&name), window, cx);
    }

    fn delete_branch(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.change_repo(move |book| open_repo(book)?.delete_branch(&name), window, cx);
    }

    /// Makes the book a repo, when the author said so.
    fn make_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.change_repo(move |book| Repo::init(book).map(|_| ()), window, cx);
    }

    fn set_identity(&mut self, name: String, email: String, window: &mut Window, cx: &mut Context<Self>) {
        self.change_repo(move |book| open_repo(book)?.set_identity(&name, &email), window, cx);
    }

    /// One step of `DRUCK_VERSIONS_RUN`, separated by semicolons: `commit:message`, `create:branch`, `switch:branch`,
    /// `merge:branch`, `delete:branch`, `abort`, `push`, `pull`, `repo`, `keep:mine`, `keep:theirs`, `keep:both`, and
    /// `side:mine` or `side:theirs` for the versions See both left. These call what the buttons call.
    #[cfg(any(debug_assertions, feature = "measure"))]
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
            "side" => {
                let choice = if value == "theirs" {
                    Choice::Theirs
                } else {
                    Choice::Mine
                };
                self.keep_one_side(choice, window, cx);
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
        // The timeline covers the window, so the card goes first and its draft waits for the next time.
        if self.card.read(cx).is_open() && !self.timeline.read(cx).is_open() {
            self.card.update(cx, |card, cx| card.close(window, cx));
        }
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

    /// How a list names a changed file: a chapter of the book by its title, as the Binder names it, anything else by
    /// its path in the book.
    fn title_of(&self, path: &Path, name: &str, cx: &App) -> String {
        let session = self.session(cx);
        (session.chapter_of_file(path))
            .filter(|id| session.book().contains(id))
            .map_or_else(|| name.to_owned(), |id| session.title_of(id))
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
        self.persist
            .update(cx, |persist, cx| persist.show_note(text, window, cx));
    }
}

impl Render for Versions {
    /// The line at the window's foot while See both's markers stand during a merge, in the saving state's form.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let Some((_, chapter)) = self.merge.both_stand_in() else {
            return div().into_any_element();
        };
        let text = crate::t!(Key::BothStand, chapter = chapter);
        let side = |id, choice| {
            crate::persist::foot_button(&tokens, id, choice)
                .on_click(cx.listener(move |view, _, window, cx| view.keep_one_side(choice, window, cx)))
        };
        div()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .px(tokens::SPACE_3)
            .bg(tokens.canvas)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.accent)
            .child(div().min_w_0().truncate().child(text))
            .child(side("both-keep-mine", Choice::Mine))
            .child(side("both-take-theirs", Choice::Theirs))
            .into_any_element()
    }
}

fn open_repo(book: &Path) -> Result<Repo, Problem> {
    Repo::open(book)?.ok_or_else(|| Problem::Other(crate::t!(Key::NotARepo).to_owned()))
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
    match (change.kind, change.words) {
        (Kind::Removed | Kind::MovedAway, _) => return crate::t!(Key::Deleted).to_owned(),
        (Kind::Moved, Some((0, 0))) => return crate::t!(Key::Moved).to_owned(),
        _ => {}
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
