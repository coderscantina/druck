//! Tables: column widths from measured cell content, cells set as blocks, and rows as lines of the flow.
//!
//! Every cell that spans one column is measured. Its natural width sets each paragraph on one line; its
//! minimum width holds its widest unbreakable piece, after hyphenation where the style hyphenates. `auto`
//! columns keep their natural widths when they fit. Otherwise each gets one common width, clamped between
//! the column's minimum and natural widths, chosen so the columns fill the available width: narrow columns
//! stay on one line and wide ones share the rest equally. `*` columns share what `auto` columns leave,
//! after reserving their own minimum widths. A cell spanning columns is set across them, and widens them
//! equally where it needs more minimum or natural width than they have together.
//!
//! A cell's style is its own custom style, else its row's, else the header or cell style. The column's
//! alignment applies unless the cell has a style of its own; a spanning cell takes its first column's.
//!
//! A row is one line of the flow, as tall as its tallest cell plus padding, so pages and columns break
//! only between rows. The caption and the header row never end a page, so they stay with the first row,
//! and neither does a row whose style keeps it with the next. The composer sets the header again where a
//! page or column starts at a later row.

use super::paragraph::{self, Prepared};
use super::structure::Numbered;
use std::borrow::Cow;

use super::{Break, Flow, Frame, Line, anchor_item, pages, translate, translate_line};
use crate::config::resolved::{CustomStyle, Rule, Style};
use crate::config::theme::{Align, RuleBelow};
use crate::config::values::Pt;
use crate::document::{Block, Cell, Column, ColumnAlign, ColumnWidth, Inline, Location, Row};
use crate::page::{Item, Rect};

/// Added to a column's width when its cells are set, so text measured to fit exactly is not
/// broken by rounding.
const SLACK: f64 = 1e-6;

/// A table as the parser delivers it.
pub(super) struct TableBlock<'d> {
    pub at: Location,
    pub columns: &'d [Column],
    pub header: &'d Row,
    pub rows: &'d [Row],
    pub caption: &'d [Inline],
    /// The table's number and anchor, if it has a caption.
    pub number: Option<Numbered>,
}

/// How wide a cell's content can be set: at least `minimum`, which `word` written at its location needs
/// if it is set, and with each paragraph on one line at `natural`.
#[derive(Clone, Default)]
struct Measure {
    minimum: f64,
    natural: f64,
    word: Option<(String, Location)>,
}

impl Measure {
    fn widen(&mut self, other: Measure) {
        if other.minimum > self.minimum {
            (self.minimum, self.word) = (other.minimum, other.word);
        }
        self.natural = self.natural.max(other.natural);
    }
}

impl<'a> Flow<'a> {
    /// Sets a table in `frame` with its numbered caption above it. A table narrower than its frame is
    /// centered in it, except a wide table, which is centered in the prose width or, if wider than that,
    /// starts where the prose starts.
    pub(super) fn table(&mut self, table: TableBlock, frame: Frame<'a>) {
        let config = self.config;
        let padding = config.tables.cell_padding.0;
        let width = self.width(frame);
        let rows: Vec<&Row> = std::iter::once(table.header).chain(table.rows).collect();
        let frames: Vec<Vec<Frame<'a>>> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| self.cell_frames(row, index == 0, table.columns))
            .collect();

        // A cell of one paragraph, such as every pipe table cell, is shaped once for measuring and setting.
        let styles: Vec<Vec<Option<Cow<'a, Style>>>> = rows
            .iter()
            .zip(&frames)
            .map(|(row, frames)| {
                let styles = row
                    .cells
                    .iter()
                    .zip(frames)
                    .map(|(cell, frame)| match cell.blocks.as_slice() {
                        [Block::Paragraph { class, .. }] => Some(self.paragraph_style(class.as_ref(), frame)),
                        _ => None,
                    });
                styles.collect()
            })
            .collect();
        let mut prepared: Vec<Vec<Option<(Prepared, f64)>>> = Vec::with_capacity(rows.len());
        let mut columns = vec![Measure::default(); table.columns.len()];
        let mut spanning = Vec::new();
        let mut failed = false;
        for ((row, frames), styles) in rows.iter().zip(&frames).zip(&styles) {
            let mut kept = Vec::with_capacity(row.cells.len());
            for (((column, cell), frame), style) in placed(row).zip(frames).zip(styles) {
                let measured = match (style, cell.blocks.as_slice()) {
                    (Some(style), [Block::Paragraph { at, content, .. }]) => {
                        let content = self.resolve(content);
                        let lang = config.document.lang;
                        match paragraph::prepare(&content, style, &config.inline, self.fonts, lang) {
                            Ok(paragraph) => {
                                let measure = widths(&paragraph, 2.0 * style.indent.0, *at);
                                kept.push(Some((paragraph, style.indent.0)));
                                Some(measure)
                            }
                            Err(problem) => {
                                self.errors.push(self.error(*at, problem));
                                None
                            }
                        }
                    }
                    _ => {
                        kept.push(None);
                        self.measure(&cell.blocks, frame)
                    }
                };
                match measured {
                    Some(measure) if cell.span == 1 => columns[column].widen(measure),
                    Some(measure) => spanning.push((column..column + cell.span, measure)),
                    None => failed = true,
                }
            }
            prepared.push(kept);
        }
        if failed {
            return;
        }
        for (span, measure) in spanning {
            spread(&mut columns[span], measure, 2.0 * padding);
        }

        let padded = 2.0 * padding * columns.len() as f64;
        let kinds: Vec<ColumnWidth> = table.columns.iter().map(|column| column.width).collect();
        let Some(widths) = allocate(&columns, &kinds, width - padded) else {
            self.too_narrow(table.at, &columns, padded, width);
            return;
        };
        let table_width = widths.iter().sum::<f64>() + padded;
        let place = match self.wide {
            Some(_) => table_width.max(config.page.prose_width.0),
            None => width,
        };
        let left = frame.left + (place - table_width) / 2.0;

        let mut caption = match table.number {
            None => Vec::new(),
            Some(number) => {
                let label = &config.labels.table;
                match self.caption_lines(table.at, label, number.number, table.caption, place) {
                    Some(lines) => lines,
                    None => return,
                }
            }
        };
        if let (Some(number), Some(line)) = (table.number, caption.first_mut()) {
            line.items.push(anchor_item(number.anchor, frame.left));
        }

        // Column edges, from the left edge of the table to its right edge.
        let mut edges = Vec::with_capacity(widths.len() + 1);
        let mut x = left;
        for width in &widths {
            edges.push(x);
            x += width + 2.0 * padding;
        }
        edges.push(x);
        let rules = &config.tables;
        let mut lines = Vec::with_capacity(rows.len());
        for (index, ((row, frames), prepared)) in rows.iter().zip(frames).zip(&prepared).enumerate() {
            let mut cells = Vec::with_capacity(row.cells.len());
            for (((column, cell), frame), paragraph) in placed(row).zip(frames).zip(prepared) {
                let content = match cell.span {
                    1 => widths[column],
                    span => edges[column + span] - edges[column] - 2.0 * padding,
                };
                let lines = match paragraph {
                    Some((paragraph, indent)) => match paragraph.lines(content - 2.0 * indent + SLACK, 0.0) {
                        Ok(lines) => Some(
                            lines
                                .into_iter()
                                .map(|line| (0.0, translate_line(line, *indent)))
                                .collect(),
                        ),
                        Err(problem) => {
                            self.errors.push(self.error(cell.blocks[0].at(), problem));
                            None
                        }
                    },
                    None => {
                        let frame = Frame {
                            right: config.page.text_width().0 - content - SLACK,
                            ..frame
                        };
                        self.cell(&cell.blocks, frame)
                    }
                };
                match lines {
                    Some(lines) => cells.push((edges[column], lines)),
                    None => return,
                }
            }
            let below = match self.row_style(row).and_then(|(_, rule)| rule) {
                Some(RuleBelow::None) => None,
                Some(RuleBelow::Header) => rules.header_rule,
                Some(RuleBelow::Row) => rules.row_rule,
                None if index == 0 => rules.header_rule,
                None => rules.row_rule,
            };
            let above = if index == 0 { rules.top_rule } else { None };
            lines.push(row_line(cells, (left, x), padding, above, below));
        }

        if self.wide.is_some() {
            self.wide = Some(width - place);
        }
        let style = &config.styles.caption;
        let start = self.lines.len();
        self.space(style.space_after.0);
        let dx = frame.left + style.indent.0;
        let mut above = 0.0;
        for line in caption {
            above += line.height;
            self.push(translate_line(line, dx), table.at, Break::Never);
        }
        if start < self.lines.len() {
            above += style.space_before.0;
            self.space(style.space_before.0);
        }
        let count = lines.len() - 1;
        let mut lines = lines.into_iter();
        let header = lines.next().expect("a table has a header row");
        let repeated = header.clone();
        let after = if count == 0 { Break::Allowed(0.0) } else { Break::Never };
        self.push(header, table.header.at, after);
        let first_row = self.lines.len();
        let rows: Vec<Line> = lines.collect();
        let room = config.page.text_height().0 - repeated.height;
        let afters: Vec<Break> = (table.rows.iter().enumerate())
            .map(|(index, row)| match pages::line_break(index, count) {
                _ if self.row_style(row).is_some_and(|(style, _)| style.keep_with_next) => Break::Never,
                // Rows too tall to stay together may strand one, at the cost a column pays.
                Break::Avoid(cost) => {
                    let caption = if index == 0 { above } else { 0.0 };
                    if rows[index].height + rows[index + 1].height + caption > room {
                        Break::Allowed(cost)
                    } else {
                        Break::Avoid(cost)
                    }
                }
                after => after,
            })
            .collect();
        for ((line, row), after) in rows.into_iter().zip(table.rows).zip(afters) {
            self.push(line, row.at, after);
        }
        self.space(style.space_after.0);
        self.table_lines.push(pages::Table {
            lines: start..self.lines.len(),
            rows: (first_row + 1).min(self.lines.len())..self.lines.len(),
            header: repeated,
            at: table.at,
        });
    }

    /// The custom style of a row with the rule below it, if the row has one.
    fn row_style(&self, row: &Row) -> Option<(&'a Style, Option<RuleBelow>)> {
        Some(match self.custom(row.class.as_ref()?) {
            CustomStyle::Paragraph { style, rule_below } => (style, *rule_below),
            other => (other.style(), None),
        })
    }

    /// The frame each cell of a row is set in: its style, and the column alignment for a cell without a
    /// style of its own. Widths are set once the columns are known.
    fn cell_frames(&self, row: &Row, header: bool, columns: &[Column]) -> Vec<Frame<'a>> {
        let styles = &self.config.styles;
        let base = match self.row_style(row) {
            Some((style, _)) => style,
            None if header => &styles.table_header,
            None => &styles.table_cell,
        };
        placed(row)
            .map(|(column, cell)| {
                let own = cell.class.as_ref().map(|class| self.custom(class).style());
                let align = columns[column]
                    .align
                    .filter(|_| own.is_none())
                    .map(|align| match align {
                        ColumnAlign::Left => Align::Left,
                        ColumnAlign::Center => Align::Center,
                        ColumnAlign::Right => Align::Right,
                    });
                Frame {
                    left: 0.0,
                    right: 0.0,
                    style: own.unwrap_or(base),
                    list_depth: 0,
                    widen: 0.0,
                    align,
                }
            })
            .collect()
    }

    /// The widths of blocks set in `frame` as [`Flow::blocks`] sets them, or `None` if a paragraph cannot
    /// be shaped, which has been reported.
    fn measure(&mut self, blocks: &[Block], frame: &Frame<'a>) -> Option<Measure> {
        let mut measure = Measure::default();
        let mut after_paragraph = false;
        for block in blocks {
            match block {
                Block::Paragraph { at, content, class } => {
                    let style = self.paragraph_style(class.as_ref(), frame);
                    let content = self.resolve(content);
                    let lang = self.config.document.lang;
                    let prepared = paragraph::prepare(&content, &style, &self.config.inline, self.fonts, lang);
                    let prepared = match prepared {
                        Ok(prepared) => prepared,
                        Err(problem) => {
                            self.errors.push(self.error(*at, problem));
                            return None;
                        }
                    };
                    let first = if after_paragraph {
                        style.first_line_indent.0
                    } else {
                        0.0
                    };
                    measure.widen(widths(&prepared, 2.0 * style.indent.0 + first, *at));
                }
                Block::List { items, class, .. } => {
                    let inner = Frame {
                        style: self.list_style(class.as_ref()).0,
                        ..*frame
                    };
                    let indent = self.config.lists.indent.0;
                    for item in items {
                        let mut item = self.measure(item, &inner)?;
                        item.minimum += indent;
                        item.natural += indent;
                        measure.widen(item);
                    }
                }
                _ => unreachable!("the parser allows only paragraphs and lists in table cells"),
            }
            after_paragraph = matches!(block, Block::Paragraph { .. });
        }
        Some(measure)
    }

    /// Lays out a cell's blocks in `frame` and returns their lines with the space above each, or `None`
    /// if one cannot be set, which has been reported.
    fn cell(&mut self, blocks: &[Block], frame: Frame<'a>) -> Option<Vec<(f64, Line)>> {
        let errors = self.errors.len();
        let outer = std::mem::take(&mut self.lines);
        let state = (self.space, self.after_paragraph, self.wide);
        self.space = 0.0;
        self.after_paragraph = false;
        self.blocks(blocks, frame);
        let lines = std::mem::replace(&mut self.lines, outer);
        (self.space, self.after_paragraph, self.wide) = state;
        let lines = lines.into_iter().map(|line| (line.space_before, line.line)).collect();
        (self.errors.len() == errors).then_some(lines)
    }

    /// Reports the widest unbreakable word, or else the table, when the columns cannot fit even at their
    /// minimum widths.
    fn too_narrow(&mut self, at: Location, columns: &[Measure], padded: f64, width: f64) {
        let need = columns.iter().map(|column| column.minimum).sum::<f64>() + padded;
        let fit = format!(
            "with every column at its narrowest this table is {need:.1}pt wide, more than the available {width:.1}pt"
        );
        let widest = columns
            .iter()
            .filter_map(|column| Some((column.minimum, column.word.as_ref()?)))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let (at, message) = match widest {
            Some((minimum, (word, at))) => (*at, format!("\"{word}\" needs {minimum:.1}pt; {fit}")),
            None => (at, fit),
        };
        self.errors.push(self.error(at, message));
    }
}

/// The cells of a row with the column each starts in.
fn placed(row: &Row) -> impl Iterator<Item = (usize, &Cell)> {
    row.cells.iter().scan(0, |column, cell| {
        let start = *column;
        *column += cell.span;
        Some((start, cell))
    })
}

/// Widens `columns`, separated by `gap`, equally where a cell spanning them needs more minimum or
/// natural width than they have together.
fn spread(columns: &mut [Measure], cell: Measure, gap: f64) {
    let gaps = gap * (columns.len() - 1) as f64;
    let count = columns.len() as f64;
    let minimum = (cell.minimum - gaps - columns.iter().map(|c| c.minimum).sum::<f64>()) / count;
    let natural = (cell.natural - gaps - columns.iter().map(|c| c.natural).sum::<f64>()) / count;
    for column in columns.iter_mut() {
        if minimum > 0.0 {
            column.minimum += minimum;
        }
        column.natural = column.minimum.max(column.natural + natural.max(0.0));
    }
}

/// The minimum and natural widths of a prepared paragraph at `at` with `inset` added.
fn widths(prepared: &Prepared, inset: f64, at: Location) -> Measure {
    let (minimum, word) = prepared.minimum();
    Measure {
        minimum: minimum + inset,
        natural: prepared.natural() + inset,
        word: word.map(|word| (prepared.word(word), at)),
    }
}

/// Content widths for columns of the given kinds and measures within `available`, or `None` if their
/// minimum widths do not fit. `auto` columns are fitted into what the minimum widths of the `*`
/// columns leave, and the `*` columns fill the rest.
fn allocate(columns: &[Measure], kinds: &[ColumnWidth], available: f64) -> Option<Vec<f64>> {
    let of = |kind: ColumnWidth| -> Vec<usize> { (0..columns.len()).filter(|&i| kinds[i] == kind).collect() };
    let (auto, fill) = (of(ColumnWidth::Auto), of(ColumnWidth::Fill));
    let reserved: f64 = fill.iter().map(|&i| columns[i].minimum).sum();
    let ranges: Vec<(f64, f64)> = auto.iter().map(|&i| (columns[i].minimum, columns[i].natural)).collect();
    let auto_widths = fit(&ranges, available - reserved)?;
    let rest = available - auto_widths.iter().sum::<f64>();
    let ranges: Vec<(f64, f64)> = fill.iter().map(|&i| (columns[i].minimum, rest)).collect();
    let fill_widths = fit(&ranges, rest)?;
    let mut widths = vec![0.0; columns.len()];
    for (index, width) in auto
        .into_iter()
        .zip(auto_widths)
        .chain(fill.into_iter().zip(fill_widths))
    {
        widths[index] = width;
    }
    Some(widths)
}

/// Content widths for columns with the given (minimum, natural) widths within `available`, or `None`
/// if the minimum widths do not fit. Columns keep their natural widths when all fit. Otherwise each
/// gets a common width clamped to its own range, with the common width found by bisection so the
/// columns fill `available`.
fn fit(columns: &[(f64, f64)], available: f64) -> Option<Vec<f64>> {
    if columns.iter().map(|column| column.1).sum::<f64>() <= available {
        return Some(columns.iter().map(|column| column.1).collect());
    }
    if columns.iter().map(|column| column.0).sum::<f64>() > available {
        return None;
    }
    let width = |level: f64, (minimum, natural): (f64, f64)| level.min(natural).max(minimum);
    let (mut low, mut high) = (0.0, columns.iter().map(|column| column.1).fold(0.0, f64::max));
    for _ in 0..64 {
        let level = (low + high) / 2.0;
        if columns.iter().map(|&column| width(level, column)).sum::<f64>() <= available {
            low = level;
        } else {
            high = level;
        }
    }
    Some(columns.iter().map(|&column| width(low, column)).collect())
}

/// One row as a line: each cell's lines stacked from the top inside the padding at the cell's left edge,
/// the rule `above` on top and `below` at the bottom, both from `left` to `right`.
fn row_line(
    cells: Vec<(f64, Vec<(f64, Line)>)>,
    (left, right): (f64, f64),
    padding: f64,
    above: Option<Rule>,
    below: Option<Rule>,
) -> Line {
    let thickness = |rule: Option<Rule>| rule.map_or(0.0, |rule| rule.thickness.0);
    let (top, bottom) = (thickness(above), thickness(below));
    let height_of = |lines: &[(f64, Line)]| -> f64 {
        let spaces = lines.iter().skip(1).map(|line| line.0).sum::<f64>();
        spaces + lines.iter().map(|line| line.1.height).sum::<f64>()
    };
    let content = cells.iter().map(|(_, lines)| height_of(lines)).fold(0.0, f64::max);
    let height = top + 2.0 * padding + content + bottom;
    let baseline = top + padding + cells[0].1.first().map_or(0.0, |line| line.1.baseline);
    let mut items = Vec::new();
    let mut notes = Vec::new();
    for (x, lines) in cells {
        let mut y = top + padding;
        for (index, (space, line)) in lines.into_iter().enumerate() {
            if index > 0 {
                y += space;
            }
            items.extend(line.items.into_iter().map(|item| translate(item, x + padding, y)));
            notes.extend(line.notes);
            y += line.height;
        }
    }
    let mut rect = |y: f64, rule: Option<Rule>| {
        if let Some(rule) = rule.filter(|rule| rule.thickness.0 > 0.0) {
            let rect = Rect {
                x: Pt(left),
                y: Pt(y),
                width: Pt(right - left),
                height: rule.thickness,
            };
            items.push(Item::Rect {
                rect,
                color: rule.color,
            });
        }
    };
    rect(0.0, above);
    rect(height - bottom, below);
    Line {
        height,
        baseline,
        items,
        notes,
        hyphenated: false,
    }
}
