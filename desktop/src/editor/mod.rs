//! Write's text: one chapter in the theme's type on the app's surface. The buffer holds the Markdown, every change is an
//! edit to it, and what shows is derived from it: blocks, hidden markers, and shaped lines, only for what is on screen.
//!
//! [`commands`] writes what the toolbar, slash menu, and gutter ask for, without gpui. [`float`] and [`gutter`] are their
//! views, and [`margin`] the chapter's notes in the right margin. [`rich`] holds the footnotes, citations, tables, and
//! images, with their floats and the Selection and Images tabs. [`input`] is what typing and the input methods do to the
//! text, [`selection`] where the caret goes, and [`review`] what shows of an agent's work.

mod accessible;
mod commands;
mod display;
mod element;
mod float;
mod gutter;
mod input;
mod keys;
mod layout;
mod margin;
mod place;
pub mod review;
pub(crate) mod rich;
mod scrollbar;
mod selection;
mod spelling;
mod strings;
mod trace;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;
#[cfg(any(debug_assertions, feature = "measure"))]
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    AnyView, App, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    KeyContext, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollEvent, SharedString,
    Subscription, Task, UTF16Selection, Window, div, point, px, size,
};

use crate::preview::BookType;
use crate::session::{ChapterId, ChapterView, Session};
use crate::settings::{self, LineLength};
use crate::source::blocks::BlockChange;
use crate::source::inline::{inline_spans, line_markers};
use crate::source::{BlockKind, Blocks, Edit, EditKind, Selection, motion, typing};
use crate::tokens::{self, Tokens};
use crate::workspace::{Parts, Workspace};
use accessible::{ShownBlock, a11y_text};
use commands::convert::BlockType;
use commands::marks::Mark;
use commands::notes;
use display::{Syntax, display_lines, hidden_ranges};
use element::Galley;
use float::Float;
pub(crate) use float::tooltip;
use gutter::{BlockGrab, CommentMarker};
pub use keys::*;
use keys::{CONTEXT, MENU_CONTEXT};
use layout::{BlockLayout, Column, GalleyStyle, ShapeRequest, shape_block};
use margin::ChapterNotes;
use trace::Trace;

/// Mounts what this slice shows around the text: the chapter's notes, and the rich blocks' tabs.
pub fn mount(parts: &Parts, window: &mut Window, cx: &mut Context<Workspace>) {
    let editor = parts.editor.clone();
    let notes = cx.new(|cx| ChapterNotes::new(editor.clone(), window, cx));
    editor.update(cx, |editor, _| editor.margin = Some(notes));
    rich::view::mount(parts, cx);
}

#[derive(Clone, Copy)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    PageUp,
    PageDown,
}

/// Where the top of the viewport is: a distance into a block. Anchoring to a block rather than to a total height means
/// a block's estimated height turning into its real one moves nothing on screen.
#[derive(Clone, Copy, Default)]
struct Anchor {
    block: usize,
    offset: Pixels,
}

/// The cached derivations of one block. A change to the block replaces its slot.
#[derive(Default, Clone)]
struct Slot {
    syntax: Option<Rc<Syntax>>,
    layout: Option<Rc<BlockLayout>>,
    icon: Option<&'static str>,
    /// The height before shaping, for the scrollbar's sum over every block.
    estimate: Cell<Option<Pixels>>,
    /// Whether its comments show in place, opened by their gutter marker.
    expanded: bool,
}

/// A mouse selection in progress. `unit` is what a double or triple click started with, which a drag extends by.
#[derive(Clone)]
struct Drag {
    origin: Range<usize>,
    unit: DragUnit,
}

/// The scrollbar in this frame.
#[derive(Clone, Copy)]
struct Scrollbar {
    track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragUnit {
    Character,
    Word,
    Line,
}

/// A caret place in the shaped text.
#[derive(Clone, Copy)]
struct Located {
    block: usize,
    line: usize,
    row: usize,
    display: usize,
}

/// The text changed. Reading it is left to whoever decides when that is worth it.
pub struct TextChanged;

impl EventEmitter<TextChanged> for EditorView {}

/// The caret moved, to this byte of the text.
pub struct CaretMoved(pub usize);

impl EventEmitter<CaretMoved> for EditorView {}

/// Something other than typing changed the session: the active chapter, the book, or the settings.
pub struct SessionChanged;

impl EventEmitter<SessionChanged> for EditorView {}

/// The room beside the text changed, after a resize, a panel, or the margin coming or going. It is known only once the
/// text has been laid out, a frame after whatever stands beside it was drawn.
pub struct RoomChanged;

impl EventEmitter<RoomChanged> for EditorView {}

/// A person asked for the text to be on disk now.
pub struct SaveRequested;

impl EventEmitter<SaveRequested> for EditorView {}

/// An undo reached a change that came from outside. `done` says whether it took it back, or stopped there first.
pub struct ExternalUndo {
    pub done: bool,
}

impl EventEmitter<ExternalUndo> for EditorView {}

pub struct EditorView {
    focus: FocusHandle,
    /// The document. It lives here because every key needs the active buffer. See [`crate::session`].
    session: Session,
    status: SharedString,
    style: GalleyStyle,
    blocks: Blocks,
    slots: Vec<Slot>,
    selection: Selection,
    /// At a soft wrap, whether the caret shows at the end of the upper row.
    upstream: bool,
    marked: Option<Range<usize>>,
    /// An agent's changes in the active chapter. See [`review`].
    review: Vec<review::Passage>,
    /// An agent's notes beside the active chapter.
    advice: Vec<review::Advice>,
    /// The passage a conflict on the line at the foot is about.
    conflict_mark: Option<Range<usize>>,
    /// The find bar's matches in the open chapter, tinted where they show.
    find_matches: Vec<Range<usize>>,
    /// The card that floats under the selection while it has the keys.
    selection_prompt: Option<AnyView>,
    /// The column up and down keep to, kept across lines that are too short for it.
    goal_x: Option<Pixels>,
    /// Where markers reveal. Follows the caret, but stays put during a drag, so text does not jump under the pointer.
    reveal_at: usize,
    drag: Option<Drag>,
    scroll: Anchor,
    #[cfg(any(debug_assertions, feature = "measure"))]
    gutter_pinned: bool,
    autoscroll: bool,
    /// How far down the pane the caret's line is to stand at the next autoscroll, when something asks it to stay.
    caret_stays_at: Option<Pixels>,
    column: Column,
    /// Blocks in the current frame, with the window y of each one's top.
    visible: Vec<(usize, Pixels)>,
    hovered: Option<usize>,
    /// Chapters showing their source, which stay so when the writer comes back to them.
    source_chapters: HashSet<ChapterId>,
    /// Whether the active chapter shows its source.
    source: bool,
    float: Float,
    /// What the toolbar last read from the text.
    toolbar_seen: Option<float::ToolbarState>,
    /// A command chose this selection, or the writer pressed Escape, so no toolbar opens on it.
    toolbar_dismissed: bool,
    /// A press on a block's gutter icon.
    grab: Option<BlockGrab>,
    markers: Vec<CommentMarker>,
    scrollbar: Option<Scrollbar>,
    /// How far down the thumb the pointer holds it.
    thumb_grab: Option<Pixels>,
    thumb_hovered: bool,
    /// Where the pointer last was, for scrolling on while it stands outside the galley.
    pointer: Point<Pixels>,
    /// Keeps scrolling during a drag while the pointer stands outside. Dropped when it comes back.
    drag_scroll: Option<Task<()>>,
    caret_on: bool,
    blink: Task<()>,
    _focus_subscriptions: Vec<Subscription>,
    trace: Trace,
    /// What the rich blocks know of the book. See [`rich`].
    rich: rich::Rich,
    /// Waits out a pause before reading the book again. Replaced by every edit.
    rich_pause: Task<()>,
    /// Reads the engine's text for chips, off the main thread.
    rich_labels: Task<()>,
    /// The settings that shape the text, kept here so a layout needs no `App`.
    writing: Writing,
    /// The chapter's opening notes, above its first text.
    margin: Option<Entity<ChapterNotes>>,
    /// The room those notes take in the flow while unfolded.
    note_room: Pixels,
    /// Where the open table or figure card was drawn last, which [`EditorView::reveal_float`] brings into view.
    float_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Frames left to bring the card into view, after it opened or its cell moved.
    reveal_float: u8,
    /// Room the view may go past the chapter's end, so a card at its foot can be scrolled into view.
    float_room: Pixels,
    /// Later comments the author opened with their More, by their text.
    open_comments: HashSet<String>,
    /// The keys of those comments' More, by their text.
    comment_focus: HashMap<String, FocusHandle>,
    /// The pointer is in the left margin.
    gutter_shown: bool,
    /// The caret last moved by a key, which typewriter scrolling follows. A click leaves the text where it is.
    caret_by_keys: bool,
    /// Misspelled words. See [`spelling`].
    spelling: spelling::Spelling,
    /// The newest revision and body style painted, and when each first was.
    #[cfg(any(debug_assertions, feature = "measure"))]
    painted_revision: (u64, Instant),
    #[cfg(any(debug_assertions, feature = "measure"))]
    painted_style: (u64, Instant),
    #[cfg(any(debug_assertions, feature = "measure"))]
    style_generation: u64,
}

const CARET_MARGIN: Pixels = tokens::TEXT_MARGIN_RIGHT;
const BLINK: Duration = Duration::from_millis(530);
/// How often a drag scrolls while the pointer stands outside the galley, and how fast for each pixel it is out.
const DRAG_SCROLL_TICK: Duration = Duration::from_millis(16);
const DRAG_SCROLL_SPEED: f32 = 0.35;
const DRAG_SCROLL_MAX: Pixels = tokens::DRAG_SCROLL_MAX;
/// A chapter's first view: its top, with its margin above it.
const START_OF_CHAPTER: Anchor = Anchor {
    block: 0,
    offset: tokens::TEXT_START,
};

/// Where the caret stands as a chapter opens: the place the session kept for it, and in the galley the first visible
/// text rather than inside the notes hidden above it.
fn opening_selection(text: &str, blocks: &Blocks, view: ChapterView, source: bool) -> Selection {
    let clamp = |offset: usize| text.floor_char_boundary(offset.min(text.len()));
    let selection = Selection {
        anchor: clamp(view.selection.anchor),
        head: clamp(view.selection.head),
    };
    match notes::leading_region(text, blocks) {
        Some(region) if !source && selection.is_empty() && region.contains(&selection.head) => {
            Selection::caret(region.end)
        }
        _ => selection,
    }
}

/// What the settings say about the text.
#[derive(Clone, Copy, PartialEq)]
struct Writing {
    line_length: LineLength,
    typewriter: bool,
    dim: bool,
    toolbar: bool,
}

impl Writing {
    fn of(cx: &App) -> Self {
        let settings = settings::get(cx);
        Self {
            line_length: settings.line_length,
            typewriter: settings.typewriter,
            dim: settings.dim_paragraphs,
            toolbar: settings.selection_toolbar,
        }
    }
}

impl EditorView {
    /// Shows the session's active chapter.
    pub fn new(session: Session, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle().tab_stop(true);
        window.focus(&focus, cx);
        let spelling_folder = session.base_dir();
        let blocks = Blocks::parse(session.active_buffer().text());
        let selection = opening_selection(
            session.active_buffer().text(),
            &blocks,
            session.active_chapter().view,
            false,
        );
        let focus_subscriptions = vec![
            cx.on_focus_in(&focus, window, |view, window, cx| view.restart_blink(window, cx)),
            cx.on_blur(&focus, window, |view, _, cx| {
                view.blink = Task::ready(());
                cx.notify();
            }),
            cx.observe_window_activation(window, |view, window, cx| view.restart_blink(window, cx)),
            cx.observe_global::<Tokens>(|view, cx| view.colors_changed(cx)),
            cx.observe_global::<settings::Store>(|view, cx| view.settings_changed(cx)),
        ];
        let status = session
            .active_chapter()
            .problem()
            .map(SharedString::from)
            .unwrap_or_default();
        let mut view = Self {
            focus,
            status,
            style: GalleyStyle::new(Tokens::get(cx)),
            slots: vec![Slot::default(); blocks.as_slice().len()],
            session,
            blocks,
            selection,
            upstream: false,
            marked: None,
            review: Vec::new(),
            advice: Vec::new(),
            conflict_mark: None,
            find_matches: Vec::new(),
            selection_prompt: None,
            goal_x: None,
            reveal_at: selection.head,
            drag: None,
            scroll: START_OF_CHAPTER,
            autoscroll: false,
            caret_stays_at: None,
            column: Column::default(),
            visible: Vec::new(),
            hovered: None,
            source_chapters: HashSet::new(),
            source: false,
            float: Float::None,
            toolbar_seen: None,
            toolbar_dismissed: false,
            grab: None,
            markers: Vec::new(),
            scrollbar: None,
            thumb_grab: None,
            thumb_hovered: false,
            pointer: Point::default(),
            drag_scroll: None,
            caret_on: true,
            blink: Task::ready(()),
            _focus_subscriptions: focus_subscriptions,
            trace: Trace::from_env(),
            rich: rich::Rich::default(),
            rich_pause: Task::ready(()),
            rich_labels: Task::ready(()),
            writing: Writing::of(cx),
            margin: None,
            note_room: px(0.),
            float_bounds: Rc::default(),
            reveal_float: 0,
            float_room: px(0.),
            open_comments: HashSet::new(),
            comment_focus: HashMap::new(),
            gutter_shown: false,
            #[cfg(any(debug_assertions, feature = "measure"))]
            gutter_pinned: false,
            caret_by_keys: false,
            spelling: spelling::Spelling::new(cx, &spelling_folder),
            #[cfg(any(debug_assertions, feature = "measure"))]
            painted_revision: (0, Instant::now()),
            #[cfg(any(debug_assertions, feature = "measure"))]
            painted_style: (0, Instant::now()),
            #[cfg(any(debug_assertions, feature = "measure"))]
            style_generation: 0,
        };
        view.rich.atoms = rich::atoms(view.session.active_buffer().text(), &view.blocks);
        view.restart_blink(window, cx);
        view.refresh_rich(window, cx);
        view
    }

    /// The document, for reading.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Changes the session from outside the galley: anything that edits a chapter, moves chapters, or changes settings
    /// goes through here. The galley then shows the result, and [`SessionChanged`] tells everyone else.
    pub fn update_session<R>(
        &mut self,
        change: impl FnOnce(&mut Session) -> R,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> R {
        self.store_view();
        let before = (self.session.active(), self.session.active_buffer().revision());
        let result = change(&mut self.session);
        if before != (self.session.active(), self.session.active_buffer().revision()) {
            self.adopt_active(window, cx);
        }
        cx.emit(SessionChanged);
        cx.notify();
        result
    }

    /// Edits a chapter from a field outside the text, such as a proof slip, as typing does: [`TextChanged`] goes out, so the
    /// pages follow on a pause and the save on its own.
    pub fn edit_outside(
        &mut self,
        id: ChapterId,
        edits: Vec<Edit>,
        kind: EditKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::source::buffer::EditError> {
        self.store_view();
        self.session.edit_chapter(id, edits, kind)?;
        if id == self.session.active() {
            self.adopt_active(window, cx);
        }
        cx.emit(TextChanged);
        cx.notify();
        Ok(())
    }

    /// The session for bookkeeping that changes no text and nothing on screen, such as recording a finished save. Sends
    /// no event and leaves the galley alone.
    pub fn session_quietly(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Whether an input method has text in progress, which an outside change must not move under it.
    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
    }

    /// Whether an outside change must wait: it would move text under an input method, and a change to the buffer closes
    /// what floats, with what was typed in its field.
    pub fn holds_outside_changes(&self) -> bool {
        self.is_composing() || self.float_takes_keys()
    }

    /// Writes what a float's fields hold, before something closes it.
    pub fn commit_floats(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_rich_float(window, cx);
    }

    /// Opens a chapter in the galley, where the caret last was in it, with its own undo history.
    pub fn show_chapter(&mut self, id: ChapterId, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_rich_float(window, cx);
        self.update_session(
            |session| {
                session.resplit();
                if session.chapter(id).is_some() {
                    session.set_active(id);
                }
            },
            window,
            cx,
        );
    }

    /// Opens a chapter with the caret at `offset`, for a click on a page.
    pub fn show_place(&mut self, id: ChapterId, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        if id == self.session.active() {
            self.jump_to(offset, window, cx);
            return;
        }
        self.commit_rich_float(window, cx);
        self.update_session(
            |session| {
                session.set_active(id);
                session.active_chapter_mut().view = ChapterView {
                    selection: Selection::caret(offset),
                    scroll: None,
                };
            },
            window,
            cx,
        );
    }

    /// Reads the active chapter's title again, after its text changed.
    pub fn refresh_outline(&mut self) {
        let active = self.session.active();
        self.session.refresh_outline(active);
        if self.session.book().first() == Some(&active) {
            self.session.refresh_details();
        }
    }

    /// Keeps the galley's place in the active chapter, to come back to it.
    fn store_view(&mut self) {
        let view = ChapterView {
            selection: self.selection,
            scroll: Some((self.scroll.block, f32::from(self.scroll.offset))),
        };
        self.session.active_chapter_mut().view = view;
    }

    /// Takes over the session's active chapter: derives its blocks again and restores its place.
    fn adopt_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forget_review();
        let chapter = self.session.active_chapter();
        let text = chapter.text();
        let view = chapter.view;
        self.status = chapter.problem().map(SharedString::from).unwrap_or_default();
        self.blocks = Blocks::parse(text);
        self.slots = vec![Slot::default(); self.blocks.as_slice().len()];
        self.rich.atoms = rich::atoms(text, &self.blocks);
        let last_block = self.blocks.as_slice().len() - 1;
        self.autoscroll = view.scroll.is_none() && view.selection != Selection::default();
        self.scroll = view.scroll.map_or(START_OF_CHAPTER, |(block, offset)| Anchor {
            block: block.min(last_block),
            offset: px(offset),
        });
        self.source = self.source_chapters.contains(&self.session.active());
        self.selection = opening_selection(text, &self.blocks, view, self.source);
        self.reveal_at = self.selection.head;
        self.upstream = false;
        self.marked = None;
        self.goal_x = None;
        self.drag = None;
        self.drag_scroll = None;
        self.grab = None;
        self.float = Float::None;
        self.toolbar_dismissed = false;
        self.hovered = None;
        self.visible.clear();
        self.restart_blink(window, cx);
        window.notify_text_input_changed(&self.focus);
        window.notify_text_selection_changed(&self.focus);
        cx.emit(CaretMoved(self.selection.head));
        self.refresh_rich(window, cx);
        cx.notify();
    }

    pub fn text(&self) -> &str {
        self.session.active_buffer().text()
    }

    /// The selected bytes of the active chapter; empty for a caret.
    pub fn selection_range(&self) -> Range<usize> {
        self.selection.range()
    }

    /// The session's revision: it changes with the text the pages are set from.
    pub fn revision(&self) -> u64 {
        self.session.revision()
    }

    /// Puts the caret at `offset`, on a character boundary at or before it, and brings it into view.
    pub fn jump_to(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        let offset = self
            .session
            .active_buffer()
            .text()
            .floor_char_boundary(offset.min(self.session.active_buffer().len()));
        self.set_selection(Selection::caret(offset), false, window, cx);
    }

    /// Shows the gutter's icons whatever the pointer does, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn pin_gutter(&mut self, cx: &mut Context<Self>) {
        self.gutter_pinned = true;
        self.gutter_shown = true;
        cx.notify();
    }

    /// Marks the passage of the open chapter a conflict is about, or nothing.
    pub fn set_conflict_mark(&mut self, mark: Option<Range<usize>>, cx: &mut Context<Self>) {
        if self.conflict_mark != mark {
            self.conflict_mark = mark;
            cx.notify();
        }
    }

    /// Tints the find bar's `matches` in the open chapter, or none.
    pub fn set_find_matches(&mut self, matches: Vec<Range<usize>>, cx: &mut Context<Self>) {
        if self.find_matches != matches {
            self.find_matches = matches;
            cx.notify();
        }
    }

    /// Where the text starts across the window, as last drawn.
    pub fn text_left(&self) -> Pixels {
        self.column.text_left
    }

    /// The theme's body font, for text elsewhere in the book's type.
    pub fn body_font(&self) -> gpui::Font {
        self.style.body.clone()
    }

    /// Selects `range` of the active chapter and brings it into view, for the find bar.
    pub fn select(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text();
        let start = text.floor_char_boundary(range.start.min(text.len()));
        let end = text.floor_char_boundary(range.end.min(text.len()));
        self.set_selection(
            Selection {
                anchor: start,
                head: end,
            },
            false,
            window,
            cx,
        );
        // The find bar has the keys: no toolbar over the text it found.
        self.toolbar_dismissed = true;
    }

    /// Selects `range` as a person would, with the toolbar showing. For screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn select_for_screenshot(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let selection = Selection {
            anchor: range.start,
            head: range.end,
        };
        self.set_selection(selection, false, window, cx);
    }

    /// Sets the text's type from the theme. Every block is shaped again as it comes into view.
    pub fn set_type(&mut self, book_type: BookType, cx: &mut Context<Self>) {
        self.style.set_type(book_type);
        #[cfg(any(debug_assertions, feature = "measure"))]
        {
            self.style_generation += 1;
        }
        self.forget_shapes();
        self.autoscroll = true;
        cx.notify();
    }

    /// The text's zoom, where 1 is 100 percent.
    pub fn zoom(&self) -> f32 {
        self.style.zoom()
    }

    /// Scales the text. The characters per line stay.
    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.style.set_zoom(zoom);
        self.forget_shapes();
        self.autoscroll = true;
        cx.notify();
    }

    fn colors_changed(&mut self, cx: &mut Context<Self>) {
        self.style.set_colors(Tokens::get(cx));
        self.forget_shapes();
        cx.notify();
    }

    fn settings_changed(&mut self, cx: &mut Context<Self>) {
        let writing = Writing::of(cx);
        self.spelling.on = settings::get(cx).spelling;
        if writing != self.writing {
            self.writing = writing;
            for slot in &mut self.slots {
                slot.estimate.set(None);
            }
            self.autoscroll = true;
        }
        cx.notify();
    }

    /// Whether a block is out of focus: dimming is on, and the selection does not touch it.
    fn is_dimmed(&self, index: usize) -> bool {
        self.writing.dim && !self.source && {
            let range = self.selection.range();
            !(self.blocks.index_at(range.start)..=self.blocks.index_at(range.end)).contains(&index)
        }
    }

    /// The width lines wrap at: the book's measure, or the author's own length.
    fn measure(&self) -> Pixels {
        match self.writing.line_length {
            LineLength::Book => self.style.book_measure(),
            LineLength::Characters(count) => self.style.width_of_characters(count),
        }
    }

    /// Every block is shaped and estimated again as it comes into view.
    fn forget_shapes(&mut self) {
        for slot in &mut self.slots {
            slot.layout = None;
            slot.estimate.set(None);
        }
    }

    /// Shows the active chapter's plain Markdown, or its galley again. Buffer, caret, and undo stay the same.
    pub fn toggle_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.session.active();
        // The two views differ in height everywhere, so the caret's line keeps its place on screen.
        self.caret_stays_at = (self.caret_bounds(window)).map(|caret| caret.top() - self.column.viewport.top());
        self.source = !self.source;
        // Measured again when the galley draws the notes.
        self.note_room = px(0.);
        if self.source {
            self.source_chapters.insert(active);
        } else {
            self.source_chapters.remove(&active);
            self.selection = self.visible_selection();
            self.reveal_at = self.selection.head;
        }
        self.forget_shapes();
        self.float = Float::None;
        self.autoscroll = true;
        window.invalidate_character_coordinates();
        cx.notify();
    }

    /// The selection, with a caret moved out of what the galley hides.
    fn visible_selection(&self) -> Selection {
        if !self.selection.is_empty() {
            return self.selection;
        }
        Selection::caret(self.skip_hidden(self.selection.head, true))
    }

    /// The text of each note that opens the chapter, as the panel shows it.
    pub fn note_texts(&self) -> Vec<String> {
        let text = self.text();
        notes::leading_notes(text, &self.blocks)
            .into_iter()
            .map(|note| text[note.shown].to_owned())
            .collect()
    }

    /// Writes a change from the notes panel: `range` of note `index`'s shown text becomes `new`. Returns whether it was
    /// allowed. The caret in the text stays where it is, on the same text.
    fn edit_note(
        &mut self,
        index: usize,
        range: Range<usize>,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.session.active_buffer().text();
        let Some(note) = notes::leading_notes(text, &self.blocks).into_iter().nth(index) else {
            return false;
        };
        let Some(edit) = notes::note_edit(text, &note, range, new) else {
            return false;
        };
        let edits = vec![edit];
        let moved = |offset| commands::moved(offset, &edits, commands::Bias::After);
        let after = Selection {
            anchor: moved(self.selection.anchor),
            head: moved(self.selection.head),
        };
        let started = self.trace.input();
        match self
            .session
            .active_buffer_mut()
            .transact(EditKind::Typing, edits, self.selection, after, started)
        {
            Ok(splice) => self.resegment(splice),
            Err(error) => {
                eprintln!("edit rejected: {error}");
                return false;
            }
        }
        self.selection = after;
        self.reveal_at = after.head;
        window.notify_text_input_changed(&self.focus);
        self.trace.handled(started);
        self.schedule_rich(window, cx);
        cx.emit(TextChanged);
        cx.notify();
        true
    }

    fn undo_from_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restored = self.session.active_buffer_mut().undo();
        self.restore(restored, window, cx);
    }

    fn redo_from_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restored = self.session.active_buffer_mut().redo();
        self.restore(restored, window, cx);
    }

    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn style_generation(&self) -> u64 {
        self.style_generation
    }

    /// When `revision` or a newer one was first painted, if it has been.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn revision_painted_at(&self, revision: u64) -> Option<Instant> {
        (self.painted_revision.0 >= revision).then_some(self.painted_revision.1)
    }

    /// When body style `generation` or a newer one was first painted, if it has been.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn style_painted_at(&self, generation: u64) -> Option<Instant> {
        (self.painted_style.0 >= generation).then_some(self.painted_style.1)
    }

    /// Called at the end of each paint.
    #[cfg(any(debug_assertions, feature = "measure"))]
    fn note_painted(&mut self) {
        let now = Instant::now();
        if self.painted_revision.0 != self.session.revision() {
            self.painted_revision = (self.session.revision(), now);
        }
        if self.painted_style.0 != self.style_generation {
            self.painted_style = (self.style_generation, now);
        }
    }

    /// Shows the caret and blinks it from now on. Only a focused galley in the active window blinks, so an idle one
    /// draws nothing.
    fn restart_blink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.caret_on = true;
        if !self.focus.is_focused(window) || !window.is_window_active() {
            self.blink = Task::ready(());
            return;
        }
        self.blink = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let blinked = this.update_in(cx, |this, window, cx| {
                    let showing = this.focus.is_focused(window) && window.is_window_active();
                    // A selection shows no caret, so it has nothing to draw.
                    if showing && this.selection.is_empty() {
                        this.caret_on = !this.caret_on;
                        cx.notify();
                    }
                    showing
                });
                if !matches!(blinked, Ok(true)) {
                    break;
                }
            }
        });
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if self.grab.take().is_some() {
            self.drag_scroll = None;
        } else if !matches!(self.float, Float::None) {
            self.float = Float::None;
        } else {
            self.toolbar_dismissed = true;
        }
        cx.notify();
    }

    fn scrollbar_mouse_down(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(bar) = self.scrollbar.filter(|bar| bar.track.contains(&position)) else {
            return false;
        };
        let grab = if bar.thumb.contains(&position) {
            position.y - bar.thumb.top()
        } else {
            // A click on the track brings the thumb's middle there.
            bar.thumb.size.height * 0.5
        };
        self.thumb_grab = Some(grab);
        self.scrollbar_mouse_moved(position, cx);
        true
    }

    fn scrollbar_mouse_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(grab), Some(bar)) = (self.thumb_grab, self.scrollbar) else {
            return;
        };
        let content: f32 = self.content_height();
        let viewport = f32::from(bar.track.size.height);
        let top = f32::from(position.y - bar.track.top() - grab);
        let scrolled = scrollbar::scrolled_at(top, f32::from(bar.thumb.size.height), content, viewport, viewport);
        let lead = f32::from(tokens::TEXT_MARGIN_Y);
        self.scroll = if scrolled < lead {
            Anchor {
                block: 0,
                offset: px(scrolled - lead),
            }
        } else {
            let heights: Vec<f32> = (0..self.blocks.as_slice().len())
                .map(|index| f32::from(self.block_height(index)))
                .collect();
            let (block, offset) = scrollbar::block_at(heights, scrolled - lead);
            Anchor {
                block,
                offset: px(offset),
            }
        };
        cx.notify();
    }

    /// The height of the whole chapter with its margins, with estimates for what is not shaped yet.
    fn content_height(&self) -> f32 {
        let lead = f32::from(tokens::TEXT_MARGIN_Y) * 2.;
        lead + (0..self.blocks.as_slice().len())
            .map(|index| f32::from(self.block_height(index)))
            .sum::<f32>()
    }

    /// The scrollbar for this frame, in the pane's right edge.
    fn update_scrollbar(&mut self) {
        let viewport = self.column.viewport;
        let content = self.content_height();
        let above: f32 = (0..self.scroll.block)
            .map(|index| f32::from(self.block_height(index)))
            .sum();
        let scrolled = above + f32::from(self.scroll.offset + tokens::TEXT_MARGIN_Y);
        let height = f32::from(viewport.size.height);
        let track = Bounds::new(
            point(viewport.right() - tokens::SCROLLBAR_WIDTH, viewport.top()),
            size(tokens::SCROLLBAR_WIDTH, viewport.size.height),
        );
        self.scrollbar = scrollbar::thumb(
            scrolled,
            content,
            height,
            height,
            f32::from(tokens::SCROLLBAR_THUMB_MIN),
        )
        .map(|(top, thumb_height)| Scrollbar {
            track,
            thumb: Bounds::new(
                point(track.left() + tokens::SCROLLBAR_INSET, track.top() + px(top)),
                size(tokens::SCROLLBAR_WIDTH - tokens::SCROLLBAR_INSET * 2., px(thumb_height)),
            ),
        });
    }

    fn scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.style.body_size * self.style.leading);
        self.scroll_by(px(0.) - delta.y);
        window.invalidate_character_coordinates();
        cx.notify();
    }

    // Layout and geometry

    /// The height of a block: real if it has been shaped for this width, otherwise estimated.
    fn block_height(&self, index: usize) -> Pixels {
        let slot = &self.slots[index];
        if let Some(layout) = &slot.layout {
            return layout.height;
        }
        if let Some(estimate) = slot.estimate.get() {
            return estimate;
        }
        let block = &self.blocks.as_slice()[index];
        let estimate = if self.is_hidden_block(index) {
            px(0.)
        } else {
            self.style.estimate_height(
                block.kind,
                &self.session.active_buffer().text()[block.range.clone()],
                self.column.text_width,
                self.source,
            )
        };
        // A hidden comment can show later without this block changing, so only a real estimate lasts.
        if !matches!(
            block.kind,
            BlockKind::Comment | BlockKind::Blank | BlockKind::FootnoteDefinition
        ) && self.rich_atom(index).is_none()
        {
            slot.estimate.set(Some(estimate));
        }
        estimate
    }

    /// Where markers reveal: at a caret, and at the start of a drag so nothing moves under the pointer. A selection shows
    /// its text clean, so nothing moves under it either.
    fn reveal_offset(&self) -> Option<usize> {
        (!self.source && (self.selection.is_empty() || self.drag.is_some())).then_some(self.reveal_at)
    }

    /// Whether a comment block takes no room here: it is closed, not opened by its marker, has no caret in it, and is not
    /// one of the marker lines between two versions, which stand in the flow while they are there.
    fn comment_is_hidden(&self, index: usize) -> bool {
        let block = &self.blocks.as_slice()[index];
        let text = &self.session.active_buffer().text()[block.range.clone()];
        block.kind == BlockKind::Comment
            && !self.slots[index].expanded
            && notes::is_closed(text)
            && !crate::persist::merge::is_marker(text)
            && self.reveal_offset().is_none_or(|at| self.blocks.index_at(at) != index)
    }

    /// Whether a block takes no room in the galley: a hidden comment, a note's text that its reference shows, the later
    /// blocks of a table shown whole, and the blank line after any of them.
    fn is_hidden_block(&self, index: usize) -> bool {
        if self.source {
            return false;
        }
        if self
            .rich_atom(index)
            .is_some_and(|atom| index > atom.first && self.atom_is_whole(atom))
        {
            return true;
        }
        match self.blocks.as_slice()[index].kind {
            BlockKind::Comment => self.comment_is_hidden(index),
            BlockKind::FootnoteDefinition => self.definition_is_hidden(index),
            BlockKind::Blank => {
                index > 0 && {
                    let before = &self.blocks.as_slice()[index - 1];
                    match before.kind {
                        BlockKind::Comment => self.comment_is_hidden(index - 1),
                        BlockKind::FootnoteDefinition => self.definition_is_hidden(index - 1),
                        _ => false,
                    }
                }
            }
            _ => false,
        }
    }

    /// `target`, moved past the hidden blocks it falls in, on the side the caret is heading.
    fn skip_hidden(&self, target: usize, forward: bool) -> usize {
        let index = self.blocks.index_at(target);
        if !self.takes_no_caret(index) || target == self.session.active_buffer().len() && self.ends_in_atom() {
            return target;
        }
        let count = self.blocks.as_slice().len();
        let mut first = index;
        while first > 0 && self.takes_no_caret(first - 1) {
            first -= 1;
        }
        let mut last = index;
        while last + 1 < count && self.takes_no_caret(last + 1) {
            last += 1;
        }
        let text = self.session.active_buffer().text();
        let before = self.blocks.as_slice()[first].range.start;
        // The blank line after a table is its separator, not a place to stand.
        let after = match self.blocks.as_slice().get(last + 1) {
            Some(next) if next.kind == BlockKind::Blank && last + 2 < count => next.range.end,
            _ => self.blocks.as_slice()[last].range.end,
        };
        if (forward && after < text.len()) || before == 0 {
            after
        } else {
            motion::left(text, before)
        }
    }

    /// The room a block keeps for the unfolded opening notes: the last hidden block before the chapter's first text. The
    /// source view shows the notes as text, so it keeps none.
    fn note_room_at(&self, index: usize) -> Pixels {
        if self.source || self.note_room == px(0.) {
            return px(0.);
        }
        let region = notes::leading_region(self.text(), &self.blocks);
        if region.is_some_and(|region| self.blocks.index_at(region.end.saturating_sub(1)) == index) {
            self.note_room
        } else {
            px(0.)
        }
    }

    /// How much of the pane stands empty right of the text and whatever is in its margin.
    pub fn room_beside_text(&self) -> Pixels {
        let column = &self.column;
        let end = column
            .note_left
            .map_or(column.text_left + column.text_width, |left| left + tokens::NOTE_WIDTH);
        column.viewport.right() - end
    }

    /// The width the text wraps at.
    pub(crate) fn text_width(&self) -> Pixels {
        self.column.text_width
    }

    /// Shapes a block for the current caret and width, or returns the shape it already has.
    fn layout_block(&mut self, index: usize, window: &Window) -> Rc<BlockLayout> {
        let block = self.blocks.as_slice()[index].clone();
        let text = self.session.active_buffer().text();
        let block_text = &text[block.range.clone()];
        let last = index + 1 == self.blocks.as_slice().len();
        let trailing_line = last && text.ends_with('\n');
        let syntax = self.slots[index]
            .syntax
            .get_or_insert_with(|| {
                Rc::new(Syntax {
                    spans: inline_spans(block.kind, block_text),
                    markers: line_markers(block.kind, block_text),
                })
            })
            .clone();
        let collapsed = self.is_hidden_block(index);
        let whole_atom = self
            .rich_atom(index)
            .filter(|atom| !collapsed && !self.source && atom.first == index && self.atom_is_whole(atom))
            .cloned();
        let hidden = if self.source {
            Vec::new()
        } else if collapsed || whole_atom.is_some() {
            let whole = 0..block_text.len();
            vec![whole]
        } else {
            let caret = self
                .reveal_offset()
                .filter(|at| self.blocks.index_at(*at) == index)
                .map(|at| at - block.range.start);
            hidden_ranges(
                block.kind,
                block_text,
                &syntax,
                caret,
                trailing_line,
                self.slots[index].expanded,
            )
        };
        let marked = self.marked.as_ref().and_then(|marked| {
            let start = marked.start.max(block.range.start);
            let end = marked.end.min(block.range.end);
            (start < end).then(|| start - block.range.start..end - block.range.start)
        });
        let width = self.column.text_width;
        let mut chips = if self.source || collapsed || whole_atom.is_some() || syntax.spans.is_empty() {
            Vec::new()
        } else {
            let caret = self
                .reveal_offset()
                .filter(|at| self.blocks.index_at(*at) == index)
                .map(|at| at - block.range.start);
            self.rich.chips(block_text, &syntax.spans, caret)
        };
        if !self.review.is_empty() && !collapsed && whole_atom.is_none() {
            chips.extend(self.review_chips(&block.range, block_text, &hidden, &chips));
            chips.sort_by_key(|chip| chip.range.start);
        }
        let below = self.review_room(index) + self.note_room_at(index);
        let dim = self.is_dimmed(index);
        let separator = !self.source && block.kind == BlockKind::Blank && motion::is_separator(text, block.range.start);
        let list = self.blocks.as_slice();
        let gap = if separator {
            let upper = index.checked_sub(1).map(|upper| list[upper].kind);
            let lower = list.get(index + 1).map(|lower| lower.kind);
            self.style.gap_between(upper, lower)
        } else {
            px(0.)
        };
        // A paragraph that continues another starts further in if the theme says so, as on the page.
        let continues = index.checked_sub(2).is_some_and(|before| {
            list[before].kind == BlockKind::Paragraph && list[index - 1].kind == BlockKind::Blank
        });
        let indent = if continues && !self.source {
            self.style.indent_of(Some(block.kind))
        } else {
            px(0.)
        };
        if let Some(layout) = &self.slots[index].layout
            && layout.width == width
            && layout.dim == dim
            && layout.gap == gap
            && layout.indent == indent
            && layout.hidden == hidden
            && layout.marked == marked
            && layout.collapsed == collapsed
            && layout.chips == chips
            && layout.below == below
            && rich::view::shapes::still_fits(layout.whole.as_deref(), whole_atom.as_ref())
        {
            return layout.clone();
        }
        let lines = display_lines(
            block.kind,
            block_text,
            &syntax,
            &hidden,
            marked.clone(),
            trailing_line,
            &chips,
        );
        let whole = whole_atom
            .as_ref()
            .map(|atom| Box::new(self.shape_atom(atom, dim, window)));
        let layout = Rc::new(shape_block(
            ShapeRequest {
                kind: block.kind,
                lines,
                separator,
                collapsed,
                source: self.source,
                hidden,
                marked,
                width,
                style: &self.style,
                chips,
                whole,
                dim,
                gap,
                indent,
                below,
            },
            window,
        ));
        self.slots[index].layout = Some(layout.clone());
        layout
    }

    /// Brings the open table or figure card into view, a frame after it was drawn where it now stands: whole if it fits,
    /// or from its top. Each scroll moves the card with its block, so it settles within a few frames.
    fn reveal_float(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.float, Float::Rich(_)) {
            self.reveal_float = 0;
            self.float_room = px(0.);
            return;
        }
        if self.reveal_float == 0 {
            return;
        }
        self.reveal_float -= 1;
        let Some(card) = self.float_bounds.get() else {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            return;
        };
        let view = self.column.viewport;
        let room = tokens::SPACE_4;
        let delta = if card.size.height > view.size.height - room * 2. || card.top() < view.top() + room {
            card.top() - view.top() - room
        } else {
            (card.bottom() - (view.bottom() - room)).max(px(0.))
        };
        if delta.abs() < px(1.) {
            self.reveal_float = 0;
            return;
        }
        if delta > px(0.) {
            self.float_room += delta;
        }
        self.scroll_by(delta);
        cx.on_next_frame(window, |_, _, cx| cx.notify());
    }

    fn scroll_by(&mut self, delta: Pixels) {
        self.scroll.offset += delta;
        self.normalize_scroll();
    }

    /// How far above the chapter's first line the view can go. Typewriter scrolling keeps the first line at its height.
    fn top_limit(&self) -> Pixels {
        if self.writing.typewriter {
            self.column.viewport.size.height * tokens::TYPEWRITER_AT
        } else {
            tokens::TEXT_MARGIN_Y
        }
    }

    fn normalize_scroll(&mut self) {
        while self.scroll.offset < px(0.) && self.scroll.block > 0 {
            self.scroll.block -= 1;
            self.scroll.offset += self.block_height(self.scroll.block);
        }
        if self.scroll.block == 0 {
            self.scroll.offset = self.scroll.offset.max(px(0.) - self.top_limit());
        }
        let last = self.blocks.as_slice().len() - 1;
        while self.scroll.block < last && self.scroll.offset >= self.block_height(self.scroll.block) {
            self.scroll.offset -= self.block_height(self.scroll.block);
            self.scroll.block += 1;
        }
    }

    fn set_viewport(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let notes = !self.advice.is_empty() || (!self.source && self.has_later_comment());
        let column = Column::place(bounds, self.measure(), notes);
        if column.text_width != self.column.text_width {
            for slot in &self.slots {
                slot.estimate.set(None);
            }
        }
        let room = self.room_beside_text();
        self.column = column;
        if self.room_beside_text() != room {
            cx.emit(RoomChanged);
        }
    }

    /// Finds and shapes the blocks on screen, moving the scroll position back if it ran past the end. With typewriter
    /// scrolling the last line may rise to the caret's height, and no further.
    fn lay_out_visible(&mut self, window: &Window) {
        let viewport = self.column.viewport;
        let block_count = self.blocks.as_slice().len();
        let limit = if self.writing.typewriter {
            viewport.top() + viewport.size.height * tokens::TYPEWRITER_AT + tokens::TEXT_MARGIN_Y
        } else {
            viewport.bottom()
        } - self.float_room;
        let mut retried = false;
        loop {
            self.visible.clear();
            let mut y = viewport.top() - self.scroll.offset;
            let mut index = self.scroll.block;
            while index < block_count && y < viewport.bottom() {
                let layout = self.layout_block(index, window);
                self.visible.push((index, y));
                y += layout.height;
                index += 1;
            }
            let end = y + tokens::TEXT_MARGIN_Y;
            let past_end = index == block_count && end < limit;
            let scrolled = self.scroll.block > 0 || self.scroll.offset > px(0.) - tokens::TEXT_MARGIN_Y;
            if !past_end || !scrolled || retried {
                return;
            }
            self.scroll_by(end - limit);
            retried = true;
        }
    }

    /// The blocks on screen, for assistive technology.
    fn a11y_lines(&self) -> Vec<ShownBlock> {
        self.visible
            .iter()
            .filter_map(|(index, _)| {
                let layout = self.slots[*index].layout.clone()?;
                Some((self.blocks.as_slice()[*index].range.start, layout))
            })
            .collect()
    }

    /// Moves the scroll position so the caret is on screen, if it is not.
    fn scroll_to_caret(&mut self, window: &Window) {
        let here = self.locate(self.selection.head, self.upstream, window);
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let caret_top = line.y + line.line_height * here.row as f32;
        let caret_bottom = caret_top + line.line_height;
        let height = self.column.viewport.size.height;

        if let Some(at) = self.caret_stays_at.take() {
            self.scroll = Anchor {
                block: here.block,
                offset: caret_top - at,
            };
            self.shape_back_from(here.block, at, window);
            self.normalize_scroll();
            return;
        }
        if self.writing.typewriter && self.caret_by_keys {
            let at = height * tokens::TYPEWRITER_AT;
            self.scroll = Anchor {
                block: here.block,
                offset: caret_top + line.line_height * 0.5 - at,
            };
            self.shape_back_from(here.block, at, window);
            self.normalize_scroll();
            return;
        }
        let is_above =
            here.block < self.scroll.block || (here.block == self.scroll.block && caret_top < self.scroll.offset);
        if is_above {
            self.scroll = Anchor {
                block: here.block,
                offset: caret_top - CARET_MARGIN,
            };
            self.shape_back_from(here.block, CARET_MARGIN, window);
            self.normalize_scroll();
            return;
        }
        let mut distance = caret_bottom - self.scroll.offset;
        let mut index = self.scroll.block;
        while index < here.block && distance <= height {
            distance += self.layout_block(index, window).height;
            index += 1;
        }
        if index < here.block || distance > height {
            self.scroll = Anchor {
                block: here.block,
                offset: caret_bottom + CARET_MARGIN - height,
            };
            self.shape_back_from(here.block, height, window);
            self.normalize_scroll();
        }
    }

    /// Shapes the blocks above `index` that cover `distance`, so that scrolling back through them lands exactly.
    fn shape_back_from(&mut self, index: usize, distance: Pixels, window: &Window) {
        let mut covered = px(0.);
        let mut block = index;
        while block > 0 && covered < distance {
            block -= 1;
            covered += self.layout_block(block, window).height;
        }
    }

    fn locate(&mut self, offset: usize, upstream: bool, window: &Window) -> Located {
        let block = self.blocks.index_at(offset);
        let layout = self.layout_block(block, window);
        let relative = offset - self.blocks.as_slice()[block].range.start;
        let line = layout
            .lines
            .iter()
            .rposition(|line| line.display.source.start <= relative)
            .unwrap_or(0);
        let shaped = &layout.lines[line];
        let display = shaped.display.to_display(relative);
        Located {
            block,
            line,
            row: shaped.row_of(display, upstream),
            display,
        }
    }

    fn block_at_y(&self, y: Pixels) -> Option<usize> {
        let at = self.visible.partition_point(|(_, top)| *top <= y);
        at.checked_sub(1).map(|at| self.visible[at].0)
    }

    /// The source offset nearest a window position, and whether it shows at the end of an upper row.
    fn offset_at(&mut self, position: Point<Pixels>, window: &Window) -> (usize, bool) {
        let at = self
            .visible
            .partition_point(|(_, top)| *top <= position.y)
            .saturating_sub(1);
        let Some(&(index, top)) = self.visible.get(at) else {
            return (0, false);
        };
        let layout = self.layout_block(index, window);
        let range = self.blocks.as_slice()[index].range.clone();
        let relative_y = (position.y - top).max(px(0.));
        let line_index = layout
            .lines
            .iter()
            .rposition(|line| line.y <= relative_y && line.height > px(0.))
            .unwrap_or(0);
        let line = &layout.lines[line_index];
        if line.skipped && line.height > px(0.) {
            return if relative_y < line.y + line.height * 0.5 {
                (
                    motion::prev_grapheme(self.session.active_buffer().text(), range.start),
                    false,
                )
            } else {
                (range.end, false)
            };
        }
        let row = (((relative_y - line.y) / line.line_height) as usize).min(line.rows.len() - 1);
        let x = position.x - self.column.text_left - line.x;
        let display = line.display_at_x(row, x.max(px(0.)));
        let upstream = display == line.rows[row].end && !line.is_last_row(row);
        (range.start + line.display.to_source(display), upstream)
    }

    fn caret_bounds(&mut self, window: &Window) -> Option<Bounds<Pixels>> {
        self.offset_bounds(self.selection.head, self.upstream, window)
    }

    /// The window rectangle of a caret at `offset`, if its block is on screen.
    fn offset_bounds(&mut self, offset: usize, upstream: bool, window: &Window) -> Option<Bounds<Pixels>> {
        let here = self.locate(offset, upstream, window);
        let (_, top) = *self.visible.iter().find(|(index, _)| *index == here.block)?;
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let origin = point(
            self.column.text_left + line.x + line.x_in_row(here.display, here.row),
            top + line.y + line.line_height * here.row as f32,
        );
        Some(Bounds::new(origin, size(tokens::CARET_WIDTH, line.line_height)))
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// One listener per caret motion, as a move and as a selection.
macro_rules! motions {
    ($element:expr, $cx:expr, $($action:ident => $motion:ident, $select:ident),* $(,)?) => {
        $element
            $(
                .on_action($cx.listener(|this, _: &$action, window, cx| {
                    this.move_caret(Motion::$motion, false, window, cx)
                }))
                .on_action($cx.listener(|this, _: &$select, window, cx| {
                    this.move_caret(Motion::$motion, true, window, cx)
                }))
            )*
    };
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let mut context = KeyContext::default();
        // The link field floats inside this pane, and its keys are its own.
        if !self.float_takes_keys() {
            context.add(CONTEXT);
        }
        if self.menu_is_open() {
            context.add(MENU_CONTEXT);
        }
        // Only why the chapter cannot be read. The text is nearly all there is.
        let header = (!self.status.is_empty()).then(|| {
            div()
                .flex_none()
                .h(tokens::SECTION_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_faint)
                .child(self.status.clone())
        });
        let chapter = self.session.active_chapter().heading().unwrap_or_default().to_owned();
        let shown = self.a11y_lines();
        let selection = self.selection;
        let pane = div()
            .id("galley")
            .role(gpui::Role::MultilineTextInput)
            .aria_label(chapter)
            .a11y_synthetic_children(move |builder| a11y_text(&shown, selection, builder))
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .key_context(context)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::new_block))
            .on_action(cx.listener(Self::new_line))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::menu_up))
            .on_action(cx.listener(Self::menu_down))
            .on_action(cx.listener(Self::menu_accept))
            .on_action(cx.listener(|this, _: &ToggleBold, window, cx| this.toggle_mark(Mark::Bold, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleItalic, window, cx| this.toggle_mark(Mark::Italic, window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleUnderline, window, cx| this.toggle_mark(Mark::Underline, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleStrike, window, cx| this.toggle_mark(Mark::Strike, window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleSuperscript, window, cx| this.toggle_mark(Mark::Superscript, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleSubscript, window, cx| this.toggle_mark(Mark::Subscript, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleQuote, window, cx| this.toggle_block(BlockType::Quote, window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleBullets, window, cx| this.toggle_block(BlockType::Bullets, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleNumbers, window, cx| this.toggle_block(BlockType::Numbers, window, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenLink, window, cx| this.open_link(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleSmallCaps, window, cx| this.toggle_mark(Mark::SmallCaps, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleSource, window, cx| this.toggle_source(window, cx)))
            .on_action(cx.listener(|this, _: &ShowSpelling, window, cx| this.show_spelling(window, cx)))
            .on_action(cx.listener(|this, _: &InsertFootnote, window, cx| this.insert_footnote(None, window, cx)))
            .on_action(cx.listener(|this, _: &InsertCitation, window, cx| this.open_citation_picker(None, window, cx)))
            .on_action(
                cx.listener(|this, _: &InsertReference, window, cx| this.open_reference_picker(None, window, cx)),
            )
            .on_action(cx.listener(|this, _: &InsertImage, window, cx| this.choose_image(None, window, cx)))
            .on_action(cx.listener(|this, _: &InsertTable, window, cx| this.insert_table(window, cx)))
            .on_action(cx.listener(|this, _: &EditRich, window, cx| this.edit_rich_at_caret(window, cx)))
            .on_drop(cx.listener(Self::drop_paths))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_scroll(cx.listener(Self::scroll));
        motions!(
            pane,
            cx,
            Left => Left, SelectLeft,
            Right => Right, SelectRight,
            Up => Up, SelectUp,
            Down => Down, SelectDown,
            WordLeft => WordLeft, SelectWordLeft,
            WordRight => WordRight, SelectWordRight,
            LineStart => LineStart, SelectLineStart,
            LineEnd => LineEnd, SelectLineEnd,
            DocStart => DocStart, SelectDocStart,
            DocEnd => DocEnd, SelectDocEnd,
            PageUp => PageUp, SelectPageUp,
            PageDown => PageDown, SelectPageDown,
        )
        .children(header)
        .child(Galley::new(cx.entity()))
    }
}

/// Keeps the blocks on screen pointing at the same blocks after an edit replaced some. Until the next layout they are all
/// there is of where things stand, and the next render reads them first, so none may point past the end. A replaced block
/// goes: its layout is gone too.
fn follow_blocks(visible: &mut Vec<(usize, Pixels)>, change: BlockChange) {
    let replaced = change.start..change.start + change.removed;
    visible.retain(|(index, _)| !replaced.contains(index));
    for (index, _) in visible.iter_mut().filter(|(index, _)| *index >= replaced.end) {
        *index = *index - change.removed + change.inserted;
    }
}

#[cfg(test)]
mod tests;
