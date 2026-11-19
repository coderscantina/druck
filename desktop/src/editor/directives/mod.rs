//! Layout directives in Write: the text shows what a directive does, and a thin bracket in the left margin names it.
//!
//! `columns` sets its text in two balanced columns inside the text column, `full-width` across both, and `page-break`
//! and the parts of a book as a gap between two sheet edges. `keep`, `bottom`, and the rest show their text as it is.
//! Fence lines take no room and the caret never stands on them. A region is changed as a block is: [`region`] selects
//! it by its grip or name, opens its menu, and moves, copies, and deletes it whole.
//!
//! [`structure`] reads which directives the engine accepts, [`balance`] parts a run between two columns, [`edits`] is
//! what a region's menu and keys write, and [`view`] draws the brackets, the grip, the menu, and the slash menu's ghost.

mod balance;
pub(super) mod edges;
pub(super) mod edits;
mod region;
mod strings;
pub(super) mod structure;
mod view;

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{App, Bounds, KeyBinding, Pixels, Window, actions, px};

pub(super) use self::edits::TableOptions;
use self::strings::Key;
pub(super) use self::structure::{Kind, Structure};
pub(super) use self::view::{Marks, ghost_kind, paint_marks};
use super::EditorView;
use super::rich::view::shapes::Whole;
use crate::source::BlockKind;
use crate::tokens;

actions!(
    directives,
    [
        OptionsUp,
        OptionsDown,
        OptionsLeft,
        OptionsRight,
        OptionsAccept,
        /// Steps the chosen value of the ghost's table options, or acts as [`OptionsAccept`] elsewhere.
        OptionsToggle,
        /// Closes the region's menu, or gives the ghost's keys back to the slash menu.
        OptionsBack,
        OptionsUnwrap,
        /// The keys the region's menu shows beside its rows.
        OptionsDelete,
        OptionsDuplicate,
        OptionsMoveUp,
        OptionsMoveDown,
        /// Sets the part of two columns at the caret across both.
        FullWidthFromCaret,
        /// Gives the keys to the options of the slash menu's ghost.
        GhostOptions,
        /// Moves the block at the caret, or a table or figure whole, a step up or down: across a region's edge counts
        /// as a step.
        MoveBlockUp,
        MoveBlockDown,
        /// With a region selected: selects the region around it, or gives the keys back to the text.
        RegionOut,
        /// Puts the caret at the start of the selected region's text.
        RegionEnter,
        /// Opens the selected region's menu.
        RegionMenu,
        RegionUp,
        RegionDown,
        RegionDuplicate,
        RegionDelete,
        RegionCopy,
        RegionCut,
    ]
);

/// Set while a region is selected and its menu is closed.
pub(super) const REGION_CONTEXT: &str = "DirectiveRegion";
/// Set while a region's menu or the ghost's options have the keys. Bound after the menu's keys, so these win.
pub(super) const OPTIONS_CONTEXT: &str = "DirectiveOptions";

pub fn bind_keys(cx: &mut App) {
    let text = Some(super::keys::CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-shift-enter", FullWidthFromCaret, text),
        KeyBinding::new("alt-shift-up", MoveBlockUp, text),
        KeyBinding::new("alt-shift-down", MoveBlockDown, text),
        KeyBinding::new("tab", GhostOptions, Some(super::keys::MENU_CONTEXT)),
    ]);
    // After the text's, so a selected region takes these keys.
    let region = Some(REGION_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", RegionOut, region),
        KeyBinding::new("enter", RegionEnter, region),
        KeyBinding::new("tab", RegionMenu, region),
        KeyBinding::new("alt-up", RegionUp, region),
        KeyBinding::new("alt-down", RegionDown, region),
        KeyBinding::new("alt-shift-up", RegionUp, region),
        KeyBinding::new("alt-shift-down", RegionDown, region),
        KeyBinding::new("cmd-d", RegionDuplicate, region),
        KeyBinding::new("backspace", RegionDelete, region),
        KeyBinding::new("delete", RegionDelete, region),
        KeyBinding::new("cmd-c", RegionCopy, region),
        KeyBinding::new("cmd-x", RegionCut, region),
    ]);
    let context = Some(OPTIONS_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", OptionsUp, context),
        KeyBinding::new("down", OptionsDown, context),
        KeyBinding::new("shift-tab", OptionsUp, context),
        KeyBinding::new("tab", OptionsDown, context),
        KeyBinding::new("left", OptionsLeft, context),
        KeyBinding::new("right", OptionsRight, context),
        KeyBinding::new("enter", OptionsAccept, context),
        KeyBinding::new("space", OptionsToggle, context),
        KeyBinding::new("escape", OptionsBack, context),
        KeyBinding::new("cmd-backspace", OptionsUnwrap, context),
        KeyBinding::new("backspace", OptionsDelete, context),
        KeyBinding::new("delete", OptionsDelete, context),
        KeyBinding::new("cmd-d", OptionsDuplicate, context),
        KeyBinding::new("alt-up", OptionsMoveUp, context),
        KeyBinding::new("alt-down", OptionsMoveDown, context),
    ]);
}

impl Kind {
    /// Its icon in a menu.
    pub fn icon(self) -> &'static str {
        match self {
            Self::FullWidth => "icons/directive-full-width.svg",
            Self::Keep => "icons/directive-keep.svg",
            Self::Bottom => "icons/directive-bottom.svg",
            _ => "icons/directive-columns.svg",
        }
    }

    /// The bracket's name.
    pub fn label(self) -> &'static str {
        crate::t!(match self {
            Self::Columns => Key::TwoColumns,
            Self::FullWidth => Key::FullWidth,
            Self::Keep => Key::KeptTogether,
            Self::Bottom => Key::BottomOfPage,
            Self::Table => Key::Table,
            Self::PageBreak => Key::NewPage,
            Self::Toc => Key::Contents,
            Self::Bibliography => Key::Bibliography,
            Self::Index => Key::Index,
            Self::FrontMatter => Key::FrontMatter,
            Self::MainMatter => Key::MainMatter,
            Self::BackMatter => Key::BackMatter,
        })
    }
}

/// The group name of the block menu's rows that wrap blocks in a region.
pub(super) fn wrap_in_label() -> &'static str {
    crate::t!(Key::WrapIn)
}

/// A selected region's menu, which has the keys while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Menu {
    pub row: usize,
    /// The kinds of Turn into, open beside its row, with this one chosen.
    pub sub: Option<usize>,
}

/// Where a region stood in the last frame, for the pointer.
#[derive(Debug, Clone, Copy)]
pub(super) struct Span {
    pub region: usize,
    pub top: Pixels,
    pub bottom: Pixels,
    pub depth: usize,
    /// It stands in the second of two columns, so only the pointer over that column is in it.
    pub second: bool,
}

/// Where a dragged block would land when it crosses a region's edge: into `region`, or out of it, with the drop line
/// starting at `x`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Edge {
    pub region: usize,
    pub into: bool,
    pub x: Pixels,
}

/// The table options of the slash menu's ghost, while a list table is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GhostTable {
    pub options: TableOptions,
    pub keys: bool,
    /// 0 for align, 1 for widths.
    pub row: usize,
    pub column: usize,
}

/// How many columns a list table the slash menu writes has.
pub(super) const GHOST_TABLE_COLUMNS: usize = 2;

impl Default for GhostTable {
    fn default() -> Self {
        Self {
            options: TableOptions {
                align: vec![edits::Align::Left; GHOST_TABLE_COLUMNS],
                fill: vec![false; GHOST_TABLE_COLUMNS],
            },
            keys: false,
            row: 0,
            column: 0,
        }
    }
}

/// What the galley knows of the active chapter's directives.
#[derive(Default)]
pub(super) struct Directives {
    pub structure: Structure,
    /// The runs of blocks of every `columns` region, sorted.
    runs: Vec<Range<usize>>,
    /// Whether the text is wide enough for two columns, and their width and gap.
    two_columns: bool,
    column_width: Pixels,
    column_gap: Pixels,
    /// How far right of the text's left edge each block on screen in a second column starts.
    shifted: HashMap<usize, Pixels>,
    /// The selected region, by where its opening line starts, so it survives the directives being read again.
    pub selected: Option<usize>,
    /// Where the caret stood when its region was selected by the keys, for Full width from here.
    origin: Option<usize>,
    pub menu: Option<Menu>,
    /// The region under the pointer, which shows its grip.
    pub hover: Option<usize>,
    pub ghost_table: Option<GhostTable>,
    /// Where the menu's region's grip stood in the last frame, which the menu stands under.
    anchor: Option<Bounds<Pixels>>,
    /// Each name and grip on screen in the last frame, with its region, where a click selects it.
    names: Vec<(Bounds<Pixels>, usize)>,
    grips: Vec<(Bounds<Pixels>, usize)>,
    spans: Vec<Span>,
    /// Each repair on screen in the last frame, with the rejected line's block.
    repairs: Vec<(Bounds<Pixels>, usize)>,
    /// The ghost's table as last shaped, for the options and width it was shaped with.
    ghost_grid: Option<(TableOptions, Pixels, Rc<Whole>)>,
}

impl Directives {
    pub fn two_columns(&self) -> bool {
        self.two_columns
    }

    /// The left edge of the leftmost bracket name standing beside the row from `top` to `bottom`, so another word in the
    /// margin, such as a style's name, can end before it.
    pub fn name_left_beside(&self, top: Pixels, bottom: Pixels) -> Option<Pixels> {
        (self.names.iter())
            .filter(|(name, _)| name.top() < bottom && name.bottom() > top)
            .map(|(name, _)| name.left())
            .reduce(Pixels::min)
    }

    pub fn column_gap(&self) -> Pixels {
        self.column_gap
    }
}

/// Where a run's blocks stand: each one's distance across from the text's left and down from the run's top, and the
/// height of the two columns side by side.
pub(super) struct RunPlaces {
    pub places: Vec<(usize, Pixels, Pixels)>,
    pub height: Pixels,
}

impl EditorView {
    /// Reads the directives again, after the text or the chapter changed.
    pub(super) fn read_directives(&mut self) {
        let text = self.session.active_buffer().text();
        let mut structure = structure::read(text, &self.blocks);
        self.forgive_what_other_chapters_balance(&mut structure);
        self.directives.runs = structure.column_runs(&self.blocks);
        self.directives.structure = structure;
        self.directives.shifted.clear();
        self.directives.hover = None;
        if self.selected_region().is_none() {
            self.directives.selected = None;
            self.directives.menu = None;
        }
    }

    /// The engine reads a book as one text, so a region may open in one chapter and close in a later one. Neither line
    /// is a fault then.
    fn forgive_what_other_chapters_balance(&self, structure: &mut Structure) {
        use structure::Fault;
        if structure.rejected.is_empty() || self.session.book().len() < 2 {
            return;
        }
        let session = &self.session;
        let at = session.book().iter().position(|id| *id == session.active());
        let Some(at) = at else {
            return;
        };
        let balance =
            |id: &crate::session::ChapterId| session.chapter(*id).map(|chapter| structure::balance(chapter.text()));
        let carried_in = session.book()[..at]
            .iter()
            .filter_map(balance)
            .fold(0, |open, (strays, left)| open - strays.min(open) + left);
        let closed_later = session.book()[at + 1..]
            .iter()
            .rev()
            .filter_map(balance)
            .fold(0, |closes, (strays, left)| closes - left.min(closes) + strays);
        // Inside a region an earlier chapter opened, what this text allows depends on that region, which it cannot see.
        if carried_in > 0 {
            structure.rejected.retain(|rejected| rejected.fault != Fault::Refused);
        }
        let (mut strays, mut unclosed) = (carried_in, closed_later);
        let mut kept: Vec<structure::Rejected> = Vec::with_capacity(structure.rejected.len());
        for rejected in structure.rejected.drain(..) {
            if rejected.fault == Fault::Stray && strays > 0 {
                strays -= 1;
            } else {
                kept.push(rejected);
            }
        }
        for at in (0..kept.len()).rev() {
            if kept[at].fault == Fault::Unclosed && unclosed > 0 {
                unclosed -= 1;
                kept.remove(at);
            }
        }
        structure.rejected = kept;
    }

    /// The directives the engine takes once that the book's other chapters already hold.
    pub(super) fn kinds_in_other_chapters(&self) -> Vec<Kind> {
        let session = &self.session;
        (session.book().iter())
            .filter(|id| **id != session.active())
            .filter_map(|id| session.chapter(*id))
            .flat_map(|chapter| {
                let text = chapter.text();
                structure::read(text, &crate::source::Blocks::parse(text)).regions
            })
            .map(|region| region.kind)
            .filter(|kind| kind.only_once())
            .collect()
    }

    /// The region standing as one mark at block `index`: a directive that holds no text, or a region with nothing in it
    /// yet, at its opening line or the blank line inside it. The caret never stands in one: it is selected whole.
    pub(super) fn marker_at(&self, index: usize) -> Option<usize> {
        if self.source {
            return None;
        }
        let structure = &self.directives.structure;
        let opening = |block: usize| structure.fence(block).filter(|&at| structure.regions[at].open == block);
        let blank = self.blocks.as_slice()[index].kind == BlockKind::Blank;
        // A blank line after a mark is text's place. Only one inside an empty region belongs to the region.
        let inside = || {
            let before = index.checked_sub(1).filter(|_| blank).and_then(opening)?;
            structure.regions[before].kind.has_body().then_some(before)
        };
        let region = opening(index).or_else(inside)?;
        self.stands_as_marker(region).then_some(region)
    }

    /// Whether `region` shows as one mark rather than as the text it holds.
    pub(super) fn stands_as_marker(&self, region: usize) -> bool {
        let found = &self.directives.structure.regions[region];
        if found.kind == Kind::Table {
            return false;
        }
        match found.close - found.open {
            0 | 1 => true,
            2 => {
                let inside = &self.blocks.as_slice()[found.open + 1];
                let text = self.session.active_buffer().text();
                inside.kind == BlockKind::Blank && text[inside.range.clone()].matches('\n').count() <= 1
            }
            _ => false,
        }
    }

    /// Takes the text's width: two columns show only while each holds [`tokens::COLUMN_MIN_CHARACTERS`].
    pub(super) fn place_columns(&mut self) {
        let gap = self.style.column_gap();
        let least = self.style.width_of_characters(tokens::COLUMN_MIN_CHARACTERS) * 2. + gap;
        let width = self.column.text_width;
        self.directives.column_gap = gap;
        self.directives.column_width = (width - gap) * 0.5;
        self.directives.two_columns = !self.source && width >= least;
    }

    /// The run set in two columns that holds block `index`.
    pub(super) fn run_at(&self, index: usize) -> Option<Range<usize>> {
        if !self.directives.two_columns {
            return None;
        }
        let runs = &self.directives.runs;
        let at = runs.partition_point(|run| run.end <= index);
        runs.get(at).filter(|run| run.start <= index).cloned()
    }

    /// The width block `index` wraps at: its column's inside two columns, the text's otherwise.
    pub(super) fn block_width(&self, index: usize) -> Pixels {
        let narrow = self.run_at(index).is_some() || self.ghost_narrows(index);
        if narrow && self.directives.two_columns {
            self.directives.column_width
        } else {
            self.column.text_width
        }
    }

    /// Where block `index` starts across the window, as last laid out.
    pub(super) fn block_left(&self, index: usize) -> Pixels {
        self.column.text_left + self.directives.shifted.get(&index).copied().unwrap_or_default()
    }

    /// The region of a fence line at block `index` that Write draws, which the engine takes and which is not a table's:
    /// the table shows whole in its place.
    fn drawn_fence(&self, index: usize) -> Option<&structure::Region> {
        if self.source {
            return None;
        }
        let region = &self.directives.structure.regions[self.directives.structure.fence(index)?];
        (region.kind != Kind::Table).then_some(region)
    }

    /// Whether block `index` is a fence line of a directive with text, which takes no room and no caret in Write.
    pub(super) fn fence_at_rest(&self, index: usize) -> bool {
        self.drawn_fence(index).is_some_and(|region| region.kind.has_body()) && self.marker_at(index).is_none()
    }

    /// Whether block `index` is a line where two columns start or end, while they show.
    pub(super) fn changes_columns(&self, index: usize) -> bool {
        self.directives.two_columns
            && self
                .drawn_fence(index)
                .is_some_and(|region| matches!(region.kind, Kind::Columns | Kind::FullWidth))
    }

    /// Whether block `index` opens a directive at rest, or stands alone at rest, so the blank line after it would only
    /// double the room above the text it holds.
    pub(super) fn opens_at_rest(&self, index: usize) -> bool {
        let opens = self
            .drawn_fence(index)
            .is_some_and(|region| region.kind.has_body() && region.open == index);
        (opens && self.fence_at_rest(index)) || self.standalone_at_rest(index).is_some()
    }

    /// Whether block `index` closes a directive at rest with a blank line after it, which keeps the room below, so the
    /// blank line before it goes.
    pub(super) fn closes_before_a_blank(&self, index: usize) -> bool {
        let list = self.blocks.as_slice();
        self.drawn_fence(index)
            .is_some_and(|region| region.kind.has_body() && region.close == index)
            && self.fence_at_rest(index)
            && list.get(index + 1).is_some_and(|after| after.kind == BlockKind::Blank)
    }

    /// The kind of the mark whose opening line is block `index`, which always shows its effect and never its line.
    pub(super) fn standalone_at_rest(&self, index: usize) -> Option<Kind> {
        let region = &self.directives.structure.regions[self.marker_at(index)?];
        (region.open == index).then_some(region.kind)
    }

    /// The room a directive standing alone takes at rest: a new page's gap, or one line for the rest.
    pub(super) fn standalone_room(&self, kind: Kind) -> Pixels {
        let line = self.style.body_size * self.style.leading;
        if kind.starts_page() {
            line * tokens::PAGE_GAP_LINES
        } else {
            line
        }
    }

    /// The height two columns of `run` take, from what each block has been shaped or estimated at.
    pub(super) fn run_height(&self, run: &Range<usize>) -> Pixels {
        let parts = self.run_parts(run, |index| self.own_height(index));
        balance::balance(&parts.iter().map(|(_, part)| *part).collect::<Vec<_>>()).height()
    }

    /// The units of a run with their heights, each with the block it starts at.
    fn run_parts(&self, run: &Range<usize>, height: impl Fn(usize) -> Pixels) -> Vec<(usize, balance::Part)> {
        let list = self.blocks.as_slice();
        self.directives
            .structure
            .units(run)
            .into_iter()
            .map(|unit| {
                let total = unit.clone().map(&height).fold(px(0.), |sum, height| sum + height);
                let last_text = unit.clone().rev().find(|&at| list[at].kind != BlockKind::Blank);
                let part = balance::Part {
                    height: total,
                    blank: last_text.is_none(),
                    keep_with_next: last_text.is_some_and(|at| matches!(list[at].kind, BlockKind::Heading { .. })),
                };
                (unit.start, part)
            })
            .collect()
    }

    /// Shapes a run's blocks and parts them between two balanced columns.
    pub(super) fn run_places(&mut self, run: Range<usize>, window: &Window) -> RunPlaces {
        for index in run.clone() {
            self.layout_block(index, window);
        }
        let parts = self.run_parts(&run, |index| self.own_height(index));
        let split = balance::balance(&parts.iter().map(|(_, part)| *part).collect::<Vec<_>>());
        let second = parts.get(split.second).map_or(run.end, |(start, _)| *start);
        let shift = self.directives.column_width + self.directives.column_gap;
        let mut places = Vec::with_capacity(run.len());
        let (mut first_y, mut second_y) = (px(0.), px(0.));
        for index in run {
            let height = self.own_height(index);
            if index < second {
                places.push((index, px(0.), first_y));
                first_y += height;
            } else {
                places.push((index, shift, second_y));
                second_y += height;
            }
        }
        RunPlaces {
            places,
            height: split.height(),
        }
    }

    /// Forgets where the last frame's second columns stood, before laying out again.
    pub(super) fn clear_column_shifts(&mut self) {
        self.directives.shifted.clear();
    }

    /// Lays out a run whose top is at window `y`, adding its blocks to what is on screen. Returns its height.
    pub(super) fn lay_out_run(&mut self, run: Range<usize>, y: Pixels, window: &Window) -> Pixels {
        let RunPlaces { places, height } = self.run_places(run, window);
        for (index, dx, dy) in places {
            self.visible.push((index, y + dy));
            if dx > px(0.) {
                self.directives.shifted.insert(index, dx);
            }
        }
        height
    }

    /// Where a distance into block `index` stands in the text's stack of blocks: a block in two columns counts from the
    /// top of its run, which holds the whole run's height.
    pub(super) fn stacked(&mut self, index: usize, offset: Pixels, window: &Window) -> (usize, Pixels) {
        let Some(run) = self.run_at(index) else {
            return (index, offset);
        };
        let start = run.start;
        let places = self.run_places(run, window).places;
        let down = places
            .iter()
            .find(|(block, _, _)| *block == index)
            .map_or(px(0.), |(_, _, dy)| *dy);
        (start, down + offset)
    }

    /// The block under a window position: in two columns, the one in the column under it.
    pub(super) fn block_at(&self, position: gpui::Point<Pixels>) -> Option<usize> {
        let middle = self.column.text_left + self.directives.column_width + self.directives.column_gap * 0.5;
        let in_second = position.x >= middle;
        self.visible
            .iter()
            .filter(|(index, top)| {
                *top <= position.y
                    && (self.run_at(*index).is_none() || self.directives.shifted.contains_key(index) == in_second)
            })
            .max_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            })
            .map(|(index, _)| *index)
    }

    /// Whether a region's menu or the ghost's options have the keys, so the text's own keys stand aside.
    pub(super) fn options_have_keys(&self) -> bool {
        self.directives.menu.is_some() || self.directives.ghost_table.as_ref().is_some_and(|ghost| ghost.keys)
    }
}
