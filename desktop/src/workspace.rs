//! One open document: the three panes, the slots later slices mount into, and the wiring between them.
//!
//! The session lives in the editor view. Panes hold no document: this module hands them what they show and carries out
//! what they ask for through [`EditorView::update_session`].

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyView, Context, Entity, Focusable, MouseButton, PathPromptOptions, StyleRefinement, Subscription, Task, Window,
    div, px,
};

use crate::editor::{CaretMoved, EditorView, Save, SaveRequested, SessionChanged, TextChanged};
use crate::inspector::details::{DetailsEdit, DetailsPanel};
use crate::inspector::{InspectorView, SpecimenRequested, Tab, ThemeChanged, ThemeSwitched};
use crate::preview::{BodyStyle, PageClicked, PreviewView, Typeset};
use crate::session::{Group, Session};
use crate::shell::text::Key;
use crate::sidebar::dialogs::{DialogEvent, SectionDialog};
use crate::sidebar::{SidebarData, SidebarEvent, SidebarView};
use crate::theme_file::blocks::Target;
use crate::tokens::{self, Tokens};

/// Below this window width each side of a split is too narrow to write in, so the app opens on Write.
const SPLIT_MIN_WIDTH: f32 = 1400.;
/// Keeps the title clear of the window controls and the canvas switch.
const TITLE_INSET: gpui::Pixels = px(240.);
/// How long typing must stop before the pages catch up.
const REFRESH_PAUSE: Duration = Duration::from_millis(150);

gpui::actions!(workspace, [ShowWrite, ShowPages, ShowSplit]);

/// What the centre pane shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Canvas {
    Write,
    Pages,
    Split,
}

impl Canvas {
    const ALL: [(Self, Key); 3] = [
        (Self::Write, Key::Write),
        (Self::Pages, Key::Pages),
        (Self::Split, Key::Split),
    ];

    fn from_env() -> Option<Self> {
        match crate::gate::var("DRUCK_CANVAS")?.as_str() {
            "write" => Some(Self::Write),
            "pages" => Some(Self::Pages),
            "split" => Some(Self::Split),
            _ => None,
        }
    }
}

/// Where a slice mounts a view of its own. See `crate::app::mount`.
#[derive(Default)]
pub struct Slots {
    /// In the title bar, before the canvas switch.
    pub toolbar: Vec<AnyView>,
    /// The strip below the canvas, left to right.
    pub footer: Vec<AnyView>,
    /// Layers over the window, last on top. A layer draws nothing while it is closed.
    pub overlays: Vec<AnyView>,
}

/// What a slice's view can hold on to. Everything about the document is in the editor's session; events are on the
/// entities: [`TextChanged`], [`CaretMoved`], and [`SessionChanged`] on the editor, [`Typeset`] on the preview.
#[derive(Clone)]
pub struct Parts {
    pub editor: Entity<EditorView>,
    pub preview: Entity<PreviewView>,
    pub sidebar: Entity<SidebarView>,
    pub inspector: Entity<InspectorView>,
}

pub struct Workspace {
    canvas: Canvas,
    /// Whether the sidebar and the inspector are shown. The View menu hides them for a wider galley.
    sidebar_shown: bool,
    inspector_shown: bool,
    parts: Parts,
    details: Entity<DetailsPanel>,
    dialog: Entity<SectionDialog>,
    slots: Slots,
    /// Waits out a pause in typing. Replaced by every edit.
    refresh: Task<()>,
    /// The revision last asked for, so an edit to a note asks for nothing.
    requested: Option<u64>,
    /// The body style the galley shows.
    body: Option<BodyStyle>,
    /// The content hashes of the font files the toolkit has.
    fonts: HashSet<u64>,
    /// Where the caret went while the pages were behind the text, to show once they catch up.
    caret: Option<usize>,
    /// The theme file named on the command line or left by a fork, which the document's `theme:` does not decide.
    explicit: Option<PathBuf>,
    /// What the document's `theme:` said when last looked at, to tell when the author changed it.
    theme_setting: Option<String>,
    /// The style whose block to show once the pages catch up with the theme.
    jump: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    /// `theme` is the file the preview typesets with and the inspector edits, if not the document's.
    pub fn new(session: Session, theme: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kind = session.kind().clone();
        let asked = theme.clone();
        let theme = theme.or_else(|| session.theme_file());
        let dirs = session.theme_dirs();
        let dirs = crate::theme_file::editor::Dirs {
            project: dirs.project,
            common: dirs.shared,
        };
        let editor = cx.new(|cx| EditorView::new(session, window, cx));
        let preview = cx.new(|cx| PreviewView::new(kind, asked.clone(), cx));
        let inspector = cx.new(|cx| InspectorView::new(theme, dirs, window, cx));
        let sidebar = cx.new(|cx| SidebarView::new(window, cx));
        let model = inspector.read(cx).model().clone();
        let details = cx.new(|cx| DetailsPanel::new(window, cx));
        let dialog = cx.new(|cx| SectionDialog::new(window, cx));
        inspector.update(cx, |inspector, _| inspector.mount(Tab::Details, details.clone().into()));
        let subscriptions = vec![
            cx.subscribe_in(&editor, window, |workspace, _, _: &TextChanged, window, cx| {
                workspace.text_changed(window, cx);
            }),
            cx.subscribe_in(&editor, window, |workspace, _, _: &SessionChanged, window, cx| {
                workspace.session_changed(window, cx);
            }),
            cx.subscribe(&inspector, |workspace, _, event: &ThemeChanged, cx| {
                workspace.theme_changed(event, cx)
            }),
            cx.subscribe(&model, |workspace, _, _: &ThemeSwitched, cx| {
                workspace.theme_switched(cx)
            }),
            cx.subscribe(&model, |workspace, _, _: &SpecimenRequested, cx| {
                workspace.specimen_requested(cx)
            }),
            cx.subscribe_in(&preview, window, |workspace, _, Typeset(body), window, cx| {
                workspace.set_body(body, cx);
                workspace.show_caret(cx);
                workspace.show_fonts(cx);
                workspace.jump_to_style(window, cx);
            }),
            cx.subscribe(&editor, |workspace, _, CaretMoved(offset), cx| {
                workspace.caret = Some(*offset);
                workspace.show_caret(cx);
            }),
            cx.subscribe_in(&preview, window, Self::page_clicked),
            cx.subscribe_in(&sidebar, window, Self::sidebar_event),
            cx.subscribe_in(&details, window, Self::details_event),
            cx.subscribe_in(&dialog, window, Self::dialog_event),
        ];
        let parts = Parts {
            editor,
            preview,
            sidebar,
            inspector,
        };
        let mut slots = Slots::default();
        crate::app::mount(&parts, &mut slots, window, cx);
        slots.overlays.push(dialog.clone().into());
        let canvas = Canvas::from_env().unwrap_or(if f32::from(window.bounds().size.width) < SPLIT_MIN_WIDTH {
            Canvas::Write
        } else {
            Canvas::Split
        });
        let mut workspace = Self {
            canvas,
            sidebar_shown: true,
            inspector_shown: true,
            parts,
            details,
            dialog,
            slots,
            refresh: Task::ready(()),
            requested: None,
            body: None,
            fonts: HashSet::new(),
            caret: None,
            theme_setting: None,
            explicit: asked,
            jump: None,
            _subscriptions: subscriptions,
        };
        workspace.theme_setting = workspace.parts.editor.read(cx).session().theme_setting();
        workspace.sync_panels(window, cx);
        // `DRUCK_SPECIMEN=1` opens on the specimen, for screenshots.
        if crate::gate::var("DRUCK_SPECIMEN").is_some() {
            workspace
                .parts
                .preview
                .update(cx, |preview, cx| preview.set_specimen(true, cx));
        }
        workspace.request_pages(cx);
        crate::gate::start(
            workspace.parts.editor.clone(),
            workspace.parts.preview.clone(),
            workspace.parts.inspector.clone(),
            window.window_handle(),
            cx,
        );
        workspace
    }

    pub fn sidebar_shown(&self) -> bool {
        self.sidebar_shown
    }

    pub fn inspector_shown(&self) -> bool {
        self.inspector_shown
    }

    /// Show a canvas, for the View menu when no pane holds the focus.
    pub fn show_write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Canvas::Write, window, cx);
    }

    pub fn show_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Canvas::Pages, window, cx);
    }

    pub fn show_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Canvas::Split, window, cx);
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_shown = !self.sidebar_shown;
        cx.notify();
    }

    pub fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspector_shown = !self.inspector_shown;
        cx.notify();
    }

    /// Whether anything is not on disk yet.
    pub fn is_dirty(&self, cx: &gpui::App) -> bool {
        self.parts.editor.read(cx).session().is_dirty()
    }

    /// Where the document is.
    pub fn location(&self, cx: &gpui::App) -> PathBuf {
        self.parts.editor.read(cx).session().location().to_owned()
    }

    /// Writes everything that changed and waits for it, before the document is replaced. Returns whether it all went.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.parts.editor.update(cx, |editor, cx| editor.save_all(window, cx))
    }

    /// Gives the side panels what the session says now.
    fn sync_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let details = session.details().clone();
        let data = SidebarData {
            title: details.title.clone(),
            authors: details.authors.clone(),
            cover: session.cover_file(),
            contents: session.contents(),
            active: Some(session.active()),
            problem: session.problem().map(str::to_owned),
        };
        let cover = data.cover.clone();
        self.parts.sidebar.update(cx, |sidebar, cx| sidebar.set_data(data, cx));
        self.details
            .update(cx, |panel, cx| panel.set_details(details, cover, window, cx));
    }

    /// After a pause in typing: titles first, then pages.
    fn text_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh = cx.spawn_in(window, async move |workspace, cx| {
            cx.background_executor().timer(REFRESH_PAUSE).await;
            workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.parts.editor.update(cx, |editor, _| editor.refresh_outline());
                    workspace.sync_panels(window, cx);
                    workspace.sync_theme(cx);
                    workspace.request_if_changed(cx);
                })
                .ok();
        });
    }

    fn session_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_panels(window, cx);
        self.sync_theme(cx);
        self.request_if_changed(cx);
    }

    /// Opens in the inspector the theme the document uses now. The pages follow the document by themselves.
    fn sync_theme(&mut self, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let setting = session.theme_setting();
        if setting != self.theme_setting {
            // The author chose a theme in the document, which ends an override from the command line.
            self.theme_setting = setting;
            if self.explicit.take().is_some() {
                self.parts.preview.update(cx, |preview, cx| preview.set_theme(None, cx));
            }
        }
        let wanted = self
            .explicit
            .clone()
            .or_else(|| self.parts.editor.read(cx).session().theme_file());
        let model = self.parts.inspector.read(cx).model().clone();
        let open = model.read(cx).file().ok().map(|file| file.path().to_owned());
        if open != wanted {
            model.update(cx, |model, cx| model.open(wanted.as_deref(), cx));
        }
    }

    /// A fork opened another file than the one the document names, which the pages must use from now on.
    fn theme_switched(&mut self, cx: &mut Context<Self>) {
        let model = self.parts.inspector.read(cx).model().clone();
        let open = model.read(cx).file().ok().map(|file| file.path().to_owned());
        let wanted = self
            .explicit
            .clone()
            .or_else(|| self.parts.editor.read(cx).session().theme_file());
        if open != wanted {
            self.explicit.clone_from(&open);
            self.parts.preview.update(cx, |preview, cx| preview.set_theme(open, cx));
        }
    }

    /// The theme file changed: typeset again, and show a block in the style that changed.
    fn theme_changed(&mut self, event: &ThemeChanged, cx: &mut Context<Self>) {
        self.jump = style_of(&event.property);
        self.parts.preview.update(cx, |preview, cx| preview.refresh(cx));
    }

    fn specimen_requested(&mut self, cx: &mut Context<Self>) {
        let model = self.parts.inspector.read(cx).model().clone();
        self.jump = model.read(cx).missing().map(str::to_owned);
        self.parts
            .preview
            .update(cx, |preview, cx| preview.set_specimen(true, cx));
    }

    /// Gives the Fonts tab the families the pages on screen use.
    fn show_fonts(&mut self, cx: &mut Context<Self>) {
        let fonts = self.parts.preview.read(cx).fonts().to_vec();
        self.parts
            .inspector
            .update(cx, |inspector, cx| inspector.set_fonts(fonts, cx));
    }

    /// Shows, on the pages and in the galley, the first block set in the style that was edited. A style with no block
    /// in the document is noted for the Styles panel, which offers the specimen.
    fn jump_to_style(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(style) = self.jump.take() else {
            return;
        };
        let editor = self.parts.editor.clone();
        let session = editor.read(cx).session();
        let prefer = session
            .chapter(session.active())
            .map(|chapter| chapter.path().to_owned());
        let text_of = |file: &std::path::Path, range: std::ops::Range<usize>| {
            let (id, start) = session.locate(file, range.start)?;
            let text = session.buffer(id).text();
            text.get(start..(start + range.len()).min(text.len()))
                .map(str::to_owned)
        };
        let preview = self.parts.preview.clone();
        let target = preview.read(cx).find_style(&style, prefer.as_deref(), &text_of);
        let place = match &target {
            Some(Target::Block(at)) => preview.read(cx).place_of(*at).map(|file| (file, at.offset as usize)),
            _ => None,
        };
        let galley = place.as_ref().and_then(|(file, offset)| session.locate(file, *offset));
        let specimen = preview.read(cx).is_specimen();
        let model = self.parts.inspector.read(cx).model().clone();
        model.update(cx, |model, cx| {
            model.set_missing(target.is_none().then(|| style.clone()), cx)
        });
        match target {
            Some(Target::Block(_)) => {
                if let Some((file, offset)) = place {
                    preview.update(cx, |preview, cx| preview.show_source(&file, offset, cx));
                }
                if let (Some((id, offset)), false) = (galley, specimen) {
                    editor.update(cx, |editor, cx| editor.show_place(id, offset, window, cx));
                }
            }
            Some(Target::FirstPage) => preview.update(cx, |preview, cx| preview.show_page(0, cx)),
            None => {}
        }
    }

    /// Asks for pages unless they are set from this revision or being set from it.
    fn request_if_changed(&mut self, cx: &mut Context<Self>) {
        if self.requested != Some(self.parts.editor.read(cx).revision()) {
            self.request_pages(cx);
        }
    }

    fn request_pages(&mut self, cx: &mut Context<Self>) {
        let joined = self.parts.editor.read(cx).session().joined();
        self.requested = Some(joined.revision);
        self.parts.preview.update(cx, |preview, cx| preview.request(joined, cx));
    }

    /// Shows the caret's text on the pages, once they are set from the text the caret is in.
    fn show_caret(&mut self, cx: &mut Context<Self>) {
        let editor = self.parts.editor.read(cx);
        if !self.parts.preview.read(cx).shows(editor.revision()) {
            return;
        }
        let Some(offset) = self.caret.take() else {
            return;
        };
        let session = editor.session();
        if let Some((file, offset)) = session.page_position(session.active(), offset) {
            self.parts
                .preview
                .update(cx, |preview, cx| preview.show_source(&file, offset, cx));
        }
    }

    /// Opens the chapter a click on a page landed in, with the caret at the text clicked. Pages behind the text have
    /// other offsets, so a click on them does nothing until they catch up.
    fn page_clicked(
        &mut self,
        _: &Entity<PreviewView>,
        clicked: &PageClicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.parts.editor.clone();
        let session = editor.read(cx);
        if !self.parts.preview.read(cx).shows(session.revision()) {
            return;
        }
        let Some((id, offset)) = session.session().locate(&clicked.file, clicked.offset) else {
            return;
        };
        editor.update(cx, |editor, cx| editor.show_place(id, offset, window, cx));
        if self.canvas == Canvas::Split {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    fn sidebar_event(
        &mut self,
        _: &Entity<SidebarView>,
        event: &SidebarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.parts.editor.clone();
        match event {
            SidebarEvent::Select(id) => {
                editor.update(cx, |editor, cx| editor.show_chapter(*id, window, cx));
                if self.canvas != Canvas::Pages {
                    window.focus(&editor.focus_handle(cx), cx);
                }
            }
            SidebarEvent::Browse(id) => {
                editor.update(cx, |editor, cx| editor.show_chapter(*id, window, cx));
            }
            SidebarEvent::Move(id, to) => {
                let (id, to) = (*id, *to);
                self.change(move |session| session.move_chapter(id, to), window, cx);
            }
            SidebarEvent::AddToBook(id) => {
                let id = *id;
                self.change(move |session| session.add_to_book(id), window, cx);
            }
            SidebarEvent::Edit(id) => {
                let session = editor.read(cx).session();
                let title = session.title_of(*id);
                let removable = matches!(session.kind(), crate::session::Kind::Project { .. })
                    && session
                        .chapter(*id)
                        .is_some_and(|chapter| chapter.group() == Group::Book);
                let id = *id;
                self.dialog
                    .update(cx, |dialog, cx| dialog.open_edit(id, title, removable, window, cx));
            }
            SidebarEvent::NewSection => {
                self.dialog.update(cx, |dialog, cx| dialog.open_new_section(window, cx));
            }
            SidebarEvent::ShowDetails => {
                self.parts
                    .inspector
                    .update(cx, |inspector, cx| inspector.show(Tab::Details, cx));
            }
        }
    }

    fn dialog_event(
        &mut self,
        _: &Entity<SectionDialog>,
        event: &DialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DialogEvent::Insert(template, name) => {
                let (template, name) = (*template, name.clone());
                let inserted = self.change(
                    move |session| {
                        let after = session.active();
                        session.new_section(template, Some(&name), after).map(|_| ())
                    },
                    window,
                    cx,
                );
                if inserted {
                    window.focus(&self.parts.editor.focus_handle(cx), cx);
                }
            }
            DialogEvent::Rename(id, name) => {
                let (id, name) = (*id, name.clone());
                self.change(move |session| session.rename(id, &name), window, cx);
            }
            DialogEvent::Remove(id) => {
                let id = *id;
                self.change(move |session| session.remove_from_book(id), window, cx);
            }
        }
    }

    fn details_event(
        &mut self,
        _: &Entity<DetailsPanel>,
        event: &DetailsEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.parts.editor.clone();
        let result = match event {
            DetailsEdit::Field(field, value) => editor.update(cx, |editor, cx| {
                editor.update_session(|session| session.set_field(*field, value), window, cx)
            }),
            DetailsEdit::Meta(key, value) => editor.update(cx, |editor, cx| {
                editor.update_session(|session| session.set_meta(key, value), window, cx)
            }),
            DetailsEdit::RemoveMeta(key) => editor.update(cx, |editor, cx| {
                editor.update_session(|session| session.remove_meta(key), window, cx)
            }),
            DetailsEdit::ChooseCover => {
                self.choose_cover(window, cx);
                return;
            }
        };
        self.details.update(cx, |panel, cx| panel.set_error(result.err(), cx));
    }

    /// Asks for an image and makes it the cover.
    fn choose_cover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            workspace
                .update_in(cx, |workspace, window, cx| {
                    let editor = workspace.parts.editor.clone();
                    let result = editor.update(cx, |editor, cx| {
                        editor.update_session(|session| session.set_cover(&path), window, cx)
                    });
                    workspace
                        .details
                        .update(cx, |panel, cx| panel.set_error(result.err(), cx));
                })
                .ok();
        })
        .detach();
    }

    /// Runs a change to the session, showing why if it was refused. Returns whether it was made.
    fn change(
        &mut self,
        change: impl FnOnce(&mut Session) -> Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = self
            .parts
            .editor
            .update(cx, |editor, cx| editor.update_session(change, window, cx));
        let made = result.is_ok();
        self.parts
            .sidebar
            .update(cx, |sidebar, cx| sidebar.set_notice(result.err(), cx));
        made
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
        self.parts.editor.update(cx, |editor, cx| {
            editor.set_body(&body.family, body.size, body.line_height, cx)
        });
        self.body = Some(body.clone());
    }

    fn show(&mut self, canvas: Canvas, window: &mut Window, cx: &mut Context<Self>) {
        self.canvas = canvas;
        // Keys go to what is on screen: a hidden pane cannot hold focus.
        if canvas == Canvas::Pages {
            window.focus(&self.parts.preview.focus_handle(cx), cx);
        } else {
            window.focus(&self.parts.editor.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn canvas_switch(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let buttons: Vec<_> = Canvas::ALL
            .iter()
            .map(|&(canvas, key)| {
                let label = crate::t!(key);
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
                    .on_click(cx.listener(move |workspace, _, window, cx| workspace.show(canvas, window, cx)))
                    .child(label)
            })
            .collect();
        div().flex().items_center().gap(px(2.)).children(buttons)
    }
}

/// The bar at the window's top, which moves the window. Also used with no document.
pub fn titlebar(title: String, tokens: &Tokens, end: impl IntoElement) -> impl IntoElement {
    div()
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
        .child(div().min_w_0().px(TITLE_INSET).truncate().child(title))
        .child(
            div()
                .absolute()
                .right(tokens::SPACE_3)
                .top_0()
                .h_full()
                .flex()
                .items_center()
                .gap(tokens::SPACE_2)
                .child(end),
        )
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let switch = self.canvas_switch(&tokens, cx);
        let title = self.parts.editor.read(cx).session().location().display().to_string();
        let bar_end = div()
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .children(self.slots.toolbar.clone())
            .child(switch);
        let panes = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(self.canvas != Canvas::Pages, |panes| {
                panes.child(self.parts.editor.clone())
            })
            .when(self.canvas == Canvas::Split, |panes| {
                panes.child(div().flex_none().w(px(1.)).bg(tokens.divider))
            })
            .when(self.canvas != Canvas::Write, |panes| {
                panes.child(self.parts.preview.clone())
            });
        let footer = div()
            .flex_none()
            .h(tokens::FOOTER_HEIGHT)
            .flex()
            .items_center()
            .border_t_1()
            .border_color(tokens.divider)
            .bg(tokens.panel)
            .children(self.slots.footer.clone());
        let canvas = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(tokens.canvas)
            .child(panes)
            .child(footer);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .on_action(cx.listener(|workspace, _: &ShowWrite, window, cx| workspace.show(Canvas::Write, window, cx)))
            .on_action(cx.listener(|workspace, _: &ShowPages, window, cx| workspace.show(Canvas::Pages, window, cx)))
            .on_action(cx.listener(|workspace, _: &ShowSplit, window, cx| workspace.show(Canvas::Split, window, cx)))
            .on_action(cx.listener(|workspace, _: &Save, _, cx| {
                workspace.parts.editor.update(cx, |_, cx| cx.emit(SaveRequested));
            }))
            .child(titlebar(title, &tokens, bar_end))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    // Cached, so a key repaints the galley and not every row and field beside it.
                    .when(self.sidebar_shown, |row| {
                        row.child(
                            self.parts
                                .sidebar
                                .clone()
                                .cached(StyleRefinement::default().flex_none().w(tokens::SIDEBAR_WIDTH).h_full()),
                        )
                    })
                    .child(canvas)
                    .when(self.inspector_shown, |row| {
                        row.child(
                            self.parts.inspector.clone().cached(
                                StyleRefinement::default()
                                    .flex_none()
                                    .w(tokens::INSPECTOR_WIDTH + tokens::RAIL_WIDTH)
                                    .h_full(),
                            ),
                        )
                    }),
            )
            .children(self.slots.overlays.clone())
    }
}

/// The style a dotted theme property belongs to, such as `heading-2` for `styles.heading-2.size`.
fn style_of(property: &str) -> Option<String> {
    let mut parts = property.split('.');
    matches!(parts.next()?, "styles" | "custom-styles").then(|| parts.next().map(str::to_owned))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_property_names_its_style() {
        assert_eq!(style_of("styles.heading-2.size").as_deref(), Some("heading-2"));
        assert_eq!(style_of("custom-styles.lead").as_deref(), Some("lead"));
        assert_eq!(style_of("tokens.sizes.big"), None);
        assert_eq!(style_of("styles"), None);
        assert_eq!(style_of(""), None);
    }
}
