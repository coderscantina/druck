//! What the editor's commands write. Every command is a list of edits to the Markdown text and the selection after it,
//! applied by the view as one undo step. Nothing here keeps a model of the document, and nothing here knows gpui.
//!
//! - [`marks`]: bold, italic, and the other inline marks and links, toggled from what the source says.
//! - [`convert`]: turning a block into another type of block.
//! - [`slash`]: the slash menu's list and what each entry inserts.
//! - [`moves`]: moving a block's text.
//! - [`notes`]: comments, and the notes that open a chapter.

pub mod convert;
pub mod marks;
pub mod moves;
pub mod notes;
pub mod slash;
#[cfg(test)]
mod tests;

use std::ops::Range;

use crate::source::{Blocks, Edit, Selection};

/// A change to the text and where the selection goes after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// In increasing order, with all ranges referring to the text before the command.
    pub edits: Vec<Edit>,
    pub selection: Selection,
}

/// Which side of an edit an offset at its start stays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bias {
    Before,
    After,
}

/// Where `offset` is after sorted `edits`.
pub fn moved(offset: usize, edits: &[Edit], bias: Bias) -> usize {
    let mut delta = 0isize;
    for edit in edits {
        if offset < edit.range.start {
            break;
        }
        let at_start = offset == edit.range.start;
        if offset < edit.range.end || at_start {
            let start = edit.range.start.wrapping_add_signed(delta);
            return match bias {
                Bias::Before if at_start => start,
                _ => start + edit.text.len(),
            };
        }
        delta += edit.text.len() as isize - edit.range.len() as isize;
    }
    offset.wrapping_add_signed(delta)
}

/// Sorts edits, joins the ones that touch, and drops any that change nothing.
fn normalized(mut edits: Vec<Edit>, text: &str) -> Vec<Edit> {
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let mut joined: Vec<Edit> = Vec::with_capacity(edits.len());
    for edit in edits {
        match joined.last_mut() {
            Some(last) if last.range.end == edit.range.start => {
                last.range.end = edit.range.end;
                last.text.push_str(&edit.text);
            }
            _ => joined.push(edit),
        }
    }
    joined.retain(|edit| text[edit.range.clone()] != edit.text);
    joined
}

/// `text` with sorted `edits` applied.
fn applied(text: &str, edits: &[Edit]) -> String {
    let mut result = String::with_capacity(text.len() + 16);
    let mut at = 0;
    for edit in edits {
        result.push_str(&text[at..edit.range.start]);
        result.push_str(&edit.text);
        at = edit.range.end;
    }
    result.push_str(&text[at..]);
    result
}

/// The one edit that turns `old`, which stands at `start` in the text, into `new`: everything they share at either end
/// stays out of it, so a caret there stays with its text.
fn smallest_edit(start: usize, old: &str, new: &str) -> Edit {
    let mut prefix = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = old[prefix..]
        .bytes()
        .rev()
        .zip(new[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    Edit::new(
        start + prefix..start + old.len() - suffix,
        &new[prefix..new.len() - suffix],
    )
}

/// The selection after `edits`, on the same text it was on.
fn selection_after(selection: Selection, edits: &[Edit]) -> Selection {
    let range = selection.range();
    if range.is_empty() {
        return Selection::caret(moved(range.start, edits, Bias::Before));
    }
    let start = moved(range.start, edits, Bias::After);
    let end = moved(range.end, edits, Bias::Before).max(start);
    if selection.reversed() {
        Selection {
            anchor: end,
            head: start,
        }
    } else {
        Selection {
            anchor: start,
            head: end,
        }
    }
}

/// The line ending this text uses.
fn line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The range of a block without the line endings closing it.
fn body_range(text: &str, range: &Range<usize>) -> Range<usize> {
    let body = text[range.clone()].trim_end_matches(['\n', '\r']);
    range.start..range.start + body.len()
}

/// Whether two blocks that meet would run together or change each other's meaning without a blank line between them.
/// A directive line needs none.
fn needs_gap(blocks: &Blocks, index: usize) -> bool {
    use crate::source::BlockKind::{Blank, Directive};
    !matches!(blocks.as_slice()[index].kind, Blank | Directive)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn an_offset_moves_with_the_text_around_it() {
        let edits = [Edit::new(2..2, "**"), Edit::new(5..7, ""), Edit::new(9..9, "x")];
        assert_eq!(moved(1, &edits, Bias::After), 1);
        assert_eq!(moved(2, &edits, Bias::Before), 2);
        assert_eq!(moved(2, &edits, Bias::After), 4);
        assert_eq!(moved(4, &edits, Bias::After), 6);
        assert_eq!(moved(6, &edits, Bias::After), 7);
        assert_eq!(moved(7, &edits, Bias::Before), 7);
        assert_eq!(moved(9, &edits, Bias::Before), 9);
        assert_eq!(moved(9, &edits, Bias::After), 10);
        assert_eq!(moved(12, &edits, Bias::After), 13);
    }

    #[test]
    fn edits_that_change_nothing_are_dropped_and_touching_ones_join() {
        let text = "**ab** c";
        let edits = vec![Edit::new(2..2, "**"), Edit::new(0..2, "")];
        assert!(normalized(edits, text).is_empty());
        let edits = vec![Edit::new(4..6, ""), Edit::new(8..8, "**")];
        assert_eq!(normalized(edits, text).len(), 2);
    }

    #[test]
    fn the_smallest_edit_keeps_what_both_ends_share_and_splits_no_character() {
        assert_eq!(smallest_edit(10, "abc", "# abc"), Edit::new(10..10, "# "));
        assert_eq!(smallest_edit(0, "aäb", "aöb"), Edit::new(1..3, "ö"));
        assert_eq!(smallest_edit(0, "abc", "abc"), Edit::new(3..3, ""));
    }
}
