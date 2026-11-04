//! What a turn changed in files, as hunks that can be found again in text that has moved on.
//!
//! The CLI reports each edit as a `structuredPatch`: hunks with line numbers and the lines on both sides. Those line
//! numbers are only true for the file right after the edit. The author goes on typing, a reload replaces a buffer, a
//! second edit shifts lines. So a hunk is never held as a position. It is found again by its lines, nearest to where
//! it was last seen, and a hunk whose lines are gone is reported as such and offers nothing.
//!
//! Hunks come from what Persistence sees on disk, `base` to `theirs` of each [`FileChange`](crate::persist::FileChange),
//! so an edit through Bash has them too. Persistence takes the text into the buffer. This module answers where a hunk
//! is now and what the author's text would be without it. A file the session does not have keeps the hunks of the CLI's
//! own patch, which can only be shown.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use super::diff::{Mark, lines};
use crate::persist::merge::{Choice, line_changes, three_way};
use crate::session::ChapterId;

/// Unchanged lines kept around a change, to find it again by.
const CONTEXT: usize = 2;

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

/// One file's change on disk, for taking a hunk of it back.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OnDisk {
    base: Arc<str>,
    theirs: Arc<str>,
    /// The hunk's bytes in `base` and in `theirs`.
    old: Range<usize>,
    new: Range<usize>,
}

/// A hunk together with where it was last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub hunk: Hunk,
    /// The change this hunk is part of. Only a file the session has can be taken back from.
    on_disk: Option<OnDisk>,
    /// The line, counting from 0, to look nearest to. It follows the hunk as it is found.
    near: usize,
}

impl Tracked {
    pub fn new(hunk: Hunk) -> Self {
        let near = hunk.first_changed_line(Side::New);
        Self {
            hunk,
            on_disk: None,
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

    /// What the author's text `mine` becomes with this hunk taken back, keeping everything else in it. `None` when the
    /// hunk's lines are not as the agent left them, since a revert must not touch the author's words.
    pub fn revert(&mut self, mine: &str) -> Option<String> {
        if !matches!(self.presence(mine), Presence::Applied(_)) {
            return None;
        }
        let disk = self.on_disk.as_ref()?;
        let without = format!(
            "{}{}{}",
            &disk.theirs[..disk.new.start],
            &disk.base[disk.old.clone()],
            &disk.theirs[disk.new.end..]
        );
        let merged = three_way(&disk.theirs, mine, &without);
        merged.is_clean().then(|| merged.text(Choice::Mine))
    }
}

/// The hunks that turn `base` into `theirs`, by whole lines.
fn hunks_between(base: &Arc<str>, theirs: &Arc<str>) -> Vec<Tracked> {
    let (old_text, new_text) = (Text::new(base), Text::new(theirs));
    line_changes(base, theirs)
        .into_iter()
        .map(|(old, new)| {
            let before = old.start.saturating_sub(CONTEXT)..old.start;
            let after = old.end..(old.end + CONTEXT).min(old_text.lines.len());
            let line = |mark, text: &&str| PatchLine {
                mark,
                text: (*text).to_owned(),
            };
            let lines = (old_text.lines[before.clone()]
                .iter()
                .map(|text| line(Mark::Context, text)))
            .chain(old_text.lines[old.clone()].iter().map(|text| line(Mark::Removed, text)))
            .chain(new_text.lines[new.clone()].iter().map(|text| line(Mark::Added, text)))
            .chain(old_text.lines[after].iter().map(|text| line(Mark::Context, text)))
            .collect();
            let hunk = Hunk {
                old_start: before.start + 1,
                new_start: new.start - before.len() + 1,
                lines,
            };
            Tracked {
                near: new.start,
                hunk,
                on_disk: Some(OnDisk {
                    base: base.clone(),
                    theirs: theirs.clone(),
                    old: old_text.start_of(old.start)..old_text.start_of(old.end),
                    new: new_text.start_of(new.start)..new_text.start_of(new.end),
                }),
            }
        })
        .collect()
}

/// A change on disk from one text to another. Changes that follow each other in a turn count as one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    base: Arc<str>,
    theirs: Arc<str>,
}

/// The changes one turn made to one file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileChanges {
    pub path: PathBuf,
    /// The chapter holding the file, once its change was seen on disk. It survives a rename.
    pub chapter: Option<ChapterId>,
    steps: Vec<Step>,
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

    /// Adds the CLI's patch to a file the session does not have, which a second edit in the turn does again.
    pub fn add(&mut self, path: PathBuf, hunks: Vec<Hunk>) {
        let tracked = hunks.into_iter().map(Tracked::new);
        match self.files.iter_mut().find(|file| file.path == path) {
            Some(file) => file.hunks.extend(tracked),
            None => self.files.push(FileChanges {
                path,
                chapter: None,
                steps: Vec::new(),
                hunks: tracked.collect(),
            }),
        }
    }

    /// Forgets the CLI's patch of a file the session has: what is on disk tells more, also about other tools.
    pub fn forget_patch(&mut self, path: &std::path::Path) {
        self.files
            .retain(|file| file.path != path || file.chapter.is_some() || !file.steps.is_empty());
    }

    /// Takes in a change seen on disk. Returns whether it was news, as an unresolved one is told again.
    pub fn observe(&mut self, path: PathBuf, chapter: Option<ChapterId>, base: Arc<str>, theirs: Arc<str>) -> bool {
        let same = |file: &FileChanges| (chapter.is_some() && file.chapter == chapter) || file.path == path;
        let at = self.files.iter().position(same).unwrap_or_else(|| {
            self.files.push(FileChanges {
                path: path.clone(),
                chapter,
                steps: Vec::new(),
                hunks: Vec::new(),
            });
            self.files.len() - 1
        });
        let file = &mut self.files[at];
        file.path = path;
        file.chapter = chapter;
        match file.steps.last_mut() {
            Some(last) if last.base == base && last.theirs == theirs => return false,
            Some(last) if last.theirs == base => last.theirs = theirs,
            _ => file.steps.push(Step { base, theirs }),
        }
        file.steps.retain(|step| step.base != step.theirs);
        file.hunks = file
            .steps
            .iter()
            .flat_map(|step| hunks_between(&step.base, &step.theirs))
            .collect();
        true
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::persist::sync::{ChangeKind, FileSync};
    use crate::session::Session;
    use crate::source::{Edit, EditKind};

    const NOTES: &str = "# Notes\n\nHello world.\n\nA second line.\n";

    fn hello_patch() -> Tracked {
        let patch = json!([{"oldStart": 1, "oldLines": 5, "newStart": 1, "newLines": 5, "lines": [
            " # Notes", " ", "-Hello world.", "+Howdy world.", " ", " A second line."]}]);
        Tracked::new(Hunk::parse_all(&patch).remove(0))
    }

    /// The one hunk between two texts.
    fn only_hunk(base: &str, theirs: &str) -> Tracked {
        let mut hunks = hunks_between(&base.into(), &theirs.into());
        assert_eq!(hunks.len(), 1);
        hunks.remove(0)
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
    fn a_change_on_disk_gives_the_same_hunk_as_the_clis_patch() {
        let theirs = "# Notes\n\nHowdy world.\n\nA second line.\n";
        let tracked = only_hunk(NOTES, theirs);
        assert_eq!(tracked.hunk, hello_patch().hunk);
    }

    #[test]
    fn a_hunk_is_found_after_text_moved_it() {
        let mut hunk = only_hunk(NOTES, "# Notes\n\nHowdy world.\n\nA second line.\n");
        let moved = "A new opening.\n\n# Notes\n\nHowdy world.\n\nA second line.\n";
        let Presence::Applied(place) = hunk.presence(moved) else {
            panic!("applied")
        };
        assert_eq!(&moved[place.range.clone()], "Howdy world.\n");
        assert_eq!(
            hunk.revert(moved).as_deref(),
            Some("A new opening.\n\n# Notes\n\nHello world.\n\nA second line.\n")
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
        let mut hunk = only_hunk(NOTES, "# Notes\n\nHowdy world.\n\nA second line.\n");
        let rewritten = "# Notes\n\nHowdy there, world.\n\nA second line.\n";
        assert_eq!(hunk.presence(rewritten), Presence::Moved);
        assert_eq!(hunk.revert(rewritten), None);
    }

    #[test]
    fn edited_lines_around_a_hunk_do_not_stop_its_revert() {
        let mut hunk = only_hunk(NOTES, "# Notes\n\nHowdy world.\n\nA second line.\n");
        let edited = "# My notes\n\nHowdy world.\n\nA second line, edited.\n";
        assert_eq!(
            hunk.revert(edited).as_deref(),
            Some("# My notes\n\nHello world.\n\nA second line, edited.\n")
        );
    }

    #[test]
    fn a_reverted_hunk_reads_as_absent() {
        let mut hunk = only_hunk(NOTES, "# Notes\n\nHowdy world.\n\nA second line.\n");
        assert!(matches!(hunk.presence(NOTES), Presence::Absent(_)));
        assert_eq!(hunk.revert(NOTES), None);
    }

    #[test]
    fn removed_lines_come_back_at_their_place() {
        let mut hunk = only_hunk("one\ntwo\nthree\nfour\n", "one\nfour\n");
        assert_eq!(hunk.revert("one\nfour\n").as_deref(), Some("one\ntwo\nthree\nfour\n"));
    }

    #[test]
    fn added_lines_at_the_end_of_a_file_without_a_line_break_revert_exactly() {
        let mut hunk = only_hunk("one\n", "one\ntwo");
        assert_eq!(hunk.revert("one\ntwo").as_deref(), Some("one\n"));
    }

    #[test]
    fn crlf_lines_keep_their_line_ends_through_a_revert() {
        let base = "a\r\nb\r\nc\r\n";
        let mut hunk = only_hunk(base, "a\r\nB\r\nc\r\n");
        assert_eq!(hunk.revert("a\r\nB\r\nc\r\n").as_deref(), Some(base));
    }

    #[test]
    fn of_two_hunks_one_can_be_taken_back_and_the_other_stays() {
        let base = "one\n\ntwo\n\nthree\n\nfour\n\nfive\n\nsix\n";
        let theirs = "ONE\n\ntwo\n\nthree\n\nfour\n\nfive\n\nSIX\n";
        let mut hunks = hunks_between(&base.into(), &theirs.into());
        assert_eq!(hunks.len(), 2);
        let without_first = hunks[0].revert(theirs).expect("revert");
        assert_eq!(without_first, "one\n\ntwo\n\nthree\n\nfour\n\nfive\n\nSIX\n");
        assert_eq!(hunks[1].revert(&without_first).as_deref(), Some(base));
    }

    #[test]
    fn writes_that_follow_each_other_in_a_turn_count_as_one() {
        let mut turn = TurnChanges::default();
        let path = PathBuf::from("chapter.md");
        assert!(turn.observe(path.clone(), None, "a\n".into(), "b\n".into()));
        assert!(turn.observe(path.clone(), None, "b\n".into(), "c\n".into()));
        assert_eq!(turn.hunk_count(), 1);
        assert_eq!(turn.files[0].hunks[0].hunk.lines[0].text, "a");
    }

    #[test]
    fn the_same_change_told_twice_is_not_news() {
        let mut turn = TurnChanges::default();
        let path = PathBuf::from("chapter.md");
        assert!(turn.observe(path.clone(), None, "a\n".into(), "b\n".into()));
        assert!(!turn.observe(path, None, "a\n".into(), "b\n".into()));
    }

    #[test]
    fn a_write_back_to_where_the_turn_began_leaves_no_hunk() {
        let mut turn = TurnChanges::default();
        let path = PathBuf::from("chapter.md");
        turn.observe(path.clone(), None, "a\n".into(), "b\n".into());
        turn.observe(path, None, "b\n".into(), "a\n".into());
        assert_eq!(turn.hunk_count(), 0);
    }

    #[test]
    fn a_clis_patch_makes_way_for_what_the_session_sees() {
        let mut turn = TurnChanges::default();
        let path = PathBuf::from("/project/chapter.md");
        turn.add(path.clone(), vec![hello_patch().hunk]);
        turn.forget_patch(&path);
        assert!(turn.files.is_empty());
    }

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("druck-changes-{name}-{}", std::process::id()));
            fs::remove_dir_all(&dir).ok();
            fs::create_dir_all(dir.join("manuscript")).expect("folder");
            fs::write(dir.join("druck.yaml"), "chapters:\n  - manuscript/one.md\n").expect("manifest");
            fs::write(dir.join("manuscript/one.md"), CHAPTER).expect("chapter");
            Self(dir)
        }

        fn chapter(&self) -> PathBuf {
            self.0.join("manuscript/one.md")
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    const CHAPTER: &str = "# One\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";

    /// What a tool wrote to the file arrives in the session as Persistence takes it, and in the turn's changes.
    fn agent_writes(session: &mut Session, path: &Path, text: &str, turn: &mut TurnChanges) -> ChangeKind {
        fs::write(path, text).expect("write");
        let mut sync = FileSync::new(None);
        let change = sync
            .on_disk(session, path, Some(text.as_bytes().to_vec()))
            .expect("a change");
        turn.observe(
            change.path.clone(),
            change.chapter,
            change.base.clone(),
            change.theirs.clone().expect("the file is there"),
        );
        change.kind
    }

    #[test]
    fn a_write_through_any_tool_arrives_as_hunks_and_the_authors_unsaved_edit_stays() {
        let folder = Folder::new("both");
        let mut session = Session::open(&folder.0).expect("opens");
        let id = session.book()[0];
        // The author types at the top while the agent works on the file on disk.
        session
            .edit_chapter(id, vec![Edit::new(0..0, "Typed by the author. ")], EditKind::Typing)
            .expect("edit");
        let mut turn = TurnChanges::default();
        let theirs = CHAPTER.replace("Second paragraph.", "Second paragraph, rewritten.");
        let kind = agent_writes(&mut session, &folder.chapter(), &theirs, &mut turn);
        assert_eq!(kind, ChangeKind::Merged);
        let expected = format!("Typed by the author. {theirs}");
        assert_eq!(session.buffer(id).text(), expected);
        assert_eq!(turn.hunk_count(), 1);
        assert_eq!(turn.files[0].chapter, Some(id));
        assert!(matches!(
            turn.files[0].hunks[0].presence(session.buffer(id).text()),
            Presence::Applied(_)
        ));
    }

    #[test]
    fn a_hunk_on_lines_the_author_edited_is_left_alone() {
        let folder = Folder::new("overlap");
        let mut session = Session::open(&folder.0).expect("opens");
        let id = session.book()[0];
        let at = session.buffer(id).text().find("Second").expect("text");
        session
            .edit_chapter(id, vec![Edit::new(at..at + 6, "My own second")], EditKind::Typing)
            .expect("edit");
        let before = session.buffer(id).text().to_owned();
        let mut turn = TurnChanges::default();
        let theirs = CHAPTER.replace("Second paragraph.", "Another paragraph.");
        let kind = agent_writes(&mut session, &folder.chapter(), &theirs, &mut turn);
        assert_eq!(kind, ChangeKind::Conflict);
        assert_eq!(session.buffer(id).text(), before);
        // The hunk reads as changed since, so the panel offers no revert over the author's words.
        assert_eq!(turn.files[0].hunks[0].presence(&before), Presence::Moved);
        assert_eq!(turn.files[0].hunks[0].revert(&before), None);
    }

    #[test]
    fn a_revert_keeps_the_authors_text_is_one_undo_step_and_undo_brings_the_change_back() {
        let folder = Folder::new("revert");
        let mut session = Session::open(&folder.0).expect("opens");
        let id = session.book()[0];
        let mut turn = TurnChanges::default();
        let theirs = CHAPTER.replace("Second paragraph.", "Second paragraph, rewritten.");
        agent_writes(&mut session, &folder.chapter(), &theirs, &mut turn);
        session
            .edit_chapter(id, vec![Edit::new(0..0, "Typed later. ")], EditKind::Typing)
            .expect("edit");
        let agent_text = session.buffer(id).text().to_owned();
        let reverted = turn.files[0].hunks[0].revert(&agent_text).expect("revert");
        assert_eq!(reverted, format!("Typed later. {CHAPTER}"));
        let edits = crate::persist::merge::edits_between(&agent_text, &reverted);
        session
            .apply_to_file(&folder.chapter(), &edits, EditKind::Discrete)
            .expect("applies");
        assert_eq!(session.buffer(id).text(), reverted);
        session.active_buffer_mut().undo().expect("one step to undo");
        assert_eq!(session.buffer(id).text(), agent_text);
    }

    #[test]
    fn a_rename_keeps_a_chapters_hunks_together() {
        let folder = Folder::new("rename");
        let session = Session::open(&folder.0).expect("opens");
        let chapter = session.book().first().copied();
        let mut turn = TurnChanges::default();
        turn.observe(PathBuf::from("old.md"), chapter, "a\n".into(), "b\n".into());
        turn.observe(PathBuf::from("new.md"), chapter, "b\n".into(), "c\n".into());
        assert_eq!(turn.files.len(), 1);
        assert_eq!(turn.files[0].path, PathBuf::from("new.md"));
        assert_eq!(turn.hunk_count(), 1);
    }
}
