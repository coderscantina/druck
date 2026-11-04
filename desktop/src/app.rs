//! The window and its three-pane shell. Each pane is a view its own module owns.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, Focusable, KeyBinding, Menu, MenuItem, MouseButton, Pixels, SharedString,
    Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowKind, WindowOptions, actions, div, point, px,
    size,
};

use crate::agent::AgentView;
use crate::document::Opened;
use crate::editor::{CaretMoved, EditorView, TextChanged};
use crate::inspector::{InspectorView, ThemeChanged};
use crate::preview::{BodyStyle, PageClicked, PreviewView, Typeset};
use crate::sidebar::SidebarView;
use crate::tokens::{self, Scheme, Tokens};

actions!(druck_desktop, [Quit, ShowWrite, ShowPages, ShowSplit]);

/// The window the app opens to, smaller on a smaller display.
const WINDOW_SIZE: (f32, f32) = (1680., 1000.);
/// Room kept around the window on a display smaller than [`WINDOW_SIZE`].
const SCREEN_MARGIN: f32 = 80.;
/// Below this window width each side of a split is too narrow to write in, so the app opens on Write.
const SPLIT_MIN_WIDTH: f32 = 1400.;
/// Keeps the title clear of the window controls and the canvas switch.
const TITLE_INSET: Pixels = px(170.);
/// How long typing must stop before the pages catch up.
const REFRESH_PAUSE: Duration = Duration::from_millis(150);

/// Opens the only window. `theme` is the file the preview typesets with and the inspector edits. A fixed `scheme`
/// overrides the system appearance.
pub fn open(opened: Option<Opened>, theme: Option<PathBuf>, scheme: Option<Scheme>, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-1", ShowWrite, None),
        KeyBinding::new("cmd-2", ShowPages, None),
        KeyBinding::new("cmd-3", ShowSplit, None),
    ]);
    crate::editor::bind_keys(cx);
    cx.set_menus([Menu::new("Druck").items([MenuItem::action("Quit Druck", Quit)])]);
    cx.on_window_closed(|cx, _| cx.quit()).detach();

    let title: SharedString = opened
        .as_ref()
        .map_or_else(|| "Druck".to_owned(), |opened| opened.path.display().to_string())
        .into();
    let display = cx.primary_display().map(|display| display.bounds().size);
    let fit = |wanted: f32, available: Option<Pixels>| {
        available.map_or(wanted, |available| wanted.min(f32::from(available) - SCREEN_MARGIN))
    };
    let window_size = size(
        px(fit(WINDOW_SIZE.0, display.map(|display| display.width))),
        px(fit(WINDOW_SIZE.1, display.map(|display| display.height))),
    );
    let measuring = std::env::var_os("DRUCK_GATE").is_some();
    // Macs stop painting a covered window. A measurement run floats on the last display without taking focus.
    let display_id = measuring
        .then(|| cx.displays().last().map(|display| display.id()))
        .flatten();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(display_id, window_size, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
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
        cx.set_global(Tokens::new(scheme.unwrap_or_else(|| window.appearance().into())));
        cx.new(|cx| Shell::new(opened, theme, title, scheme, window, cx))
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

/// What the centre pane shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Canvas {
    Write,
    Pages,
    Split,
}

impl Canvas {
    const ALL: [(Self, &'static str); 3] = [(Self::Write, "Write"), (Self::Pages, "Pages"), (Self::Split, "Split")];

    fn from_env() -> Option<Self> {
        match std::env::var("DRUCK_CANVAS").ok()?.as_str() {
            "write" => Some(Self::Write),
            "pages" => Some(Self::Pages),
            "split" => Some(Self::Split),
            _ => None,
        }
    }
}

struct Shell {
    title: SharedString,
    canvas: Canvas,
    sidebar: Entity<SidebarView>,
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    agent: Entity<AgentView>,
    /// Waits out a pause in typing. Replaced by every edit.
    refresh: Task<()>,
    /// The body style the galley shows, and the font files already given to the toolkit.
    body: Option<BodyStyle>,
    /// The content hashes of the font files the toolkit has.
    fonts: HashSet<u64>,
    /// Where the caret went while the pages were behind the text, to show once they catch up.
    caret: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    fn new(
        opened: Option<Opened>,
        theme: Option<PathBuf>,
        title: SharedString,
        scheme: Option<Scheme>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if scheme.is_none() {
            cx.observe_window_appearance(window, |_, window, cx| {
                cx.set_global(Tokens::new(window.appearance().into()));
                cx.notify();
            })
            .detach();
        }
        let path = opened.as_ref().map(|opened| opened.path.clone());
        let editor = cx.new(|cx| EditorView::new(path, window, cx));
        // The editor knows whether the path opened as a file. Without one there is nothing to typeset.
        let document = editor.read(cx).path().map(Path::to_owned);
        let has_document = document.is_some();
        let preview_theme = theme.clone();
        let preview = cx.new(|cx| PreviewView::new(document, preview_theme, cx));
        let inspector = cx.new(|cx| InspectorView::new(theme, window, cx));
        let subscriptions = vec![
            cx.subscribe(&editor, |shell, _, _: &TextChanged, cx| shell.text_changed(cx)),
            cx.subscribe(&inspector, |shell, _, _: &ThemeChanged, cx| shell.request_pages(cx)),
            cx.subscribe(&preview, |shell, _, Typeset(body), cx| {
                shell.set_body(body, cx);
                shell.show_caret(cx);
            }),
            cx.subscribe(&editor, |shell, _, CaretMoved(offset), cx| {
                shell.caret = Some(*offset);
                shell.show_caret(cx);
            }),
            cx.subscribe_in(&preview, window, |shell, _, PageClicked(offset), window, cx| {
                shell.page_clicked(*offset, window, cx);
            }),
        ];
        let canvas = Canvas::from_env().unwrap_or(if f32::from(window.bounds().size.width) < SPLIT_MIN_WIDTH {
            Canvas::Write
        } else {
            Canvas::Split
        });
        let mut shell = Self {
            title,
            canvas,
            sidebar: cx.new(|_| SidebarView::new(opened)),
            editor,
            preview,
            inspector,
            agent: cx.new(|_| AgentView::new()),
            refresh: Task::ready(()),
            body: None,
            fonts: HashSet::new(),
            caret: None,
            _subscriptions: subscriptions,
        };
        if has_document {
            shell.request_pages(cx);
        }
        crate::gate::start(
            shell.editor.clone(),
            shell.preview.clone(),
            shell.inspector.clone(),
            window.window_handle(),
            cx,
        );
        shell
    }

    /// Asks for pages once typing has paused.
    fn text_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh = cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(REFRESH_PAUSE).await;
            shell.update(cx, |shell, cx| shell.request_pages(cx)).ok();
        });
    }

    fn request_pages(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let (text, revision) = (editor.text().to_owned(), editor.revision());
        self.preview
            .update(cx, |preview, cx| preview.request(text, revision, cx));
    }

    /// Shows the caret's text on the pages, once they are set from the text the caret is in.
    fn show_caret(&mut self, cx: &mut Context<Self>) {
        let revision = self.editor.read(cx).revision();
        if !self.preview.read(cx).shows(revision) {
            return;
        }
        if let Some(offset) = self.caret.take() {
            self.preview.update(cx, |preview, cx| preview.show_source(offset, cx));
        }
    }

    /// Moves the galley's caret to the text clicked on a page. Pages behind the text have other offsets, so a click
    /// on them does nothing until they catch up.
    fn page_clicked(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.clone();
        if !self.preview.read(cx).shows(editor.read(cx).revision()) {
            return;
        }
        editor.update(cx, |editor, cx| editor.jump_to(offset, window, cx));
        if self.canvas == Canvas::Split {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    /// Gives the galley the theme's body style when it changed, with its font files if the toolkit lacks them.
    fn set_body(&mut self, body: &BodyStyle, cx: &mut Context<Self>) {
        if self.body.as_ref() == Some(body) {
            return;
        }
        let new_files: Vec<Cow<'static, [u8]>> = body
            .files
            .iter()
            .filter(|file| self.fonts.insert(file.hash()))
            .map(|file| Cow::Owned(file.to_vec()))
            .collect();
        if !new_files.is_empty()
            && let Err(error) = cx.text_system().add_fonts(new_files)
        {
            eprintln!("the galley cannot use the theme's font: {error}");
        }
        self.editor.update(cx, |editor, cx| {
            editor.set_body(&body.family, body.size, body.line_height, cx)
        });
        self.body = Some(body.clone());
    }

    fn show(&mut self, canvas: Canvas, window: &mut Window, cx: &mut Context<Self>) {
        self.canvas = canvas;
        // Keys go to what is on screen: a hidden pane cannot hold focus.
        if canvas == Canvas::Pages {
            window.focus(&self.preview.focus_handle(cx), cx);
        } else {
            window.focus(&self.editor.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn canvas_switch(&self, tokens: Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let buttons: Vec<_> = Canvas::ALL
            .iter()
            .map(|&(canvas, label)| {
                let active = canvas == self.canvas;
                div()
                    .id(label)
                    .px(tokens::SPACE_2)
                    .rounded(tokens::RADIUS)
                    .cursor_pointer()
                    .text_color(if active { tokens.text } else { tokens.text_quiet })
                    .when(active, |button| button.bg(tokens.raised))
                    .hover(|button| button.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |shell, _, window, cx| shell.show(canvas, window, cx)))
                    .child(label)
            })
            .collect();
        div().flex().items_center().gap(px(2.)).children(buttons)
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let switch = self.canvas_switch(tokens.clone(), cx);
        let titlebar = div()
            .id("titlebar")
            .flex_none()
            .h(tokens::TITLEBAR_HEIGHT)
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .bg(tokens.panel)
            .border_b_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(div().min_w_0().px(TITLE_INSET).truncate().child(self.title.clone()))
            .child(
                div()
                    .absolute()
                    .right(tokens::SPACE_3)
                    .top_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(switch),
            );
        let panes = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(self.canvas != Canvas::Pages, |panes| panes.child(self.editor.clone()))
            .when(self.canvas == Canvas::Split, |panes| {
                panes.child(div().flex_none().w(px(1.)).bg(tokens.divider))
            })
            .when(self.canvas != Canvas::Write, |panes| panes.child(self.preview.clone()));
        let canvas = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(tokens.canvas)
            .child(panes)
            .child(self.agent.clone());
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(|shell, _: &ShowWrite, window, cx| shell.show(Canvas::Write, window, cx)))
            .on_action(cx.listener(|shell, _: &ShowPages, window, cx| shell.show(Canvas::Pages, window, cx)))
            .on_action(cx.listener(|shell, _: &ShowSplit, window, cx| shell.show(Canvas::Split, window, cx)))
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .text_color(tokens.text)
            .bg(tokens.canvas)
            .child(titlebar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.sidebar.clone())
                    .child(canvas)
                    .child(self.inspector.clone()),
            )
    }
}
