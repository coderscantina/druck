//! Layout directives in Write: the text shows what a directive does, and a thin bracket in the left margin names it.
//!
//! `columns` sets its text in two balanced columns inside the text column, `full-width` across both, and `page-break`
//! and the parts of a book as a gap between two sheet edges. `keep`, `bottom`, and the rest show their text as it is.
//! Fence lines take no room at rest. The caret on one opens it as source, lights its bracket's name, and shows the
//! directive's options under the name, which Tab gives the keys and Escape takes them back from.
//!
//! [`structure`] reads which directives the engine accepts, [`balance`] parts a run between two columns, [`edits`] is
//! what the options write, and [`view`] draws the brackets, the options, and the slash menu's ghost.

mod balance;
pub(super) mod edits;
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
        /// Gives the keys to the options of the directive whose fence line holds the caret.
        OptionsFocus,
        OptionsUp,
        OptionsDown,
        OptionsLeft,
        OptionsRight,
        OptionsAccept,
        /// Steps the chosen value of the ghost's table options, or acts as [`OptionsAccept`] elsewhere.
        OptionsToggle,
        /// Gives the keys back to the line, or to the slash menu.
        OptionsBack,
        OptionsUnwrap,
        /// Sets the part of two columns at the caret across both.
        FullWidthFromCaret,
        /// Gives the keys to the options of the slash menu's ghost.
        GhostOptions,
    ]
);

/// Set while the caret stands on a fence line with options.
pub(super) const FENCE_CONTEXT: &str = "DirectiveFence";
/// Set while a directive's options have the keys. Bound after the menu's keys, so these win.
pub(super) const OPTIONS_CONTEXT: &str = "DirectiveOptions";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", OptionsFocus, Some(FENCE_CONTEXT)),
        KeyBinding::new("cmd-shift-enter", FullWidthFromCaret, Some(super::keys::CONTEXT)),
        KeyBinding::new("tab", GhostOptions, Some(super::keys::MENU_CONTEXT)),
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
    ]);
}

impl Kind {
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

    /// The kind's word in the options' segmented choice.
    fn choice(self) -> &'static str {
        crate::t!(match self {
            Self::Columns => Key::KindColumns,
            Self::FullWidth => Key::KindFullWidth,
            Self::Bottom => Key::KindBottom,
            _ => Key::KindKeep,
        })
    }
}

/// The options under a fence line's name, while the caret is on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Options {
    /// The fence line's block. The options are its region's.
    pub block: usize,
    /// They have the keys, with this row chosen.
    pub keys: bool,
    pub row: usize,
    /// The kind chosen in the kind row, as an index into what it offers.
    pub kind: usize,
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
    pub options: Option<Options>,
    pub ghost_table: Option<GhostTable>,
    /// Where the lit name stood in the last frame, which the options stand under.
    anchor: Option<Bounds<Pixels>>,
    /// The ghost's table as last shaped, for the options and width it was shaped with.
    ghost_grid: Option<(TableOptions, Pixels, Rc<Whole>)>,
}

impl Directives {
    pub fn two_columns(&self) -> bool {
        self.two_columns
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
        let structure = structure::read(text, &self.blocks);
        self.directives.runs = structure.column_runs(&self.blocks);
        self.directives.structure = structure;
        self.directives.shifted.clear();
        if let Some(options) = self.directives.options
            && self.directives.structure.fence(options.block).is_none()
        {
            self.directives.options = None;
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

    /// Whether the caret stands on block `index`.
    fn caret_on(&self, index: usize) -> bool {
        self.reveal_offset().is_some_and(|at| self.blocks.index_at(at) == index)
    }

    /// Whether block `index` is a fence line of a directive with text, which takes no room while the caret is away.
    pub(super) fn fence_at_rest(&self, index: usize) -> bool {
        self.drawn_fence(index)
            .is_some_and(|region| region.kind.has_body() && !self.caret_on(index))
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

    /// The kind of a directive standing alone at block `index`, which shows its effect while the caret is away.
    pub(super) fn standalone_at_rest(&self, index: usize) -> Option<Kind> {
        self.drawn_fence(index)
            .map(|region| region.kind)
            .filter(|kind| !kind.has_body() && !self.caret_on(index))
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

    /// The region whose fence line holds the caret, when it has options to show.
    pub(super) fn fence_under_caret(&self) -> Option<usize> {
        if self.source || !self.selection.is_empty() {
            return None;
        }
        let block = self.blocks.index_at(self.selection.head);
        let region = self.directives.structure.fence(block)?;
        (self.directives.structure.regions[region].kind != Kind::Table).then_some(region)
    }

    /// Follows the caret: the options stay while it is on the same fence line, and go when it leaves.
    pub(super) fn follow_fence(&mut self) {
        let block = self.blocks.index_at(self.selection.head);
        let on_fence = self.fence_under_caret().is_some();
        match self.directives.options {
            Some(options) if on_fence && options.block == block => {}
            _ if on_fence && self.selection.is_empty() => {
                self.directives.options = Some(Options {
                    block,
                    keys: false,
                    row: 0,
                    kind: 0,
                });
            }
            _ => self.directives.options = None,
        }
    }

    /// Whether the options have the keys, so the text's own keys stand aside.
    pub(super) fn options_have_keys(&self) -> bool {
        self.directives.options.is_some_and(|options| options.keys)
            || self.directives.ghost_table.as_ref().is_some_and(|ghost| ghost.keys)
    }
}
