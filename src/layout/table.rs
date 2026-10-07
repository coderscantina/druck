//! Tables: column widths from shaped cell text, wrapped cells, and rows as lines of the flow.
//!
//! Every cell is shaped once and measured. Its natural width sets it on one line; its minimum width
//! holds its widest unbreakable piece, after hyphenation where the cell style hyphenates. When the
//! natural widths fit, every column keeps its natural width. Otherwise each column gets one common
//! width, clamped between the column's minimum and natural widths, chosen so the columns fill the
//! available width: narrow columns stay on one line and wide ones share the rest equally.
//!
//! A row is one line of the flow, as tall as its tallest cell plus padding, so pages and columns
//! break only between rows. The caption and the header row never end a page, so they stay with the
//! first row, and the composer sets the header again where a page or column starts at a later row.

use super::paragraph::{self, Prepared};
use super::{Break, Flow, Frame, Line, pages, translate, translate_line};
use crate::config::resolved::Style;
use crate::config::theme::Align;
use crate::config::values::{Color, Pt};
use crate::document::{ColumnAlign, Inline, Location, Row};
use crate::page::{Item, Rect};

/// Added to a column's width when its cells are set, so text measured to fit exactly is not
/// broken by rounding.
const SLACK: f64 = 1e-6;

/// A table as the parser delivers it.
pub(super) struct TableBlock<'d> {
    pub at: Location,
    pub align: &'d [Option<ColumnAlign>],
    pub header: &'d Row,
    pub rows: &'d [Row],
    pub caption: &'d [Inline],
}

impl<'a> Flow<'a> {
    /// Sets a table in `frame`, centered when narrower, with its numbered caption above it.
    pub(super) fn table(&mut self, table: TableBlock, frame: Frame<'a>) {
        let config = self.config;
        let (padding, rule) = (config.tables.cell_padding.0, config.tables.rule_thickness.0);
        let width = self.width(frame);
        let styles = |base: &Style| -> Vec<Style> {
            table
                .align
                .iter()
                .map(|align| {
                    let mut style = base.clone();
                    if let Some(align) = align {
                        style.align = match align {
                            ColumnAlign::Left => Align::Left,
                            ColumnAlign::Center => Align::Center,
                            ColumnAlign::Right => Align::Right,
                        };
                    }
                    style
                })
                .collect()
        };
        let (header_styles, body_styles) = (styles(&config.styles.table_header), styles(&config.styles.table_cell));

        let rows: Vec<(&Row, &[Style])> = std::iter::once((table.header, header_styles.as_slice()))
            .chain(table.rows.iter().map(|row| (row, body_styles.as_slice())))
            .collect();
        let lang = config.document.lang;
        let mut prepared: Vec<Vec<Prepared>> = Vec::with_capacity(rows.len());
        let mut failed = false;
        for (row, styles) in &rows {
            let mut cells = Vec::with_capacity(row.cells.len());
            for (cell, style) in row.cells.iter().zip(styles.iter()) {
                match paragraph::prepare(&cell.content, style, &config.inline, self.fonts, lang) {
                    Ok(cell) => cells.push(cell),
                    Err(problem) => {
                        self.errors.push(self.error(cell.at, problem));
                        failed = true;
                    }
                }
            }
            prepared.push(cells);
        }
        if failed {
            return;
        }

        let columns: Vec<(f64, f64)> = (0..table.align.len())
            .map(|column| {
                prepared.iter().fold((0.0f64, 0.0f64), |(minimum, natural), cells| {
                    let cell = &cells[column];
                    (minimum.max(cell.minimum().0), natural.max(cell.natural()))
                })
            })
            .collect();
        let padded = 2.0 * padding * columns.len() as f64;
        let Some(widths) = allocate(&columns, width - padded) else {
            let need = columns.iter().map(|column| column.0).sum::<f64>() + padded;
            self.too_narrow(&rows, &prepared, need, width);
            return;
        };
        let table_width = widths.iter().sum::<f64>() + padded;
        let left = frame.left + (width - table_width) / 2.0;

        let caption = if table.caption.is_empty() {
            Vec::new()
        } else {
            self.tables += 1;
            let label = &config.labels.table;
            match self.caption_lines(table.at, label, self.tables, table.caption, width) {
                Some(lines) => lines,
                None => return,
            }
        };

        let mut lines = Vec::with_capacity(rows.len());
        for (index, ((row, _), cells)) in rows.iter().zip(prepared).enumerate() {
            let mut set = Vec::with_capacity(cells.len());
            for ((cell, prepared), column) in row.cells.iter().zip(&cells).zip(&widths) {
                match prepared.lines(column + SLACK, 0.0) {
                    Ok(lines) => set.push(lines),
                    Err(problem) => {
                        self.errors.push(self.error(cell.at, problem));
                        return;
                    }
                }
            }
            let top = if index == 0 { rule } else { 0.0 };
            lines.push(row_line(
                set,
                &widths,
                left,
                padding,
                top,
                rule,
                config.tables.rule_color,
            ));
        }

        let style = &config.styles.caption;
        let start = self.lines.len();
        self.space(style.space_after.0);
        let dx = frame.left + style.indent.0;
        for line in caption {
            self.push(translate_line(line, dx), table.at, Break::Never);
        }
        if start < self.lines.len() {
            self.space(style.space_before.0);
        }
        let count = lines.len() - 1;
        let mut lines = lines.into_iter();
        let header = lines.next().expect("a table has a header row");
        let repeated = header.clone();
        let after = if count == 0 { Break::Allowed(0.0) } else { Break::Never };
        self.push(header, table.header.at, after);
        let first_row = self.lines.len();
        for (index, (line, row)) in lines.zip(table.rows).enumerate() {
            self.push(line, row.at, pages::line_break(index, count));
        }
        self.space(style.space_after.0);
        self.table_lines.push(pages::Table {
            lines: start..self.lines.len(),
            rows: (first_row + 1).min(self.lines.len())..self.lines.len(),
            header: repeated,
            at: table.at,
        });
    }

    /// Reports the cell with the widest unbreakable word when the columns cannot fit even at their
    /// minimum widths.
    fn too_narrow(&mut self, rows: &[(&Row, &[Style])], prepared: &[Vec<Prepared>], need: f64, width: f64) {
        let cells = rows
            .iter()
            .zip(prepared)
            .flat_map(|((row, _), cells)| row.cells.iter().zip(cells));
        let mut widest: Option<(f64, Location, String)> = None;
        for (cell, prepared) in cells {
            if let (minimum, Some(word)) = prepared.minimum()
                && widest.as_ref().is_none_or(|(most, ..)| minimum > *most)
            {
                widest = Some((minimum, cell.at, prepared.word(word)));
            }
        }
        let (minimum, at, word) = widest.expect("only words give a column a minimum width");
        let message = format!(
            "\"{word}\" needs {minimum:.1}pt; with every column at its narrowest this table is {need:.1}pt wide, \
             more than the available {width:.1}pt"
        );
        self.errors.push(self.error(at, message));
    }
}

/// Content widths for columns with the given (minimum, natural) widths within `available`, or `None`
/// if the minimum widths do not fit. Columns keep their natural widths when all fit. Otherwise each
/// gets a common width clamped to its own range, with the common width found by bisection so the
/// columns fill `available`.
fn allocate(columns: &[(f64, f64)], available: f64) -> Option<Vec<f64>> {
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

/// One row as a line: each cell's lines stacked from the top inside the padding, the columns
/// starting at `left`, a rule of thickness `top` above and of `rule` below.
fn row_line(cells: Vec<Vec<Line>>, widths: &[f64], left: f64, padding: f64, top: f64, rule: f64, color: Color) -> Line {
    let content = cells
        .iter()
        .map(|lines| lines.iter().map(|line| line.height).sum::<f64>())
        .fold(0.0, f64::max);
    let height = top + 2.0 * padding + content + rule;
    let baseline = top + padding + cells[0].first().map_or(0.0, |line| line.baseline);
    let mut items = Vec::new();
    let mut notes = Vec::new();
    let mut x = left;
    for (lines, width) in cells.into_iter().zip(widths) {
        let mut y = top + padding;
        for line in lines {
            items.extend(line.items.into_iter().map(|item| translate(item, x + padding, y)));
            notes.extend(line.notes);
            y += line.height;
        }
        x += width + 2.0 * padding;
    }
    let mut rect = |y: f64, height: f64| {
        if height > 0.0 {
            let rect = Rect {
                x: Pt(left),
                y: Pt(y),
                width: Pt(x - left),
                height: Pt(height),
            };
            items.push(Item::Rect { rect, color });
        }
    };
    rect(0.0, top);
    rect(height - rule, rule);
    Line {
        height,
        baseline,
        items,
        notes,
    }
}
