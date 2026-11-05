//! What differs between two versions of a text, word by word. Holds no gpui types.

use std::ops::Range;
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use unicode_segmentation::UnicodeSegmentation;

/// A diff that takes longer gives a coarser result, which is still correct. Only a rewrite of a whole chapter gets here.
const DIFF_TIME: Duration = Duration::from_millis(200);

/// Text that differs: `old` in the first text became `new` in the second. Byte ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

/// The word-level differences from `old` to `new`, in order. Words changed with only a space between them count as one.
pub fn hunks(old: &str, new: &str) -> Vec<Hunk> {
    let prefix = common_prefix(old, new);
    let suffix = common_suffix(&old[prefix..], &new[prefix..]);
    let (old_middle, new_middle) = (&old[prefix..old.len() - suffix], &new[prefix..new.len() - suffix]);
    let old_tokens: Vec<&str> = old_middle.split_word_bounds().collect();
    let new_tokens: Vec<&str> = new_middle.split_word_bounds().collect();
    let old_starts = starts(&old_tokens, prefix);
    let new_starts = starts(&new_tokens, prefix);
    let deadline = Instant::now() + DIFF_TIME;
    let mut hunks: Vec<Hunk> = Vec::new();
    for op in capture_diff_slices_deadline(Algorithm::Myers, &old_tokens, &new_tokens, Some(deadline)) {
        let (old_range, new_range) = match op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (old_index..old_index + old_len, new_index..new_index),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index..old_index, new_index..new_index + new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (old_index..old_index + old_len, new_index..new_index + new_len),
        };
        let hunk = Hunk {
            old: old_starts[old_range.start]..old_starts[old_range.end],
            new: new_starts[new_range.start]..new_starts[new_range.end],
        };
        match hunks.last_mut() {
            Some(last) if only_a_space(&old[last.old.end..hunk.old.start]) => {
                last.old.end = hunk.old.end;
                last.new.end = hunk.new.end;
            }
            _ => hunks.push(hunk),
        }
    }
    hunks
}

/// Hunks that stand in one paragraph of the new text belong together: restoring a passage restores all of them.
pub fn passages(hunks: &[Hunk], old: &str) -> Vec<Range<usize>> {
    let mut passages: Vec<Range<usize>> = Vec::new();
    for (at, hunk) in hunks.iter().enumerate() {
        match passages.last_mut() {
            Some(last) if !old[hunks[at - 1].old.end..hunk.old.start].contains("\n\n") => last.end = at + 1,
            _ => passages.push(at..at + 1),
        }
    }
    passages
}

/// How many words `new` adds to `old`, and how many it takes away.
pub fn counts(old: &str, new: &str) -> (usize, usize) {
    hunks(old, new).iter().fold((0, 0), |(added, removed), hunk| {
        (
            added + new[hunk.new.clone()].unicode_words().count(),
            removed + old[hunk.old.clone()].unicode_words().count(),
        )
    })
}

fn only_a_space(text: &str) -> bool {
    !text.is_empty() && text.len() <= 2 && text.chars().all(|c| c == ' ')
}

fn starts(tokens: &[&str], from: usize) -> Vec<usize> {
    let mut at = from;
    let mut all = Vec::with_capacity(tokens.len() + 1);
    for token in tokens {
        all.push(at);
        at += token.len();
    }
    all.push(at);
    all
}

fn common_prefix(a: &str, b: &str) -> usize {
    let same = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    word_start(a, same)
}

/// Backs up to where a word starts, so a shared start of two different words is not cut out of them.
fn word_start(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    while at > 0 && at < text.len() {
        let before = text[..at].chars().next_back();
        if before.is_none_or(|c| !c.is_alphanumeric()) {
            break;
        }
        at -= before.map_or(1, char::len_utf8);
    }
    at
}

fn common_suffix(a: &str, b: &str) -> usize {
    let mut same = a.bytes().rev().zip(b.bytes().rev()).take_while(|(x, y)| x == y).count();
    while same > 0 && !(a.is_char_boundary(a.len() - same) && b.is_char_boundary(b.len() - same)) {
        same -= 1;
    }
    while same > 0 && a[a.len() - same..].chars().next().is_some_and(char::is_alphanumeric) {
        let first = a[a.len() - same..].chars().next().map_or(1, char::len_utf8);
        same -= first;
    }
    same
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(old: &str, new: &str) -> Vec<(String, String)> {
        hunks(old, new)
            .into_iter()
            .map(|hunk| (old[hunk.old].to_owned(), new[hunk.new].to_owned()))
            .collect()
    }

    #[test]
    fn whole_words_differ_and_not_their_shared_letters() {
        assert_eq!(
            shown("The fire burnt very brightly", "The fire burnt quite brightly"),
            vec![("very".to_owned(), "quite".to_owned())]
        );
        assert_eq!(
            shown("Haus und Hausboot", "Haus und Hausbau"),
            vec![("Hausboot".to_owned(), "Hausbau".to_owned())]
        );
    }

    #[test]
    fn neighbouring_words_join_across_a_space() {
        assert_eq!(
            shown("one two three four", "one 2 3 four"),
            vec![("two three".to_owned(), "2 3".to_owned())]
        );
    }

    #[test]
    fn paragraphs_make_passages() {
        let old = "a b c\n\nd e f\n\ng h i\n";
        let new = "a X c Y\n\nd e f\n\ng h Z\n";
        let all = hunks(old, new);
        assert_eq!(passages(&all, old), vec![0..2, 2..3]);
    }

    #[test]
    fn counts_are_words_not_tokens() {
        assert_eq!(counts("one two.", "one two, three four."), (2, 0));
        assert_eq!(counts("one two three", "one"), (0, 2));
        assert_eq!(counts("same", "same"), (0, 0));
    }

    #[test]
    fn multibyte_text_does_not_split_a_character() {
        let old = "Größe ändern und üben";
        let new = "Größe ändert und übte";
        for hunk in hunks(old, new) {
            assert!(old.is_char_boundary(hunk.old.start) && new.is_char_boundary(hunk.new.end));
        }
    }

    #[test]
    fn hunks_rebuild_the_new_text() {
        let old = "Erster Absatz mit Wörtern.\n\nZweiter Absatz.\n";
        let new = "Erster Absatz mit anderen Wörtern!\n\nDritter Absatz, länger.\n\nNeu.\n";
        let mut rebuilt = String::new();
        let mut at = 0;
        for hunk in hunks(old, new) {
            rebuilt.push_str(&old[at..hunk.old.start]);
            rebuilt.push_str(&new[hunk.new]);
            at = hunk.old.end;
        }
        rebuilt.push_str(&old[at..]);
        assert_eq!(rebuilt, new);
    }
}
