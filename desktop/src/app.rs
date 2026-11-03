//! The window and its three-pane shell. Each pane is a view its own module owns.

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, KeyBinding, Menu, MenuItem, MouseButton, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, actions, div, point, px, size,
};

use crate::agent::AgentView;
use crate::document::Opened;
use crate::editor::EditorView;
use crate::inspector::InspectorView;
use crate::preview::PreviewView;
use crate::sidebar::SidebarView;
use crate::tokens::{self, Scheme, Tokens};

actions!(druck_desktop, [Quit]);

/// Opens the only window. A fixed `scheme` overrides the system appearance.
pub fn open(opened: Option<Opened>, scheme: Option<Scheme>, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    crate::editor::bind_keys(cx);
    cx.set_menus([Menu::new("Druck").items([MenuItem::action("Quit Druck", Quit)])]);
    cx.on_window_closed(|cx, _| cx.quit()).detach();

    let title: SharedString = opened
        .as_ref()
        .map_or_else(|| "Druck".to_owned(), |opened| opened.path.display().to_string())
        .into();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1280.), px(800.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(7.))),
        }),
        ..Default::default()
    };
    let window = cx.open_window(options, |window, cx| {
        cx.set_global(Tokens::new(scheme.unwrap_or_else(|| window.appearance().into())));
        cx.new(|cx| Shell::new(opened, title, scheme, window, cx))
    });
    if let Err(error) = window {
        eprintln!("cannot open a window: {error}");
        cx.quit();
        return;
    }
    cx.activate(true);
}

struct Shell {
    title: SharedString,
    sidebar: Entity<SidebarView>,
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    agent: Entity<AgentView>,
}

impl Shell {
    fn new(
        opened: Option<Opened>,
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
        Self {
            title,
            sidebar: cx.new(|_| SidebarView::new(opened)),
            editor: cx.new(|cx| EditorView::new(path, window, cx)),
            preview: cx.new(|_| PreviewView::new()),
            inspector: cx.new(|_| InspectorView::new()),
            agent: cx.new(|_| AgentView::new()),
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let titlebar = div()
            .id("titlebar")
            .flex_none()
            .h(tokens::TITLEBAR_HEIGHT)
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
            .child(self.title.clone());
        let canvas = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(tokens.canvas)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.editor.clone())
                    .child(div().flex_none().w(px(1.)).bg(tokens.divider))
                    .child(self.preview.clone()),
            )
            .child(self.agent.clone());
        div()
            .size_full()
            .flex()
            .flex_col()
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
