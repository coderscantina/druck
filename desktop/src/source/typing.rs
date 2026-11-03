//! What the Enter key writes, which depends on the block around the caret.

use std::ops::Range;

use super::blocks::{BlockKind, Blocks};
use super::buffer::Edit;
use super::inline::{self, MarkerKind};
use super::motion;

/// The edit for Enter at `offset`. In prose it starts a new paragraph, in a list or quote a new item, and on an empty
/// item it ends the list. In raw blocks it is a plain line ending.
pub fn line_break(text: &str, blocks: &Blocks, offset: usize) -> Edit {
    let block = &blocks.as_slice()[blocks.index_at(offset)];
    let line = motion::line_range(text, offset);
    if line.is_empty() {
        return Edit::new(offset..offset, "\n");
    }
    match block.kind {
        BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::FootnoteDefinition => {
            Edit::new(offset..offset, "\n\n")
        }
        BlockKind::List | BlockKind::Quote => continue_marker(text, block.kind, line, offset),
        _ => Edit::new(offset..offset, "\n"),
    }
}

fn continue_marker(text: &str, kind: BlockKind, line: Range<usize>, offset: usize) -> Edit {
    let content = &text[line.clone()];
    let Some(marker) = inline::line_markers(kind, content).into_iter().next() else {
        return Edit::new(offset..offset, "\n");
    };
    if marker.range.end == content.len() {
        return Edit::new(line, "");
    }
    let lead = &content[..marker.range.start];
    let written = &content[marker.range.clone()];
    let next = match marker.kind {
        MarkerKind::Number => next_number(written),
        _ => written.to_owned(),
    };
    Edit::new(offset..offset, format!("\n{lead}{next}"))
}

/// `9. ` becomes `10. `. The delimiter and the gap after it stay as written.
fn next_number(marker: &str) -> String {
    let digits = marker.bytes().take_while(u8::is_ascii_digit).count();
    let number: u64 = marker[..digits].parse().unwrap_or(0);
    format!("{}{}", number + 1, &marker[digits..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter(text: &str, offset: usize) -> String {
        let blocks = Blocks::parse(text);
        let edit = line_break(text, &blocks, offset);
        let mut result = text.to_owned();
        result.replace_range(edit.range, &edit.text);
        result
    }

    #[test]
    fn prose_gets_a_new_paragraph_and_raw_blocks_a_line() {
        assert_eq!(enter("ab", 1), "a\n\nb");
        assert_eq!(enter("# ab\n", 4), "# ab\n\n\n");
        assert_eq!(enter("```\nab\n```\n", 5), "```\na\nb\n```\n");
        assert_eq!(enter("ab\n", 3), "ab\n\n");
    }

    #[test]
    fn lists_and_quotes_continue_with_their_marker() {
        assert_eq!(enter("- a\n", 3), "- a\n- \n");
        assert_eq!(enter("  * a", 5), "  * a\n  * ");
        assert_eq!(enter("9. a", 4), "9. a\n10. ");
        assert_eq!(enter("> a", 3), "> a\n> ");
    }

    #[test]
    fn an_empty_item_ends_the_list() {
        assert_eq!(enter("- a\n- ", 6), "- a\n");
        assert_eq!(enter("> a\n> ", 6), "> a\n");
    }
}
