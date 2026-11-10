//! Moving a block's text to another place between blocks: one delete and one insert, in one undo step.

use std::ops::Range;

use super::{Bias, Command, line_ending, moved, needs_gap, normalized, selection_after};
use crate::source::{BlockKind, Blocks, Edit, Selection};

/// Whether a block can be dragged. Front matter must stay first, and the lines of a directive that encloses text would
/// come apart from each other, so only one standing alone moves.
pub fn can_move(kind: BlockKind, text: &str) -> bool {
    match kind {
        BlockKind::FrontMatter | BlockKind::Blank => false,
        BlockKind::Directive => {
            let name = text.trim_start_matches(':').trim();
            matches!(
                name,
                "page-break" | "toc" | "bibliography" | "index" | "front-matter" | "main-matter" | "back-matter"
            )
        }
        _ => true,
    }
}

/// Moves block `from` to just above block `before`, or to the end when `before` is the number of blocks.
pub fn move_block(text: &str, blocks: &Blocks, from: usize, before: usize, selection: Selection) -> Option<Command> {
    let moving = blocks.as_slice().get(from)?;
    if !can_move(moving.kind, &text[moving.range.clone()]) {
        return None;
    }
    move_range(text, blocks, from..from + 1, before, selection)
}

/// Moves blocks `from` together to just above block `before`, or to the end when `before` is the number of blocks. The
/// caller says they may move together, such as a directive with everything it holds.
pub fn move_range(
    text: &str,
    blocks: &Blocks,
    from: Range<usize>,
    before: usize,
    selection: Selection,
) -> Option<Command> {
    let list = blocks.as_slice();
    let (first, last) = (from.start, from.end.checked_sub(1)?);
    if from.is_empty() || last >= list.len() || before > list.len() {
        return None;
    }
    if list[first].kind == BlockKind::FrontMatter {
        return None;
    }
    let range = list[first].range.start..list[last].range.end;
    let moved_text = &text[range.clone()];
    // Above the blocks themselves or the blank after them is where they already are.
    let after_blank = list.get(last + 1).is_some_and(|next| next.kind == BlockKind::Blank);
    let next_place = last + 1 + usize::from(after_blank);
    if (first..=next_place).contains(&before) {
        return None;
    }
    if list.first().is_some_and(|first| first.kind == BlockKind::FrontMatter) && before == 0 {
        return None;
    }
    let eol = line_ending(text);
    let body = moved_text.trim_end_matches(['\n', '\r']);

    // What goes away: the blocks and the blank line that kept them from the next one. At the end of the text that is
    // the blank before them.
    let mut removed = range.clone();
    let mut filler = "";
    if after_blank {
        removed.end = list[last + 1].range.end;
    } else if first > 0 && list[first - 1].kind == BlockKind::Blank && last + 1 == list.len() {
        removed.start = list[first - 1].range.start;
    } else if first > 0 && last + 1 < list.len() && needs_gap(blocks, first - 1) && needs_gap(blocks, last + 1) {
        // Two blocks that only these kept apart.
        filler = eol;
    }

    let removal = Edit::new(removed, filler);
    let (mut edits, lead) = if before == list.len() {
        let end = text.len();
        let lead = if text.ends_with("\n\n") || text.ends_with("\r\n\r\n") || text.is_empty() {
            String::new()
        } else if text.ends_with('\n') {
            eol.to_owned()
        } else {
            format!("{eol}{eol}")
        };
        let insert = Edit::new(end..end, format!("{lead}{body}{eol}"));
        (vec![removal, insert], lead.len())
    } else {
        let at = list[before].range.start;
        let lead = if before > 0 && needs_gap(blocks, before - 1) && before - 1 != last {
            eol
        } else {
            ""
        };
        let gap = if needs_gap(blocks, before) { eol } else { "" };
        let insert = Edit::new(at..at, format!("{lead}{body}{eol}{gap}"));
        (vec![insert, removal], lead.len())
    };
    edits.sort_by_key(|edit| edit.range.start);
    // The caret goes with the blocks if it was in them, and otherwise stays on its text.
    let head = selection.head;
    let selection = if range.contains(&head) || head == range.end {
        let insert = edits.iter().find(|edit| edit.range.is_empty() && !edit.text.is_empty());
        let at = insert.map_or(0, |insert| moved(insert.range.start, &edits, Bias::Before));
        Selection::caret(at + lead + (head - range.start).min(body.len()))
    } else {
        let edits = normalized(edits.clone(), text);
        selection_after(selection, &edits)
    };
    let edits = normalized(edits, text);
    Some(Command { edits, selection })
}

#[cfg(test)]
mod tests {
    use super::super::applied;
    use super::*;

    fn moving(text: &str, from: usize, before: usize) -> String {
        let blocks = Blocks::parse(text);
        match move_block(text, &blocks, from, before, Selection::caret(0)) {
            Some(command) => applied(text, &command.edits),
            None => "no command".to_owned(),
        }
    }

    const TEXT: &str = "a\n\nb\n\nc\n";

    #[test]
    fn a_block_moves_down_up_and_to_the_end() {
        assert_eq!(moving(TEXT, 0, 4), "b\n\na\n\nc\n");
        assert_eq!(moving(TEXT, 4, 0), "c\n\na\n\nb\n");
        assert_eq!(moving(TEXT, 0, 5), "b\n\nc\n\na\n");
        assert_eq!(moving(TEXT, 2, 5), "a\n\nc\n\nb\n");
        assert_eq!(moving(TEXT, 4, 2), "a\n\nc\n\nb\n");
    }

    #[test]
    fn a_block_dropped_where_it_is_writes_nothing() {
        assert_eq!(moving(TEXT, 2, 2), "no command");
        assert_eq!(moving(TEXT, 2, 3), "no command");
        assert_eq!(moving(TEXT, 2, 4), "no command");
    }

    #[test]
    fn a_text_without_a_final_line_ending_stays_readable() {
        assert_eq!(moving("a\n\nb", 0, 3), "b\n\na\n");
        assert_eq!(moving("a\n\nb", 2, 0), "b\n\na\n");
    }

    #[test]
    fn blocks_without_blank_lines_stay_apart() {
        assert_eq!(moving("# a\nb\n# c\n", 0, 2), "b\n\n# a\n\n# c\n");
        assert_eq!(moving("# a\nb\n# c\nd\n", 1, 4), "# a\n\n# c\nd\n\nb\n");
    }

    #[test]
    fn line_endings_stay_as_written() {
        assert_eq!(moving("a\r\n\r\nb\r\n\r\nc\r\n", 0, 4), "b\r\n\r\na\r\n\r\nc\r\n");
    }

    #[test]
    fn front_matter_and_enclosing_directives_do_not_move() {
        assert_eq!(moving("---\na: b\n---\n\nc\n", 0, 3), "no command");
        assert_eq!(moving("::: keep\nb\n:::\n\nc\n", 0, 4), "no command");
        assert_eq!(moving("c\n\n::: page-break\n", 2, 0), "::: page-break\n\nc\n");
        assert_eq!(moving("---\na: b\n---\n\nc\n\nd\n", 4, 0), "no command");
    }

    #[test]
    fn a_directive_moves_with_what_it_holds() {
        let text = "a\n\n::: keep\nb\n:::\n\nc\n";
        let blocks = Blocks::parse(text);
        let command = move_range(text, &blocks, 2..5, 7, Selection::caret(2)).expect("a move");
        assert_eq!(applied(text, &command.edits), "a\n\nc\n\n::: keep\nb\n:::\n");
        let command = move_range(text, &blocks, 2..5, 0, Selection::caret(3)).expect("a move");
        assert_eq!(applied(text, &command.edits), "::: keep\nb\n:::\n\na\n\nc\n");
        assert_eq!(command.selection, Selection::caret(0));
        assert!(move_range(text, &blocks, 2..5, 4, Selection::caret(0)).is_none());
    }

    #[test]
    fn the_caret_goes_with_the_block() {
        let blocks = Blocks::parse(TEXT);
        let command = move_block(TEXT, &blocks, 0, 5, Selection::caret(1)).expect("a move");
        assert_eq!(applied(TEXT, &command.edits), "b\n\nc\n\na\n");
        assert_eq!(command.selection, Selection::caret(7));
        let command = move_block(TEXT, &blocks, 4, 0, Selection::caret(3)).expect("a move");
        assert_eq!(command.selection, Selection::caret(6));
    }
}
