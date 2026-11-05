//! The layout directives a proof slip offers, as changes to the written text: `::: page-break` before a paragraph, and
//! `::: keep` around it. Holds no gpui types, and only ever touches the lines it adds.

use std::ops::Range;

use crate::source::Edit;

const PAGE_BREAK: &str = "::: page-break";
const KEEP: &str = "::: keep";
const CLOSE: &str = ":::";

/// Which directives a block has now. A wrapper counts only if it holds exactly this block.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct State {
    pub page_break: bool,
    pub keep: bool,
}

/// A change to the text, and where the block is after it.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub edits: Vec<Edit>,
    pub range: Range<usize>,
}

/// The line before `at`, which starts a line, skipping blank ones.
fn line_before(text: &str, at: usize) -> Option<Range<usize>> {
    let mut end = at;
    while end > 0 {
        let line_end = end - 1;
        let start = text[..line_end].rfind('\n').map_or(0, |found| found + 1);
        if !text[start..line_end].trim().is_empty() {
            return Some(start..line_end);
        }
        end = start;
    }
    None
}

/// The line after `at`, which ends a line, skipping blank ones.
fn line_after(text: &str, at: usize) -> Option<Range<usize>> {
    let mut start = text[at..].find('\n').map(|found| at + found + 1)?;
    while start < text.len() {
        let end = text[start..].find('\n').map_or(text.len(), |found| start + found);
        if !text[start..end].trim().is_empty() {
            return Some(start..end);
        }
        start = end + 1;
    }
    None
}

fn is_line(text: &str, line: &Option<Range<usize>>, wanted: &str) -> bool {
    line.as_ref()
        .is_some_and(|line| text[line.clone()].trim_end() == wanted)
}

/// The `::: keep` line and closing line around the block, if they hold exactly it.
fn keep_lines(text: &str, block: &Range<usize>) -> Option<(Range<usize>, Range<usize>)> {
    let open = line_before(text, block.start);
    let close = line_after(text, block.end);
    (is_line(text, &open, KEEP) && is_line(text, &close, CLOSE)).then(|| (open.unwrap_or(0..0), close.unwrap_or(0..0)))
}

/// Where a page break before the block goes: before its keep, if it has one, since a page break cannot stand inside.
fn head_of(text: &str, block: &Range<usize>) -> usize {
    keep_lines(text, block).map_or(block.start, |(open, _)| open.start)
}

/// What the block at `block` has around it now.
pub fn read(text: &str, block: &Range<usize>) -> State {
    State {
        page_break: is_line(text, &line_before(text, head_of(text, block)), PAGE_BREAK),
        keep: keep_lines(text, block).is_some(),
    }
}

/// The range of a whole line with its newline and the blank lines after it, up to `next`.
fn whole_lines(text: &str, line: Range<usize>, next: usize) -> Range<usize> {
    line.start..next.max((line.end + 1).min(text.len()))
}

/// Adds a page break before the block, or takes it away.
pub fn toggle_page_break(text: &str, block: &Range<usize>) -> Change {
    let head = head_of(text, block);
    match line_before(text, head).filter(|line| text[line.clone()].trim_end() == PAGE_BREAK) {
        Some(line) => {
            let removed = whole_lines(text, line, head);
            Change {
                range: block.start - removed.len()..block.end - removed.len(),
                edits: vec![Edit::new(removed, "")],
            }
        }
        None => {
            let added = format!("{PAGE_BREAK}\n\n");
            Change {
                range: block.start + added.len()..block.end + added.len(),
                edits: vec![Edit::new(head..head, added)],
            }
        }
    }
}

/// Wraps the block in a keep, or takes that keep away.
pub fn toggle_keep(text: &str, block: &Range<usize>) -> Change {
    match keep_lines(text, block) {
        Some((open, close)) => {
            let removed_open = whole_lines(text, open, block.start);
            // Up to the end of the closing line. The newline that ended the block's last line stays.
            let after_block = text[block.end..]
                .find('\n')
                .map_or(block.end, |found| block.end + found);
            let removed_close = after_block..close.end;
            Change {
                range: block.start - removed_open.len()..block.end - removed_open.len(),
                edits: vec![Edit::new(removed_open, ""), Edit::new(removed_close, "")],
            }
        }
        None => {
            let opening = format!("{KEEP}\n");
            Change {
                range: block.start + opening.len()..block.end + opening.len(),
                edits: vec![
                    Edit::new(block.start..block.start, opening),
                    Edit::new(block.end..block.end, format!("\n{CLOSE}")),
                ],
            }
        }
    }
}

/// The smallest edit that turns `old` into `new`, for a block at `start`.
pub fn edit_between(start: usize, old: &str, new: &str) -> Option<Edit> {
    if old == new {
        return None;
    }
    let same_start = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((at, a), _)| at + a.len_utf8());
    let same_end = old[same_start..]
        .chars()
        .rev()
        .zip(new[same_start..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    Some(Edit::new(
        start + same_start..start + old.len() - same_end,
        &new[same_start..new.len() - same_end],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# One\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird.\n";

    fn block(text: &str, words: &str) -> Range<usize> {
        let at = text.find(words).expect("the words are in the text");
        at..at + words.len()
    }

    fn applied(text: &str, change: &Change) -> String {
        let mut out = text.to_owned();
        for edit in change.edits.iter().rev() {
            out.replace_range(edit.range.clone(), &edit.text);
        }
        out
    }

    #[test]
    fn a_page_break_goes_before_the_block_and_comes_away_without_a_trace() {
        let second = block(TEXT, "Second paragraph.");
        let on = toggle_page_break(TEXT, &second);
        let with = applied(TEXT, &on);
        assert_eq!(
            with,
            "# One\n\nFirst paragraph.\n\n::: page-break\n\nSecond paragraph.\n\nThird.\n"
        );
        assert_eq!(&with[on.range.clone()], "Second paragraph.");
        assert_eq!(
            read(&with, &on.range),
            State {
                page_break: true,
                keep: false
            }
        );
        let off = toggle_page_break(&with, &on.range);
        let without = applied(&with, &off);
        assert_eq!(without, TEXT);
        assert_eq!(&without[off.range.clone()], "Second paragraph.");
    }

    #[test]
    fn keep_wraps_the_block_and_comes_away_without_a_trace() {
        let second = block(TEXT, "Second paragraph.");
        let on = toggle_keep(TEXT, &second);
        let with = applied(TEXT, &on);
        assert_eq!(
            with,
            "# One\n\nFirst paragraph.\n\n::: keep\nSecond paragraph.\n:::\n\nThird.\n"
        );
        assert_eq!(&with[on.range.clone()], "Second paragraph.");
        assert_eq!(
            read(&with, &on.range),
            State {
                page_break: false,
                keep: true
            }
        );
        let off = toggle_keep(&with, &on.range);
        let without = applied(&with, &off);
        assert_eq!(without, TEXT);
        assert_eq!(&without[off.range.clone()], "Second paragraph.");
    }

    #[test]
    fn a_page_break_stands_outside_a_keep_in_either_order() {
        let second = block(TEXT, "Second paragraph.");
        let kept = toggle_keep(TEXT, &second);
        let with_keep = applied(TEXT, &kept);
        let broken = toggle_page_break(&with_keep, &kept.range);
        let both = applied(&with_keep, &broken);
        assert_eq!(
            both,
            "# One\n\nFirst paragraph.\n\n::: page-break\n\n::: keep\nSecond paragraph.\n:::\n\nThird.\n"
        );
        assert_eq!(
            read(&both, &broken.range),
            State {
                page_break: true,
                keep: true
            }
        );
        let unkept = toggle_keep(&both, &broken.range);
        let left = applied(&both, &unkept);
        assert_eq!(
            read(&left, &unkept.range),
            State {
                page_break: true,
                keep: false
            }
        );
        let clean = applied(&left, &toggle_page_break(&left, &unkept.range));
        assert_eq!(clean, TEXT);
    }

    #[test]
    fn a_block_at_the_end_of_a_file_without_a_newline_keeps_its_bytes() {
        let text = "One.\n\nLast.";
        let last = block(text, "Last.");
        let on = toggle_keep(text, &last);
        let with = applied(text, &on);
        assert_eq!(with, "One.\n\n::: keep\nLast.\n:::");
        assert_eq!(applied(&with, &toggle_keep(&with, &on.range)), text);
    }

    #[test]
    fn a_keep_around_several_blocks_is_not_this_blocks_keep() {
        let text = "::: keep\nOne.\n\nTwo.\n:::\n";
        assert!(!read(text, &block(text, "One.")).keep);
        assert!(!read(text, &block(text, "Two.")).keep);
    }

    #[test]
    fn the_edit_between_two_texts_touches_only_what_changed() {
        let edit = edit_between(100, "the quick fox", "the slow fox").expect("a change");
        assert_eq!((edit.range, edit.text.as_str()), (104..109, "slow"));
        let edit = edit_between(0, "Füße", "Füsse").expect("a change");
        assert_eq!((edit.range, edit.text.as_str()), (3..5, "ss"));
        let edit = edit_between(0, "aaa", "aaaa").expect("a change");
        assert_eq!((edit.range, edit.text.as_str()), (3..3, "a"));
        assert_eq!(edit_between(0, "same", "same"), None);
    }
}
