//! One open document: its two modes, the slots later work mounts into, and the wiring between the panes.
//!
//! The window is in Write or Publish, see [`Mode`]. Write shows the text, with the Binder if the author wants it. Publish
//! shows the pages and the rail. Both edit the same text, and [`ModeChanged`] tells who cares.
//!
//! The session lives in the editor view. Panes hold no document: this module hands them what they show and carries out
//! what they ask for through [`EditorView::update_session`].

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyView, App, Bounds, Context, Entity, EventEmitter, Focusable, Global, MouseButton, PathPromptOptions, Pixels,
    StyleRefinement, Subscription, Task, Window, div, point, px,
};

use crate::a11y::Control as _;
use crate::bookshelf::{BookshelfView, ShelfEvent};
use crate::editor::{CaretMoved, EditorView, Save, SaveRequested, SessionChanged, TextChanged};
use crate::inspector::details::{DetailsEdit, DetailsPanel};
use crate::inspector::{
    InspectorView, QuestionCancelled, SpecimenRequested, Tab, TabShown, ThemeChanged, ThemeSwitched,
};
#[cfg(any(debug_assertions, feature = "measure"))]
use crate::preview::proof::BlockAt;
#[cfg(any(debug_assertions, feature = "measure"))]
use crate::preview::slip::Directive;
use crate::preview::slip::{ProofSlip, SlipEvent};
use crate::preview::slip_control;
use crate::preview::{
    Ahead, BookType, DetailsWanted, PageClicked, PreviewView, ProblemsWanted, TextPlaceWanted, TypeRead, Typeset,
};
use crate::session::{Group, Session};
use crate::settings;
use crate::shell::text::Key;
use crate::sidebar::dialogs::{DialogEvent, SectionDialog};
use crate::sidebar::ticks::TicksView;
use crate::sidebar::{SidebarData, SidebarEvent, SidebarView};
use crate::theme_file::blocks::Target;
use crate::tokens::{self, Tokens};

/// How long typing must stop before the pages catch up when they are on screen.
const REFRESH_PAUSE: Duration = Duration::from_millis(150);
/// The same in Write, with nothing on screen: a long pause, so Publish opens at once and chips show the engine's numbers.
const LONG_PAUSE: Duration = Duration::from_millis(2000);

gpui::actions!(
    workspace,
    [
        ShowWrite,
        ShowPublish,
        NewSection,
        FocusNext,
        FocusPrevious,
        ZoomIn,
        ZoomOut,
        ZoomReset
    ]
);

/// What the window is doing: writing, or turning the manuscript into a finished book.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Write,
    Publish,
}

impl Mode {
    const ALL: [(Self, Key); 2] = [(Self::Write, Key::Write), (Self::Publish, Key::Publish)];
}

/// The window went to another mode.
pub struct ModeChanged(pub Mode);

impl EventEmitter<ModeChanged> for Workspace {}

/// Where a slice mounts a view of its own. See `crate::app::mount`.
#[derive(Default)]
pub struct Slots {
    /// One line over the foot of the text or the pages, left to right. It has no background and takes no room, and a
    /// view that has nothing to say draws nothing. A view that takes `ml_auto` stands at the right.
    pub footer: Vec<AnyView>,
    /// Layers over the window, last on top. A layer draws nothing while it is closed.
    pub overlays: Vec<AnyView>,
    /// Panels at the left of the text or the pages, after the Binder. A view that is closed draws nothing.
    pub left: Vec<SidePanel>,
    /// Panels at the right of the text or the pages, before the rail. A view that is closed draws nothing.
    pub right: Vec<SidePanel>,
    /// Views at the title bar's left, after the traffic lights, in Write. They place themselves and keep clear of the
    /// mode switch, see [`mode_switch_width`].
    pub title: Vec<AnyView>,
}

/// A panel's width in a window of the given width, zero while it is closed. Called while the workspace renders, so it
/// must not read the workspace.
pub type PanelWidth = Rc<dyn Fn(Pixels, &App) -> Pixels>;

/// A panel beside the text, and how wide it is, so the text knows its room before anything is laid out.
#[derive(Clone)]
pub struct SidePanel {
    pub view: AnyView,
    pub width: PanelWidth,
}

/// Where the text or the pages stand across the window, between the panels, as of the workspace's last render. Cards
/// that centre over the text centre on this.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct TextColumn {
    pub left: Pixels,
    pub width: Pixels,
    /// The rail's panel stands at the column's right, as in Publish.
    pub rail: bool,
    /// The text's own left edge and width in Write, once drawn.
    pub text: Option<(Pixels, Pixels)>,
}

impl Global for TextColumn {}

/// The document window's view of type `V`, for actions and code that hold no handle to it. There is one window.
pub struct Current<V: 'static> {
    view: gpui::WeakEntity<V>,
    window: gpui::AnyWindowHandle,
}

impl<V: 'static> Global for Current<V> {}

impl<V: 'static> Current<V> {
    /// Remembers `view` as the window's.
    pub fn set(view: &Entity<V>, window: &Window, cx: &mut App) {
        cx.set_global(Self {
            view: view.downgrade(),
            window: window.window_handle(),
        });
    }

    /// The view, while it lives.
    pub fn view(cx: &App) -> Option<Entity<V>> {
        cx.try_global::<Self>()?.view.upgrade()
    }

    /// The window the view is in.
    pub fn window(cx: &App) -> Option<gpui::AnyWindowHandle> {
        Some(cx.try_global::<Self>()?.window)
    }

    /// Runs `run` on the view in its window once what asked has finished: an action runs inside its window's update,
    /// which cannot be entered again.
    pub fn defer(cx: &mut App, run: impl FnOnce(&mut V, &mut Window, &mut Context<V>) + 'static) {
        cx.defer(move |cx| {
            let Some((view, window)) = Self::view(cx).zip(Self::window(cx)) else {
                return;
            };
            window
                .update(cx, |_, window, cx| view.update(cx, |view, cx| run(view, window, cx)))
                .ok();
        });
    }
}

impl TextColumn {
    /// The left edge of something `width` wide centred over the text, kept inside the window.
    pub fn centred(cx: &App, width: Pixels) -> Pixels {
        let column = cx.try_global::<Self>().copied().unwrap_or_default();
        (column.left + (column.width - width) / 2.).max(Pixels::ZERO)
    }

    /// The left edge and width of a card over the text: at least `least` wide, and in Write as wide as the text with a
    /// little more on each side so no line end shows beside it, centred on the text.
    pub fn over_text(cx: &App, least: Pixels) -> (Pixels, Pixels) {
        let column = cx.try_global::<Self>().copied().unwrap_or_default();
        let Some((text_left, text_width)) = column.text else {
            return (Self::centred(cx, least), least);
        };
        let width = (text_width + tokens::CARD_OVERHANG * 2.).max(least);
        ((text_left + (text_width - width) / 2.).max(Pixels::ZERO), width)
    }

    /// The top of a card over the text, the palette and the commit card alike: level with the text's top margin.
    pub fn card_top() -> Pixels {
        tokens::TITLEBAR_HEIGHT + tokens::TEXT_MARGIN_Y
    }

    /// Whether a card over the text stands over Write's text, which then fades under its edge.
    pub fn over_write(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(|column| column.text.is_some())
    }

    /// The left edge of a card `width` wide that asks something, near what asked: beside the Binder, or at the window's
    /// gutter while it is hidden, and beside the rail's panel in Publish.
    pub fn asking(cx: &App, width: Pixels) -> Pixels {
        let column = cx.try_global::<Self>().copied().unwrap_or_default();
        if column.rail {
            (column.left + column.width - tokens::SPACE_4 - width).max(Pixels::ZERO)
        } else {
            column.left + tokens::SPACE_4
        }
    }
}

/// What a slice's view can hold on to. Everything about the document is in the editor's session; events are on the
/// entities: [`TextChanged`], [`CaretMoved`], and [`SessionChanged`] on the editor, [`Typeset`] on the preview.
#[derive(Clone)]
pub struct Parts {
    pub editor: Entity<EditorView>,
    pub preview: Entity<PreviewView>,
    pub sidebar: Entity<SidebarView>,
    pub ticks: Entity<TicksView>,
    pub shelf: Entity<BookshelfView>,
    pub inspector: Entity<InspectorView>,
}

pub struct Workspace {
    mode: Mode,
    parts: Parts,
    details: Entity<DetailsPanel>,
    dialog: Entity<SectionDialog>,
    slots: Slots,
    /// Waits out a pause in typing. Replaced by every edit.
    refresh: Task<()>,
    /// The revision last asked for, so an edit to a note asks for nothing.
    requested: Option<u64>,
    /// The theme's type the galley shows.
    book_type: Option<BookType>,
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
    /// Something in Write wants the author, who is elsewhere: a dot on the switch.
    write_attention: bool,
    /// An export is running: the title bar's Export is quiet meanwhile.
    exporting: bool,
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
        let preview = cx.new(|cx| PreviewView::new(kind, asked.clone(), window, cx));
        let proof_slip = preview.read(cx).slip().clone();
        let inspector = cx.new(|cx| InspectorView::new(theme, dirs, window, cx));
        let sidebar = cx.new(|cx| SidebarView::new(window, cx));
        let ticks = cx.new(|cx| TicksView::new(&editor, window, cx));
        let shelf = cx.new(|cx| BookshelfView::new(window, cx));
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
            cx.subscribe_in(&inspector, window, |workspace, _, event: &ThemeChanged, window, cx| {
                workspace.theme_changed(event, cx);
                slip_control::theme_changed(&workspace.parts, window, cx);
            }),
            cx.subscribe(&model, |workspace, _, _: &QuestionCancelled, cx| {
                slip_control::question_cancelled(&workspace.parts, cx);
            }),
            cx.subscribe_in(&model, window, |workspace, _, _: &ThemeSwitched, window, cx| {
                workspace.theme_switched(cx);
                slip_control::theme_changed(&workspace.parts, window, cx);
            }),
            cx.subscribe(&model, |workspace, _, _: &SpecimenRequested, cx| {
                workspace.specimen_requested(cx)
            }),
            cx.subscribe(&preview, |workspace, _, TypeRead(book_type), cx| {
                workspace.set_type(book_type, cx)
            }),
            cx.subscribe_in(&preview, window, |workspace, _, Typeset(book_type), window, cx| {
                workspace.set_type(book_type, cx);
                workspace.show_caret(cx);
                workspace.show_fonts(cx);
                workspace.jump_to_style(window, cx);
                workspace.ready_pages(window, cx);
                slip_control::sync(&workspace.parts, cx);
            }),
            cx.observe_global_in::<settings::Store>(window, |_, _, cx| cx.notify()),
            // A card, float, or panel that went with the keys leaves them to the nearest place still on screen, or to
            // what the mode is about, so typed letters never go nowhere.
            cx.on_focus_lost(window, |workspace, window, cx| {
                match window.focus_lost_restore_target(cx) {
                    Some(target) => window.focus(&target, cx),
                    None => workspace.focus_main(window, cx),
                }
            }),
            cx.subscribe(&editor, |workspace, _, CaretMoved(offset), cx| {
                workspace.caret = Some(*offset);
                workspace.show_caret(cx);
            }),
            cx.subscribe_in(&preview, window, Self::page_clicked),
            cx.subscribe(&preview, |workspace, _, _: &ProblemsWanted, cx| {
                workspace
                    .parts
                    .inspector
                    .update(cx, |inspector, cx| inspector.show(Tab::Proof, cx));
            }),
            cx.subscribe_in(
                &preview,
                window,
                |workspace, _, TextPlaceWanted(path, line), window, cx| {
                    workspace.show_text_place(path, *line, window, cx)
                },
            ),
            cx.subscribe(&preview, |workspace, _, _: &DetailsWanted, cx| {
                workspace
                    .parts
                    .inspector
                    .update(cx, |inspector, cx| inspector.show(Tab::Details, cx));
            }),
            cx.subscribe(&inspector, |workspace, _, TabShown(tab), cx| {
                let proof_open = *tab == Tab::Proof;
                workspace
                    .parts
                    .preview
                    .update(cx, |preview, cx| preview.set_proof_open(proof_open, cx));
            }),
            cx.subscribe_in(&proof_slip, window, Self::slip_event),
            cx.subscribe_in(&sidebar, window, |workspace, _, event, window, cx| {
                workspace.sidebar_event(event, window, cx);
            }),
            cx.subscribe_in(&ticks, window, |workspace, _, event, window, cx| {
                workspace.sidebar_event(event, window, cx);
            }),
            cx.subscribe_in(&shelf, window, Self::shelf_event),
            cx.subscribe_in(&details, window, Self::details_event),
            cx.subscribe_in(&dialog, window, Self::dialog_event),
        ];
        let parts = Parts {
            editor,
            preview,
            sidebar,
            ticks,
            shelf,
            inspector,
        };
        let mut slots = Slots::default();
        crate::app::mount(&parts, &mut slots, window, cx);
        slots.overlays.push(dialog.clone().into());
        let foot = slots.footer.clone();
        parts.preview.update(cx, |preview, _| preview.set_foot(foot));
        let mut workspace = Self {
            mode: Mode::Write,
            parts,
            details,
            dialog,
            slots,
            refresh: Task::ready(()),
            requested: None,
            book_type: None,
            fonts: HashSet::new(),
            caret: None,
            theme_setting: None,
            explicit: asked,
            jump: None,
            write_attention: false,
            exporting: false,
            _subscriptions: subscriptions,
        };
        workspace.theme_setting = workspace.parts.editor.read(cx).session().theme_setting();
        let proof_open = workspace.parts.inspector.read(cx).active() == Tab::Proof;
        workspace
            .parts
            .preview
            .update(cx, |preview, cx| preview.set_proof_open(proof_open, cx));
        workspace.sync_panels(window, cx);
        workspace.request_pages(cx);
        cx.emit(ModeChanged(Mode::Write));
        crate::gate::apply(&workspace.parts, cx.entity(), window, cx);
        workspace
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Goes to a mode. Keys go to what is on screen: a hidden pane cannot hold focus. Pages not caught up with the text
    /// are asked for now, since the author wants them.
    pub fn show(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let changed = self.mode != mode;
        self.mode = mode;
        if changed {
            self.parts.shelf.update(cx, |shelf, cx| shelf.close(cx));
        }
        if mode == Mode::Publish {
            self.request_if_changed(cx);
        } else {
            slip_control::close(&self.parts, cx);
        }
        self.focus_main(window, cx);
        if changed {
            self.ready_pages(window, cx);
            cx.emit(ModeChanged(mode));
        }
        cx.notify();
    }

    /// Gives the keys to what the mode is about.
    pub fn focus_main(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Write => window.focus(&self.parts.editor.focus_handle(cx), cx),
            Mode::Publish => window.focus(&self.parts.preview.focus_handle(cx), cx),
        }
    }

    /// Puts a dot on the Write switch, or takes it off.
    pub fn set_write_attention(&mut self, wanted: bool, cx: &mut Context<Self>) {
        if self.write_attention != wanted {
            self.write_attention = wanted;
            cx.notify();
        }
    }

    pub fn set_exporting(&mut self, running: bool, cx: &mut Context<Self>) {
        if self.exporting != running {
            self.exporting = running;
            cx.notify();
        }
    }

    /// Opens the dialog for a new section.
    pub fn new_section(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog.update(cx, |dialog, cx| dialog.open_new_section(window, cx));
    }

    /// While the pages are not on screen, has what Publish would show ready, for a window this size.
    fn ready_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ahead = (self.mode == Mode::Write).then(|| {
            let window_size = window.viewport_size();
            let width = window_size.width - tokens::INSPECTOR_WIDTH - tokens::RAIL_WIDTH;
            let height = window_size.height - tokens::TITLEBAR_HEIGHT - tokens::SECTION_HEIGHT;
            Ahead {
                bounds: Bounds::new(point(px(0.), tokens::TITLEBAR_HEIGHT), gpui::size(width, height)),
                scale: window.scale_factor(),
                window: window.window_handle(),
            }
        });
        self.parts
            .preview
            .update(cx, |preview, cx| preview.ready_ahead(ahead, cx));
    }

    /// Whether anything is not on disk yet.
    pub fn is_dirty(&self, cx: &gpui::App) -> bool {
        self.parts.editor.read(cx).session().is_dirty()
    }

    /// Where the document is.
    pub fn location(&self, cx: &gpui::App) -> PathBuf {
        self.parts.editor.read(cx).session().location().to_owned()
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
        let title = session.window_title();
        if window.window_title() != title {
            window.set_window_title(&title);
        }
        let structure = crate::navigator::book::structure(&data.contents.book, &|id| {
            session.chapter(id).map_or("", |chapter| chapter.text())
        });
        self.parts.shelf.update(cx, |shelf, cx| {
            shelf.set_contents(data.contents.clone(), structure, data.active, cx)
        });
        self.parts
            .ticks
            .update(cx, |ticks, cx| ticks.set_data(data.clone(), cx));
        self.parts.sidebar.update(cx, |sidebar, cx| sidebar.set_data(data, cx));
        self.details
            .update(cx, |panel, cx| panel.set_details(details, cover, window, cx));
    }

    /// After a pause in typing: titles first, then pages. Every edit starts the pause again.
    fn text_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        slip_control::keep(&self.parts, cx);
        let pause = match self.mode {
            Mode::Write => LONG_PAUSE,
            Mode::Publish => REFRESH_PAUSE,
        };
        self.refresh = cx.spawn_in(window, async move |workspace, cx| {
            cx.background_executor().timer(pause).await;
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
        slip_control::keep(&self.parts, cx);
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

    /// Asks for pages unless they are set from this revision or being set from it. A layout that failed asks again, since
    /// what it lacked may be there now, and that is no change of the text.
    fn request_if_changed(&mut self, cx: &mut Context<Self>) {
        if self.requested != Some(self.parts.editor.read(cx).revision()) {
            self.request_pages(cx);
        } else {
            self.retry_failed_layout(cx);
        }
    }

    /// Goes to a written place in Write: line `line` of the chapter file `path`.
    fn show_text_place(&mut self, path: &Path, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, offset)) = crate::shell::report::place(self.parts.editor.read(cx).session(), path, line) else {
            return;
        };
        self.show(Mode::Write, window, cx);
        self.parts
            .editor
            .update(cx, |editor, cx| editor.show_place(id, offset, window, cx));
    }

    /// A file the book may lack changed: if the last layout failed, typesets again.
    pub fn retry_failed_layout(&mut self, cx: &mut Context<Self>) {
        if self.parts.preview.read(cx).layout_failed() {
            self.parts.preview.update(cx, |preview, cx| preview.refresh(cx));
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

    /// Opens the proof slip on the block a click landed on, or closes it on a click on nothing.
    fn page_clicked(
        &mut self,
        _: &Entity<PreviewView>,
        PageClicked(block): &PageClicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match block {
            Some(block) => slip_control::open(&self.parts, block, self.book_type.as_ref(), window, cx),
            None => {
                slip_control::close(&self.parts, cx);
                self.focus_main(window, cx);
            }
        }
    }

    /// Closes the slip as Escape does. For scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn close_slip_for_gate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        slip_control::close(&self.parts, cx);
        self.focus_main(window, cx);
    }

    /// Presses the slip's page break button. For scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn toggle_page_break_for_gate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        slip_control::toggled(&self.parts, Directive::PageBreak, window, cx);
    }

    #[cfg(any(debug_assertions, feature = "measure"))]
    /// Opens the slip on the first paragraph with these words, as a click on them would. For scripted runs.
    pub fn open_slip_on(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        let block = self.parts.preview.read(cx).block_with_text(words);
        self.open_slip_at(block, window, cx);
    }

    #[cfg(any(debug_assertions, feature = "measure"))]
    /// Opens the slip on a block, as a click on it would. For scripted runs.
    pub fn open_slip_at(&mut self, block: Option<BlockAt>, window: &mut Window, cx: &mut Context<Self>) {
        let preview = self.parts.preview.clone();
        self.page_clicked(&preview, &PageClicked(block), window, cx);
    }

    fn slip_event(&mut self, _: &Entity<ProofSlip>, event: &SlipEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SlipEvent::Edited => slip_control::edited(&self.parts, window, cx),
            SlipEvent::Toggle(directive) => slip_control::toggled(&self.parts, *directive, window, cx),
            SlipEvent::StylesWanted => slip_control::toggle_styles(&self.parts, window, cx),
            SlipEvent::PickStyle(name) => slip_control::pick_style(&self.parts, name.as_deref(), window, cx),
            SlipEvent::PickAlignment(align) => slip_control::pick_alignment(&self.parts, align, window, cx),
            SlipEvent::ShowStyle(style) => self.parts.inspector.update(cx, |inspector, cx| {
                inspector.open_style(style, window, cx);
            }),
            SlipEvent::Closed => {
                slip_control::close(&self.parts, cx);
                self.focus_main(window, cx);
            }
        }
    }

    pub(crate) fn sidebar_event(&mut self, event: &SidebarEvent, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.parts.editor.clone();
        match event {
            SidebarEvent::Select(id) => {
                editor.update(cx, |editor, cx| editor.show_chapter(*id, window, cx));
                self.focus_main(window, cx);
            }
            SidebarEvent::Jump(id, offset) => {
                editor.update(cx, |editor, cx| editor.show_heading(*id, *offset, window, cx));
                self.focus_main(window, cx);
            }
            SidebarEvent::FocusText => self.focus_main(window, cx),
            SidebarEvent::Browse(id) => {
                editor.update(cx, |editor, cx| editor.show_chapter(*id, window, cx));
            }
            SidebarEvent::Move(id, to) => {
                let (id, to) = (*id, *to);
                self.change(move |session| session.move_chapter(id, to), window, cx);
            }
            SidebarEvent::UndoMove => {
                self.change(|session| session.undo_move().map(drop), window, cx);
            }
            SidebarEvent::RedoMove => {
                self.change(|session| session.redo_move().map(drop), window, cx);
            }
            SidebarEvent::SetStatus(id, status) => {
                let (id, status) = (*id, status.clone());
                self.change(move |session| session.set_status(id, status.as_deref()), window, cx);
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
            SidebarEvent::NewSection => self.new_section(window, cx),
            SidebarEvent::ShowDetails => {
                self.show(Mode::Publish, window, cx);
                self.parts
                    .inspector
                    .update(cx, |inspector, cx| inspector.show(Tab::Details, cx));
            }
        }
    }

    fn shelf_event(
        &mut self,
        _: &Entity<BookshelfView>,
        event: &ShelfEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ShelfEvent::Select(id) => self.sidebar_event(&SidebarEvent::Select(*id), window, cx),
            ShelfEvent::Move(id, place) => self.sidebar_event(&SidebarEvent::Move(*id, *place), window, cx),
            ShelfEvent::StepMove { undo: true } => self.sidebar_event(&SidebarEvent::UndoMove, window, cx),
            ShelfEvent::StepMove { undo: false } => self.sidebar_event(&SidebarEvent::RedoMove, window, cx),
            ShelfEvent::Closed => self.focus_main(window, cx),
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
                self.choose_file(Session::set_cover, window, cx);
                return;
            }
            DetailsEdit::ChooseBibliography => {
                self.choose_file(Session::set_bibliography, window, cx);
                return;
            }
        };
        self.details.update(cx, |panel, cx| panel.set_error(result.err(), cx));
    }

    /// Asks for a file and hands it to the session: the cover or the bibliography.
    fn choose_file(
        &mut self,
        set: fn(&mut Session, &std::path::Path) -> Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::t!(Key::ChooseFile).into()),
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
                        editor.update_session(|session| set(session, &path), window, cx)
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

    /// Gives the galley the theme's type when it changed, with its font files if the toolkit lacks them.
    fn set_type(&mut self, book_type: &BookType, cx: &mut Context<Self>) {
        if self.book_type.as_ref() == Some(book_type) {
            return;
        }
        let new_files: Vec<Cow<'static, [u8]>> = book_type
            .files
            .iter()
            .filter(|file| self.fonts.insert(file.hash()))
            .map(|file| Cow::Owned(file.to_vec()))
            .collect();
        if !new_files.is_empty() {
            let error = cx
                .text_system()
                .add_fonts(new_files)
                .err()
                .map(|error| error.to_string());
            let folder = self.parts.editor.read(cx).session().base_dir();
            crate::persist::report(&folder, crate::persist::sync::Problem::Fonts, error, cx);
        }
        self.parts
            .editor
            .update(cx, |editor, cx| editor.set_type(book_type.clone(), cx));
        self.book_type = Some(book_type.clone());
    }

    fn mode_switch(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let buttons: Vec<_> = Mode::ALL
            .iter()
            .map(|&(mode, key)| {
                let label = crate::t!(key);
                let active = mode == self.mode;
                div()
                    .id(label)
                    .toggle(tokens, label, active)
                    .h(tokens::MODE_HEIGHT)
                    .px(tokens::MODE_PADDING)
                    .flex()
                    .items_center()
                    .text_size(tokens::MODE_TEXT)
                    .rounded(tokens::RADIUS)
                    .cursor_pointer()
                    .text_color(if active { tokens.text } else { tokens.text_quiet })
                    .when(active, |button| button.bg(tokens.raised))
                    .hover(|button| button.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |workspace, _, window, cx| workspace.show(mode, window, cx)))
                    .child(label)
                    .when(mode == Mode::Write && self.write_attention, |button| {
                        button.relative().child(
                            div()
                                .absolute()
                                .top(tokens::SWITCH_DOT_INSET)
                                .right(tokens::SWITCH_DOT_INSET)
                                .size(tokens::DOT_SIZE)
                                .rounded_full()
                                .bg(tokens.accent),
                        )
                    })
            })
            .collect();
        div().flex().items_center().gap(tokens::HAIR_GAP).children(buttons)
    }

    /// Publish's Export button: the action that moves on, so it has accent text, quiet while a run goes.
    fn export_button(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        crate::shell::ui::small_button(tokens, "title-export", crate::t!(Key::ExportRun))
            .when(!self.exporting, |button| button.text_color(tokens.accent))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::shell::menus::ShowExport), cx))
    }
}

/// How wide the mode switch at the title bar's centre is, as [`Workspace::mode_switch`] lays it out.
pub fn mode_switch_width(window: &Window) -> Pixels {
    let buttons = Mode::ALL.iter().map(|(_, key)| {
        crate::shell::ui::text_width(crate::t!(*key), tokens::MODE_TEXT, window) + tokens::MODE_PADDING * 2.
    });
    buttons.fold(Pixels::ZERO, |sum, width| sum + width) + tokens::HAIR_GAP * (Mode::ALL.len() - 1) as f32
}

/// The bar at the window's top, which moves the window. It holds `start` at its left, the mode switch at its centre,
/// and `end` at its right, and shares the window's surface. Also used with no document.
pub fn titlebar(
    tokens: &Tokens,
    start: impl IntoElement,
    center: impl IntoElement,
    end: impl IntoElement,
) -> impl IntoElement {
    div()
        .id("titlebar")
        .tab_group()
        .tab_index(tokens::TAB_TITLEBAR)
        .tab_stop(false)
        .flex_none()
        .h(tokens::TITLEBAR_HEIGHT)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .bg(tokens.canvas)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
        .child(start)
        .child(center)
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let accessible = window.is_a11y_active();
        let switch = self.mode_switch(&tokens, cx);
        let export = (self.mode == Mode::Publish).then(|| self.export_button(&tokens));
        // Fills the title bar, so its views place themselves in it.
        let title = div().absolute().top_0().left_0().size_full().children(
            (self.mode == Mode::Write)
                .then(|| self.slots.title.clone())
                .into_iter()
                .flatten(),
        );
        // In Publish the same line stands in the status line, see `PreviewView::set_foot`.
        let foot = (self.mode == Mode::Write).then(|| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(tokens::FOOTER_HEIGHT)
                .flex()
                .items_center()
                .children(self.slots.footer.clone())
        });
        // The Binder gives way before the text goes below its least width, so no line is cut or covered.
        let window_width = window.viewport_size().width;
        let width_of =
            |panels: &[SidePanel]| -> Pixels { panels.iter().map(|panel| (panel.width)(window_width, cx)).sum() };
        let (left, right) = (width_of(&self.slots.left), width_of(&self.slots.right));
        let text_room = window_width - left - right - tokens::SIDEBAR_WIDTH;
        let binder_fits = text_room >= tokens::GUTTER_WIDTH + tokens::TEXT_MIN_WIDTH + tokens::TEXT_MARGIN_RIGHT;
        let binder_on = settings::get(cx).binder && binder_fits;
        let binder_width = if self.mode == Mode::Write && binder_on {
            tokens::SIDEBAR_WIDTH
        } else {
            Pixels::ZERO
        };
        let rail_width = if self.mode == Mode::Publish {
            tokens::INSPECTOR_WIDTH + tokens::RAIL_WIDTH
        } else {
            Pixels::ZERO
        };
        let column = TextColumn {
            left: binder_width + left,
            width: window_width - binder_width - left - right - rail_width,
            rail: self.mode == Mode::Publish,
            text: (self.mode == Mode::Write)
                .then(|| self.parts.editor.read(cx).text_span())
                .filter(|(_, width)| *width > Pixels::ZERO),
        };
        if cx.try_global::<TextColumn>() != Some(&column) {
            cx.set_global(column);
        }
        let centre = div()
            .tab_group()
            .tab_index(tokens::TAB_CENTRE)
            .tab_stop(false)
            .flex_1()
            .min_w_0()
            .flex()
            .relative()
            .map(|centre| match self.mode {
                // The text ends above the foot line, so nothing of it shows beside what the line says.
                Mode::Write => centre
                    .pb(tokens::FOOTER_HEIGHT)
                    .child(self.parts.editor.clone())
                    .when(!binder_on, |centre| centre.child(self.parts.ticks.clone())),
                Mode::Publish => centre.child(self.parts.preview.clone()),
            });
        let binder = (self.mode == Mode::Write && binder_on).then(|| {
            // Cached, so a key repaints the galley and not every row beside it. A cached view that is reused reports no
            // accessibility nodes in gpui, so with assistive technology nothing is.
            let style = StyleRefinement::default().flex_none().w(tokens::SIDEBAR_WIDTH).h_full();
            div()
                .tab_group()
                .tab_index(tokens::TAB_PANEL)
                .tab_stop(false)
                .h_full()
                .child(pane(self.parts.sidebar.clone(), style, accessible))
        });
        let rail = (self.mode == Mode::Publish).then(|| {
            let style = StyleRefinement::default()
                .flex_none()
                .w(tokens::INSPECTOR_WIDTH + tokens::RAIL_WIDTH)
                .h_full();
            pane(self.parts.inspector.clone(), style, accessible)
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .key_context("Workspace")
            .on_action(|_: &FocusNext, window, cx| crate::a11y::tab(true, window, cx))
            .on_action(|_: &FocusPrevious, window, cx| crate::a11y::tab(false, window, cx))
            .on_action(cx.listener(|workspace, _: &Save, _, cx| {
                workspace.parts.editor.update(cx, |_, cx| cx.emit(SaveRequested));
            }))
            .child(titlebar(&tokens, title, switch, div().children(export)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .bg(tokens.canvas)
                    .children(binder)
                    .children(self.slots.left.iter().map(|panel| panel.view.clone()))
                    .child(centre.children(foot))
                    .children(self.slots.right.iter().map(|panel| panel.view.clone()))
                    .children(rail),
            )
            .children((self.mode == Mode::Write && settings::get(cx).bookshelf).then(|| self.parts.shelf.clone()))
            .children(self.slots.overlays.clone())
    }
}

/// A side pane: cached unless accessibility is on, when its plain view has the same style on a box around it.
fn pane<V: Render>(view: Entity<V>, style: StyleRefinement, accessible: bool) -> gpui::AnyElement {
    if accessible {
        let mut holder = div();
        *holder.style() = style;
        holder.child(view).into_any_element()
    } else {
        view.cached(style).into_any_element()
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
