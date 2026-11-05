//! The window, and who owns what in it. One window holds one document session.
//!
//! - `app.rs`: opening the window, the File menu, opening and replacing a document, and [`mount`]. Each later slice
//!   adds its line to [`mount`] and keeps everything else in its own folder.
//! - `workspace.rs`: the two modes, the slots, and the wiring between panes.
//! - `settings/`: `settings.json` and its window. `settings::get` reads a value, and observers of `settings::Store` hear
//!   a change, by hand or from the window.
//! - `shell/commands.rs`: the table of commands behind the palette. `palette/`: `Cmd+P` and `Cmd+Shift+P`.
//! - `session/`: the document, without gpui. Read it through `EditorView::session`, change it through
//!   `EditorView::update_session`.
//! - `sidebar/`: identity, contents, the file lists, and its dialogs. Dots go in through `SidebarView::set_marks`.
//! - `editor/`, `source/`: the text. `preview/`: the pages. `inspector/`: Publish's tab rail, styles, and details.
//!
//! The later slices of [the beta milestone](../../docs/milestones/16-desktop-beta.md):
//!
//! - **Persistence** owns `persist/`, with `session/disk.rs` for what the session knows of its files: autosave, the
//!   watcher, merges, and the recovery copies. Cmd+S asks it for an immediate save through `SaveRequested`.
//! - **Editor commands** own `editor/`, whose text draws its own floats and margin notes.
//! - **Theme core** owns `theme_file/` and the panels on the `Tab::Styles` and `Tab::Fonts` tabs.
//! - **Shell** owns `shell/`: the `Tab::Proof` and `Tab::Export` panels, the Binder's dots, search and the word
//!   count, the string tables, and the rest of the menus.
//! - **Agent** owns `agent/` and `editor/review.rs`: `Cmd+J`, the chat card that is also the `Tab::Agent` tab of the rail,
//!   review in the text, and margin notes. Off until enabled in Settings.
//! - **Rich blocks** own `editor/rich/`, whose floats the text draws, and the `Tab::Images` panel.
//! - **Versions** own `versions/`: local history, the commit card, the timeline, and the Git panel.

use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, KeyBinding, PathPromptOptions, Pixels, PromptLevel, Subscription, Task,
    TitlebarOptions, Window, WindowBounds, WindowKind, WindowOptions, actions, div, point, px, size,
};

use crate::editor::Save;
use crate::launch::{LaunchEvent, LaunchView};
use crate::persist::Closing;
use crate::session::adopt::Offer;
use crate::session::{STARTERS, Session, Starter};
use crate::shell::text::Key;
use crate::shell::{self, menus};
use crate::tokens::{self, Scheme, Tokens};
use crate::workspace::{Parts, ShowPublish, ShowWrite, Slots, Workspace, titlebar};

actions!(druck_desktop, [Quit, Open, NewBook, NewPaper, NewArticle]);

/// The window the app opens to, smaller on a smaller display.
const WINDOW_SIZE: (f32, f32) = (1680., 1000.);
/// Room kept around the window on a display smaller than [`WINDOW_SIZE`].
const SCREEN_MARGIN: f32 = 80.;

/// Mounts what later slices own into the workspace's slots and tabs. One line per view.
pub fn mount(parts: &Parts, slots: &mut Slots, window: &mut Window, cx: &mut Context<Workspace>) {
    let persist = crate::persist::mount(parts, slots, window, cx);
    cx.subscribe(&persist, |workspace, _, _: &crate::persist::ResourceChanged, cx| {
        workspace.retry_failed_layout(cx);
    })
    .detach();
    shell::mount(parts, slots, window, cx);
    crate::palette::mount(parts, slots, window, cx);
    crate::inspector::mount_theme(parts, slots, cx);
    crate::agent::mount(parts, persist.clone(), slots, window, cx);
    let git_panel = crate::versions::mount(parts, &persist, slots, window, cx);
    let inspector = parts.inspector.clone();
    inspector.update(cx, |inspector, _| {
        inspector.mount(crate::inspector::Tab::Git, git_panel.into())
    });
    crate::versions::use_rail_tab(
        move |_, cx| inspector.update(cx, |inspector, cx| inspector.show(crate::inspector::Tab::Git, cx)),
        cx,
    );
    crate::editor::mount(parts, window, cx);
}

/// Opens the only window, with a document or with the launch screen. `error` says why a document asked for did not
/// open, and which it was. `theme` is the file the preview typesets with and the inspector edits. A fixed `scheme` overrides the system
/// appearance.
pub fn open(
    session: Option<Session>,
    error: Option<(String, PathBuf)>,
    theme: Option<PathBuf>,
    scheme: Option<Scheme>,
    documents: UnboundedReceiver<Vec<PathBuf>>,
    cx: &mut App,
) {
    shell::init(scheme, cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", Open, None),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-1", ShowWrite, None),
        KeyBinding::new("cmd-2", ShowPublish, None),
    ]);
    crate::editor::bind_keys(cx);
    crate::persist::bind_keys(cx);
    shell::install(cx);
    crate::palette::install(cx);
    crate::versions::install(cx);
    cx.on_app_quit(|cx| {
        shell::save_at_exit(cx);
        async {}
    })
    .detach();

    let display = cx.primary_display().map(|display| display.bounds().size);
    let fit = |wanted: f32, available: Option<Pixels>| {
        available.map_or(wanted, |available| wanted.min(f32::from(available) - SCREEN_MARGIN))
    };
    let (wanted_width, wanted_height) = shell::prefs(cx).window_size().unwrap_or(WINDOW_SIZE);
    let window_size = size(
        px(fit(wanted_width, display.map(|display| display.width))),
        px(fit(wanted_height, display.map(|display| display.height))),
    );
    let measuring = crate::gate::measuring();
    // Macs stop painting a covered window. A measurement run floats on the last display without taking focus.
    let display_id = measuring
        .then(|| cx.displays().last().map(|display| display.id()))
        .flatten();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(display_id, window_size, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some("Druck".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(tokens::TRAFFIC_LIGHTS.0), px(tokens::TRAFFIC_LIGHTS.1))),
        }),
        focus: !measuring,
        // An inactive window is held to 30 frames a second. A focused one, which is what is being measured, is not.
        inactive_frame_interval: if measuring {
            None
        } else {
            WindowOptions::default().inactive_frame_interval
        },
        kind: if measuring {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        },
        display_id,
        ..Default::default()
    };
    let window = cx.open_window(options, |window, cx| {
        shell::apply_appearance(cx);
        cx.new(|cx| Shell::new(session, error, theme, documents, window, cx))
    });
    let window = match window {
        Ok(window) => window.window_id(),
        Err(error) => {
            eprintln!("cannot open a window: {error}");
            cx.quit();
            return;
        }
    };
    // The settings window can close by itself. This one takes the app with it.
    cx.on_window_closed(move |cx, closed| {
        if closed == window {
            cx.quit();
        }
    })
    .detach();
    if !measuring {
        cx.activate(true);
    }
    crate::gate::at_launch(cx);
}

/// What to do once the document is saved.
type After = Box<dyn FnOnce(&mut Shell, &mut Window, &mut Context<Shell>)>;

/// The window's root: the launch screen, or the workspace of the one open document.
struct Shell {
    launch: Entity<LaunchView>,
    workspace: Option<Entity<Workspace>>,
    _subscriptions: Vec<Subscription>,
    _documents: Task<()>,
}

impl Shell {
    fn new(
        session: Option<Session>,
        error: Option<(String, PathBuf)>,
        theme: Option<PathBuf>,
        mut documents: UnboundedReceiver<Vec<PathBuf>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Cached panes hold colors of the old appearance, so this repaints them too.
        cx.observe_window_appearance(window, |_, _, cx| shell::apply_appearance(cx))
            .detach();
        window.on_window_should_close(cx, |_, cx| {
            shell::save_at_exit(cx);
            true
        });
        let launch = cx.new(|cx| LaunchView::new(error, cx));
        LaunchView::take_focus(&launch, window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&launch, window, |shell, _, event, window, cx| match event {
                LaunchEvent::Open => shell.ask_to_open(window, cx),
                LaunchEvent::New(starter) => shell.ask_for_new_project(starter, window, cx),
                LaunchEvent::MakeProject(dir, offer) => shell.make_project(dir, offer, window, cx),
            }),
        ];
        // A document opened from the Finder or dropped on the Dock icon. One window holds one document, so the first
        // of several wins.
        let opened = cx.spawn_in(window, async move |shell, cx| {
            while let Some(paths) = documents.next().await {
                let Some(path) = paths.into_iter().next() else {
                    continue;
                };
                let alive = shell.update_in(cx, |shell, window, cx| {
                    window.activate_window();
                    shell.open_path(&path, window, cx);
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        let mut shell = Self {
            launch,
            workspace: None,
            _subscriptions: subscriptions,
            _documents: opened,
        };
        if let Some(session) = session {
            shell.show_session(session, theme, window, cx);
        }
        shell
    }

    fn show_session(&mut self, session: Session, theme: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_title(&session.window_title());
        shell::document_opened(session.location(), cx);
        self.workspace = Some(cx.new(|cx| Workspace::new(session, theme, window, cx)));
        cx.notify();
    }

    fn ask_to_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(crate::t!(Key::OpenPrompt).into()),
        });
        cx.spawn_in(window, async move |shell, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            shell
                .update_in(cx, |shell, window, cx| shell.open_path(&path, window, cx))
                .ok();
        })
        .detach();
    }

    fn ask_for_new_project(&mut self, starter: &'static Starter, window: &mut Window, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&home, Some(starter.slug));
        cx.spawn_in(window, async move |shell, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            shell
                .update_in(cx, |shell, window, cx| match starter.create(&path) {
                    Ok(()) => {
                        crate::versions::start_repo(&path, cx);
                        shell.open_path(&path, window, cx);
                    }
                    Err(problem) => shell
                        .launch
                        .update(cx, |launch, cx| launch.set_error(Some(problem), None, cx)),
                })
                .ok();
        })
        .detach();
    }

    /// Makes a folder of Markdown a project, and opens it. It is the author's folder, so it stays out of Git until they ask.
    fn make_project(&mut self, dir: &Path, offer: &Offer, window: &mut Window, cx: &mut Context<Self>) {
        match crate::session::adopt::make_project(dir, offer) {
            Ok(()) => {
                crate::versions::say_at_opening(dir, offer.done(), cx);
                self.open_path(dir, window, cx);
            }
            Err(problem) => self.launch.update(cx, |launch, cx| launch.offer_failed(problem, cx)),
        }
    }

    /// Opens a document in this window. Text not yet on disk is written first, and a person is asked only if that fails.
    fn open_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.to_owned();
        self.after_saving(
            Key::CannotSaveBeforeOpening,
            window,
            cx,
            Box::new(move |shell, window, cx| {
                shell.replace_document(&path, window, cx);
            }),
        );
    }

    /// Closes the document and shows the launch screen, saving it first. With no document it closes the window.
    fn close_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() {
            window.remove_window();
            return;
        }
        self.after_saving(
            Key::CannotSaveBeforeClosing,
            window,
            cx,
            Box::new(|shell, window, cx| {
                shell.workspace = None;
                window.set_window_title("Druck");
                LaunchView::take_focus(&shell.launch, window, cx);
                cx.notify();
            }),
        );
    }

    /// Runs `then` once the document's text is on disk. A conflict that the footer holds does not stop it: the text
    /// stays in a recovery copy, and the file keeps the other version. Only text that could not be written asks
    /// whether to go on, with `message`. Cancel runs nothing.
    fn after_saving(&mut self, message: Key, window: &mut Window, cx: &mut Context<Self>, then: After) {
        crate::persist::commit_floats(window, cx);
        let dirty = self
            .workspace
            .as_ref()
            .is_some_and(|workspace| workspace.read(cx).is_dirty(cx));
        let Some(closing) = dirty.then(|| crate::persist::settle(window, cx)).flatten() else {
            then(self, window, cx);
            return;
        };
        cx.spawn_in(window, async move |shell, cx| {
            let Ok(closing) = closing.await else {
                return;
            };
            shell
                .update_in(cx, |shell, window, cx| match closing {
                    Closing::Saved | Closing::Held => then(shell, window, cx),
                    Closing::Failed { file, reason } => {
                        shell.ask_after_failure(message, &file, &reason, window, cx, then)
                    }
                })
                .ok();
        })
        .detach();
    }

    /// The text could not be written. Trying again, discarding it, and staying are the choices.
    fn ask_after_failure(
        &mut self,
        message: Key,
        file: &Path,
        reason: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: After,
    ) {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let detail = format!("{name}: {reason}");
        let answer = window.prompt(
            PromptLevel::Warning,
            crate::t!(message),
            Some(&detail),
            &[
                crate::t!(Key::TryAgain),
                crate::t!(Key::Discard),
                crate::t!(Key::Cancel),
            ],
            cx,
        );
        cx.spawn_in(window, async move |shell, cx| {
            let Ok(answer) = answer.await else {
                return;
            };
            shell
                .update_in(cx, |shell, window, cx| match answer {
                    0 => shell.after_saving(message, window, cx, then),
                    1 => {
                        if let Some(workspace) = &shell.workspace {
                            crate::persist::forget_unsaved(&workspace.read(cx).location(cx));
                        }
                        then(shell, window, cx);
                    }
                    _ => {}
                })
                .ok();
        })
        .detach();
    }

    /// Opens the recent document at `index` of the menu.
    fn open_recent(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = shell::recent(index, cx) {
            self.open_path(&path, window, cx);
        }
    }

    fn replace_document(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        match Session::open(path) {
            Ok(session) => {
                self.launch.update(cx, |launch, cx| launch.set_error(None, None, cx));
                self.show_session(session, None, window, cx);
            }
            Err(problem) => {
                eprintln!("{problem}");
                self.workspace = None;
                window.set_window_title("Druck");
                self.launch
                    .update(cx, |launch, cx| launch.set_error(Some(problem), Some(path), cx));
                LaunchView::take_focus(&self.launch, window, cx);
                cx.notify();
            }
        }
    }

    fn starter(slug: &str) -> Option<&'static Starter> {
        STARTERS.iter().find(|starter| starter.slug == slug)
    }

    fn new_project(&mut self, slug: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(starter) = Self::starter(slug) {
            self.ask_for_new_project(starter, window, cx);
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let bounds = window.bounds().size;
        shell::remember_window((f32::from(bounds.width), f32::from(bounds.height)), cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .text_color(tokens.text)
            .bg(tokens.canvas)
            .on_action(cx.listener(|shell, _: &Open, window, cx| shell.ask_to_open(window, cx)))
            .on_action(cx.listener(|shell, _: &NewBook, window, cx| shell.new_project("book", window, cx)))
            .on_action(cx.listener(|shell, _: &NewPaper, window, cx| shell.new_project("paper", window, cx)))
            .on_action(cx.listener(|shell, _: &NewArticle, window, cx| shell.new_project("article", window, cx)))
            .on_action(cx.listener(|shell, _: &menus::CloseDocument, window, cx| shell.close_document(window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent0, window, cx| shell.open_recent(0, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent1, window, cx| shell.open_recent(1, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent2, window, cx| shell.open_recent(2, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent3, window, cx| shell.open_recent(3, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent4, window, cx| shell.open_recent(4, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent5, window, cx| shell.open_recent(5, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent6, window, cx| shell.open_recent(6, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent7, window, cx| shell.open_recent(7, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent8, window, cx| shell.open_recent(8, window, cx)))
            .on_action(cx.listener(|shell, _: &menus::Recent9, window, cx| shell.open_recent(9, window, cx)))
            .map(|root| match self.workspace.clone() {
                Some(workspace) => root.child(workspace),
                None => root.child(titlebar(&tokens, div(), div())).child(self.launch.clone()),
            })
    }
}
