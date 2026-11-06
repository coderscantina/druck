//! Supporting material in Write: research, notes, and style probes, which live in `research/` and `notes/` and are never
//! built. The rail comes to Write with a tab for each, hidden at rest but for a faint tick at the right edge; its panel
//! shows one thing at a time with a trail, see [`panel`]. Material can be pinned to a paragraph, see [`pins`], and the
//! library card searches all of it, see [`library`]. The same verbs act everywhere: Return opens, Shift+Return inserts,
//! and Command+Return pins to the paragraph at the caret.
//!
//! [`Material`] holds what is known of the material and does what the verbs ask. Without a document folder, as for a
//! single file, there is none.

mod library;
pub mod model;
mod panel;
pub mod pins;
mod probe;
mod rail;
mod strings;
pub mod trail;
pub mod verbs;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{App, Context, Entity, EventEmitter, Focusable, KeyBinding, Pixels, Subscription, Task, Window, actions};

use crate::editor::{PinCard, PinClicked, PinGroup, PinVerb, SessionChanged, TextChanged};
use crate::inspector::{Tab, TabShown};
use crate::source::EditKind;
use crate::tokens;
use crate::workspace::{Current, Mode, ModeChanged, Parts, Slots, Workspace};

use self::model::{Form, Item, Kind, Uses};
use self::panel::MaterialPanel;
use self::pins::{Pin, Pins};
pub use self::strings::Key;
use self::verbs::Insert;

actions!(material, [ToggleMaterial, JumpToMaterial, ShowLibrary]);

/// How long after typing stops the uses and the pins' quotes are read again.
const PAUSE: Duration = Duration::from_millis(600);
/// How long after an insert of its own the material takes an edit of the text as that insert.
const QUIET: Duration = Duration::from_millis(250);
/// How much of an item's text a pin shows when no passage was chosen.
const EXCERPT: usize = 280;

/// The keys that reach the material from anywhere. `Cmd+[`, `Cmd+]`, and the verbs are the panel's, see [`panel`].
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-alt-m", ToggleMaterial, None),
        KeyBinding::new("cmd-l", JumpToMaterial, None),
        KeyBinding::new("cmd-shift-o", ShowLibrary, None),
    ]);
    crate::editor::bind_pin_keys(cx);
    panel::bind_keys(cx);
    library::bind_keys(cx);
    cx.on_action(|_: &ToggleMaterial, cx| {
        Current::<Material>::defer(cx, |material, window, cx| material.toggle(window, cx));
    });
    cx.on_action(|_: &JumpToMaterial, cx| {
        Current::<Material>::defer(cx, |material, window, cx| material.jump(window, cx));
    });
    cx.on_action(|_: &ShowLibrary, cx| {
        Current::<Material>::defer(cx, |material, window, cx| material.show_library(window, cx));
    });
}

/// Whether the rail stands in Write, which the panel's width and the workspace's room read while they render.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Rail {
    pub open: bool,
    pub write: bool,
}

impl gpui::Global for Rail {}

impl Rail {
    pub fn get(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }
}

/// How wide the panel stands in a window `window_width` wide, or `None` when it gives way: the text keeps its measure
/// first, narrows to its least width, then the panel narrows to its own least width, and below that it does not show.
pub fn panel_width(window_width: Pixels) -> Option<Pixels> {
    let text = tokens::GUTTER_WIDTH + tokens::TEXT_MIN_WIDTH + tokens::TEXT_MARGIN_RIGHT;
    let room = window_width - text - tokens::RAIL_WIDTH;
    (room >= tokens::MATERIAL_PANEL_MIN_WIDTH).then(|| room.min(tokens::MATERIAL_PANEL_WIDTH))
}

/// The material changed: its files, the chapter's uses, or the pins.
pub struct MaterialChanged;

/// What is known of the material, and the verbs.
pub struct Material {
    parts: Parts,
    workspace: gpui::WeakEntity<Workspace>,
    base: Option<PathBuf>,
    items: Rc<Vec<Item>>,
    uses: Rc<Vec<Uses>>,
    pins: Option<Pins>,
    /// The quotes of the pin groups the text shows, in their order, to follow its edits.
    shown_quotes: Vec<String>,
    /// What the material was read for, so a session change that changed none of it reads nothing.
    signature: u64,
    mode: Mode,
    panel: Option<Entity<MaterialPanel>>,
    library: Option<Entity<library::LibraryCard>>,
    pause: Task<()>,
    /// Until when an edit of the text is the material's own insert, which does not close the rail as typing does.
    quiet_until: Option<std::time::Instant>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MaterialChanged> for Material {}

/// Mounts the rail's tabs in Write, the tick at the right edge, and the library card.
pub fn mount(parts: &Parts, slots: &mut Slots, window: &mut Window, cx: &mut Context<Workspace>) {
    let workspace = cx.entity();
    let material = cx.new(|cx| Material::new(parts.clone(), workspace.downgrade(), cx));
    Current::set(&material, window, cx);
    let panel = cx.new(|cx| MaterialPanel::new(material.clone(), parts.editor.clone(), window, cx));
    let library = cx.new(|cx| library::LibraryCard::new(material.clone(), parts.editor.clone(), window, cx));
    material.update(cx, |material, _| {
        material.panel = Some(panel.clone());
        material.library = Some(library.clone());
    });
    parts.inspector.update(cx, |inspector, _| {
        for tab in [Tab::Research, Tab::Notes, Tab::Probes] {
            inspector.mount(tab, panel.clone().into());
        }
    });
    let rail = cx.new(|_| rail::WriteRail::new(parts.inspector.clone()));
    slots.right.push(crate::workspace::SidePanel {
        view: rail.into(),
        width: Rc::new(|window_width, cx| {
            let rail = Rail::get(cx);
            match (rail.open && rail.write, panel_width(window_width)) {
                (true, Some(width)) => width + tokens::RAIL_WIDTH,
                _ => Pixels::ZERO,
            }
        }),
    });
    slots
        .overlays
        .push(cx.new(|_| rail::RailEdge::new(material.clone())).into());
    slots.overlays.push(library.into());
    cx.subscribe_in(&workspace, window, move |_, _, ModeChanged(mode), window, cx| {
        let mode = *mode;
        material.update(cx, |material, cx| material.mode_changed(mode, window, cx));
    })
    .detach();
}

impl Material {
    fn new(parts: Parts, workspace: gpui::WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let editor = parts.editor.clone();
        let subscriptions = vec![
            cx.subscribe(&editor, |material, _, _: &SessionChanged, cx| material.reread(cx)),
            cx.subscribe(&editor, |material, _, _: &TextChanged, cx| material.text_changed(cx)),
            cx.subscribe(&editor, |_, _, PinClicked(index, verb), cx| {
                let (index, verb) = (*index, *verb);
                Current::<Material>::defer(cx, move |material, window, cx| {
                    material.pin_clicked(index, verb, window, cx)
                });
            }),
            cx.subscribe(&parts.inspector, |material, _, TabShown(tab), cx| {
                material.tab_shown(*tab, cx)
            }),
        ];
        let mut material = Self {
            parts,
            workspace,
            base: None,
            items: Rc::default(),
            uses: Rc::default(),
            pins: None,
            shown_quotes: Vec::new(),
            signature: 0,
            mode: Mode::Write,
            panel: None,
            library: None,
            pause: Task::ready(()),
            quiet_until: None,
            _subscriptions: subscriptions,
        };
        material.reread(cx);
        material
    }

    pub fn items(&self) -> &Rc<Vec<Item>> {
        &self.items
    }

    pub fn uses(&self) -> &Rc<Vec<Uses>> {
        &self.uses
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn uses_of(&self, id: &str) -> Option<&Uses> {
        self.uses.iter().find(|uses| uses.item == id)
    }

    /// The book's folder, for reading an item's file.
    pub fn base(&self) -> Option<&PathBuf> {
        self.base.as_ref()
    }

    /// How many pins `id` has in the book.
    pub fn pinned_count(&self, id: &str) -> usize {
        self.pins
            .as_ref()
            .map_or(0, |pins| pins.all().iter().filter(|pin| pin.item == id).count())
    }

    /// Whether `id` is pinned to the open chapter.
    pub fn pinned_here(&self, id: &str, cx: &App) -> bool {
        let chapter = self.chapter_path(cx);
        self.pins.as_ref().is_some_and(|pins| {
            pins.all()
                .iter()
                .any(|pin| pin.item == id && Some(&pin.chapter) == chapter.as_ref())
        })
    }

    /// The open chapter's file from the book's folder.
    fn chapter_path(&self, cx: &App) -> Option<String> {
        let session = self.parts.editor.read(cx).session();
        let base = self.base.as_ref()?;
        let path = session.chapter(session.active())?.path();
        let relative = path.strip_prefix(base).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }

    /// Reads the material again when the session's files or the bibliography may have changed, and the chapter's uses.
    fn reread(&mut self, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let base = match session.kind() {
            crate::session::Kind::Project { dir } => Some(dir.clone()),
            crate::session::Kind::Single { .. } => None,
        };
        let bibliography = session.bibliography_text();
        let mut hasher = DefaultHasher::new();
        base.hash(&mut hasher);
        bibliography.hash(&mut hasher);
        let contents = session.contents();
        (contents.notes.len(), contents.research.len()).hash(&mut hasher);
        let signature = hasher.finish();
        if signature != self.signature || base != self.base {
            self.signature = signature;
            self.rescan(base, bibliography, cx);
        }
        self.refresh_chapter(cx);
    }

    /// Reads every file of the material, for a panel or the library opening, so a file added outside shows.
    pub fn rescan_now(&mut self, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let bibliography = session.bibliography_text();
        self.rescan(self.base.clone(), bibliography, cx);
        self.refresh_chapter(cx);
    }

    fn rescan(&mut self, base: Option<PathBuf>, bibliography: Option<String>, cx: &mut Context<Self>) {
        let works = bibliography.map_or(Vec::new(), |text| druck::editor::rich::works(&text));
        self.items = Rc::new(base.as_deref().map_or(Vec::new(), |base| model::scan(base, &works)));
        if self.base != base {
            self.pins = base.as_deref().map(Pins::open);
            self.base = base;
        }
        cx.emit(MaterialChanged);
        cx.notify();
    }

    /// Reads the open chapter's uses and shows its pins.
    fn refresh_chapter(&mut self, cx: &mut Context<Self>) {
        let text = self.parts.editor.read(cx).text().to_owned();
        self.uses = Rc::new(model::uses(&self.items, &text));
        self.show_pins(cx);
        cx.emit(MaterialChanged);
        cx.notify();
    }

    /// After a pause in typing: the pins' quotes follow their paragraphs, and the uses are read again.
    fn text_changed(&mut self, cx: &mut Context<Self>) {
        self.leave_on_typing(cx);
        self.pause = cx.spawn(async move |material, cx| {
            cx.background_executor().timer(PAUSE).await;
            material
                .update(cx, |material, cx| {
                    material.requote(cx);
                    material.refresh_chapter(cx);
                })
                .ok();
        });
    }

    /// The rail leaves when the author types in the text.
    fn leave_on_typing(&mut self, cx: &mut Context<Self>) {
        let rail = Rail::get(cx);
        let inserting = self.quiet_until.is_some_and(|until| std::time::Instant::now() < until);
        if !rail.open || !rail.write || inserting {
            return;
        }
        let editor = self.parts.editor.clone();
        Current::<Material>::defer(cx, move |material, window, cx| {
            if editor.focus_handle(cx).is_focused(window) {
                material.set_open(false, cx);
            }
        });
    }

    fn requote(&mut self, cx: &mut Context<Self>) {
        let Some(chapter) = self.chapter_path(cx) else {
            return;
        };
        let editor = self.parts.editor.read(cx);
        let text = editor.text();
        let quotes: Vec<(String, String)> = (self.shown_quotes.iter())
            .zip(editor.pin_anchors())
            .filter_map(|(old, anchor)| Some((old.clone(), pins::quote_at(text, anchor)?)))
            .collect();
        if let Some(pins) = &mut self.pins
            && let Err(error) = pins.requote(&chapter, &quotes)
        {
            self.say(crate::t!(Key::CannotPin, reason = error), cx);
        }
    }

    /// Gives the text the open chapter's pins, grouped by paragraph.
    fn show_pins(&mut self, cx: &mut Context<Self>) {
        let chapter = self.chapter_path(cx);
        let folded = Rail::get(cx).open && self.mode == Mode::Write;
        let editor = self.parts.editor.clone();
        let text = editor.read(cx).text().to_owned();
        let mut groups: Vec<PinGroup> = Vec::new();
        let mut quotes: Vec<String> = Vec::new();
        let all = self.pins.as_ref().map_or(&[][..], |pins| pins.all());
        for (index, pin) in all.iter().enumerate() {
            if Some(&pin.chapter) != chapter.as_ref() {
                continue;
            }
            let (Some(anchor), Some(item)) = (pins::anchor(&text, &pin.quote), self.item(&pin.item)) else {
                continue;
            };
            let card = self.card(index, pin, item);
            match groups.iter_mut().position(|group| group.anchor == anchor) {
                Some(at) => groups[at].cards.push(card),
                None => {
                    groups.push(PinGroup {
                        anchor,
                        cards: vec![card],
                    });
                    quotes.push(pin.quote.clone());
                }
            }
        }
        let mut order: Vec<usize> = (0..groups.len()).collect();
        order.sort_by_key(|at| groups[*at].anchor);
        let groups: Vec<PinGroup> = order.iter().map(|at| groups[*at].clone()).collect();
        self.shown_quotes = order.iter().map(|at| quotes[*at].clone()).collect();
        editor.update(cx, |editor, cx| editor.set_pins(groups, folded, cx));
    }

    fn card(&self, index: usize, pin: &Pin, item: &Item) -> PinCard {
        let excerpt = pin.passage.clone().or_else(|| {
            let text = item.prose();
            (!text.is_empty()).then(|| cut(&text, EXCERPT))
        });
        PinCard {
            index,
            icon: icon(item),
            title: item.label().to_owned(),
            meta: item.date.clone().unwrap_or_else(|| kind_label(item.kind).to_owned()),
            excerpt,
            insert: insert_label(item).to_owned(),
        }
    }

    fn mode_changed(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        let open = Rail::get(cx).open;
        cx.set_global(Rail {
            open,
            write: mode == Mode::Write,
        });
        self.parts
            .inspector
            .update(cx, |inspector, cx| inspector.set_write(mode == Mode::Write, cx));
        if mode == Mode::Publish
            && let Some(library) = &self.library
        {
            library.update(cx, |library, cx| library.close(window, cx));
        }
        self.show_pins(cx);
    }

    /// Opens or closes the rail in Write. Opening reads the material again, so a file added outside shows.
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        let rail = Rail::get(cx);
        if rail.open == open {
            return;
        }
        if open {
            self.rescan_now(cx);
        }
        cx.set_global(Rail { open, ..rail });
        self.show_pins(cx);
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// The rail's key: in Write, opens it with the keys on its panel, or closes it and gives them back to the text.
    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Write {
            return;
        }
        let open = !Rail::get(cx).open;
        self.set_open(open, cx);
        if open {
            self.focus_panel(false, window, cx);
        } else {
            self.focus_text(window, cx);
        }
    }

    /// `Cmd+L`: opens the rail if need be and gives the keys to the panel's field.
    fn jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Write {
            return;
        }
        self.set_open(true, cx);
        self.focus_panel(true, window, cx);
    }

    fn focus_panel(&mut self, field: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.panel.clone() else {
            return;
        };
        if !self.parts.inspector.read(cx).active_is_material() {
            self.parts
                .inspector
                .update(cx, |inspector, cx| inspector.show(Tab::Research, cx));
        }
        // The panel is in no part of the window until the next frame draws it.
        window.on_next_frame(move |window, cx| panel.update(cx, |panel, cx| panel.take_keys(field, window, cx)));
    }

    pub fn focus_text(&self, window: &mut Window, cx: &mut App) {
        let focus = self.parts.editor.focus_handle(cx);
        window.focus(&focus, cx);
    }

    fn show_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Write {
            return;
        }
        self.rescan_now(cx);
        if let Some(library) = &self.library {
            library.update(cx, |library, cx| library.toggle(window, cx));
        }
    }

    /// A tab of the rail was chosen: the panel shows that kind's list.
    fn tab_shown(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let kind = match tab {
            Tab::Research => Kind::Research,
            Tab::Notes => Kind::Notes,
            Tab::Probes => Kind::Probes,
            _ => return,
        };
        if let Some(panel) = &self.panel {
            panel.update(cx, |panel, cx| panel.show_kind(kind, cx));
        }
    }

    /// Return: shows the item in the rail's panel, opening the rail in Write.
    pub fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.item(id) else {
            return;
        };
        let tab = match item.kind {
            Kind::Research | Kind::Sources => Tab::Research,
            Kind::Notes => Tab::Notes,
            Kind::Probes => Tab::Probes,
        };
        self.set_open(true, cx);
        let id = id.to_owned();
        if let Some(panel) = self.panel.clone() {
            panel.update(cx, |panel, cx| panel.open_item(&id, cx));
        }
        self.parts
            .inspector
            .update(cx, |inspector, cx| inspector.show_quietly(tab, cx));
        self.focus_panel(false, window, cx);
    }

    /// Shift+Return: writes what the item's kind inserts at the caret, as one undo step, and gives the keys to the text.
    pub fn insert(&mut self, id: &str, passage: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.item(id).cloned() else {
            return;
        };
        let editor = self.parts.editor.clone();
        self.quiet_until = Some(std::time::Instant::now() + QUIET);
        match Insert::of(&item) {
            Insert::Figure => {
                let (Some(base), Some(path)) = (&self.base, item.path()) else {
                    return;
                };
                let file = base.join(path);
                editor.update(cx, |editor, cx| editor.insert_image_file(file, window, cx));
            }
            Insert::Quote { .. } | Insert::Citation => {
                let language = editor.read(cx).session().details().language.clone();
                let written = match Insert::of(&item) {
                    Insert::Citation => format!("[@{}]", item.key.clone().unwrap_or_default()),
                    _ => {
                        let first = item.paragraphs().into_iter().next().unwrap_or_default();
                        verbs::quotation(passage.unwrap_or(&first), item.key.as_deref(), &language)
                    }
                };
                editor.update(cx, |editor, cx| {
                    let range = editor.selection_range();
                    let written = verbs::spaced(editor.text(), range.start, written);
                    let chapter = editor.session().active();
                    let caret = range.start + written.len();
                    let edit = crate::source::Edit::new(range, written);
                    if editor
                        .edit_outside(chapter, vec![edit], EditKind::Discrete, window, cx)
                        .is_ok()
                    {
                        editor.jump_to(caret, window, cx);
                    }
                });
            }
            Insert::Table | Insert::Passage => {
                let csv = (item.form == Form::Table)
                    .then(|| std::fs::read_to_string(self.base.as_ref()?.join(item.path()?)).ok())
                    .flatten();
                let Some(content) = verbs::block_text(&item, csv.as_deref(), passage) else {
                    self.say(crate::t!(Key::CannotRead).to_owned(), cx);
                    return;
                };
                editor.update(cx, |editor, cx| {
                    let caret = editor.selection_range().end;
                    let (edit, after) = verbs::after_block(editor.text(), caret, &content);
                    let chapter = editor.session().active();
                    if editor
                        .edit_outside(chapter, vec![edit], EditKind::Discrete, window, cx)
                        .is_ok()
                    {
                        editor.jump_to(after, window, cx);
                    }
                });
            }
        }
        self.focus_text(window, cx);
    }

    /// Command+Return: pins the item to the paragraph at the caret.
    pub fn pin(&mut self, id: &str, passage: Option<&str>, cx: &mut Context<Self>) {
        let editor = self.parts.editor.read(cx);
        let quote = pins::quote_at(editor.text(), editor.selection_range().start);
        let (Some(chapter), Some(quote)) = (self.chapter_path(cx), quote) else {
            self.say(crate::t!(Key::NoParagraph).to_owned(), cx);
            return;
        };
        let pin = Pin {
            chapter,
            quote,
            item: id.to_owned(),
            passage: passage.map(str::to_owned),
        };
        if let Some(pins) = &mut self.pins
            && let Err(error) = pins.add(pin)
        {
            self.say(crate::t!(Key::CannotPin, reason = error), cx);
        }
        self.show_pins(cx);
        cx.emit(MaterialChanged);
        cx.notify();
    }

    fn pin_clicked(&mut self, index: usize, verb: PinVerb, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pin) = self.pins.as_ref().and_then(|pins| pins.all().get(index)).cloned() else {
            return;
        };
        match verb {
            PinVerb::Open => self.open(&pin.item, window, cx),
            PinVerb::Insert => self.insert(&pin.item, pin.passage.as_deref(), window, cx),
            PinVerb::Unpin => {
                if let Some(pins) = &mut self.pins
                    && let Err(error) = pins.remove(&pin)
                {
                    self.say(crate::t!(Key::CannotPin, reason = error), cx);
                }
                self.show_pins(cx);
                self.focus_text(window, cx);
                cx.emit(MaterialChanged);
            }
        }
    }

    /// Jumps the text to `offset` of the open chapter, for a link to where an item is cited.
    pub fn show_in_text(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.parts
            .editor
            .update(cx, |editor, cx| editor.jump_to(offset, window, cx));
        self.focus_text(window, cx);
    }

    fn say(&self, text: String, cx: &mut App) {
        Current::<crate::persist::PersistView>::defer(cx, move |view, window, cx| view.show_note(text, window, cx));
    }
}

/// `text` cut to about `most` bytes at a word's end, with an ellipsis.
fn cut(text: &str, most: usize) -> String {
    if text.len() <= most {
        return text.to_owned();
    }
    let end = text.floor_char_boundary(most);
    let end = text[..end].rfind(' ').unwrap_or(end);
    format!("{}\u{2026}", &text[..end])
}

pub fn icon(item: &Item) -> &'static str {
    match (item.form, item.kind) {
        (Form::Image, _) => "icons/images.svg",
        (Form::Table, _) => "icons/block-table.svg",
        (Form::Source, _) => "icons/source.svg",
        (Form::Text, Kind::Notes) => "icons/notes.svg",
        (Form::Text, Kind::Probes) => "icons/probe.svg",
        (Form::Text, _) => "icons/file.svg",
    }
}

pub fn kind_label(kind: Kind) -> &'static str {
    crate::t!(match kind {
        Kind::Research => Key::Research,
        Kind::Notes => Key::Notes,
        Kind::Probes => Key::Probes,
        Kind::Sources => Key::Sources,
    })
}

/// What Shift+Return is called for an item.
pub fn insert_label(item: &Item) -> &'static str {
    crate::t!(match Insert::of(item) {
        Insert::Quote { cited: true } => Key::QuoteWithCitation,
        Insert::Quote { cited: false } => Key::Quote,
        Insert::Figure => Key::InsertFigure,
        Insert::Table => Key::InsertTable,
        Insert::Passage => Key::InsertPassage,
        Insert::Citation => Key::InsertCitation,
    })
}

/// Sets what a run asks for, for screenshots, with no keys: `DRUCK_MATERIAL` as `rail`, `jump`, `library`, `step` (as
/// Option+Down does), `item:<id>`, `pin:<id>` and `insert:<id>` (at the caret, as the verbs do), and `probe:<theme>`,
/// several separated by commas, in order.
#[cfg(any(debug_assertions, feature = "measure"))]
pub fn apply_gate(window: &mut Window, _: &mut App) {
    let Some(asked) = crate::gate::var("DRUCK_MATERIAL") else {
        return;
    };
    // Two frames on, after `DRUCK_SELECT` has put the caret where the steps may pin to.
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |window, cx| gate_steps(&asked, window, cx));
    });
}

#[cfg(any(debug_assertions, feature = "measure"))]
fn gate_steps(asked: &str, window: &mut Window, cx: &mut App) {
    let Some(material) = Current::<Material>::view(cx) else {
        return;
    };
    material.update(cx, |material, cx| {
        for step in asked.split(',') {
            match step.split_once(':') {
                Some(("item", id)) => material.open(id, window, cx),
                Some(("pin", id)) => material.pin(id, None, cx),
                Some(("insert", id)) => material.insert(id, None, window, cx),
                Some(("probe", theme)) => {
                    if let Some(panel) = &material.panel {
                        let theme = theme.to_owned();
                        panel.update(cx, |panel, cx| panel.choose_theme_named(&theme, cx));
                    }
                }
                _ => match step {
                    "rail" => material.set_open(true, cx),
                    "jump" => material.jump(window, cx),
                    "library" => material.show_library(window, cx),
                    "step" => {
                        let editor = material.parts.editor.clone();
                        editor.update(cx, |editor, cx| editor.step_pin_for_gate(window, cx));
                    }
                    _ => {}
                },
            }
        }
    });
}
