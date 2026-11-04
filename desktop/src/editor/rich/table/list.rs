//! `::: table` list tables: a list with one item per row, and in each row a nested list with one item per cell. A cell's
//! text keeps its own lines and indent, and a change writes only that cell, or that row's attributes.

use std::ops::Range;

use super::{Align, Cell, Grid, Refused, Source};
use crate::editor::commands::line_ending;
use crate::editor::rich::Key;
use crate::source::Edit;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListCell {
    /// From the cell's `-` to the end of its last line.
    range: Range<usize>,
    /// After the `- `: attributes and text, for swapping cells.
    inner: Range<usize>,
    /// The attributes with their braces and the space or line break up to the text.
    attributes: Option<Range<usize>>,
    style: Option<String>,
    span: usize,
    /// The text.
    content: Range<usize>,
    /// How far the text's later lines are indented.
    indent: usize,
    /// The marker's column.
    column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListRow {
    /// From the row's own marker to the end of its last cell.
    range: Range<usize>,
    /// Whole lines, line ending of the last one not included.
    line_start: usize,
    cells: Vec<ListCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    directive: Range<usize>,
    rows: Vec<ListRow>,
    columns: usize,
    /// The list's bullet as written.
    bullet: char,
}

fn indent_of(line: &str) -> usize {
    line.bytes().take_while(|byte| *byte == b' ').count()
}

fn is_marker(line: &str) -> Option<char> {
    let mut chars = line.chars();
    let bullet = chars.next().filter(|bullet| matches!(bullet, '-' | '*' | '+'))?;
    matches!(chars.next(), None | Some(' ')).then_some(bullet)
}

/// `{.name span=2}` at `at`: its end, past the braces.
fn attributes_at(text: &str, at: usize, end: usize) -> Option<usize> {
    let rest = &text[at..end];
    if !rest.starts_with('{') {
        return None;
    }
    rest.find('}').map(|close| at + close + 1)
}

fn read_attributes(group: &str) -> Result<(Option<String>, usize), Key> {
    let inner = group.trim_start_matches('{').trim_end_matches('}');
    let (mut style, mut span) = (None, 1);
    for word in inner.split_whitespace() {
        if let Some(name) = word.strip_prefix('.') {
            style = Some(name.to_owned());
        } else if let Some(count) = word.strip_prefix("span=") {
            span = count.parse().map_err(|_| Key::TableAttributes)?;
        } else {
            return Err(Key::TableAttributes);
        }
    }
    Ok((style, span))
}

/// The lines of the list, with where each starts.
fn lines_in(text: &str, range: &Range<usize>) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut at = range.start;
    for line in text[range.clone()].split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        lines.push(at..at + content.len());
        at += line.len();
    }
    lines
}

/// A word in the directive line's braces: `key="a b"` or `key=a`.
fn attribute_value<'a>(directive: &'a str, key: &str) -> Option<&'a str> {
    let at = directive.find(&format!("{key}="))?;
    let rest = &directive[at + key.len() + 1..];
    match rest.strip_prefix('"') {
        Some(quoted) => quoted.find('"').map(|end| &quoted[..end]),
        None => Some(rest.split([' ', '}']).next().unwrap_or("")),
    }
}

/// The edit that sets a directive attribute to `value`, keeping what else stands there.
fn attribute_edit(text: &str, directive: &Range<usize>, key: &str, value: &str) -> Edit {
    let line = &text[directive.clone()];
    let line = line.trim_end_matches(['\n', '\r']);
    let written = format!("{key}=\"{value}\"");
    if let Some(at) = line.find(&format!("{key}=")) {
        let rest = &line[at + key.len() + 1..];
        let len = match rest.strip_prefix('"') {
            Some(quoted) => quoted.find('"').map_or(rest.len(), |end| end + 2),
            None => rest.find([' ', '}']).unwrap_or(rest.len()),
        };
        let start = directive.start + at;
        return Edit::new(start..start + key.len() + 1 + len, written);
    }
    match line.rfind('}') {
        Some(close) => {
            let at = directive.start + close;
            Edit::new(at..at, format!(" {written}"))
        }
        None => {
            let at = directive.start + line.len();
            Edit::new(at..at, format!(" {{{written}}}"))
        }
    }
}

pub fn read(text: &str, directive: Range<usize>, list: Range<usize>) -> Result<Grid, Key> {
    let lines = lines_in(text, &list);
    let row_indent = lines.first().map_or(0, |line| indent_of(&text[line.clone()]));
    let bullet = lines
        .first()
        .and_then(|line| is_marker(&text[line.start + row_indent..line.end]))
        .ok_or(Key::TableBodyNotList)?;
    let mut rows: Vec<ListRow> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].clone();
        let written = &text[line.clone()];
        if written.trim().is_empty() {
            index += 1;
            continue;
        }
        if indent_of(written) != row_indent || is_marker(&written[row_indent..]).is_none() {
            return Err(Key::TableTextOutsideCells);
        }
        let mut row = ListRow {
            range: line.start + row_indent..line.end,
            line_start: line.start,
            cells: Vec::new(),
        };
        let after_marker = line.start + row_indent + 1;
        let after_marker = after_marker
            + text[after_marker..line.end]
                .bytes()
                .take_while(|byte| *byte == b' ')
                .count();
        let mut first_cell_at = None;
        if after_marker < line.end {
            if let Some(end) = attributes_at(text, after_marker, line.end) {
                // Row attributes: they stand alone here, or before the first cell on this line.
                let after = end + text[end..line.end].bytes().take_while(|byte| *byte == b' ').count();
                if after < line.end {
                    first_cell_at = Some(after);
                }
            } else {
                first_cell_at = Some(after_marker);
            }
        }
        index += 1;
        // The row's cells: one starts on the marker's line, and the others on lines of their own.
        let mut cell_at = first_cell_at.map(|at| (at, line.end, at - line.start));
        let cell_column = cell_at.map(|(_, _, column)| column);
        let mut expected = cell_column;
        loop {
            let (at, line_end, column) = match cell_at.take() {
                Some(found) => found,
                None => {
                    // Skip blank lines inside a row, when a cell or a line of it follows.
                    let mut peek = index;
                    while peek < lines.len() && text[lines[peek].clone()].trim().is_empty() {
                        peek += 1;
                    }
                    let Some(next) = lines.get(peek).cloned() else {
                        break;
                    };
                    let written = &text[next.clone()];
                    let column = indent_of(written);
                    if column <= row_indent {
                        break;
                    }
                    let is_cell =
                        expected.is_none_or(|expected| expected == column) && is_marker(&written[column..]).is_some();
                    if !is_cell {
                        return Err(Key::TableTextOutsideCells);
                    }
                    expected = Some(column);
                    index = peek + 1;
                    (next.start + column, next.end, column)
                }
            };
            if is_marker(&text[at..line_end]).is_none() {
                return Err(Key::TableTextOutsideCells);
            }
            let (cell, consumed) = read_cell(text, &lines, index, at, line_end, column)?;
            index = consumed;
            row.range.end = cell.range.end;
            row.cells.push(cell);
        }
        if row.cells.is_empty() {
            return Err(Key::TableTextOutsideCells);
        }
        rows.push(row);
    }
    let columns: usize = rows
        .first()
        .map_or(0, |row| row.cells.iter().map(|cell| cell.span).sum());
    if columns == 0 {
        return Err(Key::TableBodyNotList);
    }
    let directive_line = text[directive.clone()].to_owned();
    let aligns: Vec<Align> = match attribute_value(&directive_line, "align") {
        Some(value) => value
            .split_whitespace()
            .map(|word| match word {
                "left" => Align::Left,
                "center" => Align::Center,
                "right" => Align::Right,
                _ => Align::Default,
            })
            .collect(),
        None => vec![Align::Default; columns],
    };
    let grid_rows = rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| Cell {
                    text: dedented(&text[cell.content.clone()], cell.indent),
                    span: cell.span,
                })
                .collect()
        })
        .collect();
    let close_end = list.end
        + text[list.end..].find(":::").map_or(0, |at| {
            at + text[list.end + at..]
                .find(['\n', '\r'])
                .unwrap_or(text.len() - list.end - at)
        });
    Ok(Grid {
        source: Source::List(List {
            directive,
            rows,
            columns,
            bullet,
        }),
        rows: grid_rows,
        columns,
        aligns,
        caption: super::Caption::default(),
        caption_range: None,
        table_end: close_end,
    })
}

/// One cell, which starts at `at` on a line ending at `line_end`, with the lines from `next` that belong to it. Returns
/// it and the index of the first line that does not.
fn read_cell(
    text: &str,
    lines: &[Range<usize>],
    next: usize,
    at: usize,
    line_end: usize,
    column: usize,
) -> Result<(ListCell, usize), Key> {
    let after_bullet = at + 1;
    let inner = after_bullet
        + text[after_bullet..line_end]
            .bytes()
            .take_while(|byte| *byte == b' ')
            .count();
    let indent = column + (inner - at).max(2);
    let mut end_line = next;
    let mut last_end = line_end;
    let mut scan = next;
    while scan < lines.len() {
        let written = &text[lines[scan].clone()];
        if written.trim().is_empty() {
            scan += 1;
            continue;
        }
        if indent_of(written) < indent {
            break;
        }
        last_end = lines[scan].end;
        scan += 1;
        end_line = scan;
    }
    let mut style = None;
    let mut span = 1;
    let mut attributes = None;
    let mut content_start = inner;
    if let Some(close) = attributes_at(text, inner, line_end) {
        (style, span) = read_attributes(&text[inner..close])?;
        let spaces = text[close..line_end].bytes().take_while(|byte| *byte == b' ').count();
        if close + spaces < line_end {
            content_start = close + spaces;
        } else {
            // Alone on the line: the text starts on the next one, past its indent.
            let next_line = lines.get(next).filter(|_| end_line > next);
            content_start = next_line.map_or(close, |line| line.start + indent_of(&text[line.clone()]));
        }
        attributes = Some(inner..content_start);
    }
    let content_end = last_end.max(content_start);
    Ok((
        ListCell {
            range: at..content_end,
            inner: inner.min(content_end)..content_end,
            attributes,
            style,
            span,
            content: content_start..content_end,
            indent,
            column,
        },
        end_line,
    ))
}

fn dedented(content: &str, indent: usize) -> String {
    let mut text = String::with_capacity(content.len());
    for (index, line) in content.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if index > 0 {
            text.push('\n');
            let spaces = indent_of(line).min(indent);
            text.push_str(&line[spaces..]);
        } else {
            text.push_str(line);
        }
    }
    text
}

fn indented(new: &str, indent: usize, eol: &str) -> String {
    let pad = " ".repeat(indent);
    let mut text = String::with_capacity(new.len());
    for (index, line) in new.split('\n').enumerate() {
        if index > 0 {
            text.push_str(eol);
            if !line.is_empty() {
                text.push_str(&pad);
            }
        }
        text.push_str(line);
    }
    text
}

impl List {
    fn cell_at_column(&self, row: usize, column: usize) -> Option<(usize, usize)> {
        let mut covered = 0;
        for (index, cell) in self.rows[row].cells.iter().enumerate() {
            if column < covered + cell.span {
                return Some((index, column - covered));
            }
            covered += cell.span;
        }
        None
    }

    pub fn set_cell(&self, text: &str, row: usize, cell: usize, new: &str) -> Result<Vec<Edit>, Refused> {
        let cell = self.rows[row].cells.get(cell).ok_or(Key::TableNoColumnThere)?;
        let eol = line_ending(text);
        let new = indented(new, cell.indent, eol);
        if text[cell.content.clone()] == new {
            return Ok(Vec::new());
        }
        // An empty cell may have no space after its marker yet.
        let needs_space = cell.content.start == cell.range.start + 1;
        let written = if needs_space { format!(" {new}") } else { new };
        Ok(vec![Edit::new(cell.content.clone(), written)])
    }

    /// A new row's text: empty cells in this list's form.
    fn empty_row(&self, template: &ListRow, eol: &str) -> String {
        let row_indent = template.range.start - template.line_start;
        let column = template.cells.first().map_or(row_indent + 2, |cell| cell.column);
        let mut new = format!("{}{} ", " ".repeat(row_indent), self.bullet);
        for index in 0..self.columns {
            if index > 0 {
                new.push_str(eol);
                new.push_str(&" ".repeat(column));
            }
            new.push(self.bullet);
            new.push(' ');
        }
        new
    }

    pub fn add_row(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        let eol = line_ending(text);
        let template = &self.rows[row];
        let new = self.empty_row(template, eol);
        Ok(vec![Edit::new(
            template.range.end..template.range.end,
            format!("{eol}{new}"),
        )])
    }

    pub fn remove_row(&self, row: usize) -> Result<Vec<Edit>, Refused> {
        let target = &self.rows[row];
        Ok(vec![if row + 1 == self.rows.len() {
            Edit::new(self.rows[row - 1].range.end..target.range.end, "")
        } else {
            Edit::new(target.line_start..self.rows[row + 1].line_start, "")
        }])
    }

    pub fn swap_rows(&self, text: &str, row: usize) -> Result<Vec<Edit>, Refused> {
        let (upper, lower) = (&self.rows[row], &self.rows[row + 1]);
        let upper_range = upper.line_start..upper.range.end;
        let lower_range = lower.line_start..lower.range.end;
        Ok(vec![
            Edit::new(upper_range.clone(), &text[lower_range.clone()]),
            Edit::new(lower_range, &text[upper_range]),
        ])
    }

    /// The edit that writes a cell's attributes with this span, keeping its style.
    fn span_edit(&self, cell: &ListCell, span: usize) -> Edit {
        let mut words = Vec::new();
        if let Some(style) = &cell.style {
            words.push(format!(".{style}"));
        }
        if span > 1 {
            words.push(format!("span={span}"));
        }
        let group = if words.is_empty() {
            String::new()
        } else {
            format!("{{{}}} ", words.join(" "))
        };
        match &cell.attributes {
            Some(range) => Edit::new(range.clone(), group),
            None => {
                let at = cell.content.start;
                let needs_space = at == cell.range.start + 1;
                Edit::new(at..at, if needs_space { format!(" {group}") } else { group })
            }
        }
    }

    fn with_directive_lists(&self, text: &str, change: impl Fn(&mut Vec<String>)) -> Vec<Edit> {
        let line = text[self.directive.clone()].to_owned();
        ["align", "widths"]
            .into_iter()
            .filter_map(|key| {
                let value = attribute_value(&line, key)?;
                let mut words: Vec<String> = value.split_whitespace().map(str::to_owned).collect();
                change(&mut words);
                Some(attribute_edit(text, &self.directive, key, &words.join(" ")))
            })
            .collect()
    }

    pub fn add_column(&self, text: &str, column: Option<usize>) -> Result<Vec<Edit>, Refused> {
        let eol = line_ending(text);
        let new_column = column.map_or(0, |column| column + 1);
        let mut edits = self.with_directive_lists(text, |words| {
            let at = new_column.min(words.len());
            let filler = if words.iter().any(|word| word == "*" || word == "auto") {
                "auto"
            } else {
                "left"
            };
            words.insert(at, filler.to_owned());
        });
        for row in 0..self.rows.len() {
            let cells = &self.rows[row].cells;
            match column {
                None => {
                    let first = &cells[0];
                    edits.push(Edit::new(
                        first.range.start..first.range.start,
                        format!("{} {eol}{}", self.bullet, " ".repeat(first.column)),
                    ));
                }
                Some(column) => {
                    let (index, into) = self.cell_at_column(row, column).ok_or(Key::TableNoColumnThere)?;
                    let cell = &cells[index];
                    if into + 1 < cell.span {
                        // The new column lands inside a spanning cell, which then covers it too.
                        edits.push(self.span_edit(cell, cell.span + 1));
                    } else {
                        edits.push(Edit::new(
                            cell.range.end..cell.range.end,
                            format!("{eol}{}{} ", " ".repeat(cell.column), self.bullet),
                        ));
                    }
                }
            }
        }
        Ok(edits)
    }

    pub fn remove_column(&self, text: &str, column: usize) -> Result<Vec<Edit>, Refused> {
        let mut edits = self.with_directive_lists(text, |words| {
            if column < words.len() {
                words.remove(column);
            }
        });
        for row in 0..self.rows.len() {
            let cells = &self.rows[row].cells;
            let (index, _) = self.cell_at_column(row, column).ok_or(Key::TableNoColumnThere)?;
            let cell = &cells[index];
            if cell.span > 1 {
                edits.push(self.span_edit(cell, cell.span - 1));
            } else if index == 0 {
                // The next cell moves up onto the row's marker line. It keeps its own indent for later lines.
                let next = cells.get(1).ok_or(Key::TableOneColumnStays)?;
                edits.push(Edit::new(cell.range.start..next.range.start, ""));
            } else {
                edits.push(Edit::new(cells[index - 1].range.end..cell.range.end, ""));
            }
        }
        Ok(edits)
    }

    pub fn swap_columns(&self, text: &str, column: usize) -> Result<Vec<Edit>, Refused> {
        let mut edits = self.with_directive_lists(text, |words| {
            if column + 1 < words.len() {
                words.swap(column, column + 1);
            }
        });
        for row in 0..self.rows.len() {
            let (left, _) = self.cell_at_column(row, column).ok_or(Key::TableNoColumnThere)?;
            let (right, _) = self.cell_at_column(row, column + 1).ok_or(Key::TableNoColumnThere)?;
            let cells = &self.rows[row].cells;
            if left == right || cells[left].span > 1 || cells[right].span > 1 {
                return Err(Key::TableSpanInTheWay);
            }
            let (one, other) = (&cells[left], &cells[right]);
            edits.push(Edit::new(one.inner.clone(), &text[other.inner.clone()]));
            edits.push(Edit::new(other.inner.clone(), &text[one.inner.clone()]));
        }
        Ok(edits)
    }

    pub fn set_align(&self, text: &str, column: usize, align: Align) -> Result<Vec<Edit>, Refused> {
        let line = &text[self.directive.clone()];
        let mut words: Vec<String> = match attribute_value(line, "align") {
            Some(value) => value.split_whitespace().map(str::to_owned).collect(),
            // The list needs a word for every column, and `left` is what it can say for "as it was".
            None => vec!["left".to_owned(); self.columns],
        };
        let word = match align {
            Align::Default | Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        };
        *words.get_mut(column).ok_or(Key::TableNoColumnThere)? = word.to_owned();
        Ok(vec![attribute_edit(text, &self.directive, "align", &words.join(" "))])
    }

    pub fn merge_right(&self, text: &str, row: usize, cell: usize) -> Result<Vec<Edit>, Refused> {
        let cells = &self.rows[row].cells;
        let (mine, next) = (
            cells.get(cell).ok_or(Key::TableNoColumnThere)?,
            cells.get(cell + 1).ok_or(Key::TableNoColumnThere)?,
        );
        if !text[next.content.clone()].trim().is_empty() {
            return Err(Key::TableMergeNotEmpty);
        }
        Ok(vec![
            self.span_edit(mine, mine.span + next.span),
            Edit::new(mine.range.end..next.range.end, ""),
        ])
    }

    pub fn split(&self, text: &str, row: usize, cell: usize) -> Result<Vec<Edit>, Refused> {
        let eol = line_ending(text);
        let mine = self.rows[row].cells.get(cell).ok_or(Key::TableNoColumnThere)?;
        if mine.span < 2 {
            return Err(Key::TableNoSpan);
        }
        Ok(vec![
            self.span_edit(mine, mine.span - 1),
            Edit::new(
                mine.range.end..mine.range.end,
                format!("{eol}{}{} ", " ".repeat(mine.column), self.bullet),
            ),
        ])
    }
}
