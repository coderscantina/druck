//! A region as one thing, as a block is in Notion. The pointer in a region shows a grip at its bracket, and a click on
//! the grip or the name selects it and opens its menu. Escape climbs from the caret to its paragraph, then to the
//! regions around it. A selected region moves, copies, and goes whole, and a block steps across a region's edge as one
//! move.

use std::ops::Range;

use gpui::{ClipboardItem, Context, Pixels, Point, Window};

use super::{
    Edge, Kind, Menu, MoveBlockDown, MoveBlockUp, RegionCopy, RegionCut, RegionDelete, RegionDown, RegionDuplicate,
    RegionEnter, RegionMenu, RegionOut, RegionUp, edits, structure,
};
use crate::editor::EditorView;
use crate::editor::commands::{Command, applied, moves};
use crate::source::{BlockKind, Blocks, Selection};
use crate::tokens;

/// How many places a step tries before it gives up, when the engine would refuse the nearer ones.
const STEP_TRIES: usize = 16;

/// A row of a region's menu.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Row {
    TurnInto(Vec<Kind>),
    FullWidth,
    Unwrap,
    Duplicate,
    MoveUp,
    MoveDown,
    Delete,
}

impl EditorView {
    /// The selected region, as an index into the directives read from the text.
    pub(in crate::editor) fn selected_region(&self) -> Option<usize> {
        self.region_opening_at(self.directives.selected?)
    }

    /// The region a grip can select whose opening line starts at `at`.
    fn region_opening_at(&self, at: usize) -> Option<usize> {
        if self.source || at > self.session.active_buffer().len() {
            return None;
        }
        let block = self.blocks.index_at(at);
        let region = self.directives.structure.fence(block)?;
        let found = &self.directives.structure.regions[region];
        let selectable = found.open == block && found.kind.has_body() && found.kind != Kind::Table;
        (selectable && self.blocks.as_slice()[block].range.start == at).then_some(region)
    }

    /// Where the text of `region` starts: its first block's, past a heading's marker.
    fn region_text_start(&self, region: usize) -> usize {
        let found = &self.directives.structure.regions[region];
        let list = self.blocks.as_slice();
        let first = (found.open + 1..found.close)
            .find(|&at| list[at].kind != BlockKind::Blank && !self.fence_at_rest(at))
            .unwrap_or(found.open);
        let text = self.session.active_buffer().text();
        crate::editor::selection::past_heading_marker(text, &self.blocks, list[first].range.start)
    }

    /// Selects `region`, with its menu open or not. `origin` is where the caret stood inside it.
    pub(in crate::editor) fn select_region(
        &mut self,
        region: usize,
        menu: bool,
        origin: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.blocks.as_slice()[self.directives.structure.regions[region].open]
            .range
            .start;
        let caret = self.region_text_start(region);
        self.set_selection(Selection::caret(caret), false, window, cx);
        self.directives.selected = Some(open);
        self.directives.origin = origin;
        self.directives.menu = menu.then(Menu::default);
        self.toolbar_dismissed = true;
        cx.notify();
    }

    /// Forgets the selected region and its menu, as any move of the caret does.
    pub(in crate::editor) fn release_region(&mut self) {
        self.directives.selected = None;
        self.directives.origin = None;
        self.directives.menu = None;
    }

    /// Runs a command of a selected region, then selects the region whose opening line starts at `reselect`.
    fn run_region_command(
        &mut self,
        command: Command,
        reselect: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_command(command, window, cx);
        if let Some(region) = reselect.and_then(|at| self.region_opening_at(at)) {
            self.select_region(region, false, None, window, cx);
        }
    }

    // The menu

    /// The rows of `region`'s menu, in their order.
    pub(super) fn region_rows(&self, region: usize) -> Vec<Row> {
        let text = self.session.active_buffer().text();
        let structure = &self.directives.structure;
        let mut rows = Vec::new();
        let kinds = edits::kinds_offered(text, &self.blocks, structure, region);
        if kinds.len() > 1 {
            rows.push(Row::TurnInto(kinds));
        }
        if self.full_width_command(region).is_some() {
            rows.push(Row::FullWidth);
        }
        rows.extend([Row::Unwrap, Row::Duplicate, Row::MoveUp, Row::MoveDown, Row::Delete]);
        rows
    }

    /// What Full width from here writes: the part of `region`'s columns where the caret stood when the keys selected it.
    fn full_width_command(&self, region: usize) -> Option<Command> {
        let origin = self.directives.origin?;
        let structure = &self.directives.structure;
        let found = &structure.regions[region];
        let list = self.blocks.as_slice();
        let inside = list[found.open].range.end <= origin && origin <= list[found.close].range.start;
        if !inside || found.kind != Kind::Columns {
            return None;
        }
        let text = self.session.active_buffer().text();
        let block = self.blocks.index_at(origin);
        edits::full_width_at(text, &self.blocks, structure, block, Selection::caret(origin))
    }

    /// Carries out `row` of the selected region's menu, with the kind chosen from Turn into.
    pub(super) fn act_on_region(&mut self, row: &Row, kind: Option<Kind>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.selected_region() else {
            return;
        };
        let text = self.session.active_buffer().text();
        let structure = &self.directives.structure;
        let open = self.blocks.as_slice()[structure.regions[region].open].range.start;
        let caret = Selection::caret(open);
        match row {
            Row::TurnInto(_) => {
                let command =
                    kind.and_then(|kind| edits::change_kind(text, &self.blocks, structure, region, kind, caret));
                if let Some(command) = command {
                    self.run_region_command(command, Some(open), window, cx);
                }
            }
            Row::FullWidth => {
                if let Some(command) = self.full_width_command(region) {
                    self.run_region_command(command, Some(open), window, cx);
                }
            }
            Row::Unwrap => {
                if let Some(command) = edits::unwrap(text, &self.blocks, structure, region) {
                    self.run_command(command, window, cx);
                }
            }
            Row::Duplicate => self.duplicate_region(window, cx),
            Row::MoveUp => self.step_region(false, window, cx),
            Row::MoveDown => self.step_region(true, window, cx),
            Row::Delete => self.delete_region(window, cx),
        }
        cx.notify();
    }

    // Escape

    /// Escape in the text, after floats and completion: the caret's paragraph is selected, and a paragraph selected so
    /// gives way to the region around it. Returns whether it did either.
    pub(in crate::editor) fn escape_selects(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.source {
            return false;
        }
        let text = self.session.active_buffer().text();
        if self.selection.is_empty() {
            let block = self.blocks.index_at(self.selection.head);
            let found = &self.blocks.as_slice()[block];
            if found.kind == BlockKind::Blank || self.takes_no_caret(block) {
                return false;
            }
            let start = crate::editor::selection::past_heading_marker(text, &self.blocks, found.range.start);
            let end = found.range.start + text[found.range.clone()].trim_end_matches(['\n', '\r']).len();
            if start >= end {
                return false;
            }
            self.set_selection(
                Selection {
                    anchor: start,
                    head: end,
                },
                false,
                window,
                cx,
            );
            self.toolbar_dismissed = true;
            return true;
        }
        let range = self.selection.range();
        let block = self.blocks.index_at(range.start);
        let found = &self.blocks.as_slice()[block];
        let end = found.range.start + text[found.range.clone()].trim_end_matches(['\n', '\r']).len();
        let whole = range.end == end
            && range.start == crate::editor::selection::past_heading_marker(text, &self.blocks, found.range.start);
        let Some(region) = whole.then(|| self.region_around(block)).flatten() else {
            return false;
        };
        self.select_region(region, false, Some(range.start), window, cx);
        true
    }

    /// The innermost region a grip can select that holds block `index`.
    fn region_around(&self, index: usize) -> Option<usize> {
        (self.directives.structure.regions.iter().enumerate())
            .filter(|(_, region)| region.kind.has_body() && region.kind != Kind::Table)
            .filter(|(_, region)| region.open < index && index < region.close)
            .max_by_key(|(_, region)| region.depth)
            .map(|(at, _)| at)
    }

    // A selected region's keys

    pub(in crate::editor) fn region_out(&mut self, _: &RegionOut, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.selected_region() else {
            return;
        };
        let parent = self.directives.structure.regions[region]
            .parent
            .filter(|&parent| self.directives.structure.regions[parent].kind.has_body());
        let origin = self.directives.origin;
        match parent {
            Some(parent) => self.select_region(parent, false, origin, window, cx),
            None => {
                let caret = origin.unwrap_or_else(|| self.region_text_start(region));
                self.set_selection(Selection::caret(caret), false, window, cx);
            }
        }
    }

    pub(in crate::editor) fn region_enter(&mut self, _: &RegionEnter, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(region) = self.selected_region() {
            let caret = self.region_text_start(region);
            self.set_selection(Selection::caret(caret), false, window, cx);
        }
    }

    pub(in crate::editor) fn region_menu(&mut self, _: &RegionMenu, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_region().is_some() {
            self.directives.menu = Some(Menu::default());
            cx.notify();
        }
    }

    pub(in crate::editor) fn region_up(&mut self, _: &RegionUp, window: &mut Window, cx: &mut Context<Self>) {
        self.step_region(false, window, cx);
    }

    pub(in crate::editor) fn region_down(&mut self, _: &RegionDown, window: &mut Window, cx: &mut Context<Self>) {
        self.step_region(true, window, cx);
    }

    pub(in crate::editor) fn region_duplicate(
        &mut self,
        _: &RegionDuplicate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.duplicate_region(window, cx);
    }

    pub(in crate::editor) fn region_delete(&mut self, _: &RegionDelete, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_region(window, cx);
    }

    pub(in crate::editor) fn region_copy(&mut self, _: &RegionCopy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(region) = self.selected_region() {
            let range = edits::region_range(&self.blocks, &self.directives.structure, region);
            let text = self.session.active_buffer().text()[range].to_owned();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(in crate::editor) fn region_cut(&mut self, _: &RegionCut, window: &mut Window, cx: &mut Context<Self>) {
        self.region_copy(&RegionCopy, window, cx);
        self.delete_region(window, cx);
    }

    /// Option+Down in the text steps into a paragraph's pins, unless a region is selected, which it moves.
    pub(in crate::editor) fn region_takes_option_down(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.selected_region().is_none() {
            return false;
        }
        self.step_region(true, window, cx);
        true
    }

    fn duplicate_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.selected_region() else {
            return;
        };
        let text = self.session.active_buffer().text();
        if let Some(command) = edits::duplicate_region(text, &self.blocks, &self.directives.structure, region) {
            let at = command.selection.head;
            self.run_region_command(command, Some(at), window, cx);
        }
    }

    fn delete_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.selected_region() else {
            return;
        };
        let text = self.session.active_buffer().text();
        if let Some(command) = edits::delete_region(text, &self.blocks, &self.directives.structure, region) {
            self.run_command(command, window, cx);
        }
    }

    fn step_region(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.selected_region() else {
            return;
        };
        let found = &self.directives.structure.regions[region];
        let open = self.blocks.as_slice()[found.open].range.start;
        if let Some(command) = self.step_command(found.open..found.close + 1, down, Selection::caret(open)) {
            let at = command.selection.head;
            self.run_region_command(command, Some(at), window, cx);
        }
    }

    // Moving blocks a step

    pub(in crate::editor) fn move_block_up(&mut self, _: &MoveBlockUp, window: &mut Window, cx: &mut Context<Self>) {
        self.step_block(false, window, cx);
    }

    pub(in crate::editor) fn move_block_down(
        &mut self,
        _: &MoveBlockDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_block(true, window, cx);
    }

    /// Moves the caret's block a step: a table or figure goes whole, and a fence line counts as a step of its own.
    fn step_block(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.source {
            return;
        }
        let block = self.blocks.index_at(self.selection.head);
        let text = self.session.active_buffer().text();
        let found = &self.blocks.as_slice()[block];
        let range = match self.rich_atom(block) {
            Some(atom) => atom.first..atom.last + 1,
            None if moves::can_move(found.kind, &text[found.range.clone()]) => block..block + 1,
            None => return,
        };
        if let Some(command) = self.step_command(range, down, self.selection) {
            self.run_command(command, window, cx);
        }
    }

    /// The nearest move of blocks `range` a step up or down that the engine takes.
    fn step_command(&self, range: Range<usize>, down: bool, selection: Selection) -> Option<Command> {
        let text = self.session.active_buffer().text();
        self.step_places(range.clone(), down).into_iter().find_map(|before| {
            moves::move_range(text, &self.blocks, range.clone(), before, selection)
                .filter(|command| self.keeps_directives(command))
        })
    }

    /// The places blocks `range` can go, nearest first, as the block each would go above. Each shown block, table or
    /// figure, and each fence line, is one step.
    fn step_places(&self, range: Range<usize>, down: bool) -> Vec<usize> {
        let list = self.blocks.as_slice();
        let count = list.len();
        let skipped =
            |at: usize| list[at].kind == BlockKind::Blank || (self.takes_no_room(at) && !self.fence_at_rest(at));
        let mut places = Vec::new();
        if down {
            let mut at = range.end;
            while places.len() < STEP_TRIES {
                while at < count && skipped(at) {
                    at += 1;
                }
                if at >= count {
                    break;
                }
                let end = self.rich_atom(at).map_or(at, |atom| atom.last);
                places.push(end + 1);
                at = end + 1;
            }
        } else {
            let mut at = range.start;
            while places.len() < STEP_TRIES {
                while at > 0 && skipped(at - 1) {
                    at -= 1;
                }
                if at == 0 {
                    break;
                }
                let start = self.rich_atom(at - 1).map_or(at - 1, |atom| atom.first);
                places.push(start);
                at = start;
            }
        }
        places
    }

    /// Whether the text after `command` keeps every directive the engine took before it.
    pub(in crate::editor) fn keeps_directives(&self, command: &Command) -> bool {
        let after = applied(self.session.active_buffer().text(), &command.edits);
        structure::read(&after, &Blocks::parse(&after)).regions.len() == self.directives.structure.regions.len()
    }

    // The pointer

    /// Follows the pointer: the region it is in shows its grip. Returns whether that changed.
    pub(in crate::editor) fn follow_region_hover(&mut self, position: Point<Pixels>, inside: bool) -> bool {
        let reach = self.column.text_left + self.column.text_width;
        let hover = (inside && !self.source && position.x < reach)
            .then(|| {
                (self.directives.spans.iter())
                    .filter(|span| span.top <= position.y && position.y < span.bottom)
                    .max_by_key(|span| span.depth)
                    .map(|span| span.region)
            })
            .flatten();
        let changed = hover != self.directives.hover;
        self.directives.hover = hover;
        changed
    }

    /// A press on a region's grip or name, which a release turns into its menu and a move into a drag. Returns whether
    /// it was one.
    pub(in crate::editor) fn region_mouse_down(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let hit = (self.directives.grips.iter())
            .chain(self.directives.names.iter())
            .find(|(bounds, _)| bounds.contains(&position))
            .copied();
        let Some((bounds, region)) = hit else {
            return false;
        };
        let Some(found) = self.directives.structure.regions.get(region).cloned() else {
            return false;
        };
        if !found.kind.has_body() || found.kind == Kind::Table {
            return false;
        }
        self.grab_region(found.open, found.close, position, bounds, cx);
        true
    }

    /// Where a dragged block or region lands near `position`: above the block `before`, and where fence lines stand
    /// between that block and the one shown above it, on the side of each that the pointer picks. At the text it goes
    /// into the region, and left of the text out of it.
    pub(in crate::editor) fn drop_place(&self, position: Point<Pixels>) -> (usize, Option<Edge>) {
        let target = self.drop_target(position.y);
        let list = self.blocks.as_slice();
        let mut gap = target.min(list.len());
        while gap > 0 && (list[gap - 1].kind == BlockKind::Blank || self.takes_no_room(gap - 1)) {
            gap -= 1;
        }
        let fences: Vec<usize> = (gap..target.min(list.len()))
            .filter(|&at| self.fence_at_rest(at))
            .collect();
        if fences.is_empty() {
            return (target, None);
        }
        let regions = &self.directives.structure.regions;
        let drawn = |region: &&structure::Region| region.kind.has_body() && region.kind != Kind::Table;
        let depth = |before: usize| {
            (regions.iter().filter(drawn))
                .filter(|region| region.open < before && before <= region.close)
                .count()
        };
        let into = position.x >= self.column.text_left - tokens::GUTTER_WIDTH * 0.5;
        let places: Vec<usize> = fences.iter().copied().chain([target]).collect();
        let best = if into {
            places.iter().map(|&at| depth(at)).max()
        } else {
            places.iter().map(|&at| depth(at)).min()
        }
        .unwrap_or(0);
        let ties: Vec<usize> = places.into_iter().filter(|&at| depth(at) == best).collect();
        let target_top = self
            .visible
            .iter()
            .find(|(index, _)| *index == target)
            .map(|(_, top)| *top);
        let upper = target_top.is_some_and(|top| position.y < top);
        let before = if upper { ties[0] } else { ties[ties.len() - 1] };
        // The region whose edge the drop names: the one it goes into, or the deepest one it stays out of.
        let named = if into && best > 0 {
            (regions.iter().enumerate())
                .filter(|(_, region)| drawn(region) && region.open < before && before <= region.close)
                .max_by_key(|(_, region)| region.depth)
        } else {
            (fences.iter())
                .filter_map(|&fence| self.directives.structure.fence(fence))
                .map(|region| (region, &regions[region]))
                .filter(|(_, region)| !(region.open < before && before <= region.close))
                .max_by_key(|(_, region)| region.depth)
        };
        let Some((region, found)) = named else {
            return (before, None);
        };
        let edge_into = into && best > 0;
        let x = if edge_into {
            self.block_left(before.min(list.len() - 1))
        } else {
            self.bracket_x(found.depth)
        };
        (
            before,
            Some(Edge {
                region,
                into: edge_into,
                x,
            }),
        )
    }

    /// Where a bracket `depth` regions deep stands across the window.
    pub(in crate::editor) fn bracket_x(&self, depth: usize) -> Pixels {
        self.column.text_left - tokens::GUTTER_WIDTH
            + tokens::BRACKET_INSET
            + tokens::BRACKET_STEP * depth.min(tokens::BRACKET_DEPTHS - 1) as f32
    }
}
