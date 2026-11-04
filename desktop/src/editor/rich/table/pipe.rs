//! Pipe tables. Each line splits into cells at unescaped pipes. A row keeps the width each cell was written with, as far
//! as its new text allows, so a table padded to line up stays lined up and no other row moves.

use std::ops::Range;

use super::{Align, Cell, Grid, Refused, Source};
use crate::editor::commands::line_ending;
use crate::editor::rich::Key;
use crate::source::Edit;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    /// Without the line ending.
    range: Range<usize>,
    /// Each cell between its pipes, padding included.
    cells: Vec<Range<usize>>,
    lead_pipe: bool,
    trail_pipe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipe {
    lines: Vec<Line>,
    columns: usize,
}

fn split_line(text: &str, range: Range<usize>) -> Line {
    let body = &text[range.clone()];
    let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
    let trimmed_end = body.trim_end_matches([' ', '\t']).len();
    let bytes = body.as_bytes();
    let mut pipes = Vec::new();
    let mut at = indent;
    while at < trimmed_end {
        match bytes[at] {
            b'\\' => at += 1,
            b'|' => pipes.push(at),
            _ => {}
        }
        at += 1;
    }
    let lead_pipe = pipes.first() == Some(&indent);
    let trail_pipe = pipes.last().is_some_and(|last| *last + 1 == trimmed_end) && (pipes.len() > 1 || !lead_pipe);
    let mut cells = Vec::new();
    let mut start = if lead_pipe { indent + 1 } else { indent };
    for pipe in pipes.iter().skip(usize::from(lead_pipe)) {
        cells.push(range.start + start..range.start + pipe);
        start = pipe + 1;
    }
    if !trail_pipe {
        cells.push(range.start + start..range.start + body.len());
    }
    Line {
        range,
        cells,
        lead_pipe,
        trail_pipe,
    }
}

fn unescaped(cell: &str) -> String {
    cell.trim().replace("\\|", "|")
}

fn escaped(cell: &str) -> String {
    cell.replace('\n', " ").replace('|', "\\|")
}

fn align_of(delimiter: &str) -> Align {
    let cell = delimiter.trim();
    match (cell.starts_with(':'), cell.ends_with(':')) {
        (true, true) => Align::Center,
        (true, false) => Align::Left,
        (false, true) => Align::Right,
        (false, false) => Align::Default,
    }
}

pub fn read(text: &str, block: Range<usize>, directive: Option<Range<usize>>) -> Result<Grid, Key> {
    let mut lines = Vec::new();
    let mut at = block.start;
    for line in text[block.clone()].split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        lines.push(split_line(text, at..at + content.len()));
        at += line.len();
    }
    if lines.len() < 2 {
        return Err(Key::TableBodyNotList);
    }
    let columns = lines[0].cells.len();
    let rows = lines
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 1)
        .map(|(_, line)| {
            line.cells
                .iter()
                .map(|cell| Cell {
                    text: unescaped(&text[cell.clone()]),
                    span: 1,
                })
                .collect()
        })
        .collect();
    let aligns = lines[1]
        .cells
        .iter()
        .map(|cell| align_of(&text[cell.clone()]))
        .collect();
    let table_end = lines.last().map_or(block.end, |line| line.range.end);
    // A table inside `::: table` ends at the directive's close, which a caption follows.
    let table_end = match &directive {
        Some(_) => {
            let rest = &text[block.end..];
            let close = rest.find(":::").map_or(block.end, |at| block.end + at);
            close + text[close..].find(['\n', '\r']).unwrap_or(text.len() - close)
        }
        None => table_end,
    };
    Ok(Grid {
        source: Source::Pipe(Pipe { lines, columns }),
        rows,
        columns,
        aligns,
        caption: super::Caption::default(),
        caption_range: None,
        table_end,
    })
}

/// The cell text padded to the width a cell was written with, as far as it fits. A cell that lines up its text at the
/// right keeps it there.
fn padded(old: &str, new: &str) -> String {
    let lead = old.len() - old.trim_start_matches(' ').len();
    let trimmed = old.trim();
    let trail = if trimmed.is_empty() {
        0
    } else {
        old.len() - old.trim_end_matches(' ').len()
    };
    let width = old.chars().count();
    let new_width = new.chars().count();
    let right_aligned = lead > 1 && trail <= 1;
    let (min_lead, min_trail) = (
        lead.min(1),
        trail.min(1).max(usize::from(trimmed.is_empty() && !old.is_empty())),
    );
    let slack = width.saturating_sub(new_width + min_lead + min_trail);
    if right_aligned {
        format!("{}{new}{}", " ".repeat(min_lead + slack), " ".repeat(min_trail))
    } else {
        format!("{}{new}{}", " ".repeat(min_lead), " ".repeat(min_trail + slack))
    }
}

impl Pipe {
    /// The line of a grid row: the delimiter is not a row.
    fn line_of(&self, row: usize) -> &Line {
        &self.lines[if row == 0 { 0 } else { row + 1 }]
    }

    pub fn set_cell(&self, text: &str, row: usize, column: usize, new: &str) -> Result<Vec<Edit>, Refused> {
        let line = self.line_of(row);
        let new = escaped(new);
        if let Some(cell) = line.cells.get(column) {
            if unescaped(&text[cell.clone()]) == new.replace("\\|", "|") {
                return Ok(Vec::new());
            }
            return Ok(vec![Edit::new(cell.clone(), padded(&text[cell.clone()], &new))]);
        }
        // A row with fewer cells than the header gets the ones it lacks.
        let missing = column - line.cells.len();
        let mut added = String::new();
        if !line.trail_pipe {
            added.push('|');
        }
        for _ in 0..missing {
            added.push_str("  |");
        }
        added.push_str(&format!(" {new} "));
        if line.trail_pipe {
            added.push('|');
        }
        let end = line.range.start + text[line.range.clone()].trim_end_matches([' ', '\t']).len();
        Ok(vec![Edit::new(end..end, added)])
    }

    fn empty_line(&self, text: &str, template: &Line) -> String {
        let mut new = String::new();
        if template.lead_pipe {
            new.push('|');
        }
        for column in 0..self.columns {
            let width = template
                .cells
                .get(column)
                .map_or(2, |cell| text[cell.clone()].chars().count());
            new.push_str(&" ".repeat(width.max(2)));
            if column + 1 < self.columns || template.trail_pipe {
                new.push('|');
            }
        }
        new
    }

    pub fn add_row(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        let eol = line_ending(text);
        let line = if row == 0 { &self.lines[1] } else { self.line_of(row) };
        let new = self.empty_line(text, if row == 0 { &self.lines[0] } else { line });
        Ok(vec![Edit::new(line.range.end..line.range.end, format!("{eol}{new}"))])
    }

    pub fn remove_row(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        let line = self.line_of(row);
        let eol = line_ending(text);
        let last = row + 2 == self.lines.len();
        Ok(vec![if last {
            Edit::new(line.range.start.saturating_sub(eol.len())..line.range.end, "")
        } else {
            Edit::new(line.range.start..line.range.end + eol.len(), "")
        }])
    }

    pub fn swap_rows(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        let (upper, lower) = (self.line_of(row), self.line_of(row + 1));
        Ok(vec![
            Edit::new(upper.range.clone(), &text[lower.range.clone()]),
            Edit::new(lower.range.clone(), &text[upper.range.clone()]),
        ])
    }

    pub fn add_column(&self, column: Option<usize>) -> Result<Vec<Edit>, Refused> {
        Ok(self
            .lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let empty = if index == 1 { " --- " } else { "  " };
                match column {
                    None => line
                        .cells
                        .first()
                        .map(|first| Edit::new(first.start..first.start, format!("{empty}|"))),
                    Some(column) => {
                        let cell = line.cells.get(column)?;
                        // The new cell goes before the pipe that closes this one.
                        Some(Edit::new(cell.end..cell.end, format!("|{empty}")))
                    }
                }
            })
            .collect())
    }

    pub fn remove_column(&self, column: usize) -> Result<Vec<Edit>, Refused> {
        Ok(self
            .lines
            .iter()
            .filter_map(|line| {
                let cell = line.cells.get(column)?;
                Some(match line.cells.get(column + 1) {
                    Some(next) => Edit::new(cell.start..next.start, ""),
                    None if column > 0 => Edit::new(line.cells[column - 1].end..cell.end, ""),
                    None => Edit::new(cell.clone(), ""),
                })
            })
            .collect())
    }

    pub fn swap_columns(&self, text: &str, column: usize) -> Result<Vec<Edit>, Refused> {
        let mut edits = Vec::new();
        for line in &self.lines {
            if let (Some(left), Some(right)) = (line.cells.get(column), line.cells.get(column + 1)) {
                edits.push(Edit::new(left.clone(), &text[right.clone()]));
                edits.push(Edit::new(right.clone(), &text[left.clone()]));
            }
        }
        Ok(edits)
    }

    pub fn set_align(&self, text: &str, column: usize, align: Align) -> Result<Vec<Edit>, Refused> {
        let Some(cell) = self.lines[1].cells.get(column) else {
            return Err(Key::TableNoColumnThere);
        };
        let old = &text[cell.clone()];
        let trimmed = old.trim();
        let lead = old.len() - old.trim_start().len();
        let width = trimmed.len().max(3);
        let new = match align {
            Align::Default => "-".repeat(width),
            Align::Left => format!(":{}", "-".repeat(width - 1)),
            Align::Right => format!("{}:", "-".repeat(width - 1)),
            Align::Center => format!(":{}:", "-".repeat(width - 2)),
        };
        let start = cell.start + lead;
        Ok(vec![Edit::new(start..start + trimmed.len(), new)])
    }
}
