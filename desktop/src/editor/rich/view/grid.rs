//! A table edited where it stands. The table stays drawn as it is at rest, and one field stands exactly in the cell that
//! has the caret: a click or the arrow keys put it there, Tab and Shift+Tab move it, and Tab in the last cell adds a row.
//! Rows, columns, and the table's options are in the table's menu. Every change is written and the table read again
//! from the text, so what shows is always what is written.

use gpui::Focusable;
use gpui::prelude::*;
use gpui::{AnyElement, Bounds, Context, Entity, MouseDownEvent, Pixels, Point, Subscription, Window, div, point, svg};

use super::shapes::{Place, Whole};
use super::{
    CONTEXT, NextField, PreviousField, RichFloat, StepColumnAlign, StepPlacement, TableMenu, ToggleColumnWidth,
};
use crate::a11y::Control as _;
use crate::editor::commands::marks::Mark;
use crate::editor::float::Float;
use crate::editor::picks::{Pick, Row};
use crate::editor::rich::table::{Align, Caption, Grid};
use crate::editor::rich::{AtomKind, Key};
use crate::editor::{EditorView, ToggleBold, ToggleItalic, ToggleStrike, ToggleUnderline};
use crate::inspector::input::{self, Cancelled, Submitted, TextInput};
use crate::source::{BlockKind, Edit, Selection};
use crate::tokens::{self, Tokens};

/// The caret in a table: the place it stands in, and the field there.
pub struct GridFloat {
    /// Where the table's first block starts.
    start: usize,
    place: Place,
    input: Entity<TextInput>,
    refused: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl GridFloat {
    pub fn first_byte(&self) -> usize {
        self.start
    }
}

/// What the table's menu does, at the cell the caret stood in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAct {
    RowBelow,
    RowDelete,
    RowUp,
    RowDown,
    ColumnAfter,
    ColumnDelete,
    ColumnLeft,
    ColumnRight,
    Align(Align),
    /// A list table's column fills the width, or fits its content.
    Fill(bool),
    SpanMore,
    SpanLess,
    /// `top`, `bottom`, or empty for where the table stands.
    Placement(&'static str),
    Caption,
    /// Gives the table a label made from its caption, or takes its label off.
    Label(bool),
    Source,
    Delete,
}

/// A label from a caption's first words, as `two-events`.
fn label_from(caption: &str) -> String {
    let words: Vec<String> = caption
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(4)
        .map(str::to_lowercase)
        .collect();
    words.join("-")
}

/// The column a cell starts in, counting the cells before it by their spans.
fn column_of(grid: &Grid, row: usize, cell: usize) -> usize {
    grid.rows[row].iter().take(cell).map(|cell| cell.span).sum()
}

impl EditorView {
    /// The table at this byte of the active chapter, as the text says now.
    fn grid_at(&self, start: usize) -> Option<std::rc::Rc<Grid>> {
        let atom = self.rich_atom(self.blocks.index_at(start))?;
        match &atom.kind {
            AtomKind::Table(Ok(grid)) => Some(grid.clone()),
            _ => None,
        }
    }

    fn grid_float(&self) -> Option<&GridFloat> {
        match &self.float {
            Float::Rich(RichFloat::Grid(grid)) => Some(grid),
            _ => None,
        }
    }

    fn grid_float_mut(&mut self) -> Option<&mut GridFloat> {
        match &mut self.float {
            Float::Rich(RichFloat::Grid(grid)) => Some(grid),
            _ => None,
        }
    }

    /// The row and cell the caret stands in. In the caption it counts as the first cell.
    fn grid_cell(&self) -> Option<(usize, usize, usize)> {
        let float = self.grid_float()?;
        Some(match float.place {
            Place::Cell(row, cell) => (float.start, row, cell),
            Place::Caption => (float.start, 0, 0),
        })
    }

    /// Puts the caret in the first cell of the table standing at `at`.
    pub(in crate::editor) fn open_grid_at(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.open_table_place(at, Place::Cell(0, 0), window, cx);
    }

    /// Puts the caret in the table standing at `at`: in the place under `position`, or in its first cell.
    pub(super) fn open_table_under(
        &mut self,
        at: usize,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.blocks.index_at(at);
        let first = self.rich_atom(index).map_or(index, |atom| atom.first);
        let origin = self.block_top(first).map(|top| point(self.block_left(first), top));
        let layout = self.layout_block(first, window);
        let place = match (&layout.whole.as_deref(), position, origin) {
            (Some(Whole::Grid(shape)), Some(position), Some(origin)) => shape.place_at(position - origin),
            _ => None,
        };
        self.open_table_place(at, place.unwrap_or(Place::Cell(0, 0)), window, cx);
    }

    /// Puts the caret in `place` of the table standing at `at`, or the nearest place the table has.
    pub(in crate::editor) fn open_table_place(
        &mut self,
        at: usize,
        place: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.blocks.index_at(at);
        let Some(atom) = self.rich_atom(index).cloned() else {
            return;
        };
        let start = self.blocks.as_slice()[atom.first].range.start;
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        // The caret waits after the table while a cell has the keys, so the block it came from shows at rest, with no
        // list marker or heading sign revealed beside the cell.
        let after = self.blocks.as_slice()[atom.last]
            .range
            .end
            .min(self.session.active_buffer().len());
        self.selection = Selection::caret(self.skip_hidden(after, true));
        self.reveal_at = self.selection.head;
        let place = match place {
            Place::Caption if !grid.caption.text.is_empty() => Place::Caption,
            Place::Caption => Place::Cell(0, 0),
            Place::Cell(row, cell) => {
                let row = row.min(grid.rows.len() - 1);
                Place::Cell(row, cell.min(grid.rows[row].len().saturating_sub(1)))
            }
        };
        let value = match place {
            Place::Caption => grid.caption.text.clone(),
            Place::Cell(row, cell) => grid.rows[row].get(cell).map_or(String::new(), |cell| cell.text.clone()),
        };
        // A list table's cell holds paragraphs and lists, so its field takes several lines.
        let multiline = grid.is_list() && place != Place::Caption;
        let face = self.style.line_style(BlockKind::FootnoteDefinition);
        let label = match place {
            Place::Caption => crate::t!(Key::CaptionLabel).to_owned(),
            Place::Cell(row, cell) => crate::t!(Key::CellLabel, row = row + 1, column = cell + 1),
        };
        let input = cx.new(|cx| {
            let input = if multiline {
                TextInput::multiline(window, cx).in_prose().bare()
            } else {
                TextInput::new(window, cx).keep_focus_on_enter().bare()
            };
            let mut input = input.with_label(label);
            input.set_book_type(Some((face.font, face.size)), cx);
            input.set_value(value, true, cx);
            input.select_all(cx);
            input
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, _: &Submitted, window, cx| {
                this.grid_step_row(true, window, cx)
            }),
            cx.subscribe_in(&input, window, |this, _, _: &Cancelled, window, cx| {
                this.leave_grid(true, window, cx)
            }),
        ];
        input.focus_handle(cx).focus(window, cx);
        self.float = Float::Rich(RichFloat::Grid(GridFloat {
            start,
            place,
            input,
            refused: None,
            _subscriptions: subscriptions,
        }));
        self.toolbar_dismissed = true;
        self.start_revealing_float(true);
        cx.notify();
    }

    /// The table a caret moving from `head` to `target` would pass over, which it enters instead: where the table
    /// starts, and the place the caret lands in.
    pub(in crate::editor) fn table_on_the_way(&self, head: usize, target: usize) -> Option<(usize, Place)> {
        if self.source || head == target {
            return None;
        }
        let forward = target > head;
        let (low, high) = (head.min(target), head.max(target));
        let blocks = self.blocks.as_slice();
        let passed = |atom: &&crate::editor::rich::Atom| {
            let range = blocks[atom.first].range.start..blocks[atom.last].range.end;
            matches!(atom.kind, AtomKind::Table(Ok(_))) && low <= range.start && range.start < high
        };
        let mut tables = self.rich.atoms.iter().filter(passed);
        let atom = if forward { tables.next() } else { tables.next_back() }?;
        let AtomKind::Table(Ok(grid)) = &atom.kind else {
            return None;
        };
        let place = if forward {
            Place::Cell(0, 0)
        } else {
            Place::Cell(grid.rows.len() - 1, 0)
        };
        Some((blocks[atom.first].range.start, place))
    }

    /// Takes the caret out of the table, to the text after it or before it, writing what the field holds.
    fn leave_grid(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some(start) = self.grid_float().map(GridFloat::first_byte) else {
            return;
        };
        self.float = Float::None;
        window.focus(&self.focus, cx);
        let Some(atom) = self.rich_atom(self.blocks.index_at(start)).cloned() else {
            return;
        };
        let blocks = self.blocks.as_slice();
        let edge = if forward {
            blocks[atom.last].range.end.min(self.session.active_buffer().len())
        } else {
            blocks[atom.first].range.start
        };
        let caret = self.skip_hidden(edge, forward);
        self.set_selection(Selection::caret(caret), false, window, cx);
        self.toolbar_dismissed = true;
    }

    /// Writes what the field holds to its cell or the caption. Returns whether it could be.
    fn grid_write_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(float) = self.grid_float() else {
            return false;
        };
        let (start, place) = (float.start, float.place);
        let new = float.input.read(cx).text().to_owned();
        let Some(grid) = self.grid_at(start) else {
            return false;
        };
        let text = self.session.active_buffer().text();
        let edits = match place {
            Place::Caption => {
                let caption = Caption {
                    text: new,
                    ..grid.caption.clone()
                };
                grid.set_caption(text, &caption).into_iter().collect()
            }
            Place::Cell(row, cell) => {
                let unchanged = (grid.rows.get(row))
                    .and_then(|cells| cells.get(cell))
                    .is_some_and(|old| old.text == new);
                if unchanged {
                    return true;
                }
                match grid.set_cell(text, row, cell, &new) {
                    Ok(edits) => edits,
                    Err(why) => {
                        self.grid_refused(crate::t!(why).to_owned(), cx);
                        return false;
                    }
                }
            }
        };
        edits.is_empty() || self.grid_write(edits, window, cx)
    }

    /// Writes edits as one undo step. What is refused stays out, and the table says why.
    fn grid_write(&mut self, edits: Vec<Edit>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let active = self.session.active();
        match self.write_rich(active, edits, None, window, cx) {
            Ok(()) => {
                if let Some(float) = self.grid_float_mut() {
                    float.refused = None;
                }
                cx.notify();
                true
            }
            Err(why) => {
                self.grid_refused(why, cx);
                false
            }
        }
    }

    fn grid_refused(&mut self, why: String, cx: &mut Context<Self>) {
        if let Some(float) = self.grid_float_mut() {
            float.refused = Some(why);
        }
        cx.notify();
    }

    /// Runs a change the table works out, keeping the caret in its cell when it can.
    fn grid_change(
        &mut self,
        change: impl FnOnce(&Grid, &str, usize, usize) -> Result<Vec<Edit>, Key>,
        to: impl FnOnce(usize, usize) -> (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some((start, row, cell)) = self.grid_cell() else {
            return;
        };
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        let text = self.session.active_buffer().text();
        match change(&grid, text, row, cell) {
            Ok(edits) => {
                if self.grid_write(edits, window, cx) {
                    let (row, cell) = to(row, cell);
                    self.open_table_place(start, Place::Cell(row, cell), window, cx);
                }
            }
            Err(why) => self.grid_refused(crate::t!(why).to_owned(), cx),
        }
    }

    fn grid_set_align(&mut self, align: Option<Align>, window: &mut Window, cx: &mut Context<Self>) {
        self.grid_change(
            |grid, text, row, cell| {
                let column = column_of(grid, row, cell);
                // With none named, the key steps to the next one. A list table has no word for the theme's own.
                let next = align.unwrap_or_else(|| match grid.aligns.get(column).copied().unwrap_or_default() {
                    Align::Right if !grid.is_list() => Align::Default,
                    Align::Default | Align::Right => Align::Left,
                    Align::Left => Align::Center,
                    Align::Center => Align::Right,
                });
                grid.set_align(text, column, next)
            },
            |row, cell| (row, cell),
            window,
            cx,
        );
    }

    fn grid_set_fill(&mut self, fill: Option<bool>, window: &mut Window, cx: &mut Context<Self>) {
        self.grid_change(
            |grid, text, row, cell| {
                let column = column_of(grid, row, cell);
                let fills = grid.fills(text, column).ok_or(Key::TableNoColumnThere)?;
                grid.set_fill(text, column, fill.unwrap_or(!fills))
            },
            |row, cell| (row, cell),
            window,
            cx,
        );
    }

    /// Sets the table's placement, or with none named steps to the next one.
    fn grid_set_placement(&mut self, placement: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        self.grid_change(
            |grid, text, _, _| {
                let now = grid.placement(text);
                let placement = placement.unwrap_or(match now.as_str() {
                    "" => "top",
                    "top" => "bottom",
                    _ => "",
                });
                grid.set_placement(text, placement)
            },
            |row, cell| (row, cell),
            window,
            cx,
        );
    }

    /// Puts the caret in the caption, which is written first when the table has none.
    fn grid_edit_caption(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some((start, ..)) = self.grid_cell() else {
            return;
        };
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        if grid.caption.text.is_empty() {
            let caption = Caption {
                text: crate::t!(Key::CaptionLabel).to_owned(),
                ..grid.caption.clone()
            };
            let text = self.session.active_buffer().text();
            let edits: Vec<Edit> = grid.set_caption(text, &caption).into_iter().collect();
            if !self.grid_write(edits, window, cx) {
                return;
            }
        }
        self.open_table_place(start, Place::Caption, window, cx);
    }

    fn grid_set_label(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.grid_change(
            |grid, text, _, _| {
                let label = if on {
                    label_from(&grid.caption.text)
                } else {
                    String::new()
                };
                if on && label.is_empty() {
                    return Err(Key::LabelNeedsCaption);
                }
                let caption = Caption {
                    label,
                    ..grid.caption.clone()
                };
                Ok(grid.set_caption(text, &caption).into_iter().collect())
            },
            |row, cell| (row, cell),
            window,
            cx,
        );
    }

    fn grid_step_cell(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some(float) = self.grid_float() else {
            return;
        };
        let (start, place) = (float.start, float.place);
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        let Place::Cell(row, cell) = place else {
            // From the caption Tab goes on into the table.
            if forward {
                self.open_table_place(start, Place::Cell(0, 0), window, cx);
            }
            return;
        };
        if forward {
            if cell + 1 < grid.rows[row].len() {
                self.open_table_place(start, Place::Cell(row, cell + 1), window, cx);
            } else if row + 1 < grid.rows.len() {
                self.open_table_place(start, Place::Cell(row + 1, 0), window, cx);
            } else {
                self.grid_change(
                    |grid, text, row, _| grid.add_row(text, row),
                    |row, _| (row + 1, 0),
                    window,
                    cx,
                );
            }
        } else if cell > 0 {
            self.open_table_place(start, Place::Cell(row, cell - 1), window, cx);
        } else if row > 0 {
            let last = grid.rows[row - 1].len().saturating_sub(1);
            self.open_table_place(start, Place::Cell(row - 1, last), window, cx);
        } else {
            self.open_table_place(start, Place::Caption, window, cx);
        }
    }

    /// Return, Up, and Down in a one-line cell: the same column in the next or previous row, and past the table's
    /// first or last row the text before or after it.
    fn grid_step_row(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some(float) = self.grid_float() else {
            return;
        };
        let (start, place) = (float.start, float.place);
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        match place {
            Place::Caption if down => self.open_table_place(start, Place::Cell(0, 0), window, cx),
            Place::Caption => self.leave_grid(false, window, cx),
            Place::Cell(row, _) if down && row + 1 >= grid.rows.len() => self.leave_grid(true, window, cx),
            Place::Cell(row, cell) if down => self.open_table_place(start, Place::Cell(row + 1, cell), window, cx),
            Place::Cell(0, _) if grid.caption.text.is_empty() => self.leave_grid(false, window, cx),
            Place::Cell(0, _) => self.open_table_place(start, Place::Caption, window, cx),
            Place::Cell(row, cell) => self.open_table_place(start, Place::Cell(row - 1, cell), window, cx),
        }
    }

    /// Writes what the field holds and takes the caret out. Something refused keeps the caret in its cell, with the
    /// reason.
    pub(super) fn commit_grid(&mut self, float: GridFloat, window: &mut Window, cx: &mut Context<Self>) {
        self.float = Float::Rich(RichFloat::Grid(float));
        if self.grid_write_cell(window, cx) {
            self.float = Float::None;
        }
    }

    fn grid_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = self.grid_float().map(GridFloat::first_byte) else {
            return;
        };
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        let Some(atom) = self.rich_atom(self.blocks.index_at(start)).cloned() else {
            return;
        };
        let blocks = self.blocks.as_slice();
        let end = blocks[atom.last].range.start
            + self.session.active_buffer().text()[blocks[atom.last].range.clone()]
                .trim_end_matches(['\n', '\r'])
                .len();
        let edit = grid.removal(self.session.active_buffer().text(), start..end);
        self.float = Float::None;
        window.focus(&self.focus, cx);
        let active = self.session.active();
        if let Err(why) = self.write_rich(active, vec![edit], None, window, cx) {
            self.notify_rich(start, why, false, cx);
        }
    }

    fn grid_mark(&mut self, mark: Mark, cx: &mut Context<Self>) {
        if let Some(field) = self.grid_float().map(|float| float.input.clone()) {
            self.mark_in_field(&field, mark, cx);
        }
    }

    /// Cmd+Z in the field undoes its own typing first, and then, with nothing typed, the last change to the table.
    fn grid_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(float) = self.grid_float() else {
            return;
        };
        let (start, place) = (float.start, float.place);
        let typed = float.input.read(cx).text().to_owned();
        let unchanged = self.grid_at(start).is_some_and(|grid| match place {
            Place::Caption => grid.caption.text == typed,
            Place::Cell(row, cell) => (grid.rows.get(row))
                .and_then(|cells| cells.get(cell))
                .is_some_and(|old| old.text == typed),
        });
        if unchanged {
            let restored = self.session.active_buffer_mut().undo();
            self.restore(restored, window, cx);
            self.autoscroll = false;
            self.open_table_place(start, place, window, cx);
            cx.stop_propagation();
        }
    }

    // The menu

    /// The rows of the table's menu for the cell the caret stands in.
    fn table_rows(&self, start: usize, row: usize, cell: usize) -> Vec<Row> {
        let Some(grid) = self.grid_at(start) else {
            return Vec::new();
        };
        let text = self.session.active_buffer().text();
        let column = column_of(&grid, row, cell);
        let align = grid.aligns.get(column).copied().unwrap_or_default();
        let fills = grid.fills(text, column);
        let placement = grid.placement(text);
        let table = "icons/block-table.svg";
        let mut rows = Vec::new();
        let mut add = |key: Key, icon: &'static str, on: bool, act: TableAct| {
            rows.push(Row {
                label: crate::t!(key).into(),
                on,
                icon,
                pick: Pick::Table { start, row, cell, act },
            });
        };
        add(Key::RowBelow, table, false, TableAct::RowBelow);
        add(Key::RowUp, "icons/move-up.svg", false, TableAct::RowUp);
        add(Key::RowDown, "icons/move-down.svg", false, TableAct::RowDown);
        add(Key::RowDelete, "icons/trash.svg", false, TableAct::RowDelete);
        add(Key::ColumnAfter, table, false, TableAct::ColumnAfter);
        add(Key::ColumnLeft, "icons/chevron-left.svg", false, TableAct::ColumnLeft);
        add(
            Key::ColumnRight,
            "icons/chevron-right.svg",
            false,
            TableAct::ColumnRight,
        );
        add(Key::ColumnDelete, "icons/trash.svg", false, TableAct::ColumnDelete);
        if !grid.is_list() {
            let on = align == Align::Default;
            add(
                Key::AlignDefault,
                "icons/align-default.svg",
                on,
                TableAct::Align(Align::Default),
            );
        }
        for (key, icon, choice) in [
            (Key::AlignLeft, "icons/align-left.svg", Align::Left),
            (Key::AlignCenter, "icons/align-center.svg", Align::Center),
            (Key::AlignRight, "icons/align-right.svg", Align::Right),
        ] {
            add(key, icon, align == choice, TableAct::Align(choice));
        }
        if let Some(fills) = fills {
            add(Key::ColumnFits, table, !fills, TableAct::Fill(false));
            add(Key::ColumnFills, table, fills, TableAct::Fill(true));
            add(Key::MergeRight, table, false, TableAct::SpanMore);
            add(Key::SplitCell, table, false, TableAct::SpanLess);
        }
        add(Key::CaptionLabel, "icons/block-text.svg", false, TableAct::Caption);
        if !grid.caption.text.is_empty() {
            let labelled = !grid.caption.label.is_empty();
            add(
                Key::TableLabel,
                "icons/block-reference.svg",
                labelled,
                TableAct::Label(!labelled),
            );
        }
        for (key, value) in [
            (Key::PlaceHere, ""),
            (Key::PlaceTop, "top"),
            (Key::PlaceBottom, "bottom"),
        ] {
            add(
                key,
                "icons/directive-page-break.svg",
                placement == value,
                TableAct::Placement(value),
            );
        }
        add(Key::ShowSource, "icons/source.svg", false, TableAct::Source);
        add(Key::DeleteTable, "icons/trash.svg", false, TableAct::Delete);
        rows
    }

    /// Opens the table's menu at the cell the caret stands in.
    fn open_table_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start, row, cell)) = self.grid_cell() else {
            return;
        };
        // Where the cell stands now: writing it lays the table out again, a frame later.
        let anchor = self.place_bounds(start, Place::Cell(row, cell), window);
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let rows = self.table_rows(start, row, cell);
        // The keys go back to the text, which the menu's keys are bound in.
        window.focus(&self.focus, cx);
        self.open_pick(rows, anchor, cx);
    }

    /// Carries out a row of the table's menu, with the caret back in the cell it was opened from.
    pub(in crate::editor) fn run_table_act(
        &mut self,
        (start, row, cell): (usize, usize, usize),
        act: TableAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_table_place(start, Place::Cell(row, cell), window, cx);
        let same = |row: usize, cell: usize| (row, cell);
        match act {
            TableAct::RowBelow => self.grid_change(
                |grid, text, row, _| grid.add_row(text, row),
                |row, cell| (row + 1, cell),
                window,
                cx,
            ),
            TableAct::RowDelete => self.grid_change(|grid, text, row, _| grid.remove_row(text, row), same, window, cx),
            TableAct::RowUp => self.grid_change(
                |grid, text, row, _| match row {
                    0 => Err(Key::TableNoRowThere),
                    row => grid.move_row_down(text, row - 1),
                },
                |row, cell| (row.saturating_sub(1), cell),
                window,
                cx,
            ),
            TableAct::RowDown => self.grid_change(
                |grid, text, row, _| grid.move_row_down(text, row),
                |row, cell| (row + 1, cell),
                window,
                cx,
            ),
            TableAct::ColumnAfter => self.grid_change(
                |grid, text, row, cell| grid.add_column(text, Some(column_of(grid, row, cell + 1) - 1)),
                |row, cell| (row, cell + 1),
                window,
                cx,
            ),
            TableAct::ColumnDelete => self.grid_change(
                |grid, text, row, cell| grid.remove_column(text, column_of(grid, row, cell)),
                same,
                window,
                cx,
            ),
            TableAct::ColumnLeft => self.grid_change(
                |grid, text, row, cell| match column_of(grid, row, cell) {
                    0 => Err(Key::TableNoColumnThere),
                    column => grid.move_column_right(text, column - 1),
                },
                |row, cell| (row, cell.saturating_sub(1)),
                window,
                cx,
            ),
            TableAct::ColumnRight => self.grid_change(
                |grid, text, row, cell| grid.move_column_right(text, column_of(grid, row, cell)),
                |row, cell| (row, cell + 1),
                window,
                cx,
            ),
            TableAct::Align(align) => self.grid_set_align(Some(align), window, cx),
            TableAct::Fill(fill) => self.grid_set_fill(Some(fill), window, cx),
            TableAct::SpanMore => self.grid_change(
                |grid, text, row, cell| grid.merge_right(text, row, cell),
                same,
                window,
                cx,
            ),
            TableAct::SpanLess => {
                self.grid_change(|grid, text, row, cell| grid.split(text, row, cell), same, window, cx)
            }
            TableAct::Placement(placement) => self.grid_set_placement(Some(placement), window, cx),
            TableAct::Caption => self.grid_edit_caption(window, cx),
            TableAct::Label(on) => self.grid_set_label(on, window, cx),
            TableAct::Source => {
                self.float = Float::None;
                window.focus(&self.focus, cx);
                self.toggle_source(window, cx);
            }
            TableAct::Delete => self.grid_delete(window, cx),
        }
    }

    // The field

    /// Where a place of the table at `start` stands in the window, as last laid out.
    fn place_bounds(&mut self, start: usize, place: Place, window: &Window) -> Option<Bounds<Pixels>> {
        let index = self.blocks.index_at(start);
        let top = self.block_top(index)?;
        let left = self.block_left(index);
        let layout = self.layout_block(index, window);
        let Some(Whole::Grid(shape)) = layout.whole.as_deref() else {
            return None;
        };
        let bounds = shape.bounds_of(place)?;
        Some(Bounds::new(point(left, top) + bounds.origin, bounds.size))
    }

    /// The field in its cell for this frame, with the handle of the table's menu beside the table.
    pub(super) fn grid_element(
        &mut self,
        start: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Point<Pixels>)> {
        let float = self.grid_float()?;
        let (place, input, refused) = (float.place, float.input.clone(), float.refused.clone());
        let is_list = self.grid_at(start)?.is_list();
        let bounds = self.place_bounds(start, place, window)?;
        let index = self.blocks.index_at(start);
        let table_right = self.block_left(index) + self.block_width(index);
        let tokens = Tokens::get(cx).clone();
        let padding_y = if place == Place::Caption {
            gpui::px(0.)
        } else {
            tokens::CELL_Y
        };
        let padding_x = if place == Place::Caption {
            gpui::px(0.)
        } else {
            tokens::CELL_X
        };
        let field = div()
            .id("table-cell")
            .key_context(CONTEXT)
            .occlude()
            .w(bounds.size.width)
            .min_h(bounds.size.height)
            .px(padding_x)
            .py(padding_y)
            .bg(tokens.raised)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.commit_rich_float(window, cx)))
            .capture_action(cx.listener(move |this, _: &input::Up, window, cx| {
                if !is_list || place == Place::Caption {
                    this.grid_step_row(false, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(move |this, _: &input::Down, window, cx| {
                if !is_list || place == Place::Caption {
                    this.grid_step_row(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Escape, window, cx| {
                this.leave_grid(true, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Undo, window, cx| this.grid_undo(window, cx)))
            .on_action(cx.listener(|this, _: &NextField, window, cx| this.grid_step_cell(true, window, cx)))
            .on_action(cx.listener(|this, _: &PreviousField, window, cx| this.grid_step_cell(false, window, cx)))
            .on_action(cx.listener(|this, _: &StepColumnAlign, window, cx| this.grid_set_align(None, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleColumnWidth, window, cx| this.grid_set_fill(None, window, cx)))
            .on_action(cx.listener(|this, _: &StepPlacement, window, cx| this.grid_set_placement(None, window, cx)))
            .on_action(cx.listener(|this, _: &TableMenu, window, cx| this.open_table_menu(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleBold, _, cx| this.grid_mark(Mark::Bold, cx)))
            .on_action(cx.listener(|this, _: &ToggleItalic, _, cx| this.grid_mark(Mark::Italic, cx)))
            .on_action(cx.listener(|this, _: &ToggleUnderline, _, cx| this.grid_mark(Mark::Underline, cx)))
            .on_action(cx.listener(|this, _: &ToggleStrike, _, cx| this.grid_mark(Mark::Strike, cx)))
            .child(input)
            .child(super::bounds_probe(self.float_bounds.clone()))
            // The handle of the table's menu, in the margin right of the table, level with the caret's row.
            .child(
                div()
                    .id("table-menu")
                    .button(&tokens, crate::t!(Key::TableMenu))
                    .absolute()
                    .left(table_right - bounds.left() + tokens::SPACE_1)
                    .top_0()
                    .size(tokens::SMALL_BUTTON_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::CHIP_RADIUS)
                    .cursor_pointer()
                    .hover(|handle| handle.bg(tokens.raised))
                    .tooltip(crate::editor::float::tooltip(format!(
                        "{}  \u{2303}\u{21a9}",
                        crate::t!(Key::TableMenu)
                    )))
                    .on_click(cx.listener(|this, _, window, cx| this.open_table_menu(window, cx)))
                    .child(
                        svg()
                            .path("icons/more.svg")
                            .size(tokens::ICON_SIZE)
                            .text_color(tokens.text_faint),
                    ),
            )
            // What was refused, in the engine's words, under the cell.
            .children(refused.map(|why| {
                div()
                    .absolute()
                    .left_0()
                    .top(bounds.size.height + tokens::SPACE_1)
                    .w(tokens::NOTE_CARD_WIDTH)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.accent)
                    .child(why)
            }))
            .into_any_element();
        Some((field, bounds.origin))
    }
}

#[cfg(test)]
mod tests {
    use super::label_from;

    #[test]
    fn a_label_is_made_from_the_captions_first_words() {
        assert_eq!(
            label_from("Two events in the history of typesetting."),
            "two-events-in-the"
        );
        assert_eq!(label_from("Kosten für Satz"), "kosten-für-satz");
        assert_eq!(label_from(" ... "), "");
    }
}
