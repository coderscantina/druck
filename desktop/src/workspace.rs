//! One open document: its two modes, the slots later work mounts into, and the wiring between the panes.
//!
//! The window is in Write or Publish, see [`Mode`]. Write shows the text, with the Binder if the author wants it. Publish
//! shows the pages and the rail. Both edit the same text, and [`ModeChanged`] tells who cares.
//!
//! The session lives in the editor view. Panes hold no document: this module hands them what they show and carries out
//! what they ask for through [`EditorView::update_session`].

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyView, Bounds, Context, Entity, EventEmitter, Focusable, MouseButton, PathPromptOptions, StyleRefinement,
    Subscription, Task, Window, div, point, px,
};

use crate::a11y::Control as _;
use crate::editor::{CaretMoved, EditorView, Save, SaveRequested, SessionChanged, TextChanged};
use crate::inspector::details::{DetailsEdit, DetailsPanel};
use crate::inspector::{InspectorView, SpecimenRequested, Tab, ThemeChanged, ThemeSwitched};
use crate::preview::directives::{self, edit_between};
use crate::preview::proof::BlockAt;
use crate::preview::slip::{self, Directive, ProofSlip, SlipEvent};
use crate::preview::{Ahead, BookType, PageClicked, PreviewView, ProblemsWanted, Typeset};
use crate::session::{Group, Session};
use crate::settings;
use crate::shell::text::Key;
use crate::sidebar::dialogs::{DialogEvent, SectionDialog};
use crate::sidebar::ticks::TicksView;
use crate::sidebar::{SidebarData, SidebarEvent, SidebarView};
use crate::source::EditKind;
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

    /// The mode `DRUCK_MODE` names, for screenshots and measurements.
    fn from_env() -> Option<Self> {
        match crate::gate::var("DRUCK_MODE")?.as_str() {
            "write" => Some(Self::Write),
            "publish" => Some(Self::Publish),
            _ => None,
        }
    }
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
}

/// What a slice's view can hold on to. Everything about the document is in the editor's session; events are on the
/// entities: [`TextChanged`], [`CaretMoved`], and [`SessionChanged`] on the editor, [`Typeset`] on the preview.
#[derive(Clone)]
pub struct Parts {
    pub editor: Entity<EditorView>,
    pub preview: Entity<PreviewView>,
    pub sidebar: Entity<SidebarView>,
    pub ticks: Entity<TicksView>,
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
    /// `DRUCK_SLIP`: the words of a paragraph to open a slip on once there are pages, for screenshots.
    slip_words: Option<String>,
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
        let ticks = cx.new(|_| TicksView::new());
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
            cx.subscribe_in(&preview, window, |workspace, _, Typeset(book_type), window, cx| {
                workspace.set_type(book_type, cx);
                workspace.show_caret(cx);
                workspace.show_fonts(cx);
                workspace.jump_to_style(window, cx);
                workspace.ready_pages(window, cx);
                workspace.sync_slip(cx);
                if let Some(words) = workspace.slip_words.take() {
                    workspace.open_slip_on(&words, window, cx);
                    if crate::gate::var("DRUCK_SLIP_BREAK").is_some() {
                        workspace.toggle_page_break_for_gate(window, cx);
                    }
                }
            }),
            cx.observe_global_in::<settings::Store>(window, |_, _, cx| cx.notify()),
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
            cx.subscribe_in(&proof_slip, window, Self::slip_event),
            cx.subscribe_in(&sidebar, window, |workspace, _, event, window, cx| {
                workspace.sidebar_event(event, window, cx);
            }),
            cx.subscribe_in(&ticks, window, |workspace, _, event, window, cx| {
                workspace.sidebar_event(event, window, cx);
            }),
            cx.subscribe_in(&details, window, Self::details_event),
            cx.subscribe_in(&dialog, window, Self::dialog_event),
        ];
        let parts = Parts {
            editor,
            preview,
            sidebar,
            ticks,
            inspector,
        };
        let mut slots = Slots::default();
        crate::app::mount(&parts, &mut slots, window, cx);
        slots.overlays.push(dialog.clone().into());
        let mode = Mode::from_env().unwrap_or(Mode::Write);
        let mut workspace = Self {
            mode,
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
            slip_words: crate::gate::var("DRUCK_SLIP"),
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
        if mode == Mode::Publish {
            workspace.focus_main(window, cx);
        }
        cx.emit(ModeChanged(mode));
        // For screenshots: `DRUCK_CHAPTER=4` opens that chapter of the book, and `DRUCK_SELECT=word` selects a word in it.
        if let Some(chapter) = crate::gate::var("DRUCK_CHAPTER").and_then(|chapter| chapter.parse::<usize>().ok()) {
            let id = workspace.parts.editor.read(cx).session().book().get(chapter).copied();
            if let Some(id) = id {
                workspace
                    .parts
                    .editor
                    .update(cx, |editor, cx| editor.show_chapter(id, window, cx));
            }
        }
        if let Some(word) = crate::gate::var("DRUCK_SELECT") {
            workspace.parts.editor.update(cx, |editor, cx| {
                if let Some(at) = editor.text().find(&word) {
                    editor.select_for_screenshot(at..at + word.len(), window, cx);
                }
            });
        }
        crate::gate::start(
            cx.entity(),
            workspace.parts.editor.clone(),
            workspace.parts.preview.clone(),
            workspace.parts.inspector.clone(),
            window.window_handle(),
            cx,
        );
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
        if mode == Mode::Publish {
            self.request_if_changed(cx);
        } else {
            self.close_slip(cx);
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
        self.parts
            .ticks
            .update(cx, |ticks, cx| ticks.set_data(data.clone(), cx));
        self.parts.sidebar.update(cx, |sidebar, cx| sidebar.set_data(data, cx));
        self.details
            .update(cx, |panel, cx| panel.set_details(details, cover, window, cx));
    }

    /// After a pause in typing: titles first, then pages. Every edit starts the pause again.
    fn text_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keep_slip(cx);
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
        self.keep_slip(cx);
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

    /// Opens the proof slip on the block a click landed on, or closes it on a click on nothing. Pages behind the text have
    /// other offsets, so a click on them does nothing until they catch up. The text goes to the same place, so Write opens
    /// there.
    fn page_clicked(
        &mut self,
        _: &Entity<PreviewView>,
        PageClicked(block): &PageClicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = block else {
            self.close_slip(cx);
            self.focus_main(window, cx);
            return;
        };
        let editor = self.parts.editor.clone();
        if !self.parts.preview.read(cx).shows(editor.read(cx).revision()) {
            return;
        }
        let session = editor.read(cx).session();
        let Some(opening) = slip::opening(session, block) else {
            return;
        };
        let Some(chapter) = session.chapter(opening.chapter) else {
            return;
        };
        let text = chapter.text();
        let written = text[opening.range.clone()].to_owned();
        let state = directives::read(text, &opening.range);
        let (id, at) = (opening.chapter, opening.range.start);
        let stand_in = tokens::stand_in_type();
        let book_type = self.book_type.as_ref().unwrap_or(&stand_in);
        let face = book_type.face_of(opening.style.as_deref().unwrap_or("body")).clone();
        self.slip(cx).update(cx, |slip, cx| {
            slip.open(opening, &written, state, &face, window, cx);
        });
        editor.update(cx, |editor, cx| editor.show_place(id, at, window, cx));
        self.sync_slip(cx);
    }

    /// Closes the slip as Escape does. For scripted runs.
    pub fn close_slip_for_gate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_slip(cx);
        self.focus_main(window, cx);
    }

    /// Presses the slip's page break button. For scripted runs.
    pub fn toggle_page_break_for_gate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.slip_toggled(Directive::PageBreak, window, cx);
    }

    /// Opens the slip on the first paragraph with these words, as a click on them would. For scripted runs.
    pub fn open_slip_on(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        let block = self.parts.preview.read(cx).block_with_text(words);
        self.open_slip_at(block, window, cx);
    }

    /// Opens the slip on a block, as a click on it would. For scripted runs.
    pub fn open_slip_at(&mut self, block: Option<BlockAt>, window: &mut Window, cx: &mut Context<Self>) {
        let preview = self.parts.preview.clone();
        self.page_clicked(&preview, &PageClicked(block), window, cx);
    }

    fn slip(&self, cx: &gpui::App) -> Entity<ProofSlip> {
        self.parts.preview.read(cx).slip().clone()
    }

    fn close_slip(&mut self, cx: &mut Context<Self>) {
        self.slip(cx).update(cx, |slip, cx| slip.close(cx));
        self.parts.preview.update(cx, |preview, cx| preview.set_slip(None, cx));
    }

    /// Shows the slip at its paragraph on the pages, once they are set from the text it is in.
    fn sync_slip(&mut self, cx: &mut Context<Self>) {
        let Some(opened) = self.slip(cx).read(cx).opened().cloned() else {
            return;
        };
        let editor = self.parts.editor.read(cx);
        if !self.parts.preview.read(cx).shows(editor.revision()) {
            return;
        }
        let Some((file, start)) = editor.session().file_position(opened.chapter, opened.range.start) else {
            return;
        };
        let source = (file, start..start + opened.range.len());
        self.parts
            .preview
            .update(cx, |preview, cx| preview.set_slip(Some(source), cx));
    }

    /// Keeps the slip on its paragraph when something else changed the chapter, or closes it if the paragraph is gone.
    fn keep_slip(&mut self, cx: &mut Context<Self>) {
        let slip = self.slip(cx);
        let Some(opened) = slip.read(cx).opened().cloned() else {
            return;
        };
        if slip.read(cx).input().read(cx).is_composing() {
            return;
        }
        let held = slip.read(cx).text(cx).to_owned();
        let session = self.parts.editor.read(cx).session();
        let found = session.chapter(opened.chapter).and_then(|chapter| {
            let text = chapter.text();
            let range = slip::relocated(text, &held, &opened.range)?;
            Some((directives::read(text, &range), range))
        });
        match found {
            Some((_, range)) if range == opened.range => {}
            Some((state, range)) => slip.update(cx, |slip, cx| slip.moved_to(range, state, cx)),
            None => self.close_slip(cx),
        }
    }

    fn slip_event(&mut self, _: &Entity<ProofSlip>, event: &SlipEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SlipEvent::Edited => self.slip_edited(window, cx),
            SlipEvent::Toggle(directive) => self.slip_toggled(*directive, window, cx),
            SlipEvent::ShowStyle(style) => self.parts.inspector.update(cx, |inspector, cx| {
                inspector.open_style(style, window, cx);
            }),
            SlipEvent::Closed => {
                self.close_slip(cx);
                self.focus_main(window, cx);
            }
        }
    }

    /// Carries what changed in the slip over to the chapter, which is the truth.
    fn slip_edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let slip = self.slip(cx);
        let Some(opened) = slip.read(cx).opened().cloned() else {
            return;
        };
        let new = slip.read(cx).text(cx).to_owned();
        let editor = self.parts.editor.clone();
        let Some(chapter) = editor.read(cx).session().chapter(opened.chapter) else {
            return;
        };
        let Some(old) = chapter.text().get(opened.range.clone()).map(str::to_owned) else {
            return;
        };
        let Some(edit) = edit_between(opened.range.start, &old, &new) else {
            return;
        };
        let kind = if new.len() < old.len() {
            EditKind::Deleting
        } else {
            EditKind::Typing
        };
        let range = opened.range.start..opened.range.start + new.len();
        let made = editor.update(cx, |editor, cx| {
            editor.edit_outside(opened.chapter, vec![edit], kind, window, cx)
        });
        if made.is_ok() {
            let text = editor.read(cx).session().buffer(opened.chapter).text();
            let state = directives::read(text, &range);
            slip.update(cx, |slip, cx| slip.moved_to(range, state, cx));
        }
    }

    /// Adds or takes away a directive around the slip's paragraph.
    fn slip_toggled(&mut self, directive: Directive, window: &mut Window, cx: &mut Context<Self>) {
        let slip = self.slip(cx);
        let Some(opened) = slip.read(cx).opened().cloned() else {
            return;
        };
        let editor = self.parts.editor.clone();
        let text = editor.read(cx).session().buffer(opened.chapter).text();
        let change = match directive {
            Directive::PageBreak => directives::toggle_page_break(text, &opened.range),
            Directive::Keep => directives::toggle_keep(text, &opened.range),
        };
        let range = change.range.clone();
        let made = editor.update(cx, |editor, cx| {
            editor.edit_outside(opened.chapter, change.edits, EditKind::Discrete, window, cx)
        });
        if made.is_ok() {
            let text = editor.read(cx).session().buffer(opened.chapter).text();
            let state = directives::read(text, &range);
            slip.update(cx, |slip, cx| slip.moved_to(range, state, cx));
        }
    }

    fn sidebar_event(&mut self, event: &SidebarEvent, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.parts.editor.clone();
        match event {
            SidebarEvent::Select(id) => {
                editor.update(cx, |editor, cx| editor.show_chapter(*id, window, cx));
                self.focus_main(window, cx);
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
            SidebarEvent::NewSection => self.new_section(window, cx),
            SidebarEvent::ShowDetails => {
                self.show(Mode::Publish, window, cx);
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
        if !new_files.is_empty()
            && let Err(error) = cx.text_system().add_fonts(new_files)
        {
            eprintln!("the galley cannot use the theme's font: {error}");
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
                    .px(tokens::SPACE_2)
                    .rounded(tokens::RADIUS)
                    .cursor_pointer()
                    .text_color(if active { tokens.text } else { tokens.text_quiet })
                    .when(active, |button| button.bg(tokens.raised))
                    .hover(|button| button.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |workspace, _, window, cx| workspace.show(mode, window, cx)))
                    .child(label)
            })
            .collect();
        div().flex().items_center().gap(px(2.)).children(buttons)
    }

    /// Publish's Export button: the action that moves on, so it has the accent.
    fn export_button(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let label = crate::t!(Key::ExportRun);
        div()
            .id("title-export")
            .button(tokens, label)
            .px(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .text_color(tokens.accent)
            .hover(|button| button.bg(tokens.raised))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::shell::menus::ShowExport), cx))
            .child(label)
    }
}

/// The bar at the window's top, which moves the window. It holds the mode switch at its centre and `end` at its right, and
/// shares the window's surface. Also used with no document.
pub fn titlebar(tokens: &Tokens, center: impl IntoElement, end: impl IntoElement) -> impl IntoElement {
    div()
        .id("titlebar")
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
        let foot = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(if self.mode == Mode::Publish {
                tokens::SECTION_HEIGHT
            } else {
                px(0.)
            })
            .h(tokens::FOOTER_HEIGHT)
            .flex()
            .items_center()
            .children(self.slots.footer.clone());
        let binder_on = settings::get(cx).binder;
        let centre = div()
            .flex_1()
            .min_w_0()
            .flex()
            .relative()
            .map(|centre| match self.mode {
                Mode::Write => centre
                    .child(self.parts.editor.clone())
                    .when(!binder_on, |centre| centre.child(self.parts.ticks.clone())),
                Mode::Publish => centre.child(self.parts.preview.clone()),
            });
        let binder = (self.mode == Mode::Write && binder_on).then(|| {
            // Cached, so a key repaints the galley and not every row beside it. A cached view that is reused reports no
            // accessibility nodes in gpui, so with assistive technology nothing is.
            let style = StyleRefinement::default().flex_none().w(tokens::SIDEBAR_WIDTH).h_full();
            pane(self.parts.sidebar.clone(), style, accessible)
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
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
            .on_action(cx.listener(|workspace, _: &Save, _, cx| {
                workspace.parts.editor.update(cx, |_, cx| cx.emit(SaveRequested));
            }))
            .child(titlebar(&tokens, switch, div().children(export)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .bg(tokens.canvas)
                    .children(binder)
                    .child(centre.child(foot))
                    .children(rail),
            )
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
