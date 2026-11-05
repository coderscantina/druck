//! The author's settings: one user-level `settings.json`, read at the start and whenever it changes, with a window over
//! it. What belongs to a book stays in `druck.yaml`.
//!
//! Read a value with [`get`], and change one with [`change`], which writes only what differs. Anything that depends on a
//! setting observes [`Store`] with `cx.observe_global`.

pub mod file;
mod strings;
pub mod window;

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt as _;
use gpui::prelude::*;
use gpui::{App, Context, Global, Window, div};
use notify::Watcher as _;

pub use self::file::{Appearance, LineLength, Settings};
use self::strings::Key;
use crate::tokens::{self, Tokens};

/// How long a change to the file settles before it is read, since an editor's save can be several events.
const SETTLE: Duration = Duration::from_millis(80);

/// The settings as they stand, and what is wrong with the file if anything.
pub struct Store {
    settings: Settings,
    file: Option<PathBuf>,
    problem: Option<String>,
    _watching: Option<notify::RecommendedWatcher>,
}

impl Global for Store {}

/// Reads the file. A broken one is reported and leaves the defaults.
pub fn init(cx: &mut App) {
    let file = file::settings_file();
    let (settings, problem) = match file.as_deref().map(file::load) {
        Some(Err(problem)) => (Settings::default(), Some(problem)),
        Some(Ok(settings)) => (settings, None),
        None => (Settings::default(), None),
    };
    cx.set_global(Store {
        settings,
        file,
        problem,
        _watching: None,
    });
    watch(cx);
}

pub fn get(cx: &App) -> &Settings {
    &cx.global::<Store>().settings
}

/// Why the file cannot be used.
pub fn problem(cx: &App) -> Option<&str> {
    cx.global::<Store>().problem.as_deref()
}

/// Where the file is.
pub fn file_path(cx: &App) -> Option<&Path> {
    cx.global::<Store>().file.as_deref()
}

/// Changes settings and writes what differs. The choice holds for this run even if the file cannot take it, and the
/// problem shows.
pub fn change(cx: &mut App, edit: impl FnOnce(&mut Settings)) {
    let store = cx.global_mut::<Store>();
    let before = store.settings.clone();
    edit(&mut store.settings);
    if store.settings == before {
        return;
    }
    if let Some(file) = &store.file
        && let Err(problem) = file::write_changes(file, &before, &store.settings)
    {
        store.problem = Some(problem);
    }
}

/// Opens the file in the author's editor, creating it if it is not there.
pub fn open_file(cx: &mut App) {
    let Some(file) = cx.global::<Store>().file.clone() else {
        return;
    };
    if !file.exists() {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).ok();
        }
        std::fs::write(&file, "{\n}\n").ok();
    }
    cx.open_with_system(&file);
}

/// Follows the file's folder and takes a hand edit in as soon as it lands. A folder that cannot be watched only means
/// a hand edit waits for the next run.
fn watch(cx: &mut App) {
    let Some(file) = cx.global::<Store>().file.clone() else {
        return;
    };
    let Some(folder) = file.parent().map(Path::to_owned) else {
        return;
    };
    std::fs::create_dir_all(&folder).ok();
    let (changed, mut changes) = futures::channel::mpsc::unbounded();
    let name = file.file_name().map(ToOwned::to_owned);
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let concerns_file = event.is_ok_and(|event| event.paths.iter().any(|path| path.file_name() == name.as_deref()));
        if concerns_file {
            changed.unbounded_send(()).ok();
        }
    });
    let Ok(mut watcher) = watcher else {
        return;
    };
    if watcher.watch(&folder, notify::RecursiveMode::NonRecursive).is_err() {
        return;
    }
    cx.global_mut::<Store>()._watching = Some(watcher);
    cx.spawn(async move |cx| {
        while changes.next().await.is_some() {
            cx.background_executor().timer(SETTLE).await;
            while changes.try_recv().is_ok() {}
            cx.update(reload);
        }
    })
    .detach();
}

/// Reads the file again. An invalid one keeps what was in use, and says why.
fn reload(cx: &mut App) {
    let store = cx.global::<Store>();
    let Some(file) = &store.file else {
        return;
    };
    let (settings, problem) = match file::load(file) {
        Ok(settings) => (settings, None),
        Err(problem) => (store.settings.clone(), Some(problem)),
    };
    if settings != store.settings || problem != store.problem {
        let store = cx.global_mut::<Store>();
        store.settings = settings;
        store.problem = problem;
    }
}

/// The line at the foot of the window that says the file cannot be used. It draws nothing otherwise.
pub struct ProblemLine;

impl ProblemLine {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<Store>(|_, cx| cx.notify()).detach();
        Self
    }
}

impl Render for ProblemLine {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div().when_some(problem(cx), |line, problem| {
            line.flex_1()
                .min_w_0()
                .truncate()
                .px(tokens::SPACE_3)
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.accent)
                .bg(tokens.canvas)
                .child(crate::t!(Key::Invalid, problem = problem))
        })
    }
}
