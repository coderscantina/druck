//! A table as a grid over its text. [`Grid`] reads a pipe table or a `::: table` list table with its caption, and every
//! change is a list of edits that leaves untouched rows, and in them untouched cells, byte for byte. Nothing is written
//! from a model: the grid is read again from the text after each edit.

mod list;
mod pipe;
#[cfg(test)]
mod tests;

use std::ops::Range;

use super::Key;
use super::facts::caption_words;
use crate::editor::commands::{line_ending, smallest_edit};
use crate::source::{BlockKind, Blocks, Edit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// As the theme's cell style has it.
    #[default]
    Default,
    Left,
    Center,
    Right,
}

/// One cell as the grid shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cell {
    /// The cell's Markdown, without the table's own escapes and indent.
    pub text: String,
    /// How many columns it covers. Only list tables have more than one.
    pub span: usize,
}

/// A caption's words and what stands in its braces, as written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Caption {
    pub text: String,
    /// The name after `tbl:`.
    pub label: String,
    /// `top` or `bottom`, or empty.
    pub placement: String,
}

/// Where the table's source is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    Pipe(pipe::Pipe),
    List(list::List),
}

/// A table read from the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub(crate) source: Source,
    /// Rows of cells. The first is the header.
    pub rows: Vec<Vec<Cell>>,
    pub columns: usize,
    pub aligns: Vec<Align>,
    pub caption: Caption,
    /// The caption's paragraph without its line endings, and the end of the block before it.
    caption_range: Option<Range<usize>>,
    /// The end of the table's last line, without its line ending: where a new caption follows.
    pub(crate) table_end: usize,
}

/// The kinds of table a block run can be: which blocks it takes, first to last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub first: usize,
    /// The last block, the caption if there is one.
    pub last: usize,
    /// The grid, or why the grid cannot show it.
    pub grid: Result<Grid, Key>,
}

/// The text after a `:::` in a directive line, trimmed.
fn directive_name(line: &str) -> &str {
    line.trim_start_matches(':').trim()
}

fn is_table_open(text: &str, block: &crate::source::blocks::Block) -> bool {
    block.kind == BlockKind::Directive && {
        let name = directive_name(text[block.range.clone()].lines().next().unwrap_or(""));
        name == "table" || name.starts_with("table ") || name.starts_with("table{")
    }
}

/// The caption paragraph standing right after block `last`, if there is one.
fn caption_block(text: &str, blocks: &Blocks, last: usize) -> Option<usize> {
    let list = blocks.as_slice();
    let (gap, block) = (list.get(last + 1)?, list.get(last + 2)?);
    (gap.kind == BlockKind::Blank && block.kind == BlockKind::Paragraph && text[block.range.clone()].starts_with(": "))
        .then_some(last + 2)
}

/// Every table of the text, in order.
pub fn regions(text: &str, blocks: &Blocks) -> Vec<Region> {
    let list = blocks.as_slice();
    let mut regions = Vec::new();
    let mut index = 0;
    while index < list.len() {
        let block = &list[index];
        if block.kind == BlockKind::Table {
            let caption = caption_block(text, blocks, index);
            let last = caption.unwrap_or(index);
            let grid =
                pipe::read(text, block.range.clone(), None).map(|grid| with_caption(grid, text, blocks, caption));
            regions.push(Region {
                first: index,
                last,
                grid,
            });
            index = last + 1;
            continue;
        }
        if is_table_open(text, block) {
            let close = list[index + 1..]
                .iter()
                .position(|block| block.kind == BlockKind::Directive)
                .map(|offset| index + 1 + offset);
            if let Some(close) = close.filter(|close| directive_name(&text[list[*close].range.clone()]).is_empty()) {
                let caption = caption_block(text, blocks, close);
                let last = caption.unwrap_or(close);
                let inner: Vec<usize> = (index + 1..close)
                    .filter(|at| list[*at].kind != BlockKind::Blank)
                    .collect();
                let grid = match inner.as_slice() {
                    [only] if list[*only].kind == BlockKind::Table => {
                        pipe::read(text, list[*only].range.clone(), Some(block.range.clone()))
                    }
                    [only] if list[*only].kind == BlockKind::List => {
                        list::read(text, block.range.clone(), list[*only].range.clone())
                    }
                    _ => Err(Key::TableBodyNotList),
                }
                .map(|grid| with_caption(grid, text, blocks, caption));
                regions.push(Region {
                    first: index,
                    last,
                    grid,
                });
                index = last + 1;
                continue;
            }
        }
        index += 1;
    }
    regions
}

fn with_caption(mut grid: Grid, text: &str, blocks: &Blocks, caption: Option<usize>) -> Grid {
    if let Some(block) = caption.map(|index| &blocks.as_slice()[index]) {
        let body = text[block.range.clone()].trim_end_matches(['\n', '\r']);
        grid.caption_range = Some(block.range.start..block.range.start + body.len());
        grid.caption = read_caption(body);
    }
    grid
}

fn read_caption(body: &str) -> Caption {
    let trimmed = body.trim_end();
    let mut caption = Caption {
        text: caption_words(body).to_owned(),
        ..Caption::default()
    };
    if let Some(open) = trimmed.rfind('{').filter(|_| trimmed.ends_with('}')) {
        for word in trimmed[open + 1..trimmed.len() - 1].split_whitespace() {
            if let Some(label) = word.strip_prefix("#tbl:") {
                caption.label = label.to_owned();
            } else if let Some(placement) = word.strip_prefix("placement=") {
                caption.placement = placement.to_owned();
            }
        }
    }
    caption
}

fn write_caption(caption: &Caption) -> String {
    let mut words = Vec::new();
    if !caption.label.trim().is_empty() {
        words.push(format!("#tbl:{}", caption.label.trim()));
    }
    if !caption.placement.is_empty() {
        words.push(format!("placement={}", caption.placement));
    }
    let braces = if words.is_empty() {
        String::new()
    } else {
        format!(" {{{}}}", words.join(" "))
    };
    format!(": {}{braces}", caption.text.trim())
}

/// Why an edit to a table cannot be written.
pub type Refused = Key;

impl Grid {
    pub fn is_list(&self) -> bool {
        matches!(self.source, Source::List(_))
    }

    /// The edits that make a cell's text `new`. Nothing outside the cell changes.
    pub fn set_cell(&self, text: &str, row: usize, cell: usize, new: &str) -> Result<Vec<Edit>, Refused> {
        match &self.source {
            Source::Pipe(pipe) => pipe.set_cell(text, row, cell, new),
            Source::List(list) => list.set_cell(text, row, cell, new),
        }
    }

    /// A new empty row after `row`. The header stays the first row.
    pub fn add_row(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        match &self.source {
            Source::Pipe(pipe) => pipe.add_row(text, row),
            Source::List(list) => list.add_row(text, row),
        }
    }

    pub fn remove_row(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        if row == 0 {
            return Err(Key::TableHeaderStays);
        }
        match &self.source {
            Source::Pipe(pipe) => pipe.remove_row(text, row),
            Source::List(list) => list.remove_row(row),
        }
    }

    /// Swaps a row with the one below it.
    pub fn move_row_down(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        if row + 1 >= self.rows.len() {
            return Err(Key::TableNoRowThere);
        }
        match &self.source {
            Source::Pipe(pipe) => pipe.swap_rows(text, row),
            Source::List(list) => list.swap_rows(text, row),
        }
    }

    /// A new empty column after `column`, or before the first with `None`.
    pub fn add_column(&self, text: &str, column: Option<usize>) -> Result<Vec<Edit>, Refused> {
        match &self.source {
            Source::Pipe(pipe) => pipe.add_column(column),
            Source::List(list) => list.add_column(text, column),
        }
    }

    pub fn remove_column(&self, text: &str, column: usize) -> Result<Vec<Edit>, Refused> {
        if self.columns < 2 {
            return Err(Key::TableOneColumnStays);
        }
        match &self.source {
            Source::Pipe(pipe) => pipe.remove_column(column),
            Source::List(list) => list.remove_column(text, column),
        }
    }

    /// Swaps a column with the one to its right.
    pub fn move_column_right(&self, text: &str, column: usize) -> Result<Vec<Edit>, Refused> {
        if column + 1 >= self.columns {
            return Err(Key::TableNoColumnThere);
        }
        match &self.source {
            Source::Pipe(pipe) => pipe.swap_columns(text, column),
            Source::List(list) => list.swap_columns(text, column),
        }
    }

    pub fn set_align(&self, text: &str, column: usize, align: Align) -> Result<Vec<Edit>, Refused> {
        match &self.source {
            Source::Pipe(pipe) => pipe.set_align(text, column, align),
            Source::List(list) => list.set_align(text, column, align),
        }
    }

    /// Makes a cell of a list table take in the empty cell to its right.
    pub fn merge_right(&self, text: &str, row: usize, cell: usize) -> Result<Vec<Edit>, Refused> {
        match &self.source {
            Source::Pipe(_) => Err(Key::TablePipeNoSpans),
            Source::List(list) => list.merge_right(text, row, cell),
        }
    }

    /// Gives back the last column a spanning cell of a list table covers, as an empty cell.
    pub fn split(&self, text: &str, row: usize, cell: usize) -> Result<Vec<Edit>, Refused> {
        match &self.source {
            Source::Pipe(_) => Err(Key::TablePipeNoSpans),
            Source::List(list) => list.split(text, row, cell),
        }
    }

    /// The edits that make the caption this one. An empty text takes the caption out, with its blank line.
    pub fn set_caption(&self, text: &str, caption: &Caption) -> Option<Edit> {
        let eol = line_ending(text);
        let written = write_caption(caption);
        let empty = caption.text.trim().is_empty();
        match (&self.caption_range, empty) {
            (Some(range), true) => Some(Edit::new(self.table_end..range.end, "")),
            (Some(range), false) => {
                let old = &text[range.clone()];
                (old != written).then(|| smallest_edit(range.start, old, &written))
            }
            (None, true) => None,
            (None, false) => Some(Edit::new(
                self.table_end..self.table_end,
                format!("{eol}{eol}{written}"),
            )),
        }
    }

    /// The whole table with its caption, for taking it out. The blank line after it goes too.
    pub fn removal(&self, text: &str, whole: Range<usize>) -> Edit {
        let after = text[whole.end..]
            .bytes()
            .take_while(|byte| *byte == b'\n' || *byte == b'\r')
            .count();
        if whole.end + after >= text.len() && whole.start > 0 {
            let start = text[..whole.start].trim_end_matches(['\n', '\r']).len();
            return Edit::new(start..whole.end, "");
        }
        Edit::new(whole.start..whole.end + after, "")
    }
}
