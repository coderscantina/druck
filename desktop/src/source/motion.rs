//! Caret targets in the text: grapheme, word, and source line steps.
//!
//! The single empty line between two blocks is how Markdown spells a block boundary, not a place a writer stands.
//! [`left`] and [`right`] step over it, so one arrow press or backspace crosses from one block into the next.

use std::ops::Range;

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

pub fn next_grapheme(text: &str, offset: usize) -> usize {
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    cursor.next_boundary(text, 0).ok().flatten().unwrap_or(text.len())
}

pub fn prev_grapheme(text: &str, offset: usize) -> usize {
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
}

/// One caret step right.
pub fn right(text: &str, offset: usize) -> usize {
    let next = next_grapheme(text, offset);
    if is_separator(text, next) {
        next_grapheme(text, next)
    } else {
        next
    }
}

/// One caret step left.
pub fn left(text: &str, offset: usize) -> usize {
    let previous = prev_grapheme(text, offset);
    if is_separator(text, previous) {
        prev_grapheme(text, previous)
    } else {
        previous
    }
}

/// Whether `offset` stands on an empty line that is the only one between two lines with content.
pub fn is_separator(text: &str, offset: usize) -> bool {
    let (before, after) = text.split_at(offset);
    let Some(before) = before.strip_suffix('\n') else {
        return false;
    };
    let Some(after) = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')) else {
        return false;
    };
    let before = before.strip_suffix('\r').unwrap_or(before);
    let before_empty = before.is_empty() || before.ends_with('\n');
    let after_empty = after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n");
    !before_empty && !after_empty
}

/// The content of the source line around `offset`, without its line ending.
pub fn line_range(text: &str, offset: usize) -> Range<usize> {
    let start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |at| offset + at);
    let end = if end > start && text.as_bytes()[end - 1] == b'\r' && end < text.len() {
        end - 1
    } else {
        end
    };
    start..end
}

/// The end of the next word, past any spaces and punctuation. At the end of a line, one step right.
pub fn next_word(text: &str, offset: usize) -> usize {
    let line = line_range(text, offset);
    if offset >= line.end {
        return right(text, offset);
    }
    text[offset..line.end]
        .split_word_bound_indices()
        .find(|(_, word)| is_word(word))
        .map_or(line.end, |(at, word)| offset + at + word.len())
}

/// The start of the previous word, past any spaces and punctuation. At the start of a line, one step left.
pub fn prev_word(text: &str, offset: usize) -> usize {
    let line = line_range(text, offset);
    if offset <= line.start {
        return left(text, offset);
    }
    text[line.start..offset]
        .split_word_bound_indices()
        .rev()
        .find(|(_, word)| is_word(word))
        .map_or(line.start, |(at, _)| line.start + at)
}

/// The word around `offset`, or the run of other characters there, for a double click.
pub fn word_range(text: &str, offset: usize) -> Range<usize> {
    let line = line_range(text, offset);
    let mut touching = text[line.clone()]
        .split_word_bound_indices()
        .map(|(at, token)| (line.start + at..line.start + at + token.len(), is_word(token)))
        .filter(|(range, _)| range.contains(&offset) || range.end == offset);
    let first = touching.next();
    let second = touching.next();
    match (first, second) {
        (Some((_, false)), Some((word, true))) | (Some((word, _)), _) => word,
        (None, _) => offset..offset,
    }
}

fn is_word(token: &str) -> bool {
    token.chars().any(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_by_grapheme_not_by_byte() {
        let text = "a\u{308}😀\r\nb";
        assert_eq!(next_grapheme(text, 0), 3);
        assert_eq!(next_grapheme(text, 3), 7);
        assert_eq!(next_grapheme(text, 7), 9);
        assert_eq!(prev_grapheme(text, 9), 7);
    }

    #[test]
    fn arrows_cross_a_block_boundary_in_one_step() {
        let text = "ab\n\ncd";
        assert_eq!(right(text, 2), 4);
        assert_eq!(left(text, 4), 2);
    }

    #[test]
    fn arrows_stop_on_blank_lines_that_are_not_a_single_separator() {
        let text = "ab\n\n\ncd";
        assert_eq!(right(text, 2), 3);
        assert_eq!(right(text, 3), 4);
        assert_eq!(right(text, 4), 5);
        let end = "ab\n\n";
        assert_eq!(right(end, 2), 3);
        assert_eq!(right(end, 3), 4);
    }

    #[test]
    fn a_crlf_separator_is_one_step() {
        let text = "ab\r\n\r\ncd";
        assert!(is_separator(text, 4));
        assert_eq!(right(text, 2), 6);
        assert_eq!(left(text, 6), 2);
    }

    #[test]
    fn words_skip_punctuation_and_stop_at_line_edges() {
        let text = "Grüße, Welt!\nneu";
        assert_eq!(next_word(text, 0), "Grüße".len());
        assert_eq!(next_word(text, "Grüße".len()), "Grüße, Welt".len());
        assert_eq!(next_word(text, "Grüße, Welt".len()), "Grüße, Welt!".len());
        assert_eq!(next_word(text, "Grüße, Welt!".len()), "Grüße, Welt!\n".len());
        assert_eq!(prev_word(text, "Grüße, Welt!".len()), "Grüße, ".len());
        assert_eq!(prev_word(text, "Grüße, ".len()), 0);
    }

    #[test]
    fn a_word_range_covers_the_word_under_the_caret() {
        let text = "ein Wort.";
        assert_eq!(word_range(text, 5), 4..8);
        assert_eq!(word_range(text, 8), 4..8);
    }
}
