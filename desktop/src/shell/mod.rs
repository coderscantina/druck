//! The shell: the diagnostics and export panels, the Binder's dots, project search and the find bar, the word count,
//! the string tables, the menus, the command table, and what applies the settings.
//!
//! Everything without gpui is in plain modules with their own tests: [`report`] groups diagnostics, [`facts`] reads
//! placeholders and word counts, [`search`] finds and replaces, [`manifest`] splices exports into `druck.yaml`,
//! [`export`] runs an export, [`state`] keeps what the app remembers, and [`strings`] is the string table.
//!
//! The views are one per file, and [`Hub`] ties them to the document: it listens to the editor and the preview, and runs
//! the work behind each panel. It is mounted by [`mount`] and reached by the menus through [`install`].
//!
//! # Strings for other slices
//!
//! A slice keeps its strings in a `strings.rs` of its own, made with [`strings!`](crate::strings), and reads them with
//! [`t!`](crate::t). See [`strings`].

pub mod commands;
pub mod diagnostics;
pub mod engine_text;
pub mod export;
pub mod export_panel;
pub mod facts;
pub mod find_bar;
pub mod footer;
pub mod manifest;
pub mod menus;
pub mod report;
pub mod search;
pub mod search_panel;
pub mod state;
pub mod strings;
pub mod text;
mod ui;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use druck::Failure;
use druck::cancel::Cancel;
use druck::diagnostic::Diagnostic;
use druck::input::MANIFEST;
use gpui::{
    AnyWindowHandle, App, AppContext, Context, Entity, Focusable, Global, PromptLevel, Task, WeakEntity, Window,
    prelude::*,
};

use self::diagnostics::{DiagnosticsEvent, DiagnosticsPanel};
use self::export::{Request, Stage, Target};
use self::export_panel::{ExportEvent, ExportPanel, RunState, Setup};
use self::find_bar::{FindBar, FindEvent};
use self::footer::Footer;
use self::menus::MenuState;
use self::search::{Matcher, Options, Source};
use self::search_panel::{SearchEvent, SearchPanel, Status};
use self::state::State;
use self::strings::LanguageSetting;
use self::text::Key;
use crate::editor::{CaretMoved, SessionChanged, TextChanged};
use crate::inspector::Tab;
use crate::preview::proof::Problem;
use crate::preview::{Laid, ZoomChanged};
use crate::session::{ChapterId, Kind};
use crate::settings;
use crate::source::EditKind;
use crate::tokens::{self, Scheme, Tokens};
use crate::workspace::{
    Mode, ModeChanged, NewSection, Parts, ShowPublish, ShowWrite, Slots, Workspace, ZoomIn, ZoomOut, ZoomReset,
};

/// How long typing in the search panel pauses before it searches.
const SEARCH_PAUSE: Duration = Duration::from_millis(150);
/// How often the export panel hears how far a run is.
const PROGRESS_PAUSE: Duration = Duration::from_millis(100);
/// Stack of the export thread, as the preview's layout thread has: the engine reads a nested theme.
const EXPORT_STACK: usize = 64 * 1024 * 1024;

// Preferences

/// What the app remembers, and what the menus show, for the whole process.
pub struct Prefs {
    state: State,
    file: Option<std::path::PathBuf>,
    /// An appearance the command line fixed, until the settings choose another.
    forced: Option<Scheme>,
    menu: MenuState,
    /// The window's size as last drawn, written with the rest at quit.
    window: Option<(f32, f32)>,
    /// The settings' language and appearance as last applied, to tell what changed.
    applied: (LanguageSetting, settings::Appearance),
    /// A measurement run leaves the owner's saved state alone.
    persist: bool,
}

impl Global for Prefs {}

impl Prefs {
    pub fn window_size(&self) -> Option<(f32, f32)> {
        self.state.window
    }

    /// Write's zoom from the last run.
    pub fn write_zoom(&self) -> Option<f32> {
        self.state.write_zoom
    }

    /// The pages' zoom from the last run, or `None` to fit them.
    pub fn publish_zoom(&self) -> Option<f32> {
        self.state.publish_zoom
    }
}

/// Loads the settings and what was saved, and sets the language. `forced` is an appearance from the command line.
pub fn init(forced: Option<Scheme>, cx: &mut App) {
    settings::init(cx);
    let file = state::state_file();
    let state = file.as_deref().map(State::load).unwrap_or_default();
    let applied = {
        let settings = settings::get(cx);
        (settings.language, settings.appearance)
    };
    strings::set_language(applied.0.resolve());
    cx.set_global(Prefs {
        state,
        file,
        forced,
        menu: MenuState {
            binder_shown: settings::get(cx).binder,
            agent: settings::get(cx).agent,
            ..MenuState::default()
        },
        window: None,
        applied,
        persist: crate::gate::var("DRUCK_GATE").is_none(),
    });
    cx.observe_global::<settings::Store>(settings_changed).detach();
}

pub fn prefs(cx: &App) -> &Prefs {
    cx.global()
}

fn save_prefs(cx: &mut App) {
    let prefs = cx.global_mut::<Prefs>();
    if !prefs.persist {
        return;
    }
    prefs.state.window = prefs.window.or(prefs.state.window);
    if let Some(file) = &prefs.file
        && let Err(problem) = prefs.state.save(file)
    {
        eprintln!("{problem}");
    }
}

/// Keeps the window's size to write at quit.
pub fn remember_window(size: (f32, f32), cx: &mut App) {
    cx.global_mut::<Prefs>().window = Some(size);
}

/// Writes the state when the window closes or the app quits.
pub fn save_at_exit(cx: &mut App) {
    save_prefs(cx);
}

/// Sets the tokens from the settings and the system, and repaints every window, including cached panes.
pub fn apply_appearance(cx: &mut App) {
    let settings = settings::get(cx);
    let scheme = cx
        .global::<Prefs>()
        .forced
        .or(settings.appearance.scheme())
        .unwrap_or_else(|| cx.window_appearance().into());
    let tokens = Tokens::new(scheme, settings.accent);
    cx.set_global(tokens);
    cx.refresh_windows();
}

/// Applies what the settings changed that is not the text's own: language, appearance, and accent. The text and the
/// panels observe the settings for the rest.
fn settings_changed(cx: &mut App) {
    let (language, appearance, binder) = {
        let settings = settings::get(cx);
        (settings.language, settings.appearance, settings.binder)
    };
    let agent = settings::get(cx).agent;
    let prefs = cx.global_mut::<Prefs>();
    let was = prefs.applied;
    prefs.applied = (language, appearance);
    if appearance != was.1 {
        prefs.forced = None;
    }
    let binder_changed = prefs.menu.binder_shown != binder || prefs.menu.agent != agent;
    prefs.menu.binder_shown = binder;
    prefs.menu.agent = agent;
    apply_appearance(cx);
    if language != was.0 {
        strings::set_language(language.resolve());
    }
    if language != was.0 || binder_changed {
        refresh_menus(cx);
    }
}

/// Adds a document to the recent list, in the app and in the dock.
pub fn document_opened(path: &Path, cx: &mut App) {
    cx.global_mut::<Prefs>().state.remember(path);
    cx.add_recent_document(path);
    save_prefs(cx);
    refresh_menus(cx);
}

/// Opens the recent document at `index` of the menu, by the path it lists.
pub fn recent(index: usize, cx: &App) -> Option<std::path::PathBuf> {
    existing(&cx.global::<Prefs>().state.recent).into_iter().nth(index)
}

/// The recent documents that still exist, in the menu's order.
pub fn recent_documents(cx: &App) -> Vec<std::path::PathBuf> {
    existing(&cx.global::<Prefs>().state.recent)
}

fn existing(paths: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    paths.iter().filter(|path| path.exists()).cloned().collect()
}

/// Builds the menus again, after the language, the recent documents, or what is shown changed.
pub fn refresh_menus(cx: &mut App) {
    let prefs = cx.global::<Prefs>();
    let state = MenuState {
        recent: existing(&prefs.state.recent),
        ..prefs.menu.clone()
    };
    cx.set_menus(menus::build(&state));
}

// Reaching the document from actions

/// The hub of the document, and the window it is in, for the actions that have no view of their own to land on.
struct Current(WeakEntity<Hub>, AnyWindowHandle);

impl Global for Current {}

/// Runs `work` on the hub once the action that asked for it has finished, since the window is busy until then.
fn with_hub(cx: &mut App, work: impl FnOnce(&mut Hub, &mut Window, &mut Context<Hub>) + 'static) {
    cx.defer(move |cx| {
        let Some((hub, window)) = cx
            .try_global::<Current>()
            .and_then(|current| Some((current.0.upgrade()?, current.1)))
        else {
            return;
        };
        window
            .update(cx, |_, window, cx| hub.update(cx, |hub, cx| work(hub, window, cx)))
            .ok();
    });
}

fn with_window(cx: &mut App, work: impl FnOnce(&mut Window, &mut App) + 'static) {
    cx.defer(move |cx| {
        if let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) {
            window.update(cx, |_, window, cx| work(window, cx)).ok();
        }
    });
}

/// Binds the shell's keys, answers its actions, and sets the menus. Once, when the app starts.
pub fn install(cx: &mut App) {
    menus::bind_keys(cx);
    cx.on_action(|_: &menus::Find, cx| with_hub(cx, Hub::find));
    cx.on_action(|_: &menus::FindNext, cx| with_hub(cx, |hub, window, cx| hub.step_find(false, window, cx)));
    cx.on_action(|_: &menus::FindPrevious, cx| with_hub(cx, |hub, window, cx| hub.step_find(true, window, cx)));
    cx.on_action(|_: &menus::FindInProject, cx| with_hub(cx, Hub::search_project));
    cx.on_action(|_: &menus::ShowSettings, cx| settings::window::open(None, cx));
    cx.on_action(|_: &menus::ShowExport, cx| with_hub(cx, |hub, w, cx| hub.show_tab(Tab::Export, w, cx)));
    cx.on_action(|_: &menus::ShowStyles, cx| with_hub(cx, |hub, w, cx| hub.show_tab(Tab::Styles, w, cx)));
    cx.on_action(|_: &menus::ChooseTheme, cx| with_hub(cx, Hub::choose_theme));
    cx.on_action(|_: &menus::ShowFonts, cx| with_hub(cx, |hub, w, cx| hub.show_tab(Tab::Fonts, w, cx)));
    cx.on_action(|_: &menus::ShowImages, cx| with_hub(cx, |hub, w, cx| hub.show_tab(Tab::Images, w, cx)));
    cx.on_action(|_: &menus::ShowDetails, cx| with_hub(cx, |hub, w, cx| hub.show_tab(Tab::Details, w, cx)));
    cx.on_action(|_: &menus::ShowAgent, cx| with_hub(cx, |hub, w, cx| hub.show_tab(Tab::Agent, w, cx)));
    cx.on_action(|_: &menus::AskAgent, cx| crate::agent::ask(cx));
    cx.on_action(|_: &menus::FocusEditor, cx| with_hub(cx, Hub::focus_editor));
    cx.on_action(|_: &menus::ShowDiagnostics, cx| {
        with_hub(cx, |hub, w, cx| hub.show_tab(Tab::Proof, w, cx));
    });
    cx.on_action(|_: &menus::NextProblem, cx| with_hub(cx, |hub, w, cx| hub.walk_problems(1, w, cx)));
    cx.on_action(|_: &menus::PreviousProblem, cx| with_hub(cx, |hub, w, cx| hub.walk_problems(-1, w, cx)));
    cx.on_action(|_: &menus::ToggleBinder, cx| {
        settings::change(cx, |settings| settings.binder = !settings.binder);
        with_hub(cx, Hub::binder_toggled);
    });
    cx.on_action(|_: &menus::FocusBinder, cx| with_hub(cx, Hub::focus_binder));
    cx.on_action(|_: &menus::ToggleSpelling, cx| settings::change(cx, |s| s.spelling = !s.spelling));
    cx.on_action(|_: &menus::ToggleTypewriter, cx| settings::change(cx, |s| s.typewriter = !s.typewriter));
    cx.on_action(|_: &menus::ToggleDim, cx| settings::change(cx, |s| s.dim_paragraphs = !s.dim_paragraphs));
    cx.on_action(|_: &menus::ToggleSelectionToolbar, cx| {
        settings::change(cx, |s| s.selection_toolbar = !s.selection_toolbar);
    });
    cx.on_action(|_: &ShowWrite, cx| {
        with_hub(cx, |hub, window, cx| {
            hub.with_workspace(cx, |w, cx| w.show(Mode::Write, window, cx))
        });
    });
    cx.on_action(|_: &ShowPublish, cx| {
        with_hub(cx, |hub, window, cx| {
            hub.with_workspace(cx, |w, cx| w.show(Mode::Publish, window, cx))
        });
    });
    cx.on_action(|_: &NewSection, cx| {
        with_hub(cx, |hub, window, cx| {
            hub.with_workspace(cx, |w, cx| w.new_section(window, cx))
        });
    });
    cx.on_action(|_: &ZoomIn, cx| with_hub(cx, |hub, _, cx| hub.zoom(1, cx)));
    cx.on_action(|_: &ZoomOut, cx| with_hub(cx, |hub, _, cx| hub.zoom(-1, cx)));
    cx.on_action(|_: &ZoomReset, cx| with_hub(cx, |hub, _, cx| hub.zoom(0, cx)));
    cx.on_action(|_: &menus::About, cx| with_window(cx, about));
    cx.on_action(|_: &menus::HideApp, cx| cx.hide());
    cx.on_action(|_: &menus::HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &menus::MinimizeWindow, cx| with_window(cx, |window, _| window.minimize_window()));
    cx.on_action(|_: &menus::ZoomWindow, cx| with_window(cx, |window, _| window.zoom_window()));
    cx.on_action(|_: &menus::OpenHelp, cx| cx.open_url("https://github.com/coderscantina/druck"));
    cx.on_action(|_: &menus::ClearRecent, cx| {
        cx.global_mut::<Prefs>().state.forget_recent();
        save_prefs(cx);
        refresh_menus(cx);
    });
    refresh_menus(cx);
}

/// The licenses of the fonts and crates in the app, which travel with it in `Contents/Resources`. A build run from
/// cargo is not in a bundle and has none beside it.
fn notices_file() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let file = exe.parent()?.parent()?.join("Resources/THIRD_PARTY_NOTICES.md");
    file.is_file().then_some(file)
}

fn about(window: &mut Window, cx: &mut App) {
    let detail = crate::t!(Key::AboutVersion, version = druck::editor::VERSION);
    let notices = notices_file();
    let mut buttons = vec!["OK"];
    if notices.is_some() {
        buttons.push(crate::t!(Key::Notices));
    }
    let answer = window.prompt(PromptLevel::Info, "Druck", Some(&detail), &buttons, cx);
    cx.spawn(async move |cx| {
        if let (Ok(1), Some(file)) = (answer.await, notices) {
            cx.update(|cx| cx.open_with_system(&file));
        }
    })
    .detach();
}

/// Mounts the shell's panels, footer, and overlays. The one line `app::mount` calls.
pub fn mount(parts: &Parts, slots: &mut Slots, window: &mut Window, cx: &mut Context<Workspace>) {
    let workspace = cx.entity().downgrade();
    let hub = cx.new(|cx| Hub::new(parts.clone(), workspace.clone(), window, cx));
    cx.set_global(Current(hub.downgrade(), window.window_handle()));
    refresh_menus(cx);
    let problem_line = cx.new(settings::ProblemLine::new);
    slots.footer.push(problem_line.into());
    // The workspace keeps the hub alive by holding it as a layer that draws nothing.
    slots.overlays.push(hub.clone().into());
    let hub = hub.read(cx);
    slots.footer.push(hub.footer.clone().into());
    slots.overlays.push(hub.search.clone().into());
    slots.overlays.push(hub.find.clone().into());
}

// The hub

/// A running export.
struct Running {
    target: Target,
    cancel: Cancel,
    stage: Arc<AtomicU8>,
    _tasks: Vec<Task<()>>,
}

pub struct Hub {
    parts: Parts,
    workspace: WeakEntity<Workspace>,
    diagnostics: Entity<DiagnosticsPanel>,
    export: Entity<ExportPanel>,
    search: Entity<SearchPanel>,
    find: Entity<FindBar>,
    footer: Entity<Footer>,
    facts: facts::Cache,
    facts_task: Task<()>,
    marks: std::collections::HashMap<ChapterId, crate::sidebar::Mark>,
    /// Why the last export failed, with its errors, for the diagnostics panel.
    export_failure: Option<(String, Vec<Diagnostic>)>,
    /// Why the last change to the exports did not take.
    export_edit_error: Option<String>,
    running: Option<Running>,
    search_task: Task<()>,
    /// The count of a replacement, shown with the search that follows it.
    replaced: Option<usize>,
    /// The places of the report's entries in its order, and the one being looked at.
    problem_places: Vec<Option<(ChapterId, usize)>>,
    current_problem: Option<usize>,
    proof_hook: Option<usize>,
    /// Where the caret is, and where the find bar started.
    caret: usize,
    find_origin: usize,
    _subscriptions: Vec<gpui::Subscription>,
}

impl Hub {
    fn new(parts: Parts, workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let diagnostics = cx.new(|_| DiagnosticsPanel::new());
        let export = cx.new(|cx| ExportPanel::new(window, cx));
        let search = cx.new(|cx| SearchPanel::new(window, cx));
        let find = cx.new(|cx| FindBar::new(window, cx));
        let footer = cx.new(|_| Footer::new(parts.editor.clone()));
        parts.inspector.update(cx, |inspector, _| {
            inspector.mount(Tab::Proof, diagnostics.clone().into());
            inspector.mount(Tab::Export, export.clone().into());
        });
        let subscriptions = vec![
            cx.subscribe_in(&parts.preview, window, |hub, _, _: &Laid, window, cx| {
                hub.laid(window, cx);
                // `DRUCK_PROOF=3` looks at a problem once there are pages, for screenshots.
                if let Some(at) = hub.proof_hook.take() {
                    hub.go_to_problem(at, window, cx);
                }
            }),
            cx.subscribe(&parts.preview, |_, preview, _: &ZoomChanged, cx| {
                let zoom = preview.read(cx).zoom();
                cx.global_mut::<Prefs>().state.publish_zoom = zoom;
            }),
            cx.subscribe_in(&parts.editor, window, |hub, _, _: &SessionChanged, window, cx| {
                hub.session_changed(window, cx);
            }),
            cx.subscribe(&parts.editor, |hub, _, CaretMoved(offset), _| hub.caret = *offset),
            cx.subscribe(&parts.editor, |hub, _, _: &TextChanged, cx| {
                hub.footer.update(cx, |footer, cx| footer.typed(cx));
            }),
            cx.observe_global_in::<settings::Store>(window, |hub, window, cx| hub.settings_changed(window, cx)),
            cx.subscribe_in(&diagnostics, window, Self::diagnostics_event),
            cx.subscribe_in(&export, window, Self::export_event),
            cx.subscribe_in(&search, window, Self::search_event),
            cx.subscribe_in(&find, window, Self::find_event),
        ];
        let mut hub = Self {
            parts,
            workspace,
            diagnostics,
            export,
            search,
            find,
            footer,
            facts: facts::Cache::default(),
            facts_task: Task::ready(()),
            marks: std::collections::HashMap::new(),
            export_failure: None,
            export_edit_error: None,
            running: None,
            search_task: Task::ready(()),
            replaced: None,
            problem_places: Vec::new(),
            current_problem: None,
            proof_hook: crate::gate::var("DRUCK_PROOF").and_then(|at| at.parse().ok()),
            caret: 0,
            find_origin: 0,
            _subscriptions: subscriptions,
        };
        hub.refresh_export(window, cx);
        hub.refresh_facts(cx);
        if let Some(workspace) = hub.workspace.upgrade() {
            hub._subscriptions
                .push(cx.subscribe(&workspace, |hub, _, ModeChanged(mode), cx| {
                    let write = *mode == Mode::Write;
                    hub.footer.update(cx, |footer, cx| footer.set_shown(write, cx));
                }));
        }
        hub.restore_zoom(cx);
        hub
    }

    fn with_workspace(&self, cx: &mut Context<Self>, work: impl FnOnce(&mut Workspace, &mut Context<Workspace>)) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, work);
        }
    }

    // After a layout, and the facts of the text

    fn laid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_report(cx);
        self.refresh_facts(cx);
        self.refresh_export(window, cx);
    }

    fn session_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_export(window, cx);
        self.refresh_facts(cx);
        self.update_footer(cx);
    }

    /// The report of the latest layout and of a failed export, shown in the panel and as the sidebar's dots.
    fn refresh_report(&mut self, cx: &mut Context<Self>) {
        let preview = self.parts.preview.read(cx);
        if preview.is_specimen() {
            return;
        }
        let mut errors: Vec<Diagnostic> = preview.diagnostics().to_vec();
        // Warnings of pages that are no longer the latest text say nothing.
        let warnings = if errors.is_empty() {
            preview.warnings().to_vec()
        } else {
            Vec::new()
        };
        let notice = self.export_failure.as_ref().map(|(name, failed)| {
            errors.extend(failed.iter().cloned());
            crate::t!(Key::ExportFailed, name = name)
        });
        let session = self.parts.editor.read(cx).session();
        let title = session.details().title.clone();
        let general = if title.is_empty() {
            crate::t!(Key::ProjectSection)
        } else {
            title.as_str()
        };
        let report = report::build(session, &errors, &warnings, &self.facts.all(), general, &|page| {
            let (file, offset) = preview.first_place(page)?;
            session.locate(&file, offset)
        });
        let marks = report.marks();
        let problems: Vec<Problem> = report
            .entries()
            .map(|entry| Problem {
                // A warning about a whole page has a place only to say what chapter it is in, and marks the page.
                place: (entry.page.is_none() || entry.line.is_some())
                    .then_some(entry.place)
                    .flatten()
                    .and_then(|(id, offset)| session.file_position(id, offset)),
                page: entry.page,
                warning: entry.kind == report::Kind::Warning,
            })
            .collect();
        self.problem_places = report.entries().map(|entry| entry.place).collect();
        self.current_problem = self.current_problem.filter(|at| *at < self.problem_places.len());
        let current = self.current_problem;
        self.parts
            .preview
            .update(cx, |preview, cx| preview.set_problems(problems, cx));
        self.diagnostics.update(cx, |panel, cx| {
            panel.set(&report, notice, cx);
            panel.set_current(current, cx);
        });
        if marks != self.marks {
            self.marks.clone_from(&marks);
            self.parts
                .ticks
                .update(cx, |ticks, cx| ticks.set_marks(marks.clone(), cx));
            self.parts
                .sidebar
                .update(cx, |sidebar, cx| sidebar.set_marks(marks, cx));
        }
    }

    /// Reads the chapters whose text changed since, on a thread, and shows the result.
    fn refresh_facts(&mut self, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let stale = self.facts.stale(session);
        if stale.is_empty() {
            self.update_footer(cx);
            return;
        }
        let lang = session.details().language.clone();
        self.facts_task = cx.spawn(async move |hub, cx| {
            let read = cx
                .background_spawn(async move {
                    stale
                        .into_iter()
                        .map(|chapter| (chapter.id, chapter.revision, facts::compute(&chapter.text, &lang)))
                        .collect::<Vec<_>>()
                })
                .await;
            hub.update(cx, |hub, cx| {
                for (id, revision, found) in read {
                    hub.facts.store(id, revision, found);
                }
                hub.refresh_report(cx);
                hub.update_footer(cx);
            })
            .ok();
        });
    }

    fn update_footer(&mut self, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let chapter = self
            .facts
            .get(session.active())
            .and_then(|found| found.statistics)
            .map(|statistics| statistics.words);
        let book = self.facts.book_words(session);
        self.footer.update(cx, |footer, cx| footer.set(chapter, book, cx));
    }

    // Diagnostics

    fn diagnostics_event(
        &mut self,
        _: &Entity<DiagnosticsPanel>,
        event: &DiagnosticsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DiagnosticsEvent::Select(at) => self.go_to_problem(*at, window, cx),
            DiagnosticsEvent::DismissNotice => {
                self.export_failure = None;
                self.refresh_report(cx);
            }
        }
    }

    /// Goes to a problem on the pages, in Publish, with Write opening at the same place next.
    fn go_to_problem(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.with_workspace(cx, |workspace, cx| workspace.show(Mode::Publish, window, cx));
        self.current_problem = Some(at);
        if let Some(Some((id, offset))) = self.problem_places.get(at).copied() {
            self.open_place(id, offset..offset, window, cx);
        }
        self.parts
            .preview
            .update(cx, |preview, cx| preview.show_problem(Some(at), cx));
        self.diagnostics.update(cx, |panel, cx| panel.set_current(Some(at), cx));
    }

    /// The next problem, or the one before it, around the ends of the list.
    fn walk_problems(&mut self, direction: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = next_problem(self.current_problem, self.problem_places.len(), direction) else {
            return;
        };
        self.show_tab(Tab::Proof, window, cx);
        self.go_to_problem(at, window, cx);
    }

    /// Opens a chapter with `range` selected, or the caret where an empty range is.
    fn open_place(
        &mut self,
        id: ChapterId,
        range: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.parts.editor.update(cx, |editor, cx| {
            editor.show_place(id, range.start, window, cx);
            if !range.is_empty() {
                editor.select(range, window, cx);
            }
        });
    }

    // Export

    /// Goes to Publish, where the rail is, and shows a tab of it.
    fn show_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.with_workspace(cx, |workspace, cx| workspace.show(Mode::Publish, window, cx));
        self.parts.inspector.update(cx, |inspector, cx| inspector.show(tab, cx));
    }

    fn choose_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_workspace(cx, |workspace, cx| workspace.show(Mode::Publish, window, cx));
        self.parts
            .inspector
            .update(cx, |inspector, cx| inspector.choose_theme(cx));
    }

    /// Reads the exports from the manifest as it is on disk, which every change to it writes at once.
    fn refresh_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let setup = match session.kind() {
            Kind::Single { .. } => Setup {
                single: true,
                ..Setup::default()
            },
            Kind::Project { dir } => {
                let read = std::fs::read_to_string(dir.join(MANIFEST))
                    .map_err(|error| error.to_string())
                    .and_then(|text| manifest::read(&text));
                match read {
                    Ok(defs) => Setup {
                        single: false,
                        defs,
                        problem: self.export_edit_error.clone(),
                    },
                    Err(problem) => Setup {
                        single: false,
                        defs: Vec::new(),
                        problem: Some(format!("{} {problem}", crate::t!(Key::ManifestProblem))),
                    },
                }
            }
        };
        self.export.update(cx, |panel, cx| panel.set_setup(setup, window, cx));
    }

    fn export_event(
        &mut self,
        _: &Entity<ExportPanel>,
        event: &ExportEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExportEvent::Run(target) => self.start_export(target.clone(), window, cx),
            ExportEvent::Cancel => {
                if let Some(running) = &self.running {
                    running.cancel.cancel();
                }
            }
            ExportEvent::Set { name, key, value } => self.set_export_field(name, key, value.clone(), window, cx),
            ExportEvent::Add(name) => {
                let output = format!("build/{name}.pdf");
                self.set_export_field(name, "output", Some(output), window, cx);
            }
            ExportEvent::Open(path) => cx.open_with_system(path),
            ExportEvent::Reveal(path) => cx.reveal_path(path),
        }
    }

    /// Changes one value of an export in `druck.yaml` as a text splice the engine must read back as asked.
    fn set_export_field(
        &mut self,
        name: &str,
        key: &'static str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (edit_name, check_name) = (name.to_owned(), name.to_owned());
        let written = value.as_deref().map(|value| manifest::written(key, value));
        let result = self.parts.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    session.write_settings(
                        |text| manifest::set_field(text, &edit_name, key, written.as_deref()),
                        |settings| manifest::agrees(settings, &check_name, key, value.as_deref()),
                    )
                },
                window,
                cx,
            )
        });
        self.export_edit_error = result.err();
        self.refresh_export(window, cx);
    }

    fn start_export(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let session = self.parts.editor.read(cx).session();
        let request = Request {
            kind: session.kind().clone(),
            joined: session.joined(),
            target: target.clone(),
        };
        let cancel = Cancel::default();
        let stage = Arc::new(AtomicU8::new(0));
        let started = Instant::now();
        self.export_failure = None;
        self.refresh_report(cx);
        self.export.update(cx, |panel, cx| {
            panel.set_run(
                RunState::Running {
                    target: target.clone(),
                    stage: Stage::Preparing,
                },
                cx,
            );
        });
        let work = {
            let (cancel, stage) = (cancel.clone(), stage.clone());
            cx.background_spawn(async move {
                std::thread::scope(|scope| {
                    std::thread::Builder::new()
                        .name("druck-export".into())
                        .stack_size(EXPORT_STACK)
                        .spawn_scoped(scope, || {
                            export::run(request, &cancel, &|reached| {
                                stage.store(reached as u8, Ordering::Relaxed)
                            })
                        })
                        .expect("the system starts a thread")
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
            })
        };
        let finish = cx.spawn_in(window, {
            let target = target.clone();
            async move |hub, cx| {
                let result = work.await;
                hub.update_in(cx, |hub, window, cx| {
                    hub.finish_export(&target, result, started, window, cx)
                })
                .ok();
            }
        });
        let progress = cx.spawn(async move |hub, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_PAUSE).await;
                let alive = hub
                    .update(cx, |hub, cx| {
                        let Some(running) = &hub.running else { return false };
                        let stage = match running.stage.load(Ordering::Relaxed) {
                            0 => Stage::Preparing,
                            1 => Stage::Typesetting,
                            _ => Stage::Writing,
                        };
                        let target = running.target.clone();
                        hub.export
                            .update(cx, |panel, cx| panel.set_run(RunState::Running { target, stage }, cx));
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        });
        self.running = Some(Running {
            target,
            cancel,
            stage,
            _tasks: vec![finish, progress],
        });
    }

    fn finish_export(
        &mut self,
        target: &Target,
        result: Result<export::Written, Failure>,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.running = None;
        let state = match result {
            Ok(written) => RunState::Done {
                epub: written
                    .output
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("epub")),
                output: written.output,
                parts: written.parts,
                seconds: started.elapsed().as_secs_f32(),
            },
            Err(Failure::Cancelled) => RunState::Cancelled,
            Err(Failure::Errors(errors)) => {
                let name = match target {
                    Target::Named(name) => name.clone(),
                    Target::Pdf => "PDF".to_owned(),
                    Target::Epub => "EPUB".to_owned(),
                };
                self.export_failure = Some((name, errors));
                self.parts
                    .inspector
                    .update(cx, |inspector, cx| inspector.show(Tab::Proof, cx));
                RunState::Failed
            }
        };
        let finished = matches!(state, RunState::Done { .. });
        self.export.update(cx, |panel, cx| panel.set_run(state, cx));
        self.refresh_report(cx);
        self.refresh_export(window, cx);
        // The run button that had the keys is gone. An author who went on to the text keeps them there.
        let in_the_text = self.parts.editor.read(cx).focus_handle(cx).contains_focused(window, cx)
            || self
                .parts
                .preview
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx);
        if finished && !in_the_text {
            self.export.update(cx, |panel, cx| panel.focus_result(window, cx));
        }
    }

    // Search across the project

    fn search_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |panel, cx| panel.open(window, cx));
    }

    fn search_event(
        &mut self,
        _: &Entity<SearchPanel>,
        event: &SearchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchEvent::Changed => self.start_search(cx),
            SearchEvent::Open(id, range) => self.open_place(*id, range.clone(), window, cx),
            SearchEvent::ReplaceAll => self.replace(None, window, cx),
            SearchEvent::ReplaceFile(id) => self.replace(Some(*id), window, cx),
        }
    }

    /// Every chapter the search covers: the book, files not in it, notes, and research.
    fn searchable(&self, cx: &Context<Self>) -> Vec<(ChapterId, String, Option<String>, std::path::PathBuf)> {
        let session = self.parts.editor.read(cx).session();
        let contents = session.contents();
        let rows = contents
            .book
            .iter()
            .chain(&contents.not_in_book)
            .chain(&contents.notes)
            .chain(&contents.research);
        rows.filter_map(|row| {
            let chapter = session.chapter(row.id)?;
            let text = chapter.is_read().then(|| chapter.text().to_owned());
            Some((row.id, row.title.clone(), text, chapter.path().to_owned()))
        })
        .collect()
    }

    fn start_search(&mut self, cx: &mut Context<Self>) {
        let (query, replacement, options) = self.search.read(cx).request(cx);
        let matcher = match Matcher::new(&query, options) {
            Ok(Some(matcher)) => matcher,
            Ok(None) => {
                self.search_task = Task::ready(());
                self.search
                    .update(cx, |panel, cx| panel.set_results(Vec::new(), Status::Idle, cx));
                return;
            }
            Err(message) => {
                self.search_task = Task::ready(());
                self.search.update(cx, |panel, cx| {
                    panel.set_results(Vec::new(), Status::Invalid(message), cx)
                });
                return;
            }
        };
        let sources = self.searchable(cx);
        let after_replace = self.replaced.take();
        self.search
            .update(cx, |panel, cx| panel.set_status(Status::Searching, cx));
        self.search_task = cx.spawn(async move |hub, cx| {
            cx.background_executor().timer(SEARCH_PAUSE).await;
            let found = cx
                .background_spawn(async move {
                    let sources: Vec<Source> = sources
                        .into_iter()
                        .filter_map(|(id, title, text, path)| {
                            let text = text.or_else(|| std::fs::read_to_string(path).ok())?;
                            Some(Source { id, title, text })
                        })
                        .collect();
                    search::search(&sources, &matcher, &replacement)
                })
                .await;
            hub.update(cx, |hub, cx| {
                let status = after_replace.map_or(Status::Idle, Status::Replaced);
                hub.search.update(cx, |panel, cx| panel.set_results(found, status, cx));
            })
            .ok();
        });
    }

    /// Replaces in one chapter or in all, as one undo step per chapter, then searches again.
    fn replace(&mut self, only: Option<ChapterId>, window: &mut Window, cx: &mut Context<Self>) {
        let (query, replacement, options) = self.search.read(cx).request(cx);
        if replacement.is_empty() {
            return;
        }
        let Ok(Some(matcher)) = Matcher::new(&query, options) else {
            return;
        };
        let ids: Vec<ChapterId> = self
            .searchable(cx)
            .into_iter()
            .map(|(id, ..)| id)
            .filter(|id| only.is_none_or(|only| only == *id))
            .collect();
        let count = self.parts.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    let mut count = 0;
                    for id in ids {
                        session.load(id);
                        let edits = matcher.edits(session.buffer(id).text(), &replacement);
                        let made = edits.len();
                        if made > 0 && session.edit_chapter(id, edits, EditKind::Discrete).is_ok() {
                            count += made;
                        }
                    }
                    count
                },
                window,
                cx,
            )
        });
        self.replaced = Some(count);
        self.start_search(cx);
    }

    // Find in the open chapter

    fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_origin = self.caret;
        self.find.update(cx, |bar, cx| bar.open(window, cx));
    }

    fn find_event(&mut self, _: &Entity<FindBar>, event: &FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FindEvent::Query => self.go_to_match(self.find_origin, false, window, cx),
            FindEvent::Next => self.step_find(false, window, cx),
            FindEvent::Previous => self.step_find(true, window, cx),
            FindEvent::Closed => {
                let focus = self.parts.editor.read(cx).focus_handle(cx);
                focus.focus(window, cx);
            }
        }
    }

    /// Selects the next match from the caret, or the one before it. The bar's own text decides what a match is.
    fn step_find(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find.read(cx).is_open() {
            self.find(window, cx);
            return;
        }
        self.go_to_match(self.caret, backwards, window, cx);
    }

    fn go_to_match(&mut self, from: usize, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.find.read(cx).query(cx);
        let matches = match Matcher::new(&query, Options::default()) {
            Ok(Some(matcher)) => matcher.find(self.parts.editor.read(cx).text()),
            _ => Vec::new(),
        };
        let position = search::next_match(&matches, from, backwards).map(|at| (at, matches[at].clone()));
        if let Some((_, range)) = &position {
            self.parts
                .editor
                .update(cx, |editor, cx| editor.select(range.clone(), window, cx));
        }
        let shown = position.map(|(at, _)| (at + 1, matches.len()));
        self.find.update(cx, |bar, cx| bar.set_position(shown, cx));
    }

    // Settings, zoom, and what the menus toggle

    /// Has everything that reads strings read them again, if the language changed.
    fn settings_changed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |panel, cx| panel.refresh_text(cx));
        self.find.update(cx, |bar, cx| bar.refresh_text(cx));
        self.export.update(cx, |panel, cx| panel.refresh_text(cx));
        self.refresh_report(cx);
        self.update_footer(cx);
    }

    /// Takes each mode's zoom from the last run.
    fn restore_zoom(&mut self, cx: &mut Context<Self>) {
        let (write, publish) = {
            let prefs = prefs(cx);
            (prefs.write_zoom(), prefs.publish_zoom())
        };
        if let Some(zoom) = write {
            self.parts.editor.update(cx, |editor, cx| editor.set_zoom(zoom, cx));
        }
        if publish.is_some() {
            self.parts
                .preview
                .update(cx, |preview, cx| preview.set_zoom(publish, cx));
        }
    }

    /// Zooms what the mode on screen shows, and keeps it for the next run. `direction` is a step up or down, or 0 for back
    /// to the start.
    fn zoom(&mut self, direction: isize, cx: &mut Context<Self>) {
        let mut mode = Mode::Write;
        self.with_workspace(cx, |workspace, _| mode = workspace.mode());
        match mode {
            Mode::Write => {
                let now = self.parts.editor.read(cx).zoom();
                let zoom = next_write_zoom(now, direction);
                self.parts.editor.update(cx, |editor, cx| editor.set_zoom(zoom, cx));
                cx.global_mut::<Prefs>().state.write_zoom = (zoom != 1.).then_some(zoom);
                self.footer.update(cx, |footer, cx| footer.show_zoom(zoom, cx));
            }
            Mode::Publish => {
                self.parts.preview.update(cx, |preview, cx| {
                    if direction == 0 {
                        preview.zoom_to_fit(cx);
                    } else {
                        preview.zoom_step(direction, cx);
                    }
                });
            }
        }
    }

    /// Moves the keys back to the text when the Binder that had them went away.
    fn binder_toggled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !settings::get(cx).binder
            && self
                .parts
                .sidebar
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        {
            self.focus_editor(window, cx);
        }
    }

    fn focus_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_workspace(cx, |workspace, cx| workspace.show(Mode::Write, window, cx));
    }

    /// Moves the keys to the Binder, going to Write and showing it first if need be.
    fn focus_binder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_workspace(cx, |workspace, cx| workspace.show(Mode::Write, window, cx));
        settings::change(cx, |settings| settings.binder = true);
        let focus = self.parts.sidebar.read(cx).focus_handle(cx);
        focus.focus(window, cx);
    }
}

/// The problem after `current` in a list of `count`, or before it, around the ends.
fn next_problem(current: Option<usize>, count: usize, direction: isize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current, direction > 0) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(at), true) => (at + 1) % count,
        (Some(at), false) => (at + count - 1) % count,
    })
}

/// The next of Write's zoom steps up or down from `now`, or 100 percent for a `direction` of 0.
fn next_write_zoom(now: f32, direction: isize) -> f32 {
    let steps = tokens::WRITE_ZOOMS;
    let found = match direction {
        0 => Some(&1.),
        up if up > 0 => steps.iter().find(|step| **step > now * 1.01),
        _ => steps.iter().rev().find(|step| **step < now * 0.99),
    };
    found.copied().unwrap_or(now)
}

impl Render for Hub {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

#[cfg(test)]
mod tests {
    use super::{next_problem, next_write_zoom};

    #[test]
    fn walking_the_problems_goes_around_the_ends() {
        assert_eq!(next_problem(None, 3, 1), Some(0));
        assert_eq!(next_problem(None, 3, -1), Some(2));
        assert_eq!(next_problem(Some(2), 3, 1), Some(0));
        assert_eq!(next_problem(Some(0), 3, -1), Some(2));
        assert_eq!(next_problem(None, 0, 1), None);
    }

    #[test]
    fn write_zoom_steps_stop_at_the_ends_and_zero_goes_back() {
        assert_eq!(next_write_zoom(1., 1), 1.1);
        assert_eq!(next_write_zoom(1.1, -1), 1.);
        assert_eq!(next_write_zoom(2., 1), 2.);
        assert_eq!(next_write_zoom(0.8, -1), 0.8);
        assert_eq!(next_write_zoom(1.5, 0), 1.);
    }
}
