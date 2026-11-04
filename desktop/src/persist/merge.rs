//! Three-way merge of two versions of a text against what they started from. Holds no gpui types.
//!
//! The merge works on lines. Where both sides changed the same lines it tries again on words, so two edits in one
//! paragraph merge if they do not touch the same words. Edits on adjacent lines do not overlap. What overlaps stays
//! as a [`Chunk::Conflict`] of whole lines, for the person to decide.

use std::ops::Range;
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use unicode_segmentation::UnicodeSegmentation;

use crate::source::Edit;

/// A diff that takes longer gives a coarser result, which is still correct. Only a rewrite of a whole file gets here.
const DIFF_TIME: Duration = Duration::from_millis(100);

/// Lines of a comment around a [`Choice::Both`], so the pages and the engine see both versions as plain text. The
/// usual marker characters, so a search for them finds these too.
const BOTH_MINE: &str = "<!-- <<<<<<< yours -->\n";
const BOTH_BETWEEN: &str = "<!-- ======= -->\n";
const BOTH_THEIRS: &str = "<!-- >>>>>>> from outside -->\n";

/// A piece of the merge's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    Clean(String),
    Conflict { mine: String, theirs: String },
}

/// How a person settled the conflicts in a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Mine,
    Theirs,
    /// Both versions stay in the text, one after the other between comment lines.
    Both,
}

/// The result of a merge, in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Merged {
    chunks: Vec<Chunk>,
}

impl Merged {
    pub fn is_clean(&self) -> bool {
        !self.chunks.iter().any(|chunk| matches!(chunk, Chunk::Conflict { .. }))
    }

    /// The text, with every conflict settled by `choice`.
    pub fn text(&self, choice: Choice) -> String {
        let mut text = String::new();
        for chunk in &self.chunks {
            match (chunk, choice) {
                (Chunk::Clean(clean), _) => text.push_str(clean),
                (Chunk::Conflict { mine, .. }, Choice::Mine) => text.push_str(mine),
                (Chunk::Conflict { theirs, .. }, Choice::Theirs) => text.push_str(theirs),
                (Chunk::Conflict { mine, theirs }, Choice::Both) => {
                    text.push_str(BOTH_MINE);
                    push_line(&mut text, mine);
                    text.push_str(BOTH_BETWEEN);
                    push_line(&mut text, theirs);
                    text.push_str(BOTH_THEIRS);
                }
            }
        }
        text
    }

    fn clean(&mut self, tokens: &[&str]) {
        if tokens.is_empty() {
            return;
        }
        if let Some(Chunk::Clean(last)) = self.chunks.last_mut() {
            tokens.iter().for_each(|token| last.push_str(token));
        } else {
            self.chunks.push(Chunk::Clean(tokens.concat()));
        }
    }
}

/// A marker needs a line of its own, also after a last line without a newline.
fn push_line(text: &mut String, line: &str) {
    text.push_str(line);
    if !line.is_empty() && !line.ends_with('\n') {
        text.push('\n');
    }
}

/// Merges what `mine` and `theirs` changed in `base`.
pub fn three_way(base: &str, mine: &str, theirs: &str) -> Merged {
    let mut merged = Merged::default();
    merge_tokens(&lines(base), &lines(mine), &lines(theirs), true, &mut merged);
    merged
}

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn words(text: &str) -> Vec<&str> {
    text.split_word_bounds().collect()
}

/// A run of changed tokens: `base` was replaced by `other`.
#[derive(Debug, Clone)]
struct Hunk {
    base: Range<usize>,
    other: Range<usize>,
}

fn hunks(old: &[&str], new: &[&str]) -> Vec<Hunk> {
    let deadline = Instant::now() + DIFF_TIME;
    capture_diff_slices_deadline(Algorithm::Myers, old, new, Some(deadline))
        .into_iter()
        .filter_map(|op| match op {
            DiffOp::Equal { .. } => None,
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => Some(Hunk {
                base: old_index..old_index + old_len,
                other: new_index..new_index,
            }),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => Some(Hunk {
                base: old_index..old_index,
                other: new_index..new_index + new_len,
            }),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => Some(Hunk {
                base: old_index..old_index + old_len,
                other: new_index..new_index + new_len,
            }),
        })
        .fold(Vec::new(), |mut all: Vec<Hunk>, hunk| {
            match all.last_mut() {
                Some(last) if last.base.end == hunk.base.start && last.other.end == hunk.other.start => {
                    last.base.end = hunk.base.end;
                    last.other.end = hunk.other.end;
                }
                _ => all.push(hunk),
            }
            all
        })
}

/// Whether a hunk's change meets the region of base tokens `start..end` others already claim. An insertion at the very
/// edge of a change does not, except two insertions at one place, which are one place both sides wrote.
fn meets(hunk: &Hunk, start: usize, end: usize) -> bool {
    if start == end && hunk.base.is_empty() {
        return hunk.base.start == start;
    }
    if hunk.base.is_empty() {
        return start < hunk.base.start && hunk.base.start < end;
    }
    if start == end {
        return hunk.base.start < start && start < hunk.base.end;
    }
    hunk.base.start < end && start < hunk.base.end
}

fn merge_tokens(base: &[&str], mine: &[&str], theirs: &[&str], refine: bool, merged: &mut Merged) {
    let mine_hunks = hunks(base, mine);
    let their_hunks = hunks(base, theirs);
    let (mut next_mine, mut next_theirs) = (0, 0);
    let mut at = 0;
    loop {
        let (mine_next, their_next) = (mine_hunks.get(next_mine), their_hunks.get(next_theirs));
        // The next change in base order. At one place, an insertion comes before what replaces the tokens after it.
        let take_mine = match (mine_next, their_next) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(a), Some(b)) => (a.base.start, a.base.end) <= (b.base.start, b.base.end),
        };
        let first = if take_mine { mine_next } else { their_next };
        let Some(first) = first else { break };
        let (start, mut end) = (first.base.start, first.base.end);
        let (mine_from, theirs_from) = (next_mine, next_theirs);
        if take_mine {
            next_mine += 1;
        } else {
            next_theirs += 1;
        }
        // Everything either side changed that meets this change, growing until nothing more does.
        loop {
            let grown = if let Some(hunk) = mine_hunks.get(next_mine).filter(|hunk| meets(hunk, start, end)) {
                next_mine += 1;
                hunk.base.end
            } else if let Some(hunk) = their_hunks.get(next_theirs).filter(|hunk| meets(hunk, start, end)) {
                next_theirs += 1;
                hunk.base.end
            } else {
                break;
            };
            end = end.max(grown);
        }
        merged.clean(&base[at..start]);
        let mine_part = side(mine, &mine_hunks[mine_from..next_mine], start, end);
        let theirs_part = side(theirs, &their_hunks[theirs_from..next_theirs], start, end);
        match (mine_part, theirs_part) {
            (Some(part), None) | (None, Some(part)) => merged.clean(part),
            (Some(mine_part), Some(theirs_part)) if mine_part == theirs_part => merged.clean(mine_part),
            (Some(mine_part), Some(theirs_part)) => {
                settle(&base[start..end], mine_part, theirs_part, refine, merged);
            }
            (None, None) => {}
        }
        at = end;
    }
    merged.clean(&base[at..]);
}

/// What one side made of the base tokens `start..end`, given its hunks there. `None` if it changed nothing there.
fn side<'a, 'b>(tokens: &'a [&'b str], hunks: &[Hunk], start: usize, end: usize) -> Option<&'a [&'b str]> {
    let (first, last) = (hunks.first()?, hunks.last()?);
    let from = first.other.start - (first.base.start - start);
    let to = last.other.end + (end - last.base.end);
    Some(&tokens[from..to])
}

/// Both sides changed these tokens differently. Tries the same merge on words, and otherwise keeps both as a conflict.
fn settle(base: &[&str], mine: &[&str], theirs: &[&str], refine: bool, merged: &mut Merged) {
    let (base, mine, theirs) = (base.concat(), mine.concat(), theirs.concat());
    if refine {
        let mut inner = Merged::default();
        merge_tokens(&words(&base), &words(&mine), &words(&theirs), false, &mut inner);
        if inner.is_clean() {
            for chunk in inner.chunks {
                if let Chunk::Clean(text) = chunk {
                    merged.clean(&[&text]);
                }
            }
            return;
        }
    }
    merged.chunks.push(Chunk::Conflict { mine, theirs });
}

/// Which lines changed from `old` to `new`, as pairs of line ranges, for showing whole lines. See [`edits_between`] for
/// the same changes as small as words.
pub fn line_changes(old: &str, new: &str) -> Vec<(Range<usize>, Range<usize>)> {
    hunks(&lines(old), &lines(new))
        .into_iter()
        .map(|hunk| (hunk.base, hunk.other))
        .collect()
}

/// The edits that turn `old` into `new`, as small as words, for an undo step that keeps the caret in place.
pub fn edits_between(old: &str, new: &str) -> Vec<Edit> {
    let (old_lines, new_lines) = (lines(old), lines(new));
    let starts = |tokens: &[&str]| -> Vec<usize> {
        let mut start = 0;
        let mut all: Vec<usize> = tokens
            .iter()
            .map(|token| {
                let at = start;
                start += token.len();
                at
            })
            .collect();
        all.push(start);
        all
    };
    let (old_starts, new_starts) = (starts(&old_lines), starts(&new_lines));
    let mut edits = Vec::new();
    for hunk in hunks(&old_lines, &new_lines) {
        let old_from = old_starts[hunk.base.start];
        let old_part = &old[old_from..old_starts[hunk.base.end]];
        let new_part = &new[new_starts[hunk.other.start]..new_starts[hunk.other.end]];
        let (old_words, new_words) = (words(old_part), words(new_part));
        let (old_word_starts, new_word_starts) = (starts(&old_words), starts(&new_words));
        for inner in hunks(&old_words, &new_words) {
            edits.push(Edit::new(
                old_from + old_word_starts[inner.base.start]..old_from + old_word_starts[inner.base.end],
                &new_part[new_word_starts[inner.other.start]..new_word_starts[inner.other.end]],
            ));
        }
    }
    edits
}

/// Where a place in the text stands after `edits`, sorted and against the text before them. Text inserted right at a
/// caret goes after it. In replaced text the place keeps its distance from the start, up to the new text's end.
pub fn map_offset(offset: usize, edits: &[Edit]) -> usize {
    let mut delta = 0isize;
    for edit in edits {
        if offset <= edit.range.start {
            break;
        }
        if offset < edit.range.end {
            let inside = (offset - edit.range.start).min(edit.text.len());
            return (edit.range.start as isize + delta) as usize + inside;
        }
        delta += edit.text.len() as isize - edit.range.len() as isize;
    }
    (offset as isize + delta) as usize
}

/// How much text separates two versions, in bytes changed.
fn distance(old: &str, new: &str) -> usize {
    edits_between(old, new)
        .iter()
        .map(|edit| edit.range.len() + edit.text.len())
        .sum()
}

/// Which of the file's earlier versions a change from outside started from.
///
/// A program may read a file, take its time, and write the whole of it back. Measured against the newest version, that
/// write also takes back everything saved since it read. So the base of the merge is the version closest to what it
/// wrote, and what was saved since then stays. A file exactly like an earlier version is another matter: nobody
/// edited that, it was put back on purpose by something like `git checkout`, and it counts as a change to `current`.
pub fn starting_point<'a>(current: &'a str, earlier: &'a [impl AsRef<str>], theirs: &str) -> &'a str {
    let mut best = (current, distance(current, theirs));
    if best.1 == 0 || earlier.iter().any(|version| version.as_ref() == theirs) {
        return current;
    }
    for version in earlier.iter().rev() {
        let measured = distance(version.as_ref(), theirs);
        if measured < best.1 {
            best = (version.as_ref(), measured);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(base: &str, mine: &str, theirs: &str) -> String {
        let merged = three_way(base, mine, theirs);
        assert!(merged.is_clean(), "expected a clean merge: {merged:?}");
        merged.text(Choice::Mine)
    }

    fn apply(old: &str, edits: &[Edit]) -> String {
        let mut text = old.to_owned();
        for edit in edits.iter().rev() {
            text.replace_range(edit.range.clone(), &edit.text);
        }
        text
    }

    #[test]
    fn edits_in_different_paragraphs_merge() {
        let base = "One.\n\nTwo.\n\nThree.\n";
        assert_eq!(
            clean(
                base,
                "One, mine.\n\nTwo.\n\nThree.\n",
                "One.\n\nTwo.\n\nThree, theirs.\n"
            ),
            "One, mine.\n\nTwo.\n\nThree, theirs.\n"
        );
    }

    #[test]
    fn edits_on_adjacent_lines_merge() {
        let base = "a\nb\nc\nd\n";
        assert_eq!(clean(base, "a\nB\nc\nd\n", "a\nb\nC\nd\n"), "a\nB\nC\nd\n");
    }

    #[test]
    fn the_same_line_merges_when_the_words_differ() {
        let base = "The quick brown fox jumps over the lazy dog.\n";
        assert_eq!(
            clean(
                base,
                "The slow brown fox jumps over the lazy dog.\n",
                "The quick brown fox jumps over the sleepy dog.\n"
            ),
            "The slow brown fox jumps over the sleepy dog.\n"
        );
    }

    #[test]
    fn the_same_word_changed_differently_conflicts_on_its_whole_line() {
        let merged = three_way(
            "Before.\nThe quick fox.\nAfter.\n",
            "Before.\nThe slow fox.\nAfter.\n",
            "Before.\nThe sly fox.\nAfter.\n",
        );
        assert!(!merged.is_clean());
        assert_eq!(merged.text(Choice::Mine), "Before.\nThe slow fox.\nAfter.\n");
        assert_eq!(merged.text(Choice::Theirs), "Before.\nThe sly fox.\nAfter.\n");
    }

    #[test]
    fn seeing_both_keeps_both_between_comments() {
        let merged = three_way("x\nold\ny\n", "x\nmine\ny\n", "x\ntheirs\ny\n");
        assert_eq!(
            merged.text(Choice::Both),
            "x\n<!-- <<<<<<< yours -->\nmine\n<!-- ======= -->\ntheirs\n<!-- >>>>>>> from outside -->\ny\n"
        );
    }

    #[test]
    fn the_same_change_on_both_sides_is_not_a_conflict() {
        assert_eq!(clean("a\nb\n", "a\nB\n", "a\nB\n"), "a\nB\n");
    }

    #[test]
    fn one_side_inserting_at_the_end_merges() {
        assert_eq!(clean("a\nb\n", "a\nb\nc\n", "A\nb\n"), "A\nb\nc\n");
        assert_eq!(clean("a\nb", "a\nb c", "A\nb"), "A\nb c");
    }

    #[test]
    fn both_sides_inserting_at_the_end_ask() {
        let merged = three_way("a\n", "a\nmine\n", "a\ntheirs\n");
        assert!(!merged.is_clean());
        assert_eq!(merged.text(Choice::Mine), "a\nmine\n");
        assert_eq!(merged.text(Choice::Theirs), "a\ntheirs\n");
    }

    #[test]
    fn multibyte_text_merges_on_character_boundaries() {
        let base = "Größe und Maß.\n\nÜberall 🦀 Straße.\n";
        assert_eq!(
            clean(
                base,
                "Größe und Maße.\n\nÜberall 🦀 Straße.\n",
                "Größe und Maß.\n\nÜberall 🦀🦀 Straße.\n"
            ),
            "Größe und Maße.\n\nÜberall 🦀🦀 Straße.\n"
        );
        assert_eq!(
            clean(
                "Größe und Maß hier.\n",
                "Größte und Maß hier.\n",
                "Größe und Maß dort.\n"
            ),
            "Größte und Maß dort.\n"
        );
    }

    #[test]
    fn an_insert_beside_a_replacement_keeps_both() {
        assert_eq!(clean("a\nb\nc\n", "a\nnew\nb\nc\n", "a\nB\nc\n"), "a\nnew\nB\nc\n");
    }

    #[test]
    fn a_deletion_against_an_edit_inside_it_conflicts() {
        let merged = three_way("a\nb\nc\nd\n", "a\nd\n", "a\nb\nC\nd\n");
        assert!(!merged.is_clean());
        assert_eq!(merged.text(Choice::Mine), "a\nd\n");
        assert_eq!(merged.text(Choice::Theirs), "a\nb\nC\nd\n");
    }

    #[test]
    fn edits_between_rebuild_the_new_text_with_small_edits() {
        let old = "Größe eins.\nZwei.\nDrei.\n";
        let new = "Größe vier.\nZwei.\nDrei.\nFünf.\n";
        let edits = edits_between(old, new);
        assert_eq!(apply(old, &edits), new);
        assert_eq!(edits[0].text, "vier");
        assert!(edits_between(old, old).is_empty());
    }

    #[test]
    fn a_caret_moves_with_the_text_around_it() {
        let old = "one two three\n";
        let edits = edits_between(old, "zero one two four three\n");
        let two = old.find("two").unwrap();
        assert_eq!(map_offset(two, &edits), "zero one ".len());
        assert_eq!(map_offset(old.len(), &edits), "zero one two four three\n".len());
    }

    #[test]
    fn text_inserted_at_the_caret_goes_after_it() {
        let edits = vec![Edit::new(3..3, "new")];
        assert_eq!(map_offset(3, &edits), 3);
        assert_eq!(map_offset(4, &edits), 7);
    }

    #[test]
    fn a_caret_in_replaced_text_stays_inside_it() {
        let edits = vec![Edit::new(2..8, "xy")];
        assert_eq!(map_offset(4, &edits), 4);
        assert_eq!(map_offset(7, &edits), 4);
        assert_eq!(map_offset(8, &edits), 4);
    }

    #[test]
    fn a_write_from_an_older_read_starts_from_that_version() {
        let old = "One.\n\nTwo.\n\nThree.\n";
        let saved = "One.\n\nTwo, typed since.\n\nThree.\n";
        let theirs = "One.\n\nTwo.\n\nThree, by the agent.\n";
        let earlier = [old];
        let start = starting_point(saved, &earlier, theirs);
        assert_eq!(start, old);
        assert_eq!(
            clean(start, saved, theirs),
            "One.\n\nTwo, typed since.\n\nThree, by the agent.\n"
        );
    }

    #[test]
    fn a_write_from_an_older_read_keeps_words_typed_in_its_own_paragraph() {
        let old = "One two three.\n";
        let saved = "One two three four.\n";
        let theirs = "One TWO three.\n";
        let earlier = [old];
        let start = starting_point(saved, &earlier, theirs);
        assert_eq!(clean(start, saved, theirs), "One TWO three four.\n");
    }

    #[test]
    fn a_version_put_back_exactly_counts_as_a_change_to_the_newest() {
        let old = "One.\n";
        let saved = "One, typed.\n";
        let earlier = [old];
        assert_eq!(starting_point(saved, &earlier, old), saved);
        assert_eq!(clean(saved, saved, old), old);
    }

    #[test]
    fn a_write_from_the_newest_version_starts_from_it() {
        let saved = "One two.\nThree.\n";
        let earlier = ["One.\n", "One two.\n"];
        assert_eq!(starting_point(saved, &earlier, "One two.\nThree!\n"), saved);
    }
}
