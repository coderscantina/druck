//! What a turn changed in files, as hunks that can be found again in text that has moved on.
//!
//! The CLI reports each edit as a `structuredPatch`: hunks with line numbers and the lines on both sides. Those line
//! numbers are only true for the file right after the edit. The author goes on typing, a reload replaces a buffer, a
//! second edit shifts lines. So a hunk is never held as a position. It is found again by its lines, nearest to where
//! it was last seen, and a hunk whose lines are gone is reported as such and offers nothing.
//!
//! This is the seam to the Persistence slice. It takes a hunk and the buffer's current text and answers where the
//! hunk is, what undoes it, and what applies it, without knowing how text reached the buffer.

use std::ops::Range;
use std::path::PathBuf;

use serde_json::Value;

use super::diff::{Mark, lines};
use crate::session::ChapterId;

/// One line of a patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchLine {
    pub mark: Mark,
    pub text: String,
}

/// A run of changed lines with the unchanged lines around it, as the CLI reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The line the hunk starts at before the change and after it, counting from 1.
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<PatchLine>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Old,
    New,
}

/// The lines of one side of a hunk: the unchanged lines before the first change, everything from the first change to
/// the last, and the unchanged lines after.
struct Sides<'a> {
    before: Vec<&'a str>,
    core: Vec<&'a str>,
    after: Vec<&'a str>,
}

impl Hunk {
    /// Reads the hunks of a `structuredPatch`. A hunk that is not shaped as expected is left out.
    pub fn parse_all(patch: &Value) -> Vec<Self> {
        let Some(hunks) = patch.as_array() else {
            return Vec::new();
        };
        hunks.iter().filter_map(Self::parse).collect()
    }

    fn parse(hunk: &Value) -> Option<Self> {
        let lines = hunk["lines"]
            .as_array()?
            .iter()
            .filter_map(|line| {
                let line = line.as_str()?;
                let mark = match line.chars().next()? {
                    ' ' => Mark::Context,
                    '+' => Mark::Added,
                    '-' => Mark::Removed,
                    // "\ No newline at end of file"
                    _ => return None,
                };
                Some(PatchLine {
                    mark,
                    text: line[1..].to_owned(),
                })
            })
            .collect();
        Some(Self {
            old_start: usize::try_from(hunk["oldStart"].as_u64()?).ok()?,
            new_start: usize::try_from(hunk["newStart"].as_u64()?).ok()?,
            lines,
        })
    }

    /// The line, counting from 1, where the first changed line is after the change.
    pub fn first_changed_line_number(&self) -> usize {
        self.first_changed_line(Side::New) + 1
    }

    pub fn added(&self) -> usize {
        self.lines.iter().filter(|line| line.mark == Mark::Added).count()
    }

    pub fn removed(&self) -> usize {
        self.lines.iter().filter(|line| line.mark == Mark::Removed).count()
    }

    fn sides(&self, side: Side) -> Sides<'_> {
        let changed = |line: &PatchLine| line.mark != Mark::Context;
        let first = self.lines.iter().position(changed).unwrap_or(self.lines.len());
        let last = self.lines.iter().rposition(changed).map_or(first, |last| last + 1);
        let on_side = |line: &&PatchLine| match (line.mark, side) {
            (Mark::Context, _) | (Mark::Removed, Side::Old) | (Mark::Added, Side::New) => true,
            (Mark::Removed, Side::New) | (Mark::Added, Side::Old) => false,
        };
        let texts = |range: Range<usize>| {
            self.lines[range]
                .iter()
                .filter(on_side)
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        };
        Sides {
            before: texts(0..first),
            core: texts(first..last),
            after: texts(last..self.lines.len()),
        }
    }

    /// The lines before the change, for a hunk's own line number to guess from.
    fn first_changed_line(&self, side: Side) -> usize {
        let start = match side {
            Side::Old => self.old_start,
            Side::New => self.new_start,
        };
        let before = self.lines.iter().take_while(|line| line.mark == Mark::Context).count();
        start.saturating_sub(1) + before
    }
}

/// The text's lines with where each one starts.
struct Text<'a> {
    text: &'a str,
    lines: Vec<&'a str>,
    starts: Vec<usize>,
}

impl<'a> Text<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = Vec::new();
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            starts.push(at);
            at += line.len();
        }
        Self {
            text,
            lines: lines(text),
            starts,
        }
    }

    /// Where line `index` ends, past its line break, and the text's end for a line past the last.
    fn line_end(&self, index: usize) -> usize {
        self.starts.get(index + 1).copied().unwrap_or(self.text.len())
    }

    fn start_of(&self, index: usize) -> usize {
        self.starts.get(index).copied().unwrap_or(self.text.len())
    }

    /// Every line index where `pattern` is found, nearest to `near` first.
    fn find(&self, pattern: &[&str], near: usize) -> Vec<usize> {
        if pattern.is_empty() || pattern.len() > self.lines.len() {
            return Vec::new();
        }
        let mut found: Vec<usize> = (0..=self.lines.len() - pattern.len())
            .filter(|&at| self.lines[at..at + pattern.len()] == *pattern)
            .collect();
        found.sort_by_key(|&at| at.abs_diff(near));
        found
    }
}

/// Where a side of a hunk sits in a text: the bytes of its changed lines, which is empty where lines were removed
/// and none are left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub range: Range<usize>,
    /// The line the changed lines start at, counting from 0.
    pub line: usize,
}

fn locate(hunk: &Hunk, side: Side, text: &Text, near: usize) -> Option<Place> {
    let sides = hunk.sides(side);
    let whole: Vec<&str> = [&sides.before[..], &sides.core[..], &sides.after[..]].concat();
    let first = match text.find(&whole, near.saturating_sub(sides.before.len())).first() {
        Some(at) => at + sides.before.len(),
        None => {
            // The lines around may have been edited. The changed lines alone count if they are found once.
            let alone = text.find(&sides.core, near);
            match alone[..] {
                [only] => only,
                _ => return None,
            }
        }
    };
    let range = if sides.core.is_empty() {
        let at = text.start_of(first);
        at..at
    } else {
        text.start_of(first)..text.line_end(first + sides.core.len() - 1)
    };
    Some(Place { range, line: first })
}

/// Lines to write over `range` of `text`, each ended by a line break unless it is the text's last line and had none.
fn join(lines: &[&str], text: &Text, range: &Range<usize>) -> String {
    let unterminated = range.end == text.text.len() && !text.text.is_empty() && !text.text.ends_with('\n');
    let mut out = lines.join("\n");
    if lines.is_empty() {
        return out;
    }
    if range.is_empty() && unterminated {
        // The lines go after a last line that has no line break yet.
        out.insert(0, '\n');
    } else if range.is_empty() || !unterminated {
        out.push('\n');
    }
    out
}

/// A change to make to a text: replace `range` with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub range: Range<usize>,
    pub text: String,
}

/// The hunk as the author sees it in their text now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// The changed lines are there.
    Applied(Place),
    /// The lines from before the change are there, so it was undone, or never reached this text.
    Absent(Place),
    /// Neither: the text around it was edited or moved on.
    Moved,
}

/// A hunk together with where it was last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub hunk: Hunk,
    /// The chapter whose buffer holds the file, once a merge found it open.
    pub chapter: Option<ChapterId>,
    /// The line, counting from 0, to look nearest to. It follows the hunk as it is found.
    near: usize,
}

impl Tracked {
    pub fn new(hunk: Hunk) -> Self {
        let near = hunk.first_changed_line(Side::New);
        Self {
            hunk,
            chapter: None,
            near,
        }
    }

    pub fn presence(&mut self, text: &str) -> Presence {
        let text = Text::new(text);
        if let Some(place) = locate(&self.hunk, Side::New, &text, self.near) {
            self.near = place.line;
            return Presence::Applied(place);
        }
        match locate(&self.hunk, Side::Old, &text, self.near) {
            Some(place) => Presence::Absent(place),
            None => Presence::Moved,
        }
    }

    /// What puts the lines from before the change back, if the changed lines are in the text.
    pub fn revert(&mut self, text: &str) -> Option<Replacement> {
        let Presence::Applied(place) = self.presence(text) else {
            return None;
        };
        let sides = self.hunk.sides(Side::Old);
        let text = Text::new(text);
        Some(Replacement {
            text: join(&sides.core, &text, &place.range),
            range: place.range,
        })
    }

    /// What makes the changed lines appear, if the lines from before the change are in the text. A text that
    /// already has them needs nothing, and a text that has neither is a conflict, both reported by [`Self::presence`].
    pub fn apply(&mut self, text: &str) -> Option<Replacement> {
        let Presence::Absent(place) = self.presence(text) else {
            return None;
        };
        let sides = self.hunk.sides(Side::New);
        let text = Text::new(text);
        Some(Replacement {
            text: join(&sides.core, &text, &place.range),
            range: place.range,
        })
    }
}

/// What carrying one hunk of an agent's patch into a text came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Make this change. All of a text's replacements apply in one transaction, in order and without overlap.
    Replace(Replacement),
    /// The text already has the hunk.
    Already,
    /// The lines the hunk changed were edited since: the text keeps the author's version.
    Conflict,
}

/// Plans carrying `hunks` into `text` without touching anything else in it. Used until the Persistence slice
/// merges files on disk into buffers itself.
pub fn merge(hunks: &mut [Tracked], text: &str) -> Vec<Outcome> {
    let mut planned: Vec<Range<usize>> = Vec::new();
    hunks
        .iter_mut()
        .map(|tracked| match tracked.presence(text) {
            Presence::Applied(_) => Outcome::Already,
            Presence::Moved => Outcome::Conflict,
            Presence::Absent(_) => match tracked.apply(text) {
                Some(replacement)
                    if !planned
                        .iter()
                        .any(|other| replacement.range.start < other.end && other.start < replacement.range.end) =>
                {
                    planned.push(replacement.range.clone());
                    Outcome::Replace(replacement)
                }
                _ => Outcome::Conflict,
            },
        })
        .collect()
}

/// The changes one turn made to one file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileChanges {
    pub path: PathBuf,
    pub hunks: Vec<Tracked>,
}

/// Everything a turn changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnChanges {
    pub files: Vec<FileChanges>,
    /// Files a tool wrote that did not exist. They have no hunks to revert.
    pub created: Vec<PathBuf>,
}

impl TurnChanges {
    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.files.iter().all(|file| file.hunks.is_empty())
    }

    pub fn hunk_count(&self) -> usize {
        self.files.iter().map(|file| file.hunks.len()).sum()
    }

    /// Adds a patch to a file, which a second edit in the turn does again.
    pub fn add(&mut self, path: PathBuf, hunks: Vec<Hunk>) {
        let tracked = hunks.into_iter().map(Tracked::new);
        match self.files.iter_mut().find(|file| file.path == path) {
            Some(file) => file.hunks.extend(tracked),
            None => self.files.push(FileChanges {
                path,
                hunks: tracked.collect(),
            }),
        }
    }

    pub fn file_mut(&mut self, path: &std::path::Path) -> Option<&mut FileChanges> {
        self.files.iter_mut().find(|file| file.path == path)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOTES: &str = "# Notes\n\nHello world.\n\nA second line.\n";

    fn hello_patch() -> Tracked {
        let patch = json!([{"oldStart": 1, "oldLines": 5, "newStart": 1, "newLines": 5, "lines": [
            " # Notes", " ", "-Hello world.", "+Howdy world.", " ", " A second line."]}]);
        Tracked::new(Hunk::parse_all(&patch).remove(0))
    }

    fn applied(text: &str, replacement: &Replacement) -> String {
        let mut out = text.to_owned();
        out.replace_range(replacement.range.clone(), &replacement.text);
        out
    }

    #[test]
    fn a_patch_from_the_fixture_parses() {
        let capture = include_str!("../../tests/fixtures/agent-turns.jsonl");
        let line = capture
            .lines()
            .find(|line| line.contains("structuredPatch"))
            .expect("patch");
        let value: Value = serde_json::from_str(line).expect("json");
        let hunks = Hunk::parse_all(&value["tool_use_result"]["structuredPatch"]);
        assert_eq!(hunks.len(), 1);
        assert_eq!((hunks[0].added(), hunks[0].removed()), (1, 1));
        assert_eq!(hunks[0].lines[2].text, "Hello world.");
    }

    #[test]
    fn a_hunk_is_found_and_put_back() {
        let mut hunk = hello_patch();
        let after = "# Notes\n\nHowdy world.\n\nA second line.\n";
        let Presence::Applied(place) = hunk.presence(after) else {
            panic!("applied")
        };
        assert_eq!(&after[place.range.clone()], "Howdy world.\n");
        let revert = hunk.revert(after).expect("revert");
        assert_eq!(applied(after, &revert), NOTES);
    }

    #[test]
    fn a_hunk_follows_text_typed_above_it() {
        let mut hunk = hello_patch();
        let moved = "A new opening paragraph.\n\nAnd another one.\n\n# Notes\n\nHowdy world.\n\nA second line.\n";
        let revert = hunk.revert(moved).expect("still found");
        assert_eq!(
            applied(moved, &revert),
            "A new opening paragraph.\n\nAnd another one.\n\n# Notes\n\nHello world.\n\nA second line.\n"
        );
    }

    #[test]
    fn a_repeated_passage_resolves_to_the_nearest() {
        let mut hunk = hello_patch();
        let twice = "# Notes\n\nHowdy world.\n\nA second line.\n\n# Notes\n\nHowdy world.\n\nA second line.\n";
        let Presence::Applied(place) = hunk.presence(twice) else {
            panic!("applied")
        };
        assert_eq!(place.line, 2);
        // A hunk last seen at line 13 resolves to the copy at line 12, not the one at 18.
        let mut other = hello_patch();
        let shifted = format!("1\n\n2\n\n3\n\n4\n\n5\n\n{twice}");
        other.near = 13;
        let Presence::Applied(place) = other.presence(&shifted) else {
            panic!("applied")
        };
        assert_eq!(place.line, 12);
    }

    #[test]
    fn a_hunk_the_author_rewrote_offers_nothing() {
        let mut hunk = hello_patch();
        let rewritten = "# Notes\n\nHowdy there, world.\n\nA second line.\n";
        assert_eq!(hunk.presence(rewritten), Presence::Moved);
        assert_eq!(hunk.revert(rewritten), None);
    }

    #[test]
    fn edited_lines_around_a_hunk_do_not_stop_its_revert() {
        let mut hunk = hello_patch();
        let edited = "# My notes\n\nHowdy world.\n\nA second line, edited.\n";
        let revert = hunk.revert(edited).expect("the changed line is still there once");
        assert_eq!(
            applied(edited, &revert),
            "# My notes\n\nHello world.\n\nA second line, edited.\n"
        );
    }

    #[test]
    fn a_reverted_hunk_is_absent_and_can_come_back() {
        let mut hunk = hello_patch();
        assert!(matches!(hunk.presence(NOTES), Presence::Absent(_)));
        let apply = hunk.apply(NOTES).expect("apply");
        assert_eq!(applied(NOTES, &apply), "# Notes\n\nHowdy world.\n\nA second line.\n");
    }

    #[test]
    fn removed_lines_come_back_at_their_place() {
        let patch = json!([{"oldStart": 1, "oldLines": 4, "newStart": 1, "newLines": 2, "lines": [
            " one", "-two", "-three", " four"]}]);
        let mut hunk = Tracked::new(Hunk::parse_all(&patch).remove(0));
        let after = "one\nfour\n";
        let revert = hunk.revert(after).expect("revert");
        assert_eq!(applied(after, &revert), "one\ntwo\nthree\nfour\n");
    }

    #[test]
    fn added_lines_at_the_end_of_a_file_without_a_line_break_revert_cleanly() {
        let patch = json!([{"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 2, "lines": [
            " one", "+two"]}]);
        let mut hunk = Tracked::new(Hunk::parse_all(&patch).remove(0));
        let after = "one\ntwo";
        let revert = hunk.revert(after).expect("revert");
        assert_eq!(applied(after, &revert), "one\n");
    }

    #[test]
    fn merging_keeps_the_authors_other_edits() {
        let mut hunks = [hello_patch()];
        // The author typed in another paragraph, unsaved, while the agent edited the file on disk.
        let buffer = "# Notes\n\nHello world.\n\nA second line, written by the author.\n";
        let [Outcome::Replace(replacement)] = &merge(&mut hunks, buffer)[..] else {
            panic!("a replacement")
        };
        assert_eq!(
            applied(buffer, replacement),
            "# Notes\n\nHowdy world.\n\nA second line, written by the author.\n"
        );
    }

    #[test]
    fn merging_leaves_a_line_the_author_also_edited() {
        let mut hunks = [hello_patch()];
        let buffer = "# Notes\n\nHello, dear world.\n\nA second line.\n";
        assert_eq!(merge(&mut hunks, buffer), [Outcome::Conflict]);
    }

    #[test]
    fn merging_skips_a_text_that_already_has_the_change() {
        let mut hunks = [hello_patch()];
        assert_eq!(
            merge(&mut hunks, "# Notes\n\nHowdy world.\n\nA second line.\n"),
            [Outcome::Already]
        );
    }
}
