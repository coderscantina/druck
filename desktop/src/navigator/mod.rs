//! Moving through the book in Write: the path bar at the title bar's left names where the caret is, a click on a
//! segment or `Cmd+Shift+J` opens columns over the text at that level, and `Cmd+Escape` opens the overview, laid out
//! like the book's contents page, which is the one place where the book's order changes.
//!
//! [`book`] reads the structure from the session's text and holds no gpui types. [`Navigator`] keeps where the caret
//! is and what is open, and is the layer over the window. [`path::PathBar`] draws the path in the title bar.
//! [`columns`] and [`overview`] draw the two views and take their keys.

pub mod book;
mod columns;
mod overview;
pub mod path;
mod strings;

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    App, Context, FocusHandle, Focusable, KeyBinding, MouseButton, SharedString, Subscription, Task, WeakEntity,
    Window, actions, div,
};

use self::book::{Book, Place};
use self::columns::Columns;
use self::overview::Overview;
use crate::editor::{CaretMoved, Redo, SessionChanged, TextChanged, Undo};
use crate::session::ChapterId;
use crate::sidebar::SidebarEvent;
use crate::tokens::{self, Tokens};
use crate::workspace::{Current, Mode, ModeChanged, Parts, Slots, Workspace};

actions!(navigator, [ShowColumns, ShowOverview, NextEntry, PreviousEntry]);

/// Longer than a click takes from press to release.
const CLICK_TIME: Duration = Duration::from_millis(600);
/// How long after the last key the path comes back from faint, and the book is read again.
const TYPING_PAUSE: Duration = Duration::from_millis(1200);

/// Binds the keys. Once, when the app starts.
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-j", ShowColumns, None),
        KeyBinding::new("cmd-escape", ShowOverview, None),
        KeyBinding::new("tab", NextEntry, Some("Navigator")),
        KeyBinding::new("shift-tab", PreviousEntry, Some("Navigator")),
        // Where the book's order changes, undo and redo take back its moves, never text.
        KeyBinding::new("cmd-z", Undo, Some("Navigator")),
        KeyBinding::new("cmd-shift-z", Redo, Some("Navigator")),
        KeyBinding::new("cmd-z", Undo, Some("Sidebar")),
        KeyBinding::new("cmd-shift-z", Redo, Some("Sidebar")),
        KeyBinding::new("cmd-z", Undo, Some("Bookshelf")),
        KeyBinding::new("cmd-shift-z", Redo, Some("Bookshelf")),
    ]);
    cx.on_action(|_: &ShowColumns, cx| {
        Current::<Navigator>::defer(cx, |navigator, window, cx| {
            navigator.toggle_columns(Level::Chapters, window, cx)
        })
    });
    cx.on_action(|_: &ShowOverview, cx| {
        Current::<Navigator>::defer(cx, |navigator, window, cx| navigator.toggle_overview(window, cx))
    });
}

/// Mounts the path bar in the title bar and the columns and overview as a layer over the window.
pub fn mount(parts: &Parts, slots: &mut Slots, window: &mut Window, cx: &mut Context<Workspace>) {
    let workspace = cx.entity();
    let navigator = cx.new(|cx| Navigator::new(parts.clone(), &workspace, window, cx));
    Current::set(&navigator, window, cx);
    let path = cx.new(|cx| path::PathBar::new(navigator.clone(), cx));
    slots.title.push(path.into());
    slots.overlays.push(navigator.into());
}

/// `DRUCK_NAVIGATOR=columns`, `parts`, `headings`, or `overview` opens it, for screenshots. `states` opens the overview
/// with the menu of writing states on the focused entry.
#[cfg(any(debug_assertions, feature = "measure"))]
pub fn apply_gate(window: &mut Window, cx: &mut App) {
    let Some(name) = crate::gate::var("DRUCK_NAVIGATOR") else {
        return;
    };
    let Some(navigator) = Current::<Navigator>::view(cx) else {
        return;
    };
    navigator.update(cx, |navigator, cx| match name.as_str() {
        "overview" => navigator.toggle_overview(window, cx),
        "states" => {
            navigator.toggle_overview(window, cx);
            // The book is read after the window mounts, so the menu waits for its entries.
            cx.spawn(async move |navigator, cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                navigator
                    .update(cx, |navigator, cx| navigator.open_focused_status_menu(cx))
                    .ok();
            })
            .detach();
        }
        "parts" => navigator.toggle_columns(Level::Parts, window, cx),
        "headings" => navigator.toggle_columns(Level::Headings, window, cx),
        _ => navigator.toggle_columns(Level::Chapters, window, cx),
    });
}

/// A column, and the path segment that opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Parts,
    Chapters,
    Headings,
}

/// What is open over the text.
enum Shown {
    Nothing,
    Columns(Columns),
    Overview(Overview),
}

/// One segment of the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub label: SharedString,
    pub level: Level,
}

pub struct Navigator {
    parts: Parts,
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    book: Rc<Book>,
    /// The session's revision and the open chapter's that `book` was read at.
    read_at: Option<(u64, u64)>,
    place: Option<Place>,
    /// Where the caret is in the open chapter.
    caret: usize,
    typing: bool,
    typing_pause: Task<()>,
    shown: Shown,
    /// What had the keys when a view opened, which get them back on Escape.
    before: Option<FocusHandle>,
    /// The level whose columns a press outside them just closed, so the click on its own segment that follows does
    /// not open them again.
    pressed_closed: Option<(Level, Instant)>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Navigator {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Navigator {
    fn new(parts: Parts, workspace: &gpui::Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&parts.editor, |navigator, _, _: &TextChanged, cx| navigator.typed(cx)),
            cx.subscribe(&parts.editor, |navigator, _, CaretMoved(offset), cx| {
                navigator.caret = *offset;
                if !navigator.typing {
                    navigator.refresh(cx);
                }
            }),
            cx.subscribe(&parts.editor, |navigator, _, _: &SessionChanged, cx| {
                navigator.refresh(cx)
            }),
            cx.subscribe_in(workspace, window, |navigator, _, ModeChanged(mode), window, cx| {
                if *mode == Mode::Publish {
                    navigator.close(false, window, cx);
                }
            }),
        ];
        let caret = parts.editor.read(cx).selection_range().start;
        let mut navigator = Self {
            parts,
            workspace: workspace.downgrade(),
            focus: cx.focus_handle(),
            book: Rc::default(),
            read_at: None,
            place: None,
            caret,
            typing: false,
            typing_pause: Task::ready(()),
            shown: Shown::Nothing,
            before: None,
            pressed_closed: None,
            _subscriptions: subscriptions,
        };
        navigator.refresh(cx);
        navigator
    }

    /// Reads the book again if its text changed since, and finds the caret in it.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let active = session.active();
        let key = (session.revision(), session.buffer(active).revision());
        if self.read_at != Some(key) {
            self.book = Rc::new(Book::read(session));
            self.read_at = Some(key);
            self.book_changed();
        }
        self.place = self.book.place(active, self.caret);
        cx.notify();
    }

    /// A key was typed: the path goes faint, and comes back with the book read again once typing stops.
    fn typed(&mut self, cx: &mut Context<Self>) {
        if !self.typing {
            self.typing = true;
            cx.notify();
        }
        self.typing_pause = cx.spawn(async move |navigator, cx| {
            cx.background_executor().timer(TYPING_PAUSE).await;
            navigator
                .update(cx, |navigator, cx| {
                    navigator.typing = false;
                    navigator.refresh(cx);
                })
                .ok();
        });
    }

    /// Keeps what an open view has chosen on the same files after the book was read again.
    fn book_changed(&mut self) {
        let book = self.book.clone();
        match &mut self.shown {
            Shown::Columns(columns) => columns.keep(&book),
            Shown::Overview(overview) => overview.keep(&book),
            Shown::Nothing => {}
        }
    }

    /// The path's segments from the outside in, with the overview's name alone while it is open.
    pub fn segments(&self) -> Vec<Segment> {
        if matches!(self.shown, Shown::Overview(_)) {
            return vec![Segment {
                label: crate::t!(strings::Key::Overview).into(),
                level: Level::Parts,
            }];
        }
        let Some(place) = &self.place else {
            return Vec::new();
        };
        let mut segments = Vec::new();
        let opener = self
            .book
            .groups
            .get(place.group)
            .and_then(|group| group.opener.as_ref())
            .map(|opener| opener.row.id);
        if let Some(label) = group_label(&self.book, place.group, false) {
            segments.push(Segment {
                label: label.into(),
                level: Level::Parts,
            });
        }
        let entry = self.book.entry(place.chapter);
        if opener != Some(place.chapter)
            && let Some(entry) = entry
        {
            segments.push(Segment {
                label: entry.numbered_title().into(),
                level: Level::Chapters,
            });
        }
        if let Some(heading) = (entry.zip(place.section)).and_then(|(entry, at)| entry.headings.get(at)) {
            segments.push(Segment {
                label: heading.title.clone().into(),
                level: Level::Headings,
            });
        }
        segments
    }

    /// The level whose column has the keys, while the columns are open.
    pub fn open_level(&self) -> Option<Level> {
        match &self.shown {
            Shown::Columns(columns) => Some(columns.level),
            _ => None,
        }
    }

    pub fn overview_open(&self) -> bool {
        matches!(self.shown, Shown::Overview(_))
    }

    pub fn is_typing(&self) -> bool {
        self.typing
    }

    /// Opens the columns with the keys at `level`, moves them there if open at another, and closes them if open there.
    pub fn toggle_columns(&mut self, level: Level, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_level() == Some(level) {
            self.close(true, window, cx);
            return;
        }
        self.to_write(window, cx);
        self.refresh(cx);
        match &mut self.shown {
            Shown::Columns(columns) => columns.level = level,
            _ => {
                let columns = Columns::new(&self.book, self.place.as_ref(), level);
                self.show(Shown::Columns(columns), window, cx);
            }
        }
        cx.notify();
    }

    /// A click on a path segment: toggles its columns, unless the press of this click just closed them.
    pub fn segment_clicked(&mut self, level: Level, window: &mut Window, cx: &mut Context<Self>) {
        let just_closed = self
            .pressed_closed
            .take()
            .is_some_and(|(closed, at)| closed == level && at.elapsed() < CLICK_TIME);
        if !just_closed {
            self.toggle_columns(level, window, cx);
        }
    }

    /// A press outside the columns closes them.
    fn pressed_outside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pressed_closed = self.open_level().map(|level| (level, Instant::now()));
        self.close(true, window, cx);
    }

    /// Opens the overview, or closes it if it is open.
    pub fn toggle_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overview_open() {
            self.close(true, window, cx);
            return;
        }
        self.to_write(window, cx);
        self.refresh(cx);
        let overview = Overview::new(&self.book, self.place.as_ref());
        self.show(Shown::Overview(overview), window, cx);
    }

    fn show(&mut self, shown: Shown, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.shown, Shown::Nothing) {
            self.before = window.focused(cx).filter(|focused| *focused != self.focus);
        }
        self.shown = shown;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Both views are in Write.
    fn to_write(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade()
            && workspace.read(cx).mode() != Mode::Write
        {
            workspace.update(cx, |workspace, cx| workspace.show(Mode::Write, window, cx));
        }
    }

    /// Closes what is open. With `give_back`, the keys go back to what had them, otherwise they stay where they went.
    fn close(&mut self, give_back: bool, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.shown, Shown::Nothing) {
            return;
        }
        self.shown = Shown::Nothing;
        let before = self.before.take();
        if give_back {
            match before {
                Some(before) => window.focus(&before, cx),
                None => self.focus_text(window, cx),
            }
        }
        cx.notify();
    }

    fn focus_text(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| workspace.focus_main(window, cx));
        }
    }

    /// Opens a file, at a heading's place if given, and gives the keys to the text.
    fn open(&mut self, id: ChapterId, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.close(false, window, cx);
        self.parts.editor.update(cx, |editor, cx| match at {
            Some(offset) => editor.show_heading(id, offset, window, cx),
            None => editor.show_chapter(id, window, cx),
        });
        self.focus_text(window, cx);
    }

    /// Moves a file of the book so it stands before the one now at `place`, as the Binder does.
    fn move_to(&mut self, id: ChapterId, place: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_event(SidebarEvent::Move(id, place), window, cx);
    }

    /// Changes the book as the Binder would.
    fn workspace_event(&mut self, event: SidebarEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| workspace.sidebar_event(&event, window, cx));
        }
    }

    /// The words of a file, once the shell has counted them.
    fn words(id: ChapterId, cx: &App) -> Option<usize> {
        crate::shell::chapter_facts(id, cx)?
            .statistics
            .map(|statistics| statistics.words)
    }

    /// The open placeholders of a file, once the shell has read them.
    fn open_placeholders(id: ChapterId, cx: &App) -> usize {
        crate::shell::chapter_facts(id, cx).map_or(0, |facts| facts.todos.len())
    }
}

/// A group's or shelf's name at `index`, counting shelves after the groups. A group of chapters outside any part has
/// none in the path, since it names nothing; `listed` gives it one for a list.
fn group_label(book: &Book, index: usize, listed: bool) -> Option<String> {
    use self::book::{Kind, Shelf};
    use self::strings::Key;
    if let Some(group) = book.groups.get(index) {
        let has_parts = book.groups.iter().any(|group| group.kind == Kind::Part);
        return match group.kind {
            Kind::FrontMatter => Some(crate::t!(Key::FrontMatter).to_owned()),
            Kind::BackMatter => Some(crate::t!(Key::BackMatter).to_owned()),
            Kind::Part => group.title().map(str::to_owned),
            Kind::Chapters if !listed => None,
            Kind::Chapters if has_parts => Some(crate::t!(Key::BeforeParts).to_owned()),
            Kind::Chapters => Some(crate::t!(Key::Chapters).to_owned()),
        };
    }
    let (shelf, _) = book.shelves.get(index - book.groups.len())?;
    Some(
        crate::t!(match shelf {
            Shelf::NotInBook => Key::NotInBook,
            Shelf::Research => Key::Research,
            Shelf::Notes => Key::Notes,
            Shelf::Probes => Key::Probes,
        })
        .to_owned(),
    )
}

/// A number with thousands separated by a thin space, as the word count shows it.
fn grouped(number: usize) -> String {
    crate::shell::footer::grouped(number)
}

/// A quiet key name and what it does, for the foot of the columns and the overview.
fn hint(tokens: &Tokens, key: &'static str, does: &'static str) -> gpui::Div {
    div()
        .flex_none()
        .flex()
        .gap(tokens::SPACE_1)
        .child(div().text_color(tokens.text_quiet).child(key))
        .child(div().text_color(tokens.text_faint).child(does))
}

impl Render for Navigator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let view = match &self.shown {
            Shown::Nothing => return div().into_any_element(),
            Shown::Columns(_) => self.render_columns(&tokens, window, cx),
            Shown::Overview(_) => self.render_overview(&tokens, window, cx),
        };
        // Above the text's floats, such as the selection's tools, as a card that holds the keys is.
        gpui::deferred(
            view.key_context("Navigator")
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|navigator, event, window, cx| navigator.on_key(event, window, cx)))
                .on_action(cx.listener(|navigator, _: &NextEntry, _, cx| navigator.step_entry(true, cx)))
                .on_action(cx.listener(|navigator, _: &PreviousEntry, _, cx| navigator.step_entry(false, cx)))
                .on_action(cx.listener(|navigator, _: &Undo, window, cx| {
                    navigator.workspace_event(SidebarEvent::UndoMove, window, cx)
                }))
                .on_action(cx.listener(|navigator, _: &Redo, window, cx| {
                    navigator.workspace_event(SidebarEvent::RedoMove, window, cx)
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
        )
        .with_priority(tokens::LAYER_QUESTION)
        .into_any_element()
    }
}

impl Navigator {
    fn on_key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let handled = match &self.shown {
            Shown::Columns(_) => self.columns_key(event, window, cx),
            Shown::Overview(_) => self.overview_key(event, window, cx),
            Shown::Nothing => false,
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// Tab and Shift+Tab walk the overview's entries, or the columns' rows.
    fn step_entry(&mut self, forward: bool, cx: &mut Context<Self>) {
        let book = self.book.clone();
        match &mut self.shown {
            Shown::Columns(columns) => columns.step(&book, forward),
            Shown::Overview(overview) => overview.step(forward),
            Shown::Nothing => return,
        }
        cx.notify();
    }
}
