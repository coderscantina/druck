//! What the directives draw: brackets with their names in the left margin, a new page's sheet edges, the options under
//! a lit name, and the slash menu's ghost of the directive it would insert.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, BoxShadow, Context, Hsla, MouseButton, Pixels, Point, ShapedLine, SharedString, TextAlign,
    TextRun, TransformationMatrix, Window, div, fill, font, point, px, size, svg,
};

use super::edits::{self, Align};
use super::region::Row;
use super::strings::Key;
use super::structure::Fault;
use super::{
    FullWidthFromCaret, GhostOptions, GhostTable, Kind, OptionsAccept, OptionsBack, OptionsDown, OptionsLeft,
    OptionsRight, OptionsToggle, OptionsUnwrap, OptionsUp, Span,
};
use crate::editor::commands::slash::{Action, Standalone};
use crate::editor::element::{Frame, FrameBlock};
use crate::editor::float::Spot;
use crate::editor::rich::view::shapes::Whole;
use crate::editor::rich::{Atom, AtomKind};
use crate::editor::{EditorView, Float};
use crate::source::{BlockKind, Blocks};
use crate::tokens::{self, Tokens};

/// A bracket beside a directive's text, or a short line beside one standing alone.
pub(in crate::editor) struct Bracket {
    x: Pixels,
    top: Pixels,
    bottom: Pixels,
    /// Stands alone: a short line across instead of a bracket.
    tick: bool,
    name: Option<(ShapedLine, Point<Pixels>)>,
    lit: bool,
    /// A dragged block would drop into it.
    accent: bool,
    ghost: bool,
}

/// What the directives paint in one frame.
#[derive(Default)]
pub(in crate::editor) struct Marks {
    brackets: Vec<Bracket>,
    /// The gaps between two sheet edges, and whether each is a ghost.
    sheets: Vec<(Bounds<Pixels>, bool)>,
    /// The slash menu's ghost text, and a table it would insert.
    ghost_text: Option<(ShapedLine, Point<Pixels>, Pixels)>,
    ghost_whole: Option<(Rc<Whole>, Point<Pixels>)>,
    /// The grips that show, the accent washes with their tint, and the label of a drop across a region's edge.
    grips: Vec<Bounds<Pixels>>,
    /// What is wrong with a directive line the engine will not take, beside the line.
    repairs: Vec<(ShapedLine, Point<Pixels>)>,
    washes: Vec<(Bounds<Pixels>, f32)>,
    drop_label: Option<(ShapedLine, Point<Pixels>)>,
}

/// The slash menu's chosen directive, drawn into the text before Return inserts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::editor) struct Ghost {
    pub kind: Kind,
    /// The slash's block.
    pub block: usize,
    /// It wraps the slash's block, rather than writing a new block after it.
    pub around: bool,
}

/// The slash menu's directive entries and what each would insert.
pub(in crate::editor) fn ghost_kind(action: &Action) -> Option<Kind> {
    match action {
        Action::Wrap(name) => Kind::from_name(name),
        Action::Insert(Standalone::PageBreak) => Some(Kind::PageBreak),
        Action::Insert(Standalone::Toc) => Some(Kind::Toc),
        Action::Insert(Standalone::Bibliography) => Some(Kind::Bibliography),
        Action::Insert(Standalone::Index) => Some(Kind::Index),
        Action::Insert(Standalone::ListTable) => Some(Kind::Table),
        _ => None,
    }
}

fn ui_line(text: &str, color: Hsla, window: &Window) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: font(tokens::FONT_UI),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text.to_owned()), tokens::TEXT_SMALL, &[run], None)
}

impl EditorView {
    /// The ghost of the slash menu's chosen entry, while it is a directive.
    pub(in crate::editor) fn ghost(&self) -> Option<Ghost> {
        let (start, action) = self.slash_choice()?;
        let kind = ghost_kind(action)?;
        let text = self.session.active_buffer().text();
        let around = matches!(action, Action::Wrap(_))
            && crate::editor::commands::slash::wraps_block(text, &self.blocks, start..self.selection.head);
        Some(Ghost {
            kind,
            block: self.blocks.index_at(start),
            around,
        })
    }

    /// Whether the ghost of two columns around block `index` narrows it to a column.
    pub(super) fn ghost_narrows(&self, index: usize) -> bool {
        self.ghost()
            .is_some_and(|ghost| ghost.around && ghost.kind == Kind::Columns && ghost.block == index)
    }

    /// The room the ghost takes below block `index`, for what a new block of it would show.
    pub(in crate::editor) fn ghost_room(&mut self, index: usize, window: &Window) -> Pixels {
        let Some(ghost) = self.ghost().filter(|ghost| ghost.block == index && !ghost.around) else {
            return px(0.);
        };
        let gap = self
            .style
            .gap_between(Some(BlockKind::Paragraph), Some(BlockKind::Paragraph));
        let line = self.style.body_size * self.style.leading;
        gap + match ghost.kind {
            Kind::Table => self.ghost_grid(index, window).map_or(line, |whole| whole.height()),
            kind if !kind.has_body() => self.standalone_room(kind),
            _ => line,
        }
    }

    /// The ghost's table, shaped for the width of block `index`, from the options it has now.
    fn ghost_grid(&mut self, index: usize, window: &Window) -> Option<Rc<Whole>> {
        let options = self.directives.ghost_table.clone().unwrap_or_default().options;
        let width = self.block_width(index);
        if let Some((seen, seen_width, whole)) = &self.directives.ghost_grid
            && *seen == options
            && *seen_width == width
        {
            return Some(whole.clone());
        }
        let language = crate::shell::strings::language();
        let (template, _) = crate::editor::commands::slash::list_table(&options.opening_line(), "\n", language);
        let blocks = Blocks::parse(&template);
        let region = crate::editor::rich::table::regions(&template, &blocks)
            .into_iter()
            .next()?;
        let atom = Atom {
            first: region.first,
            last: region.last,
            kind: AtomKind::Table(region.grid.map(Rc::new)),
        };
        let whole = Rc::new(self.shape_atom(&atom, true, width, window));
        self.directives.ghost_grid = Some((options, width, whole.clone()));
        Some(whole)
    }

    /// The brackets, sheet gaps, and ghost of this frame's blocks, with the grip and wash of the region under the
    /// pointer, the selected one, and the one a dragged block would drop into. Also keeps where names and grips stand,
    /// for the pointer and the menu.
    pub(in crate::editor) fn directive_marks(
        &mut self,
        blocks: &[FrameBlock],
        indices: &[usize],
        window: &Window,
        cx: &App,
    ) -> Marks {
        let mut marks = Marks::default();
        self.directives.anchor = None;
        self.directives.names.clear();
        self.directives.grips.clear();
        self.directives.spans.clear();
        self.directives.repairs.clear();
        if self.source || indices.is_empty() {
            return marks;
        }
        let colors = Tokens::get(cx);
        let base = self.bracket_x(0);
        let slot = |depth: usize| base + tokens::BRACKET_STEP * depth.min(tokens::BRACKET_DEPTHS - 1) as f32;
        let name_right = slot(0) - tokens::BRACKET_NAME_GAP;
        let room_left = self.column.viewport.left() + tokens::SPACE_1;
        let (first, last) = (indices[0], indices[indices.len() - 1]);
        let selected = self.selected_region();
        let menu_open = self.directives.menu.is_some();
        let hover = self.directives.hover;
        let drop = (self.grab.as_ref())
            .filter(|grab| grab.dragging)
            .and_then(|grab| Some((grab.edge?, grab.before?)));
        let drop_y = self.drop_line().map(|line| line.top());
        let shown = |index: usize| indices.iter().position(|at| *at == index).map(|at| &blocks[at]);
        let gap_middle = self.column.text_left + self.directives.column_width + self.directives.column_gap * 0.5;
        let wash_left = self.column.text_left - tokens::SPACE_2;
        let wash_width = self.column.text_width + tokens::SPACE_2 * 2.;
        let regions = self.directives.structure.regions.clone();
        for (number, region) in regions.iter().enumerate() {
            if region.close < first || region.open > last {
                continue;
            }
            let span = (region.open..=region.close)
                .filter_map(shown)
                .filter(|block| block.layout.height > px(0.))
                .fold(None::<(Pixels, Pixels)>, |span, block| {
                    let (top, bottom) = (block.top, block.top + block.layout.height);
                    Some(span.map_or((top, bottom), |(a, b)| (a.min(top), b.max(bottom))))
                });
            let Some((top, bottom)) = span else {
                continue;
            };
            let body = region.kind != Kind::Table;
            // A group inside a second column has its bracket in the gap between the columns, where no name fits.
            let in_second = region.kind != Kind::Columns
                && (region.open..=region.close).any(|at| self.directives.shifted.contains_key(&at));
            if body {
                self.directives.spans.push(Span {
                    region: number,
                    top,
                    bottom,
                    depth: region.depth,
                    second: in_second,
                });
            }
            let is_selected = selected == Some(number);
            let into = drop.is_some_and(|(edge, _)| edge.into && edge.region == number);
            let is_lit = body && (hover == Some(number) || is_selected || into);
            let x = if in_second { gap_middle } else { slot(region.depth) };
            // A drop into the region stretches its bracket to the drop line.
            let (top, bottom) = match drop_y.filter(|_| into) {
                Some(y) => (top.min(y), bottom.max(y + tokens::KEY_BAR)),
                None => (top, bottom),
            };
            let grip_left = if in_second {
                gap_middle - tokens::REGION_GRIP * 0.5
            } else {
                name_right - tokens::REGION_GRIP
            };
            let grip = (body && (hover == Some(number) || is_selected)).then(|| {
                Bounds::new(
                    point(grip_left, top),
                    size(tokens::REGION_GRIP, tokens::TEXT_SMALL * 1.5),
                )
            });
            let name_end = grip.map_or(name_right, |grip| grip.left() - tokens::SPACE_1);
            let name = (!in_second)
                .then(|| {
                    let color = if is_lit { colors.text } else { colors.text_faint };
                    let line = ui_line(region.kind.label(), color.into(), window);
                    let left = name_end - line.width;
                    (left >= room_left).then(|| (line, point(left, top)))
                })
                .flatten();
            if let Some((line, at)) = &name
                && body
            {
                let bounds = Bounds::new(*at, size(line.width, tokens::TEXT_SMALL * 1.5));
                self.directives.names.push((bounds, number));
            }
            if let Some(grip) = grip {
                self.directives.grips.push((grip, number));
                marks.grips.push(grip);
            }
            if is_selected && menu_open {
                let at = grip.or_else(|| {
                    name.as_ref()
                        .map(|(line, at)| Bounds::new(*at, size(line.width, tokens::TEXT_SMALL * 1.5)))
                });
                self.directives.anchor = Some(at.unwrap_or(Bounds::new(point(x, top), size(px(1.), px(16.)))));
            }
            if is_selected {
                let wash = Bounds::new(point(wash_left, top), size(wash_width, bottom - top));
                marks.washes.push((wash, tokens::REGION_SELECTED_TINT));
            } else if into {
                let wash = Bounds::new(point(wash_left, top), size(wash_width, bottom - top));
                marks.washes.push((wash, tokens::REGION_DROP_TINT));
            }
            let standalone = !region.kind.has_body();
            if standalone
                && region.kind.starts_page()
                && let Some(block) = shown(region.open)
                && self.standalone_at_rest(region.open).is_some()
            {
                marks.sheets.push((
                    Bounds::new(
                        point(self.block_left(region.open), block.top),
                        size(block.width, block.layout.height),
                    ),
                    false,
                ));
            }
            marks.brackets.push(Bracket {
                x,
                top,
                bottom,
                tick: standalone,
                name,
                lit: is_lit,
                accent: into,
                ghost: false,
            });
        }
        for rejected in self.directives.structure.rejected.clone() {
            let Some(block) = shown(rejected.block).filter(|block| block.layout.height > px(0.)) else {
                continue;
            };
            let Some(line) = block.layout.lines.first() else {
                continue;
            };
            let reason = crate::t!(match rejected.fault {
                Fault::Unknown => Key::FaultUnknown,
                Fault::Refused => Key::FaultRefused,
                Fault::Unclosed => Key::FaultUnclosed,
                Fault::Stray => Key::FaultStray,
            });
            let hint = format!("{reason} \u{b7} {} \u{2318}\u{21a9}", crate::t!(Key::RepairHint));
            let shaped = ui_line(&hint, colors.accent.into(), window);
            let end = line.rows.first().map_or(px(0.), |row| line.x_in_row(row.end, 0));
            let at = point(block.left + line.x + end + tokens::SPACE_3, block.top + line.y);
            let bounds = Bounds::new(at, size(shaped.width, line.line_height));
            self.directives.repairs.push((bounds, rejected.block));
            marks.repairs.push((shaped, at));
        }
        // The drop's label names where the block lands, above the line's right end.
        if let (Some((edge, before)), Some(y)) = (drop, drop_y)
            && let Some(region) = regions.get(edge.region)
        {
            let word = crate::t!(if edge.into {
                Key::DropInto
            } else if before <= region.open {
                Key::DropBefore
            } else {
                Key::DropAfter
            });
            let line = ui_line(&format!("{word} {}", region.kind.label()), colors.accent.into(), window);
            let right = self.column.text_left + self.column.text_width;
            let at = point(right - line.width, y - tokens::TEXT_SMALL * 1.5 - tokens::SPACE_1);
            marks.drop_label = Some((line, at));
        }
        self.ghost_marks(&mut marks, blocks, indices, slot(0), name_right, room_left, window, cx);
        marks
    }

    #[allow(clippy::too_many_arguments)]
    fn ghost_marks(
        &mut self,
        marks: &mut Marks,
        blocks: &[FrameBlock],
        indices: &[usize],
        x: Pixels,
        name_right: Pixels,
        room_left: Pixels,
        window: &Window,
        cx: &App,
    ) {
        let Some(ghost) = self.ghost() else {
            return;
        };
        let Some(block) = indices.iter().position(|at| *at == ghost.block).map(|at| &blocks[at]) else {
            return;
        };
        let colors = Tokens::get(cx);
        let room = if ghost.around {
            px(0.)
        } else {
            self.ghost_room(ghost.block, window)
        };
        let gap = self
            .style
            .gap_between(Some(BlockKind::Paragraph), Some(BlockKind::Paragraph));
        let (top, bottom) = if ghost.around {
            (block.top, block.top + block.layout.height)
        } else {
            (
                block.top + block.layout.height - room + gap,
                block.top + block.layout.height,
            )
        };
        let left = self.block_left(ghost.block);
        if !ghost.around {
            match ghost.kind {
                Kind::Table => {
                    if let Some(whole) = self.ghost_grid(ghost.block, window) {
                        marks.ghost_whole = Some((whole, point(left, top)));
                    }
                }
                kind if kind.starts_page() => {
                    marks
                        .sheets
                        .push((Bounds::new(point(left, top), size(block.width, bottom - top)), true));
                }
                kind if kind.has_body() => {
                    let placeholder = crate::t!(crate::editor::strings::Key::PlaceholderText);
                    let run = TextRun {
                        len: placeholder.len(),
                        font: self.style.body.clone(),
                        color: self.style.ink.opacity(tokens::GHOST_OPACITY),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let line = window.text_system().shape_line(
                        SharedString::from(placeholder),
                        self.style.body_size,
                        &[run],
                        None,
                    );
                    marks.ghost_text = Some((line, point(left, top), self.style.body_size * self.style.leading));
                }
                _ => {}
            }
        }
        let faint: Hsla = colors.text_faint.into();
        let line = ui_line(ghost.kind.label(), faint.opacity(tokens::GHOST_OPACITY), window);
        let name_left = name_right - line.width;
        let name = (name_left >= room_left).then(|| (line, point(name_left, top)));
        let anchor = name
            .as_ref()
            .map_or(Bounds::new(point(x, top), size(px(1.), px(16.))), |(line, at)| {
                Bounds::new(*at, size(line.width, tokens::TEXT_SMALL * 1.5))
            });
        if ghost.kind == Kind::Table {
            self.directives.anchor = Some(anchor);
        }
        marks.brackets.push(Bracket {
            x,
            top,
            bottom,
            tick: !ghost.kind.has_body(),
            name,
            lit: false,
            accent: false,
            ghost: true,
        });
    }

    // Keys

    /// The selected region with its menu's rows, while its menu is open.
    fn open_menu(&self) -> Option<(usize, Vec<Row>, super::Menu)> {
        let menu = self.directives.menu?;
        let region = self.selected_region()?;
        Some((region, self.region_rows(region), menu))
    }

    fn options_move(&mut self, down: bool, cx: &mut Context<Self>) {
        if let Some(ghost) = self.directives.ghost_table.as_mut().filter(|ghost| ghost.keys) {
            ghost.row = 1 - ghost.row;
            cx.notify();
            return;
        }
        let Some((_, rows, mut menu)) = self.open_menu() else {
            return;
        };
        let step = |at: usize, count: usize| {
            if down {
                (at + 1) % count
            } else {
                (at + count - 1) % count
            }
        };
        match (menu.sub, rows.get(menu.row)) {
            (Some(sub), Some(Row::TurnInto(kinds))) => menu.sub = Some(step(sub, kinds.len())),
            _ => menu.row = step(menu.row, rows.len()),
        }
        self.directives.menu = Some(menu);
        cx.notify();
    }

    pub(in crate::editor) fn options_up(&mut self, _: &OptionsUp, _: &mut Window, cx: &mut Context<Self>) {
        self.options_move(false, cx);
    }

    pub(in crate::editor) fn options_down(&mut self, _: &OptionsDown, _: &mut Window, cx: &mut Context<Self>) {
        self.options_move(true, cx);
    }

    fn options_across(&mut self, right: bool, cx: &mut Context<Self>) {
        if let Some(ghost) = self.directives.ghost_table.as_mut().filter(|ghost| ghost.keys) {
            let count = ghost.options.align.len();
            ghost.column = if right {
                (ghost.column + 1) % count
            } else {
                (ghost.column + count - 1) % count
            };
            cx.notify();
            return;
        }
        let Some((region, rows, mut menu)) = self.open_menu() else {
            return;
        };
        if let Some(Row::TurnInto(kinds)) = rows.get(menu.row) {
            menu.sub = right.then(|| self.own_kind_at(region, kinds));
            self.directives.menu = Some(menu);
            cx.notify();
        }
    }

    /// Where `region`'s own kind stands among `kinds`, so Turn into opens on it.
    fn own_kind_at(&self, region: usize, kinds: &[Kind]) -> usize {
        let own = self.directives.structure.regions[region].kind;
        kinds.iter().position(|kind| *kind == own).unwrap_or(0)
    }

    pub(in crate::editor) fn options_left(&mut self, _: &OptionsLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.options_across(false, cx);
    }

    pub(in crate::editor) fn options_right(&mut self, _: &OptionsRight, _: &mut Window, cx: &mut Context<Self>) {
        self.options_across(true, cx);
    }

    pub(in crate::editor) fn options_accept(&mut self, _: &OptionsAccept, window: &mut Window, cx: &mut Context<Self>) {
        // Return on the ghost's options inserts the table with them, as Return in the menu does.
        if let Some(ghost) = self.directives.ghost_table.as_mut().filter(|ghost| ghost.keys) {
            ghost.keys = false;
            self.menu_accept(&crate::editor::MenuAccept, window, cx);
            return;
        }
        self.options_toggle(&OptionsToggle, window, cx);
    }

    pub(in crate::editor) fn options_toggle(&mut self, _: &OptionsToggle, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ghost) = self.directives.ghost_table.as_mut().filter(|ghost| ghost.keys) {
            let column = ghost.column;
            if ghost.row == 0 {
                ghost.options.align[column] = ghost.options.align[column].next();
            } else {
                ghost.options.fill[column] = !ghost.options.fill[column];
            }
            cx.notify();
            return;
        }
        let Some((region, rows, menu)) = self.open_menu() else {
            return;
        };
        match (rows.get(menu.row), menu.sub) {
            (Some(Row::TurnInto(kinds)), None) => {
                let sub = self.own_kind_at(region, kinds);
                self.directives.menu = Some(super::Menu { sub: Some(sub), ..menu });
                cx.notify();
            }
            (Some(row @ Row::TurnInto(kinds)), Some(sub)) => {
                let kind = kinds.get(sub).copied();
                self.choose_row(&row.clone(), kind, window, cx);
            }
            (Some(row), _) => self.choose_row(&row.clone(), None, window, cx),
            (None, _) => {}
        }
    }

    /// Closes the menu and carries out `row`.
    fn choose_row(&mut self, row: &Row, kind: Option<Kind>, window: &mut Window, cx: &mut Context<Self>) {
        self.directives.menu = None;
        self.act_on_region(row, kind, window, cx);
    }

    pub(in crate::editor) fn options_back(&mut self, _: &OptionsBack, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ghost) = &mut self.directives.ghost_table {
            ghost.keys = false;
        }
        // Escape closes Turn into first, then the menu, and the region stays selected.
        if let Some(menu) = &mut self.directives.menu {
            if menu.sub.is_some() {
                menu.sub = None;
            } else {
                self.directives.menu = None;
            }
        }
        cx.notify();
    }

    pub(in crate::editor) fn options_unwrap(&mut self, _: &OptionsUnwrap, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_menu().is_some() {
            self.choose_row(&Row::Unwrap, None, window, cx);
        }
    }

    /// A press on what is wrong with a directive line takes the line out. Returns whether it was one.
    pub(in crate::editor) fn repair_mouse_down(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let hit = self
            .directives
            .repairs
            .iter()
            .find(|(bounds, _)| bounds.contains(&position));
        let Some(block) = hit.map(|(_, block)| *block) else {
            return false;
        };
        self.repair_line(block, window, cx)
    }

    /// `Cmd+Return` on a directive line the engine will not take. Returns whether the caret stood on one.
    pub(in crate::editor) fn repair_at_caret(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let block = self.blocks.index_at(self.selection.head);
        self.repair_line(block, window, cx)
    }

    fn repair_line(&mut self, block: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.source {
            return false;
        }
        let structure = &self.directives.structure;
        let Some(rejected) = structure
            .rejected
            .iter()
            .find(|rejected| rejected.block == block)
            .copied()
        else {
            return false;
        };
        let text = self.session.active_buffer().text();
        let command = edits::repair(text, &self.blocks, &rejected);
        self.run_command(command, window, cx);
        true
    }

    /// Says why pasted regions went in as their text alone.
    pub(in crate::editor) fn notify_paste_refused(&mut self, cx: &mut Context<Self>) {
        let at = self.selection.head;
        self.notify_rich(at, crate::t!(Key::PasteRefused).to_owned(), false, cx);
    }

    pub(in crate::editor) fn full_width_from_caret(
        &mut self,
        _: &FullWidthFromCaret,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // With the region's menu open the key means its row, which remembers where the caret stood.
        if self.open_menu().is_some() {
            self.choose_row(&Row::FullWidth, None, window, cx);
            return;
        }
        let text = self.session.active_buffer().text();
        let block = self.blocks.index_at(self.selection.head);
        if let Some(command) =
            edits::full_width_at(text, &self.blocks, &self.directives.structure, block, self.selection)
        {
            self.run_command(command, window, cx);
        }
    }

    pub(in crate::editor) fn ghost_options(&mut self, _: &GhostOptions, _: &mut Window, cx: &mut Context<Self>) {
        if self.ghost().is_some_and(|ghost| ghost.kind == Kind::Table) {
            let ghost = self.directives.ghost_table.get_or_insert_with(GhostTable::default);
            ghost.keys = !ghost.keys;
            cx.notify();
        }
    }

    /// Typing goes to the text: the ghost's options give the keys back to the menu.
    pub(in crate::editor) fn options_release_keys(&mut self) {
        if let Some(ghost) = &mut self.directives.ghost_table {
            ghost.keys = false;
        }
    }

    // The float

    /// The selected region's menu under its grip, or the ghost table's options, for this frame.
    pub(in crate::editor) fn options_float(&mut self, cx: &mut Context<Self>) -> Option<(AnyElement, Spot)> {
        let anchor = self.directives.anchor?;
        let at = point(
            anchor.left().max(self.column.viewport.left() + tokens::SPACE_2),
            anchor.bottom() + tokens::FLOAT_GAP,
        );
        if matches!(self.float, Float::Menu(_)) {
            self.ghost().filter(|ghost| ghost.kind == Kind::Table)?;
            let table = self.directives.ghost_table.clone().unwrap_or_default();
            return Some((self.ghost_table_element(&table, cx).into_any_element(), Spot::At(at)));
        }
        if !matches!(self.float, Float::None) {
            return None;
        }
        let (region, rows, menu) = self.open_menu()?;
        let own = self.directives.structure.regions[region].kind;
        Some((
            self.region_menu_element(&rows, menu, own, cx).into_any_element(),
            Spot::At(at),
        ))
    }

    fn region_menu_element(
        &self,
        rows: &[Row],
        menu: super::Menu,
        own: Kind,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx).clone();
        let row_base = |id: (&'static str, usize), chosen: bool, label: SharedString, icon: &'static str| {
            div()
                .id(id)
                .role(gpui::Role::MenuItem)
                .aria_label(label.clone())
                .aria_selected(chosen)
                .flex_none()
                .h(tokens::MENU_ROW_HEIGHT)
                .mx(tokens::SPACE_1)
                .px(tokens::SPACE_2)
                .flex()
                .items_center()
                .gap(tokens::SPACE_2)
                .rounded(tokens::RADIUS)
                .cursor_pointer()
                .text_size(tokens::TEXT_BODY)
                .text_color(if chosen { tokens.text } else { tokens.text_quiet })
                .when(chosen, |row| row.bg(tokens.raised))
                .hover(|row| row.bg(tokens.raised))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    svg()
                        .flex_none()
                        .path(icon)
                        .size(tokens::ICON_SIZE)
                        .text_color(if chosen { tokens.text } else { tokens.text_faint }),
                )
                .child(div().flex_1().min_w_0().truncate().child(label))
        };
        let end = |text: &'static str| {
            div()
                .flex_none()
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_faint)
                .child(text)
        };
        let mut sub_row = 0;
        let mut elements: Vec<AnyElement> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let chosen = menu.row == index;
            if *row == Row::Duplicate {
                elements.push(
                    div()
                        .my(tokens::SPACE_1)
                        .h(tokens::HAIRLINE)
                        .bg(tokens.divider)
                        .into_any_element(),
                );
            }
            let (label, icon, keys): (&'static str, &'static str, Option<&'static str>) = match row {
                Row::TurnInto(_) => {
                    sub_row = index;
                    (crate::t!(Key::TurnInto), own.icon(), None)
                }
                Row::FullWidth => (
                    crate::t!(Key::FullWidthFromHere),
                    "icons/directive-full-width.svg",
                    Some("\u{21e7}\u{2318}\u{21a9}"),
                ),
                Row::Unwrap => (
                    crate::t!(Key::Unwrap),
                    "icons/directive-unwrap.svg",
                    Some("\u{2318}\u{232b}"),
                ),
                Row::Duplicate => (crate::t!(Key::Duplicate), "icons/duplicate.svg", Some("\u{2318}D")),
                Row::MoveUp => (crate::t!(Key::MoveUp), "icons/move-up.svg", Some("\u{2325}\u{2191}")),
                Row::MoveDown => (
                    crate::t!(Key::MoveDown),
                    "icons/move-down.svg",
                    Some("\u{2325}\u{2193}"),
                ),
                Row::Delete if own.has_body() => (crate::t!(Key::DeleteWithText), "icons/trash.svg", Some("\u{232b}")),
                Row::Delete => (crate::t!(Key::Delete), "icons/trash.svg", Some("\u{232b}")),
            };
            let picked = row.clone();
            let element = row_base(("region-row", index), chosen, label.into(), icon).on_click(cx.listener(
                move |this, _, window, cx| match &picked {
                    Row::TurnInto(kinds) => {
                        let sub = this.own_kind_at(this.selected_region().unwrap_or(0), kinds);
                        if let Some(menu) = &mut this.directives.menu {
                            menu.row = index;
                            menu.sub = if menu.sub.is_some() { None } else { Some(sub) };
                        }
                        cx.notify();
                    }
                    row => this.choose_row(row, None, window, cx),
                },
            ));
            let element = match keys {
                Some(keys) => element.child(end(keys)),
                None => element.child(
                    svg()
                        .flex_none()
                        .path("icons/chevron-right.svg")
                        .size(tokens::ICON_SIZE)
                        .text_color(tokens.text_faint),
                ),
            };
            elements.push(element.into_any_element());
        }
        let main = card(&tokens)
            .id("region-menu")
            .role(gpui::Role::Menu)
            .child(
                div()
                    .px(tokens::SPACE_3)
                    .pt(tokens::SPACE_1)
                    .pb(tokens::SPACE_1)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(own.label()),
            )
            .children(elements);
        let kinds = match (menu.sub, rows.get(menu.row)) {
            (Some(sub), Some(Row::TurnInto(kinds))) => Some((sub, kinds.clone())),
            _ => None,
        };
        let sub = kinds.map(|(sub, kinds)| {
            let rows = kinds.into_iter().enumerate().map(|(at, kind)| {
                let label: SharedString = kind.label().into();
                row_base(("region-kind", at), at == sub, label, kind.icon())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.choose_row(&Row::TurnInto(Vec::new()), Some(kind), window, cx)
                    }))
                    .when(kind == own, |row| row.child(end("\u{2713}")))
            });
            // Level with Turn into, past the menu's quiet name.
            let down = tokens::TEXT_SMALL * 1.5 + tokens::SPACE_2 + tokens::MENU_ROW_HEIGHT * sub_row as f32;
            card(&tokens)
                .id("region-kinds")
                .role(gpui::Role::Menu)
                .w(tokens::REGION_KINDS_WIDTH)
                .mt(down)
                .children(rows)
        });
        div()
            .flex()
            .items_start()
            .gap(tokens::SPACE_1)
            .child(main)
            .children(sub)
    }

    fn ghost_table_element(&self, table: &GhostTable, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx).clone();
        let ring = ring(&tokens);
        let line = |row: usize, label: &'static str, values: Vec<&'static str>| {
            let segments = values.into_iter().enumerate().map(|(column, value)| {
                let focused = table.keys && table.row == row && table.column == column;
                div()
                    .id(("ghost-option", row * 16 + column))
                    .h(tokens::SEGMENT_HEIGHT)
                    .px(tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .rounded(tokens::SEGMENT_RADIUS)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text)
                    .bg(tokens.raised)
                    .cursor_pointer()
                    .when(focused, |segment| segment.shadow(vec![ring.clone()]))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(ghost) = &mut this.directives.ghost_table {
                            if row == 0 {
                                ghost.options.align[column] = ghost.options.align[column].next();
                            } else {
                                ghost.options.fill[column] = !ghost.options.fill[column];
                            }
                        } else {
                            let mut ghost = GhostTable::default();
                            if row == 0 {
                                ghost.options.align[column] = Align::Center;
                            } else {
                                ghost.options.fill[column] = true;
                            }
                            this.directives.ghost_table = Some(ghost);
                        }
                        cx.notify();
                    }))
                    .child(value)
            });
            div()
                .flex()
                .flex_col()
                .gap(tokens::SPACE_1)
                .child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_quiet)
                        .child(label),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(2.))
                        .p(px(2.))
                        .rounded(tokens::SEGMENT_RADIUS + px(1.))
                        .bg(tokens.field)
                        .self_start()
                        .children(segments),
                )
        };
        let aligns = table
            .options
            .align
            .iter()
            .map(|align| {
                crate::t!(match align {
                    Align::Left => Key::AlignLeft,
                    Align::Center => Key::AlignCenter,
                    Align::Right => Key::AlignRight,
                })
            })
            .collect();
        let widths = table
            .options
            .fill
            .iter()
            .map(|fill| crate::t!(if *fill { Key::WidthFill } else { Key::WidthAuto }))
            .collect();
        card(&tokens)
            .id("ghost-table-options")
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .w_auto()
            .child(line(0, crate::t!(Key::Align), aligns))
            .child(line(1, crate::t!(Key::Widths), widths))
            .when(table.keys, |card| {
                card.child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_faint)
                        .child(crate::t!(Key::GhostFoot)),
                )
            })
    }
}

/// A float on the panel colour, as the slash menu's.
fn card(tokens: &Tokens) -> gpui::Div {
    div()
        .occlude()
        .w(tokens::OPTIONS_WIDTH)
        .py(tokens::SPACE_1)
        .flex()
        .flex_col()
        .rounded(tokens::RADIUS)
        .bg(tokens.panel)
        .border_1()
        .border_color(tokens.divider)
        .shadow(tokens::float_shadow())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// The keyboard's ring on the segment the arrow keys are on.
fn ring(tokens: &Tokens) -> BoxShadow {
    BoxShadow {
        color: tokens.accent.into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: tokens::FOCUS_RING,
        inset: true,
    }
}

/// Paints the brackets, sheet gaps, and ghost of a frame.
pub(in crate::editor) fn paint_marks(marks: &Marks, frame: &Frame, window: &mut Window, cx: &mut App) {
    let colors = Tokens::get(cx).clone();
    let faint: Hsla = colors.text_faint.into();
    let line_color = faint.opacity(0.7);
    let lit_color: Hsla = colors.text_quiet.into();
    let accent: Hsla = colors.accent.into();
    for (bounds, tint) in &marks.washes {
        window.paint_quad(fill(*bounds, accent.opacity(*tint)).corner_radii(tokens::RADIUS));
    }
    for (bounds, ghost) in &marks.sheets {
        let opacity = if *ghost { tokens::GHOST_OPACITY } else { 1. };
        let height = bounds.size.height;
        let top = bounds.top() + height * 0.3;
        let band = Bounds::new(point(bounds.left(), top), size(bounds.size.width, height * 0.4));
        let text: Hsla = colors.text.into();
        window.paint_quad(fill(band, text.opacity(tokens::SHEET_GAP_TINT * opacity)));
        let rule = frame.rule.opacity(opacity);
        for y in [band.top(), band.bottom() - tokens::HAIRLINE] {
            window.paint_quad(fill(
                Bounds::new(point(band.left(), y), size(band.size.width, tokens::HAIRLINE)),
                rule,
            ));
        }
    }
    if let Some((line, at, height)) = &marks.ghost_text {
        line.paint(*at, *height, TextAlign::Left, None, window, cx).ok();
    }
    if let Some((whole, at)) = &marks.ghost_whole {
        whole.paint(*at, frame, window, cx);
    }
    for bracket in &marks.brackets {
        let color = if bracket.accent {
            accent
        } else if bracket.lit {
            lit_color
        } else {
            line_color
        };
        let color = if bracket.ghost {
            color.opacity(tokens::GHOST_OPACITY)
        } else {
            color
        };
        let hair = tokens::HAIRLINE;
        if bracket.tick {
            let y = bracket.top + tokens::TEXT_SMALL * 0.75;
            let y = y.min(bracket.bottom);
            window.paint_quad(fill(
                Bounds::new(point(bracket.x, y), size(tokens::BRACKET_TICK * 4., hair)),
                color,
            ));
        } else {
            let top = bracket.top + tokens::BRACKET_END_INSET;
            let bottom = (bracket.bottom - tokens::BRACKET_END_INSET).max(top + hair);
            window.paint_quad(fill(
                Bounds::new(point(bracket.x, top), size(hair, bottom - top)),
                color,
            ));
            for y in [top, bottom - hair] {
                window.paint_quad(fill(
                    Bounds::new(point(bracket.x, y), size(tokens::BRACKET_TICK, hair)),
                    color,
                ));
            }
        }
        if let Some((name, at)) = &bracket.name {
            name.paint(*at, tokens::TEXT_SMALL * 1.5, TextAlign::Left, None, window, cx)
                .ok();
        }
    }
    let ink: Hsla = colors.text.into();
    for grip in &marks.grips {
        let icon = Bounds::new(
            point(
                grip.left() + (grip.size.width - tokens::GUTTER_ICON) * 0.5,
                grip.top() + (grip.size.height - tokens::GUTTER_ICON) * 0.5,
            ),
            size(tokens::GUTTER_ICON, tokens::GUTTER_ICON),
        );
        window
            .paint_svg(
                icon,
                "icons/grip.svg".into(),
                None,
                TransformationMatrix::unit(),
                ink,
                cx,
            )
            .ok();
    }
    for (line, at) in &marks.repairs {
        line.paint(*at, tokens::TEXT_SMALL * 1.5, TextAlign::Left, None, window, cx)
            .ok();
    }
    if let Some((label, at)) = &marks.drop_label {
        label
            .paint(*at, tokens::TEXT_SMALL * 1.5, TextAlign::Left, None, window, cx)
            .ok();
    }
}
