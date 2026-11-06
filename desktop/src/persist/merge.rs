//! Three-way merge of two versions of a text against what they started from. Holds no gpui types.
//!
//! The merge works on lines. Where both sides changed the same lines it tries again on sentences, so two edits in one
//! paragraph merge if they do not touch the same sentence. Edits on adjacent lines do not overlap. What overlaps stays
//! as a [`Chunk::Conflict`] of whole lines, for the person to decide. Two edits in one sentence are never blended,
//! since that can make a sentence neither side wrote.
//!
//! [`git_conflict`] is the same merge by Git's rule, for a file Git reported as conflicting: lines only, and lines next
//! to each other do overlap, so a place Git would not merge is never merged here by sentences.

use std::ops::Range;
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use unicode_segmentation::UnicodeSegmentation;

use crate::source::Edit;

/// A diff that takes longer gives a coarser result, which is still correct. Only a rewrite of a whole file gets here.
const DIFF_TIME: Duration = Duration::from_millis(100);

/// The lines of a comment around a [`Choice::Both`], so the pages and the engine see both versions as plain text. The
/// usual marker characters, so a search for them finds these too. The first and last name their side.
const BOTH_MINE: &str = "<!-- <<<<<<< ";
const BOTH_BETWEEN: &str = "<!-- ======= -->";
const BOTH_THEIRS: &str = "<!-- >>>>>>> ";
const COMMENT_END: &str = " -->";

/// What the marker lines of a [`Choice::Both`] name: the author's side, and what changed the file, in the book's language.
#[derive(Debug, Clone, Copy)]
pub struct Sides<'a> {
    pub mine: &'a str,
    pub theirs: &'a str,
}

#[derive(PartialEq)]
enum MarkerLine {
    Mine,
    Between,
    Theirs,
}

/// Which marker line `line` is, whatever side it names, so markers written in any language or an older form are found.
fn marker_line(line: &str) -> Option<MarkerLine> {
    let line = line.trim_end();
    let names_a_side = |start: &str| {
        line.strip_prefix(start)
            .and_then(|rest| rest.strip_suffix(COMMENT_END))
            .is_some_and(|side| !side.is_empty() && !side.contains("--"))
    };
    if line == BOTH_BETWEEN {
        Some(MarkerLine::Between)
    } else if names_a_side(BOTH_MINE) {
        Some(MarkerLine::Mine)
    } else if names_a_side(BOTH_THEIRS) {
        Some(MarkerLine::Theirs)
    } else {
        None
    }
}

/// A marker line naming `side`. A comment cannot hold `--`, which a branch's name may.
fn marker(start: &str, side: &str) -> String {
    format!("{start}{}{COMMENT_END}\n", side.replace("--", "-"))
}

/// The marker lines of each complete [`Choice::Both`] in `lines`: mine, between, and theirs, in that order. A marker
/// line outside such a group is ordinary text, so a lone `<!-- ======= -->` someone wrote is never taken for one.
fn marker_groups(lines: &[&str]) -> Vec<[usize; 3]> {
    let mut groups = Vec::new();
    let (mut mine, mut between) = (None, None);
    for (index, line) in lines.iter().enumerate() {
        match marker_line(line) {
            Some(MarkerLine::Mine) => (mine, between) = (Some(index), None),
            Some(MarkerLine::Between) if mine.is_some() && between.is_none() => between = Some(index),
            Some(MarkerLine::Theirs) => {
                if let (Some(mine), Some(between)) = (mine, between) {
                    groups.push([mine, between, index]);
                }
                (mine, between) = (None, None);
            }
            _ => {}
        }
    }
    groups
}

/// Whether a text still has the marker comments of a [`Choice::Both`].
pub fn has_markers(text: &str) -> bool {
    !marker_groups(&text.lines().collect::<Vec<_>>()).is_empty()
}

/// Whether a block or line is one of the marker comments of a [`Choice::Both`].
pub fn is_marker(text: &str) -> bool {
    !text.trim_end().contains('\n') && marker_line(text).is_some()
}

/// The text with the versions [`Choice::Both`] left in it reduced to `choice`'s side, and the marker lines gone.
pub fn keep_one(text: &str, choice: Choice) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut kept = String::with_capacity(text.len());
    let mut next = 0;
    for [mine, between, theirs] in marker_groups(&lines) {
        let side = |range: std::ops::Range<usize>| lines[range].concat();
        kept.push_str(&side(next..mine));
        if choice != Choice::Theirs {
            kept.push_str(&side(mine + 1..between));
        }
        if choice != Choice::Mine {
            kept.push_str(&side(between + 1..theirs));
        }
        next = theirs + 1;
    }
    kept.push_str(&lines[next..].concat());
    kept
}

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

    /// The first place both sides changed, as each side wrote it.
    pub fn first_conflict(&self) -> Option<(&str, &str)> {
        self.chunks.iter().find_map(|chunk| match chunk {
            Chunk::Conflict { mine, theirs } => Some((mine.as_str(), theirs.as_str())),
            Chunk::Clean(_) => None,
        })
    }

    /// The text, with every conflict settled by `choice`. [`Choice::Both`] names `sides` in its marker lines.
    pub fn text(&self, choice: Choice, sides: Sides) -> String {
        let mut text = String::new();
        for chunk in &self.chunks {
            match (chunk, choice) {
                (Chunk::Clean(clean), _) => text.push_str(clean),
                (Chunk::Conflict { mine, .. }, Choice::Mine) => text.push_str(mine),
                (Chunk::Conflict { theirs, .. }, Choice::Theirs) => text.push_str(theirs),
                (Chunk::Conflict { mine, theirs }, Choice::Both) => {
                    text.push_str(&marker(BOTH_MINE, sides.mine));
                    push_line(&mut text, mine);
                    text.push_str(BOTH_BETWEEN);
                    text.push('\n');
                    push_line(&mut text, theirs);
                    text.push_str(&marker(BOTH_THEIRS, sides.theirs));
                }
            }
        }
        text
    }

    /// The text with every conflict settled by one side, which needs no names.
    pub fn one_side(&self, choice: Choice) -> String {
        debug_assert_ne!(choice, Choice::Both);
        self.text(choice, Sides { mine: "", theirs: "" })
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
    let rules = Rules {
        by_sentences: true,
        touching_overlaps: false,
    };
    merge_tokens(&lines(base), &lines(mine), &lines(theirs), rules, &mut merged);
    merged
}

/// Merges a file Git stopped on. Every place Git reported stays a conflict. Should this merge find none, where its diff and
/// Git's differ, the whole file is the conflict, since Git said there is one.
pub fn git_conflict(base: &str, mine: &str, theirs: &str) -> Merged {
    let mut merged = Merged::default();
    let rules = Rules {
        by_sentences: false,
        touching_overlaps: true,
    };
    merge_tokens(&lines(base), &lines(mine), &lines(theirs), rules, &mut merged);
    if merged.is_clean() {
        merged.chunks = vec![Chunk::Conflict {
            mine: mine.to_owned(),
            theirs: theirs.to_owned(),
        }];
    }
    merged
}

/// What counts as an overlap in a merge.
#[derive(Clone, Copy)]
struct Rules {
    /// Lines both sides changed get a second try, sentence by sentence.
    by_sentences: bool,
    /// Changes with nothing unchanged between them meet.
    touching_overlaps: bool,
}

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn words(text: &str) -> Vec<&str> {
    text.split_word_bounds().collect()
}

/// UAX #29 sentences, each without the white space after it, which is a token of its own. So a sentence added after
/// the last one of a line does not change that one, and another edit to it still merges.
fn sentences(text: &str) -> Vec<&str> {
    text.split_sentence_bounds()
        .flat_map(|sentence| {
            let (body, space) = sentence.split_at(sentence.trim_end().len());
            [body, space].into_iter().filter(|token| !token.is_empty())
        })
        .collect()
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
fn meets(hunk: &Hunk, start: usize, end: usize, rules: Rules) -> bool {
    if rules.touching_overlaps {
        return hunk.base.start <= end && start <= hunk.base.end;
    }
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

fn merge_tokens(base: &[&str], mine: &[&str], theirs: &[&str], rules: Rules, merged: &mut Merged) {
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
            let grown = if let Some(hunk) = mine_hunks.get(next_mine).filter(|hunk| meets(hunk, start, end, rules)) {
                next_mine += 1;
                hunk.base.end
            } else if let Some(hunk) = their_hunks
                .get(next_theirs)
                .filter(|hunk| meets(hunk, start, end, rules))
            {
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
                settle(&base[start..end], mine_part, theirs_part, rules, merged);
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

/// Both sides changed these tokens differently. Tries the same merge on sentences, and otherwise keeps both as a
/// conflict.
fn settle(base: &[&str], mine: &[&str], theirs: &[&str], rules: Rules, merged: &mut Merged) {
    let (base, mine, theirs) = (base.concat(), mine.concat(), theirs.concat());
    if rules.by_sentences {
        let inner_rules = Rules {
            by_sentences: false,
            ..rules
        };
        let mut inner = Merged::default();
        merge_tokens(
            &sentences(&base),
            &sentences(&mine),
            &sentences(&theirs),
            inner_rules,
            &mut inner,
        );
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

/// Which version of the file a change from outside started from.
///
/// A program may read a file, take its time, and write all of it back. Its text then undoes every change saved since it
/// read, and only then is that earlier version the starting point, so what was saved meanwhile stays. Any other text is
/// a change to `current`, also one that takes back part of what was saved: that was done on purpose, as by deleting a
/// passage, settling a conflict, or putting a file back with `git checkout`.
pub fn starting_point<'a>(current: &'a str, earlier: &'a [impl AsRef<str>], theirs: &str) -> &'a str {
    if earlier.iter().any(|version| version.as_ref() == theirs) {
        return current;
    }
    (earlier.iter().rev())
        .map(AsRef::as_ref)
        .find(|version| undoes_since(version, current, theirs))
        .unwrap_or(current)
}

/// Whether `theirs` undoes every change from `version` to `current`: going back to `version` from `theirs` would
/// change nothing.
fn undoes_since(version: &str, current: &str, theirs: &str) -> bool {
    let merged = three_way(current, version, theirs);
    merged.is_clean() && merged.one_side(Choice::Mine) == theirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(base: &str, mine: &str, theirs: &str) -> String {
        let merged = three_way(base, mine, theirs);
        assert!(merged.is_clean(), "expected a clean merge: {merged:?}");
        merged.one_side(Choice::Mine)
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
    fn different_sentences_of_one_line_merge() {
        let base = "The quick fox jumps. The dog sleeps. It is late.\n";
        assert_eq!(
            clean(
                base,
                "The slow fox jumps. The dog sleeps. It is late.\n",
                "The quick fox jumps. The dog sleeps. It is very late.\n"
            ),
            "The slow fox jumps. The dog sleeps. It is very late.\n"
        );
    }

    #[test]
    fn a_sentence_added_after_one_the_other_side_edited_merges() {
        assert_eq!(
            clean("Foo. Bar.\n", "Foo. Bar. Baz.\n", "Foo. Bars.\n"),
            "Foo. Bars. Baz.\n"
        );
    }

    /// What an outside change once did to the real book's sentence: "gar nur Leere.", which neither side wrote.
    #[test]
    fn different_words_of_one_sentence_conflict_on_the_whole_line() {
        let (base, mine, theirs) = (
            "Er sah. Da war nichts.\n",
            "Er sah. Da war nur Leere.\n",
            "Er sah. Da war gar nichts.\n",
        );
        let merged = three_way(base, mine, theirs);
        assert_eq!(merged.first_conflict(), Some((mine, theirs)));
        let merged = three_way(
            "The quick brown fox jumps over the lazy dog.\n",
            "The slow brown fox jumps over the lazy dog.\n",
            "The quick brown fox jumps over the sleepy dog.\n",
        );
        assert!(!merged.is_clean());
    }

    #[test]
    fn the_same_word_changed_differently_conflicts_on_its_whole_line() {
        let merged = three_way(
            "Before.\nThe quick fox.\nAfter.\n",
            "Before.\nThe slow fox.\nAfter.\n",
            "Before.\nThe sly fox.\nAfter.\n",
        );
        assert!(!merged.is_clean());
        assert_eq!(merged.one_side(Choice::Mine), "Before.\nThe slow fox.\nAfter.\n");
        assert_eq!(merged.one_side(Choice::Theirs), "Before.\nThe sly fox.\nAfter.\n");
    }

    /// Git reported a conflict, so even different sentences of one line stay one.
    #[test]
    fn git_s_conflict_is_never_merged_by_sentences() {
        let (base, mine, theirs) = (
            "Er sah. Da war es.\n",
            "Er sah nichts. Da war es.\n",
            "Er sah. Da war er.\n",
        );
        assert!(three_way(base, mine, theirs).is_clean());
        let merged = git_conflict(base, mine, theirs);
        assert_eq!(merged.first_conflict(), Some((mine, theirs)));
        assert_eq!(merged.one_side(Choice::Theirs), theirs);
    }

    #[test]
    fn a_git_conflict_keeps_what_merges_by_lines_and_lines_that_touch_overlap() {
        let base = "a\nb\nc\nd\ne\nf\n";
        let merged = git_conflict(base, "a\nB\nc\nd\ne\nf\n", "a\nb\nC\nd\ne\nF\n");
        assert!(!merged.is_clean());
        assert_eq!(merged.one_side(Choice::Mine), "a\nB\nc\nd\ne\nF\n");
    }

    #[test]
    fn a_git_conflict_the_lines_do_not_show_is_the_whole_file() {
        let merged = git_conflict("a\n", "b\n", "b\n");
        assert_eq!(merged.first_conflict(), Some(("b\n", "b\n")));
    }

    const SIDES: Sides = Sides {
        mine: "deins",
        theirs: "entwurf--2",
    };

    #[test]
    fn marker_comments_are_found() {
        let merged = three_way("x\nold\ny\n", "x\nmine\ny\n", "x\ntheirs\ny\n");
        assert!(has_markers(&merged.text(Choice::Both, SIDES)));
        assert!(!has_markers(&merged.one_side(Choice::Mine)));
    }

    #[test]
    fn seeing_both_keeps_both_between_comments_that_name_the_sides() {
        let merged = three_way("x\nold\ny\n", "x\nmine\ny\n", "x\ntheirs\ny\n");
        let both = merged.text(Choice::Both, SIDES);
        assert_eq!(
            both,
            "x\n<!-- <<<<<<< deins -->\nmine\n<!-- ======= -->\ntheirs\n<!-- >>>>>>> entwurf-2 -->\ny\n"
        );
        assert_eq!(keep_one(&both, Choice::Theirs), "x\ntheirs\ny\n");
    }

    #[test]
    fn a_marker_line_outside_a_complete_group_is_text() {
        let text = "Before.\n<!-- ======= -->\nAfter.\n<!-- >>>>>>> deins -->\nEnd.\n";
        assert!(!has_markers(text));
        assert_eq!(keep_one(text, Choice::Mine), text);
        let both = format!("{text}<!-- <<<<<<< yours -->\nMine.\n<!-- ======= -->\nTheirs.\n<!-- >>>>>>> app -->\n");
        assert!(has_markers(&both));
        assert_eq!(keep_one(&both, Choice::Mine), format!("{text}Mine.\n"));
    }

    #[test]
    fn markers_written_in_the_old_form_still_settle() {
        let old = "x\n<!-- <<<<<<< yours -->\nmine\n<!-- ======= -->\ntheirs\n<!-- >>>>>>> from outside -->\ny\n";
        assert!(has_markers(old));
        assert!(is_marker("<!-- >>>>>>> from outside -->\n"));
        assert_eq!(keep_one(old, Choice::Mine), "x\nmine\ny\n");
        assert!(!is_marker("<!-- a note -->\n"));
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
        assert_eq!(merged.one_side(Choice::Mine), "a\nmine\n");
        assert_eq!(merged.one_side(Choice::Theirs), "a\ntheirs\n");
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
                "Größe und Maß. Überall hier.\n",
                "Größte und Maß. Überall hier.\n",
                "Größe und Maß. Überall dort.\n"
            ),
            "Größte und Maß. Überall dort.\n"
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
        assert_eq!(merged.one_side(Choice::Mine), "a\nd\n");
        assert_eq!(merged.one_side(Choice::Theirs), "a\nb\nC\nd\n");
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
    fn a_write_from_an_older_read_keeps_a_sentence_typed_in_its_own_paragraph() {
        let old = "One two three. Four.\n";
        let saved = "One two three. Four five.\n";
        let theirs = "One TWO three. Four.\n";
        let earlier = [old];
        let start = starting_point(saved, &earlier, theirs);
        assert_eq!(clean(start, saved, theirs), "One TWO three. Four five.\n");
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
    fn keeping_one_side_takes_the_markers_and_the_other_side_out() {
        let merged = three_way("A.\nB.\nC.\n", "A.\nMine.\nC.\n", "A.\nTheirs.\nC.\n");
        let both = merged.text(Choice::Both, SIDES);
        assert!(both.lines().filter(|line| is_marker(line)).count() == 3, "{both}");
        assert_eq!(keep_one(&both, Choice::Mine), "A.\nMine.\nC.\n");
        assert_eq!(keep_one(&both, Choice::Theirs), "A.\nTheirs.\nC.\n");
    }

    #[test]
    fn a_deletion_of_saved_text_starts_from_the_newest_version() {
        let old = "One.\n\nTwo.\n";
        let saved = "One.\n\nTyped.\n\nTwo.\n\nThree.\n";
        // Deletes what was typed, and keeps the paragraph saved after it, so it read the newest version.
        let theirs = "One.\n\nTwo.\n\nThree.\n";
        assert_eq!(starting_point(saved, &[old], theirs), saved);
        assert_eq!(clean(saved, saved, theirs), theirs);
    }

    #[test]
    fn a_write_from_the_newest_version_starts_from_it() {
        let saved = "One two.\nThree.\n";
        let earlier = ["One.\n", "One two.\n"];
        assert_eq!(starting_point(saved, &earlier, "One two.\nThree!\n"), saved);
    }
}
