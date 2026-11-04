//! The window, and who owns what in it. One window holds one document session.
//!
//! - `app.rs`: opening the window, the File menu, opening and replacing a document, and [`mount`]. Each later slice
//!   adds its line to [`mount`] and keeps everything else in its own folder.
//! - `workspace.rs`: the three panes, the canvas switch, the slots, and the wiring between panes.
//! - `session/`: the document, without gpui. Read it through `EditorView::session`, change it through
//!   `EditorView::update_session`.
//! - `sidebar/`: identity, contents, the file lists, and its dialogs. Dots go in through `SidebarView::set_marks`.
//! - `editor/`, `source/`: the galley. `preview/`: the pages. `inspector/`: the tab rail, styles, and details.
//!
//! The later slices of [the beta milestone](../../docs/milestones/16-desktop-beta.md):
//!
//! - **Persistence** owns `persist/`, with `session/disk.rs` for what the session knows of its files: autosave, the
//!   watcher, merges, and the recovery copies. Cmd+S asks it for an immediate save through `SaveRequested`.
//! - **Editor commands** own `editor/`, whose galley draws its own floats, the source toggle in `toolbar`, and the
//!   `Tab::Notes` panel.
//! - **Theme core** owns `theme_file/` and new panels on the `Tab::Fonts` tab and a theme picker in `toolbar`.
//! - **Shell** owns a new `shell/`: the `Tab::Diagnostics` and `Tab::Export` panels, sidebar dots, search and word count
//!   in `footer`, the string tables, and the rest of the menus.
//! - **Agent** owns `agent/`, whose view is mounted in `footer` until its panel has a place.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, KeyBinding, PathPromptOptions, Pixels, PromptLevel, Subscription, TitlebarOptions,
    Window, WindowBounds, WindowKind, WindowOptions, actions, div, point, px, size,
};

use crate::agent::AgentView;
use crate::editor::Save;
use crate::launch::{LaunchEvent, LaunchView};
use crate::session::{STARTERS, Session, Starter};
use crate::shell::text::Key;
use crate::shell::{self, menus};
use crate::tokens::{self, Scheme, Tokens};
use crate::workspace::{Parts, ShowPages, ShowSplit, ShowWrite, Slots, Workspace, titlebar};

actions!(druck_desktop, [Quit, Open, NewBook, NewReport, NewOffer]);

/// The window the app opens to, smaller on a smaller display.
const WINDOW_SIZE: (f32, f32) = (1680., 1000.);
/// Room kept around the window on a display smaller than [`WINDOW_SIZE`].
const SCREEN_MARGIN: f32 = 80.;

/// Mounts what later slices own into the workspace's slots and tabs. One line per view.
pub fn mount(parts: &Parts, slots: &mut Slots, window: &mut Window, cx: &mut Context<Workspace>) {
    crate::persist::mount(parts, slots, window, cx);
    shell::mount(parts, slots, window, cx);
    crate::inspector::mount_theme(parts, slots, cx);
    slots.footer.push(cx.new(|_| AgentView::new()).into());
    crate::editor::mount(parts, slots, cx);
}

/// Opens the only window, with a document or with the launch screen. `error` says why a document asked for did not
/// open. `theme` is the file the preview typesets with and the inspector edits. A fixed `scheme` overrides the system
/// appearance.
pub fn open(
    session: Option<Session>,
    error: Option<String>,
    theme: Option<PathBuf>,
    scheme: Option<Scheme>,
    cx: &mut App,
) {
    shell::init(scheme, cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", Open, None),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-1", ShowWrite, None),
        KeyBinding::new("cmd-2", ShowPages, None),
        KeyBinding::new("cmd-3", ShowSplit, None),
    ]);
    crate::editor::bind_keys(cx);
    crate::persist::bind_keys(cx);
    shell::install(cx);
    cx.on_app_quit(|cx| {
        shell::save_at_exit(cx);
        async {}
    })
    .detach();
    cx.on_window_closed(|cx, _| cx.quit()).detach();

    let display = cx.primary_display().map(|display| display.bounds().size);
    let fit = |wanted: f32, available: Option<Pixels>| {
        available.map_or(wanted, |available| wanted.min(f32::from(available) - SCREEN_MARGIN))
    };
    let (wanted_width, wanted_height) = shell::prefs(cx).window_size().unwrap_or(WINDOW_SIZE);
    let window_size = size(
        px(fit(wanted_width, display.map(|display| display.width))),
        px(fit(wanted_height, display.map(|display| display.height))),
    );
    let measuring = std::env::var_os("DRUCK_GATE").is_some();
    // Macs stop painting a covered window. A measurement run floats on the last display without taking focus.
    let display_id = measuring
        .then(|| cx.displays().last().map(|display| display.id()))
        .flatten();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(display_id, window_size, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some("Druck".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(7.))),
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
        cx.set_global(Tokens::new(shell::current_scheme(window, cx)));
        cx.new(|cx| Shell::new(session, error, theme, window, cx))
    });
    if let Err(error) = window {
        eprintln!("cannot open a window: {error}");
        cx.quit();
        return;
    }
    if !measuring {
        cx.activate(true);
    }
}

/// The window's root: the launch screen, or the workspace of the one open document.
struct Shell {
    launch: Entity<LaunchView>,
    workspace: Option<Entity<Workspace>>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    fn new(
        session: Option<Session>,
        error: Option<String>,
        theme: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Cached panes hold colors of the old appearance, so this repaints them too.
        cx.observe_window_appearance(window, |_, window, cx| shell::apply_appearance(window, cx))
            .detach();
        window.on_window_should_close(cx, |_, cx| {
            shell::save_at_exit(cx);
            true
        });
        let launch = cx.new(|_| LaunchView::new(error));
        let subscriptions = vec![
            cx.subscribe_in(&launch, window, |shell, _, event, window, cx| match event {
                LaunchEvent::Open => shell.ask_to_open(window, cx),
                LaunchEvent::New(starter) => shell.ask_for_new_project(starter, window, cx),
            }),
        ];
        let mut shell = Self {
            launch,
            workspace: None,
            _subscriptions: subscriptions,
        };
        if let Some(session) = session {
            shell.show_session(session, theme, window, cx);
        }
        shell
    }

    fn show_session(&mut self, session: Session, theme: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_title(&session.location().display().to_string());
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
                    Ok(()) => shell.open_path(&path, window, cx),
                    Err(problem) => shell
                        .launch
                        .update(cx, |launch, cx| launch.set_error(Some(problem), cx)),
                })
                .ok();
        })
        .detach();
    }

    /// Opens a document in this window, asking first if the one open has unsaved text.
    fn open_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.to_owned();
        self.after_saving(Key::SaveBeforeOpening, window, cx, move |shell, window, cx| {
            shell.replace_document(&path, window, cx);
        });
    }

    /// Closes the document and shows the launch screen, asking first if it has unsaved text. With no document it
    /// closes the window.
    fn close_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() {
            window.remove_window();
            return;
        }
        self.after_saving(Key::SaveBeforeClosing, window, cx, |shell, window, cx| {
            shell.workspace = None;
            window.set_window_title("Druck");
            cx.notify();
        });
    }

    /// Runs `then`, after asking whether to save if the document has unsaved text. Cancel runs nothing.
    fn after_saving(
        &mut self,
        message: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let dirty = self
            .workspace
            .as_ref()
            .is_some_and(|workspace| workspace.read(cx).is_dirty(cx));
        if !dirty {
            then(self, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            crate::t!(message),
            None,
            &[crate::t!(Key::Save), crate::t!(Key::Discard), crate::t!(Key::Cancel)],
            cx,
        );
        cx.spawn_in(window, async move |shell, cx| {
            let Ok(answer) = answer.await else {
                return;
            };
            shell
                .update_in(cx, |shell, window, cx| {
                    let workspace = shell.workspace.clone();
                    let saved = match (answer, workspace) {
                        (0, Some(workspace)) => workspace.update(cx, |workspace, cx| workspace.save(window, cx)),
                        (1, Some(workspace)) => {
                            crate::persist::forget_unsaved(&workspace.read(cx).location(cx));
                            true
                        }
                        _ => answer == 1,
                    };
                    // Text that could not be saved stays open rather than be lost with the window's document.
                    if answer != 2 && saved {
                        then(shell, window, cx);
                    }
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
                self.launch.update(cx, |launch, cx| launch.set_error(None, cx));
                self.show_session(session, None, window, cx);
            }
            Err(problem) => {
                eprintln!("{problem}");
                self.workspace = None;
                window.set_window_title("Druck");
                self.launch.update(cx, |launch, cx| launch.set_error(Some(problem), cx));
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
            .on_action(cx.listener(|shell, _: &NewReport, window, cx| shell.new_project("report", window, cx)))
            .on_action(cx.listener(|shell, _: &NewOffer, window, cx| shell.new_project("offer", window, cx)))
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
                None => root
                    .child(titlebar("Druck".to_owned(), &tokens, div()))
                    .child(self.launch.clone()),
            })
    }
}
