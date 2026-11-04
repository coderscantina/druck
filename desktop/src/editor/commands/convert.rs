//! Turning a block into another type of block, by rewriting the block's own text.
//!
//! A block is read into items of plain lines and written back in the target's syntax. A paragraph is one item. List
//! items, and the parts of a quotation between blank lines, are one item each. Blocks that are not running text, such as
//! tables and directives, have no conversions and keep their text.

use std::ops::Range;

use super::{Command, body_range, line_ending, needs_gap, normalized, selection_after, smallest_edit};
use crate::source::inline::{SpanKind, inline_spans, line_markers};
use crate::source::{BlockKind, Blocks, Edit, Selection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callout {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl Callout {
    pub const ALL: [Self; 5] = [Self::Note, Self::Tip, Self::Important, Self::Warning, Self::Caution];

    pub fn marker(self) -> &'static str {
        match self {
            Self::Note => "[!NOTE]",
            Self::Tip => "[!TIP]",
            Self::Important => "[!IMPORTANT]",
            Self::Warning => "[!WARNING]",
            Self::Caution => "[!CAUTION]",
        }
    }

    fn from_line(line: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|callout| line.trim_end() == callout.marker())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Paragraph,
    Heading(u8),
    Bullets,
    Numbers,
    Quote,
    Callout(Callout),
    Code,
}

/// What a gutter menu offers to turn a block into, in order.
pub const TARGETS: [BlockType; 15] = [
    BlockType::Paragraph,
    BlockType::Heading(1),
    BlockType::Heading(2),
    BlockType::Heading(3),
    BlockType::Heading(4),
    BlockType::Heading(5),
    BlockType::Heading(6),
    BlockType::Bullets,
    BlockType::Numbers,
    BlockType::Quote,
    BlockType::Callout(Callout::Note),
    BlockType::Callout(Callout::Tip),
    BlockType::Callout(Callout::Warning),
    BlockType::Callout(Callout::Important),
    BlockType::Code,
];

/// A code line wider than the text area is an error, so longer lines do not become code. The narrowest bundled theme
/// holds this many characters.
const MAX_CODE_LINE: usize = 60;

/// The type of a block that can be converted, from its text.
pub fn type_of(kind: BlockKind, text: &str) -> Option<BlockType> {
    match kind {
        BlockKind::Paragraph => (!is_image(text)).then_some(BlockType::Paragraph),
        BlockKind::Heading { level } => Some(BlockType::Heading(level)),
        BlockKind::Quote => {
            let first = quote_lines(text).into_iter().next().unwrap_or_default();
            Some(Callout::from_line(&first).map_or(BlockType::Quote, BlockType::Callout))
        }
        BlockKind::Code => Some(BlockType::Code),
        BlockKind::List => {
            let marker = line_markers(kind, text).into_iter().next()?;
            let numbered = text[marker.range].starts_with(|c: char| c.is_ascii_digit());
            Some(if numbered {
                BlockType::Numbers
            } else {
                BlockType::Bullets
            })
        }
        _ => None,
    }
}

/// Whether a paragraph is an image, which stands alone and has no other form.
fn is_image(text: &str) -> bool {
    text.trim_start().starts_with("![")
}

struct Item {
    level: usize,
    lines: Vec<String>,
}

fn plain_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

fn quote_lines(text: &str) -> Vec<String> {
    let markers = line_markers(BlockKind::Quote, text);
    let mut lines = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let end = start + content.len();
        let lead = markers
            .iter()
            .find(|marker| marker.range.start == start)
            .map_or(start, |marker| marker.range.end.min(end));
        lines.push(text[lead..end].to_owned());
        start += line.len();
    }
    lines
}

/// Splits lines into items at blank lines.
fn split_at_blanks(lines: Vec<String>) -> Vec<Item> {
    let mut items = Vec::new();
    let mut current = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            if !current.is_empty() {
                items.push(Item {
                    level: 0,
                    lines: std::mem::take(&mut current),
                });
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        items.push(Item {
            level: 0,
            lines: current,
        });
    }
    items
}

fn items_of(kind: BlockKind, text: &str) -> Option<Vec<Item>> {
    let items = match kind {
        BlockKind::Paragraph => vec![Item {
            level: 0,
            lines: plain_lines(text),
        }],
        BlockKind::Heading { .. } => {
            let markers = line_markers(kind, text);
            let line_end = text.lines().next().map_or(0, str::len);
            let setext = text.contains('\n') && markers.first().is_some_and(|marker| marker.range.start > 0);
            let lines = if setext {
                let underline = markers.first()?.range.start;
                plain_lines(&text[..underline])
            } else {
                let start = markers.first().map_or(0, |marker| marker.range.end).min(line_end);
                let end = markers.get(1).map_or(line_end, |marker| marker.range.start);
                vec![text[start..end.max(start)].to_owned()]
            };
            vec![Item { level: 0, lines }]
        }
        BlockKind::Quote => {
            let mut lines = quote_lines(text);
            if lines.first().is_some_and(|line| Callout::from_line(line).is_some()) {
                lines.remove(0);
            }
            split_at_blanks(lines)
        }
        BlockKind::List => list_items(text),
        BlockKind::Code => vec![Item {
            level: 0,
            lines: code_lines(text),
        }],
        _ => return None,
    };
    (!items.is_empty()).then_some(items)
}

/// The items of a list with their depth, and each item's lines without its marker and indentation.
fn list_items(text: &str) -> Vec<Item> {
    let markers = line_markers(BlockKind::List, text);
    let mut items: Vec<Item> = Vec::new();
    // The indentation of each open level, and of the text in the current item.
    let mut levels: Vec<usize> = Vec::new();
    let mut text_indent = 0;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let end = start + content.len();
        let spaces = content.bytes().take_while(|byte| *byte == b' ').count();
        if let Some(marker) = markers
            .iter()
            .find(|marker| start <= marker.range.start && marker.range.start <= end)
        {
            while levels.last().is_some_and(|open| *open > spaces) {
                levels.pop();
            }
            if levels.last() != Some(&spaces) {
                levels.push(spaces);
            }
            text_indent = marker.range.end - start;
            items.push(Item {
                level: levels.len() - 1,
                lines: vec![text[marker.range.end.min(end)..end].to_owned()],
            });
        } else if let Some(item) = items.last_mut() {
            let lead = spaces.min(text_indent);
            item.lines.push(content[lead..].to_owned());
        }
        start += line.len();
    }
    for item in &mut items {
        while item.lines.last().is_some_and(|line| line.trim().is_empty()) {
            item.lines.pop();
        }
    }
    items
}

fn code_lines(text: &str) -> Vec<String> {
    let lines = plain_lines(text);
    let fenced = lines
        .first()
        .is_some_and(|first| first.trim_start().starts_with("```") || first.trim_start().starts_with("~~~"));
    if !fenced {
        return lines
            .into_iter()
            .map(|line| line.strip_prefix("    ").map_or(line.clone(), str::to_owned))
            .collect();
    }
    let marker = lines[0].trim_start().chars().next().unwrap_or('`');
    let closed = lines.len() > 1
        && lines
            .last()
            .is_some_and(|last| !last.trim().is_empty() && last.trim().chars().all(|c| c == marker));
    lines[1..lines.len() - usize::from(closed)].to_vec()
}

/// One item's text on one line, as a heading needs it.
fn joined(item: &Item) -> String {
    let parts: Vec<&str> = item
        .lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    parts.join(" ")
}

fn written(items: &[Item], target: BlockType, first_number: usize, eol: &str) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    match target {
        BlockType::Paragraph => {
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    lines.push(String::new());
                }
                lines.extend(item.lines.iter().cloned());
            }
        }
        BlockType::Heading(level) => {
            let marker = "#".repeat(usize::from(level));
            for (at, item) in items.iter().enumerate() {
                let text = joined(item);
                // These are errors in a heading.
                let cites = inline_spans(BlockKind::Paragraph, &text)
                    .iter()
                    .any(|span| span.kind == SpanKind::Citation);
                if text.is_empty() || cites || text.contains("{todo:") || text.contains("![") {
                    return None;
                }
                if at > 0 {
                    lines.push(String::new());
                }
                lines.push(format!("{marker} {text}"));
            }
        }
        BlockType::Bullets | BlockType::Numbers => {
            // The indentation of each level, which the marker widths of the levels above decide.
            let mut indents = vec![0usize];
            let mut numbers = vec![first_number];
            for item in items {
                indents.truncate(item.level + 1);
                numbers.truncate(item.level + 1);
                while indents.len() <= item.level {
                    let width = marker_width(target, numbers.last().copied().unwrap_or(1).saturating_sub(1).max(1));
                    indents.push(indents.last().copied().unwrap_or(0) + width);
                    numbers.push(1);
                }
                let number = numbers[item.level];
                numbers[item.level] += 1;
                let marker = if target == BlockType::Bullets {
                    "- ".to_owned()
                } else {
                    format!("{number}. ")
                };
                let indent = " ".repeat(indents[item.level]);
                let hang = " ".repeat(indents[item.level] + marker.len());
                for (at, line) in item.lines.iter().enumerate() {
                    if at == 0 {
                        lines.push(format!("{indent}{marker}{line}"));
                    } else if line.is_empty() {
                        lines.push(String::new());
                    } else {
                        lines.push(format!("{hang}{line}"));
                    }
                }
            }
        }
        BlockType::Quote | BlockType::Callout(_) => {
            if let BlockType::Callout(callout) = target {
                lines.push(format!("> {}", callout.marker()));
            }
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    lines.push(">".to_owned());
                }
                for line in &item.lines {
                    lines.push(if line.is_empty() {
                        ">".to_owned()
                    } else {
                        format!("> {line}")
                    });
                }
            }
        }
        BlockType::Code => {
            let mut body: Vec<&str> = Vec::new();
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    body.push("");
                }
                body.extend(item.lines.iter().map(String::as_str));
            }
            let references = body.iter().any(|line| {
                inline_spans(BlockKind::Paragraph, line)
                    .iter()
                    .any(|span| span.kind == SpanKind::FootnoteReference)
            });
            // In code a reference is text, which would leave its footnote with no reference, and that is an error.
            if references || body.iter().any(|line| line.chars().count() > MAX_CODE_LINE) {
                return None;
            }
            let fence = "`".repeat(fence_length(&body));
            lines.push(fence.clone());
            lines.extend(body.into_iter().map(str::to_owned));
            lines.push(fence);
        }
    }
    Some(lines.join(eol))
}

fn marker_width(target: BlockType, number: usize) -> usize {
    if target == BlockType::Bullets {
        2
    } else {
        number.to_string().len() + 2
    }
}

/// One backtick more than the longest run opening a line of the code, and at least three.
fn fence_length(lines: &[&str]) -> usize {
    lines
        .iter()
        .map(|line| line.trim_start().bytes().take_while(|byte| *byte == b'`').count())
        .max()
        .map_or(3, |longest| (longest + 1).max(3))
}

/// The text a block's text becomes as `target`, without its final line ending.
pub(super) fn rewritten(kind: BlockKind, text: &str, target: BlockType, first_number: usize) -> Option<String> {
    let current = type_of(kind, text)?;
    if current == target && current != BlockType::Numbers {
        return None;
    }
    let body = text.trim_end_matches(['\n', '\r']);
    let items = items_of(kind, body)?;
    let result = written(&items, target, first_number, line_ending(body))?;
    (result != body).then_some(result)
}

/// The edit that turns a block into `target`, with the blank lines it needs to stay apart from its neighbours.
fn edit_for(text: &str, blocks: &Blocks, index: usize, target: BlockType, first_number: usize) -> Option<Edit> {
    let block = &blocks.as_slice()[index];
    let new = rewritten(block.kind, &text[block.range.clone()], target, first_number)?;
    let body = body_range(text, &block.range);
    Some(gapped_edit(text, blocks, index, body, new))
}

/// An edit replacing `body`, the text of block `index`, that keeps a blank line to the blocks around it.
pub(super) fn gapped_edit(text: &str, blocks: &Blocks, index: usize, body: Range<usize>, new: String) -> Edit {
    let eol = line_ending(text);
    let before = index > 0 && needs_gap(blocks, index - 1);
    let after = index + 1 < blocks.as_slice().len() && needs_gap(blocks, index + 1);
    let new = format!("{}{new}{}", if before { eol } else { "" }, if after { eol } else { "" });
    smallest_edit(body.start, &text[body.clone()], &new)
}

/// Turns one block into `target`.
pub fn convert(text: &str, blocks: &Blocks, index: usize, target: BlockType, selection: Selection) -> Option<Command> {
    let edit = edit_for(text, blocks, index, target, 1)?;
    let edits = normalized(vec![edit], text);
    let selection = selection_after(selection, &edits);
    Some(Command { edits, selection })
}

/// Turns every block the selection reaches into `target`, or if all of them already are, into paragraphs. Numbers run on
/// across the blocks, so they read as one list.
pub fn toggle(text: &str, blocks: &Blocks, selection: Selection, target: BlockType) -> Option<Command> {
    let range = selection.range();
    let first = blocks.index_at(range.start);
    let mut indices = Vec::new();
    for (index, block) in blocks.as_slice().iter().enumerate().skip(first) {
        if index > first && block.range.start >= range.end {
            break;
        }
        if type_of(block.kind, &text[block.range.clone()]).is_some() {
            indices.push(index);
        }
    }
    let every = !indices.is_empty()
        && indices.iter().all(|index| {
            let block = &blocks.as_slice()[*index];
            type_of(block.kind, &text[block.range.clone()]) == Some(target)
        });
    let target = if every { BlockType::Paragraph } else { target };
    let mut number = 1;
    let mut edits = Vec::new();
    for index in indices {
        if let Some(edit) = edit_for(text, blocks, index, target, number) {
            edits.push(edit);
        }
        let block = &blocks.as_slice()[index];
        number += line_markers(BlockKind::List, &text[block.range.clone()]).len().max(1);
    }
    let edits = normalized(edits, text);
    if edits.is_empty() {
        return None;
    }
    let selection = selection_after(selection, &edits);
    Some(Command { edits, selection })
}

#[cfg(test)]
mod tests {
    use super::super::applied;
    use super::*;

    fn to(text: &str, target: BlockType) -> String {
        let blocks = Blocks::parse(text);
        match convert(text, &blocks, 0, target, Selection::caret(0)) {
            Some(command) => applied(text, &command.edits),
            None => format!("no command for {text}"),
        }
    }

    use BlockType::*;

    #[test]
    fn paragraphs_become_every_type_and_back() {
        let text = "One\ntwo\n";
        for (target, expected) in [
            (Heading(2), "## One two\n"),
            (Bullets, "- One\n  two\n"),
            (Numbers, "1. One\n   two\n"),
            (Quote, "> One\n> two\n"),
            (Callout(super::Callout::Tip), "> [!TIP]\n> One\n> two\n"),
            (Code, "```\nOne\ntwo\n```\n"),
        ] {
            assert_eq!(to(text, target), expected);
        }
        for (written, source) in [
            ("- One\n  two\n", text),
            ("1. One\n   two\n", text),
            ("> One\n> two\n", text),
            ("> [!TIP]\n> One\n> two\n", text),
            ("```\nOne\ntwo\n```\n", text),
            ("## One two\n", "One two\n"),
            ("Title\n===\n", "Title\n"),
            ("# Title #\n", "Title\n"),
        ] {
            assert_eq!(to(written, Paragraph), source);
        }
    }

    #[test]
    fn list_items_become_one_block_each() {
        assert_eq!(to("- a\n- b\n", Paragraph), "a\n\nb\n");
        assert_eq!(to("- a\n- b\n", Heading(3)), "### a\n\n### b\n");
        assert_eq!(to("- a\n- b\n", Quote), "> a\n>\n> b\n");
        assert_eq!(to("> a\n>\n> b\n", Bullets), "- a\n- b\n");
    }

    #[test]
    fn bullets_and_numbers_swap_and_keep_nesting() {
        assert_eq!(to("- a\n  - b\n- c\n", Numbers), "1. a\n   1. b\n2. c\n");
        assert_eq!(to("1. a\n   1. b\n2. c\n", Bullets), "- a\n  - b\n- c\n");
    }

    #[test]
    fn a_code_fence_is_longer_than_any_fence_in_its_text() {
        assert_eq!(to("> ```\n> x\n> ```\n", Code), "````\n```\nx\n```\n````\n");
    }

    #[test]
    fn line_endings_stay_as_written() {
        assert_eq!(to("One\r\ntwo\r\n", Quote), "> One\r\n> two\r\n");
    }

    #[test]
    fn a_neighbour_without_a_blank_line_gets_one() {
        assert_eq!(to("# a\nb\n", Paragraph), "a\n\nb\n");
        assert_eq!(to("::: keep\n", Paragraph), "no command for ::: keep\n");
    }

    #[test]
    fn what_has_no_valid_form_writes_nothing() {
        for text in [
            "| a |\n|---|\n",
            "::: keep\n",
            "<!-- c -->\n",
            "***\n",
            "[^n]: x\n",
            "![a](b.png)\n",
            "<div>\n",
        ] {
            assert_eq!(to(text, Heading(1)), format!("no command for {text}"));
        }
        assert_eq!(to("a {todo: b}\n", Heading(1)), "no command for a {todo: b}\n");
        let long = format!("{}\n", "x".repeat(MAX_CODE_LINE + 1));
        assert_eq!(to(&long, Code), format!("no command for {long}"));
        assert_eq!(to("# a\n", Heading(1)), "no command for # a\n");
    }

    #[test]
    fn a_toggle_covers_the_blocks_in_the_selection_and_returns_them() {
        let text = "a\n\nb\n\n# c\n";
        let blocks = Blocks::parse(text);
        let all = Selection {
            anchor: 0,
            head: text.len(),
        };
        let command = toggle(text, &blocks, all, Numbers).expect("a change");
        let result = applied(text, &command.edits);
        assert_eq!(result, "1. a\n\n2. b\n\n3. c\n");
        let blocks = Blocks::parse(&result);
        let all = Selection {
            anchor: 0,
            head: result.len(),
        };
        let command = toggle(&result, &blocks, all, Numbers).expect("a change");
        assert_eq!(applied(&result, &command.edits), "a\n\nb\n\nc\n");
    }

    #[test]
    fn the_caret_stays_with_its_text() {
        let text = "abc\n";
        let blocks = Blocks::parse(text);
        let command = convert(text, &blocks, 0, Heading(1), Selection::caret(1)).expect("a change");
        assert_eq!(command.selection, Selection::caret(3));
    }
}
