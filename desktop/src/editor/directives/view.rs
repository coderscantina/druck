//! What the directives draw: brackets with their names in the left margin, a new page's sheet edges, the options under
//! a lit name, and the slash menu's ghost of the directive it would insert.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, BoxShadow, Context, Hsla, MouseButton, Pixels, Point, ShapedLine, SharedString, TextAlign,
    TextRun, Window, div, fill, font, point, px, size, svg,
};

use super::edits::{self, Align};
use super::strings::Key;
use super::{
    FullWidthFromCaret, GhostOptions, GhostTable, Kind, OptionsAccept, OptionsBack, OptionsDown, OptionsFocus,
    OptionsLeft, OptionsRight, OptionsToggle, OptionsUnwrap, OptionsUp,
};
use crate::editor::commands::slash::{Action, Standalone};
use crate::editor::element::{Frame, FrameBlock};
use crate::editor::float::Spot;
use crate::editor::rich::view::shapes::Whole;
use crate::editor::rich::{Atom, AtomKind};
use crate::editor::{EditorView, Float};
use crate::source::{BlockKind, Blocks, Selection};
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

/// A row of a directive's options.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Kind(Vec<Kind>),
    FullWidth,
    GoTo(String),
    Unwrap,
}

/// The slash menu's directive entries and what each would insert.
pub(in crate::editor) fn ghost_kind(action: &Action) -> Option<Kind> {
    match action {
        Action::Wrap(name) => Kind::from_name(name),
        Action::Insert(Standalone::PageBreak) => Some(Kind::PageBreak),
        Action::Insert(Standalone::Toc) => Some(Kind::Toc),
        Action::Insert(Standalone::Bibliography) => Some(Kind::Bibliography),
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

    /// The options of a region, in their order.
    fn option_rows(&self, region: usize) -> Vec<Row> {
        let text = self.session.active_buffer().text();
        let structure = &self.directives.structure;
        let found = &structure.regions[region];
        if !found.kind.has_body() {
            return Vec::new();
        }
        let mut rows = Vec::new();
        let kinds = edits::kinds_offered(text, &self.blocks, structure, region);
        if kinds.len() > 1 {
            rows.push(Row::Kind(kinds));
        }
        let in_columns = found.kind == Kind::Columns
            || found
                .parent
                .is_some_and(|parent| structure.regions[parent].kind == Kind::Columns);
        if in_columns && found.kind != Kind::FullWidth && self.full_width_target(region).is_some() {
            rows.push(Row::FullWidth);
        }
        if found.kind == Kind::Columns
            && let Some(at) = edits::full_width_part(&self.blocks, structure, region)
        {
            let words: String = text[at..].lines().next().unwrap_or("").chars().take(18).collect();
            rows.push(Row::GoTo(words));
        }
        rows.push(Row::Unwrap);
        rows
    }

    /// What "Full width from the caret" writes for the options of `region`: the part after a columns region's opening
    /// line or before its closing line, or a nested group whole.
    fn full_width_target(&self, region: usize) -> Option<crate::editor::commands::Command> {
        let text = self.session.active_buffer().text();
        let structure = &self.directives.structure;
        let found = &structure.regions[region];
        let list = self.blocks.as_slice();
        let caret_block = self.blocks.index_at(self.selection.head);
        let block = if found.kind != Kind::Columns {
            found.open
        } else if caret_block == found.close {
            (found.open + 1..found.close)
                .rev()
                .find(|&at| list[at].kind != BlockKind::Blank)?
        } else {
            (found.open + 1..found.close).find(|&at| list[at].kind != BlockKind::Blank)?
        };
        edits::full_width_at(text, &self.blocks, structure, block, self.selection)
    }

    /// The brackets, sheet gaps, and ghost of this frame's blocks. Also keeps where the lit name stands, for the options.
    pub(in crate::editor) fn directive_marks(
        &mut self,
        blocks: &[FrameBlock],
        indices: &[usize],
        window: &Window,
        cx: &App,
    ) -> Marks {
        let mut marks = Marks::default();
        self.directives.anchor = None;
        if self.source || indices.is_empty() {
            return marks;
        }
        let colors = Tokens::get(cx);
        let gutter = self.column.text_left - tokens::GUTTER_WIDTH + tokens::BRACKET_INSET;
        let slot = |depth: usize| gutter + tokens::BRACKET_STEP * depth.min(tokens::BRACKET_DEPTHS - 1) as f32;
        let name_right = slot(0) - tokens::BRACKET_NAME_GAP;
        let room_left = self.column.viewport.left() + tokens::SPACE_1;
        let (first, last) = (indices[0], indices[indices.len() - 1]);
        let lit = self.fence_under_caret();
        let shown = |index: usize| indices.iter().position(|at| *at == index).map(|at| &blocks[at]);
        let gap_middle = self.column.text_left + self.directives.column_width + self.directives.column_gap * 0.5;
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
            // A group inside a second column has its bracket in the gap between the columns, where no name fits.
            let in_second = region.kind != Kind::Columns
                && (region.open..=region.close).any(|at| self.directives.shifted.contains_key(&at));
            let is_lit = lit == Some(number);
            let x = if in_second { gap_middle } else { slot(region.depth) };
            let name = (!in_second)
                .then(|| {
                    let color = if is_lit { colors.text } else { colors.text_faint };
                    let line = ui_line(region.kind.label(), color.into(), window);
                    let left = name_right - line.width;
                    (left >= room_left).then(|| (line, point(left, top)))
                })
                .flatten();
            if is_lit {
                let anchor = name
                    .as_ref()
                    .map_or(Bounds::new(point(x, top), size(px(1.), px(16.))), |(line, at)| {
                        Bounds::new(*at, size(line.width, tokens::TEXT_SMALL * 1.5))
                    });
                self.directives.anchor = Some(anchor);
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
                ghost: false,
            });
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
            ghost: true,
        });
    }

    // Keys

    pub(in crate::editor) fn options_focus(&mut self, _: &OptionsFocus, _: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.fence_under_caret() else {
            return;
        };
        let rows = self.option_rows(region);
        let own = self.directives.structure.regions[region].kind;
        if let Some(options) = &mut self.directives.options
            && !rows.is_empty()
        {
            options.keys = true;
            options.row = 0;
            options.kind = match rows.first() {
                Some(Row::Kind(kinds)) => kinds.iter().position(|kind| *kind == own).unwrap_or(0),
                _ => 0,
            };
            cx.notify();
        }
    }

    fn options_move(&mut self, down: bool, cx: &mut Context<Self>) {
        if let Some(ghost) = self.directives.ghost_table.as_mut().filter(|ghost| ghost.keys) {
            ghost.row = 1 - ghost.row;
            cx.notify();
            return;
        }
        let Some(region) = self.fence_under_caret() else {
            return;
        };
        let count = self.option_rows(region).len();
        if let Some(options) = self.directives.options.as_mut().filter(|options| options.keys)
            && count > 0
        {
            options.row = if down {
                (options.row + 1) % count
            } else {
                (options.row + count - 1) % count
            };
            cx.notify();
        }
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
        let Some(region) = self.fence_under_caret() else {
            return;
        };
        let rows = self.option_rows(region);
        if let Some(options) = self.directives.options.as_mut().filter(|options| options.keys)
            && let Some(Row::Kind(kinds)) = rows.get(options.row)
        {
            let count = kinds.len();
            options.kind = if right {
                (options.kind + 1) % count
            } else {
                (options.kind + count - 1) % count
            };
            cx.notify();
        }
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
        let Some(options) = self.directives.options.filter(|options| options.keys) else {
            return;
        };
        self.act(options.row, Some(options.kind), window, cx);
    }

    /// Carries out row `row` of the options, with `kind` chosen in the kind row.
    fn act(&mut self, row: usize, kind: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.fence_under_caret() else {
            return;
        };
        let text = self.session.active_buffer().text();
        let structure = &self.directives.structure;
        let command = match self.option_rows(region).get(row) {
            Some(Row::Kind(kinds)) => kinds
                .get(kind.unwrap_or(0))
                .and_then(|kind| edits::change_kind(text, &self.blocks, structure, region, *kind, self.selection)),
            Some(Row::FullWidth) => self.full_width_target(region),
            Some(Row::GoTo(_)) => {
                if let Some(at) = edits::full_width_part(&self.blocks, structure, region) {
                    self.directives.options = None;
                    self.set_selection(Selection::caret(at), false, window, cx);
                }
                return;
            }
            Some(Row::Unwrap) => edits::unwrap(text, &self.blocks, structure, region),
            None => None,
        };
        let Some(command) = command else {
            return;
        };
        let keys = self.directives.options.is_some_and(|options| options.keys);
        self.run_command(command, window, cx);
        // The keys stay with the options while the caret stays on the line.
        if let Some(options) = &mut self.directives.options {
            options.keys = keys;
            options.row = row;
        }
        cx.notify();
    }

    pub(in crate::editor) fn options_back(&mut self, _: &OptionsBack, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ghost) = &mut self.directives.ghost_table {
            ghost.keys = false;
        }
        if let Some(options) = &mut self.directives.options {
            options.keys = false;
        }
        cx.notify();
    }

    pub(in crate::editor) fn options_unwrap(&mut self, _: &OptionsUnwrap, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.fence_under_caret() else {
            return;
        };
        let rows = self.option_rows(region);
        if let Some(row) = rows.iter().position(|row| *row == Row::Unwrap) {
            self.act(row, None, window, cx);
        }
    }

    pub(in crate::editor) fn full_width_from_caret(
        &mut self,
        _: &FullWidthFromCaret,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match self.fence_under_caret() {
            Some(region) => self.full_width_target(region),
            None => {
                let text = self.session.active_buffer().text();
                let block = self.blocks.index_at(self.selection.head);
                edits::full_width_at(text, &self.blocks, &self.directives.structure, block, self.selection)
            }
        };
        if let Some(command) = command {
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

    /// Typing goes to the text: the options give the keys back to the line, and the ghost's to the menu.
    pub(in crate::editor) fn options_release_keys(&mut self) {
        if let Some(options) = &mut self.directives.options {
            options.keys = false;
        }
        if let Some(ghost) = &mut self.directives.ghost_table {
            ghost.keys = false;
        }
    }

    // The float

    /// The options under the lit name, or the ghost table's, for this frame.
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
        let region = self.fence_under_caret()?;
        let rows = self.option_rows(region);
        let options = self.directives.options?;
        if rows.is_empty() {
            return None;
        }
        let own = self.directives.structure.regions[region].kind;
        Some((
            self.options_element(&rows, options, own, cx).into_any_element(),
            Spot::At(at),
        ))
    }

    fn options_element(
        &self,
        rows: &[Row],
        options: super::Options,
        own: Kind,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx).clone();
        let ring = ring(&tokens);
        let row_base = |index: usize, label: SharedString, icon: &'static str| {
            let chosen = options.keys && options.row == index;
            div()
                .id(("directive-option", index))
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
                .text_size(tokens::TEXT_BODY)
                .text_color(if chosen { tokens.text } else { tokens.text_quiet })
                .when(chosen, |row| row.bg(tokens.raised))
                .child(
                    svg()
                        .flex_none()
                        .path(icon)
                        .size(tokens::ICON_SIZE)
                        .text_color(if chosen { tokens.text } else { tokens.text_faint }),
                )
                .child(div().flex_none().child(label))
        };
        let end = |text: SharedString| {
            div()
                .ml_auto()
                .min_w_0()
                .truncate()
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_faint)
                .child(text)
        };
        let elements: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| match row {
                Row::Kind(kinds) => {
                    let segments = kinds.iter().enumerate().map(|(at, kind)| {
                        let on = *kind == own;
                        let focused = options.keys && options.row == index && options.kind == at;
                        div()
                            .id(("directive-kind", at))
                            .role(gpui::Role::RadioButton)
                            .aria_label(kind.choice())
                            .h(tokens::SEGMENT_HEIGHT)
                            .px(tokens::SPACE_2)
                            .flex()
                            .items_center()
                            .rounded(tokens::SEGMENT_RADIUS)
                            .text_size(tokens::TEXT_SMALL)
                            .cursor_pointer()
                            .text_color(if on { tokens.text } else { tokens.text_quiet })
                            .when(on, |segment| segment.bg(tokens.raised))
                            .when(focused, |segment| segment.shadow(vec![ring.clone()]))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| this.act(index, Some(at), window, cx)))
                            .child(kind.choice())
                    });
                    row_base(index, crate::t!(Key::Kind).into(), "icons/directive-columns.svg")
                        .child(
                            div()
                                .ml_auto()
                                .flex()
                                .gap(px(2.))
                                .p(px(2.))
                                .rounded(tokens::SEGMENT_RADIUS + px(1.))
                                .bg(tokens.field)
                                .children(segments),
                        )
                        .into_any_element()
                }
                Row::FullWidth => row_base(
                    index,
                    crate::t!(Key::FullWidthFromCaret).into(),
                    "icons/directive-full-width.svg",
                )
                .cursor_pointer()
                .hover(|row| row.bg(tokens.raised))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| this.act(index, None, window, cx)))
                .child(end("\u{21e7}\u{2318}\u{21a9}".into()))
                .into_any_element(),
                Row::GoTo(words) => row_base(index, crate::t!(Key::GoToFullWidth).into(), "icons/directive-jump.svg")
                    .cursor_pointer()
                    .hover(|row| row.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| this.act(index, None, window, cx)))
                    .child(end(words.clone().into()))
                    .into_any_element(),
                Row::Unwrap => row_base(index, crate::t!(Key::Unwrap).into(), "icons/directive-unwrap.svg")
                    .cursor_pointer()
                    .hover(|row| row.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| this.act(index, None, window, cx)))
                    .child(end("\u{2318}\u{232b}".into()))
                    .into_any_element(),
            })
            .collect();
        card(&tokens)
            .id("directive-options")
            .role(gpui::Role::Menu)
            .children(elements)
            .child(div().my(tokens::SPACE_1).h(tokens::HAIRLINE).bg(tokens.divider))
            .child(
                div()
                    .px(tokens::SPACE_3)
                    .pb(tokens::SPACE_1)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(crate::t!(Key::OptionsFoot)),
            )
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
        let color = if bracket.lit { lit_color } else { line_color };
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
}
