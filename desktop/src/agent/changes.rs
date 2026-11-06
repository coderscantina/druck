//! What a turn changed in files, as hunks that can be found again in text that has moved on.
//!
//! The CLI reports each edit as a `structuredPatch`: hunks with line numbers and the lines on both sides. Those line
//! numbers are only true for the file right after the edit. The author goes on typing, a reload replaces a buffer, a
//! second edit shifts lines. So a hunk is never held as a position. It is found again by its lines, nearest to where
//! it was last seen. The author may type around the words the agent changed and the hunk stays, with what they typed
//! kept apart from it. A hunk whose changed words are gone is reported as such and offers nothing.
//!
//! Hunks come from what Persistence sees on disk, `base` to `theirs` of each [`FileChange`](crate::persist::FileChange),
//! so an edit through Bash has them too. Persistence takes the text into the buffer. This module answers where a hunk
//! is now and what the author's text would be without it. A file the session does not have keeps the hunks of the CLI's
//! own patch, whose lines are all there is to take back with.
//!
//! The author reviews these hunks as passages. Accepting one moves it into what its change started from, so it is no
//! longer a change. Rejecting one takes it out of the text and leaves it tracked, so undo brings it back under review.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::diff::{Mark, lines};
use crate::editor::review::Passage;
use crate::persist::merge::{edits_between, line_changes, map_offset};
use crate::session::ChapterId;
use crate::source::{BlockKind, Blocks, Edit};

/// Unchanged lines kept around a change, to find it again by.
const CONTEXT: usize = 2;
/// How many lines from where a hunk was last seen its changed lines may stand, when they are found more than once.
const CLOSE_BY: usize = 8;
/// Changes in a line closer than this many words read as one.
const WORDS_APART: usize = 3;
/// What stands for a paragraph break in removed text, which shows inside a line.
const PARAGRAPH_MARK: &str = " \u{b6} ";

/// A hunk's name for as long as it exists. Hunks are worked out again when a new write arrives, and get new names.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

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

    /// One side's changed lines as text, for a hunk known only from the CLI's patch.
    fn core_lines(&self, side: Side) -> String {
        self.sides(side).core.iter().map(|line| format!("{line}\n")).collect()
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
    /// What the author typed in these lines since the agent wrote them, against the agent's lines. It never touches the
    /// words the agent changed.
    pub typed: Vec<Edit>,
}

fn locate(hunk: &Hunk, side: Side, text: &Text, near: usize) -> Option<Place> {
    let sides = hunk.sides(side);
    let whole: Vec<&str> = [&sides.before[..], &sides.core[..], &sides.after[..]].concat();
    if whole.is_empty() {
        return text.text.is_empty().then_some(Place {
            range: 0..0,
            line: 0,
            typed: Vec::new(),
        });
    }
    let before = sides.before.len();
    let mut found = text
        .find(&whole, near.saturating_sub(before))
        .first()
        .map(|at| at + before);
    if found.is_none() && !sides.core.is_empty() {
        // The lines on one side may be gone, as another passage's when it was rejected.
        let leading = [&sides.before[..], &sides.core[..]].concat();
        let trailing = [&sides.core[..], &sides.after[..]].concat();
        found = (!sides.before.is_empty())
            .then(|| {
                text.find(&leading, near.saturating_sub(before))
                    .first()
                    .map(|at| at + before)
            })
            .flatten()
            .or_else(|| {
                (!sides.after.is_empty())
                    .then(|| text.find(&trailing, near).first().copied())
                    .flatten()
            });
    }
    let first = match found {
        Some(first) => first,
        None => {
            // The lines around were edited. The changed lines alone count if they are found once, or stand close by, as
            // a scene break does when the passages around it were rejected.
            let alone = text.find(&sides.core, near);
            match alone[..] {
                [only] => only,
                [nearest, ..] if nearest.abs_diff(near) <= CLOSE_BY => nearest,
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
    Some(Place {
        range,
        line: first,
        typed: Vec::new(),
    })
}

/// Whether what the author typed reaches the words the agent changed. `changes` are the edits from the agent's lines back
/// to what stood there. Typing right beside a changed word leaves it alone, but a word that only went has no text of its
/// own, so typing where it stood counts.
fn touches(typed: &[Edit], changes: &[Edit]) -> bool {
    typed.iter().any(|typed| {
        changes.iter().any(|change| {
            if change.range.is_empty() {
                typed.range.start <= change.range.start && change.range.start <= typed.range.end
            } else {
                typed.range.start < change.range.end && change.range.start < typed.range.end
            }
        })
    })
}

/// Finds the hunk's changed lines near where they were, with words of the author's among them. Lines the author changed
/// more than half of are another text, whatever words they share with this one.
fn locate_typed(core: &str, old: &str, text: &Text, near: usize) -> Option<Place> {
    let count = core.split_inclusive('\n').count();
    if core.is_empty() || count > text.lines.len() {
        return None;
    }
    let changes = edits_between(core, old);
    let mut starts: Vec<usize> = (0..=text.lines.len() - count)
        .filter(|at| at.abs_diff(near) <= CLOSE_BY)
        .collect();
    starts.sort_by_key(|at| at.abs_diff(near));
    starts.into_iter().find_map(|first| {
        let range = text.start_of(first)..text.line_end(first + count - 1);
        let typed = edits_between(core, &text.text[range.clone()]);
        let replaced: usize = typed.iter().map(|edit| edit.range.len()).sum();
        (!typed.is_empty() && replaced * 2 <= core.len() && !touches(&typed, &changes)).then_some(Place {
            range,
            line: first,
            typed,
        })
    })
}

/// A range of the agent's lines in the text with `typed` in it. The range's start stays with its first word, past what
/// was typed right before it.
fn typed_range(range: &Range<usize>, typed: &[Edit]) -> Range<usize> {
    if range.is_empty() {
        let at = map_offset(range.start, typed);
        return at..at;
    }
    map_offset(range.start + 1, typed) - 1..map_offset(range.end, typed)
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
    /// The change's place among the file's steps.
    step: usize,
    base: Arc<str>,
    theirs: Arc<str>,
    /// The hunk's bytes in `base` and in `theirs`.
    old: Range<usize>,
    new: Range<usize>,
}

/// A hunk together with where it was last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub id: u64,
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
            id: next_id(),
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
        if let Some(place) = locate(&self.hunk, Side::Old, &text, self.near) {
            return Presence::Absent(place);
        }
        match locate_typed(&self.new_core(), &self.old_core(), &text, self.near) {
            Some(place) => {
                self.near = place.line;
                Presence::Applied(place)
            }
            None => Presence::Moved,
        }
    }

    /// What the author's text `mine` becomes with this hunk taken back, keeping everything else in it. `None` when the
    /// hunk's lines are not as the agent left them, since a reject must not touch the author's words.
    pub fn revert(&mut self, mine: &str) -> Option<String> {
        let Presence::Applied(place) = self.presence(mine) else {
            return None;
        };
        let mut old = self.old_core();
        if !place.typed.is_empty() {
            let mut lines = mine[place.range.clone()].to_owned();
            for change in edits_between(&self.new_core(), &old).iter().rev() {
                lines.replace_range(typed_range(&change.range, &place.typed), &change.text);
            }
            return Some(format!(
                "{}{lines}{}",
                &mine[..place.range.start],
                &mine[place.range.end..]
            ));
        }
        // A last line that had no line break, with text the author added after it.
        let now_inside = place.range.end < mine.len() && mine[place.range.clone()].ends_with('\n');
        if now_inside && !old.is_empty() && !old.ends_with('\n') {
            old.push('\n');
        }
        Some(format!(
            "{}{old}{}",
            &mine[..place.range.start],
            &mine[place.range.end..]
        ))
    }

    /// The hunk's text from before the change.
    pub fn old_core(&self) -> String {
        match &self.on_disk {
            Some(disk) => disk.base[disk.old.clone()].to_owned(),
            None => self.hunk.core_lines(Side::Old),
        }
    }

    /// The hunk's text from after the change.
    pub fn new_core(&self) -> String {
        match &self.on_disk {
            Some(disk) => disk.theirs[disk.new.clone()].to_owned(),
            None => self.hunk.core_lines(Side::New),
        }
    }

    /// What the galley shows for this hunk at `place` in `text`: the words that came in, the words that went, and where
    /// the buttons go. Offsets start at `shift`, for a file whose chapter starts further in.
    pub fn passage(&self, text: &str, place: &Place, shift: isize) -> Passage {
        let moved = |offset: usize| offset.saturating_add_signed(shift);
        let new = &text[place.range.clone()];
        let agents = if place.typed.is_empty() {
            new.to_owned()
        } else {
            self.new_core()
        };
        let old = self.old_core();
        let mut added = Vec::new();
        let mut removed = Vec::new();
        for (range, gone) in word_changes(&agents, &old) {
            let range = typed_range(&range, &place.typed);
            let range = place.range.start + range.start..place.range.start + range.end;
            let gone = shown_inline(&gone);
            if !gone.is_empty() {
                let at = readable_place(text, range.start);
                removed.push((moved(at), with_own_space(text, at, &gone)));
            }
            let kept = text[range.clone()].trim_end().len();
            if kept > 0 {
                added.push(moved(range.start)..moved(range.start + kept));
            }
        }
        let trimmed = new.trim_end_matches(['\n', '\r']);
        let last_line = trimmed.rfind('\n').map_or(0, |at| at + 1);
        let end = if trimmed.is_empty() {
            readable_place(text, place.range.start)
        } else {
            place.range.start + last_line
        };
        Passage {
            id: self.id,
            added,
            removed,
            end: moved(end),
        }
    }
}

/// The word changes that turn `new` into `old`, as the range in `new` and what stood there. Changes a few words apart
/// join, or a reworded sentence reads as confetti.
fn word_changes(new: &str, old: &str) -> Vec<(Range<usize>, String)> {
    let mut joined: Vec<(Range<usize>, String)> = Vec::new();
    for edit in edits_between(new, old) {
        match joined.last_mut() {
            Some((range, text)) if new[range.end..edit.range.start].split_whitespace().count() < WORDS_APART => {
                text.push_str(&new[range.end..edit.range.start]);
                text.push_str(&edit.text);
                range.end = edit.range.end;
            }
            _ => joined.push((edit.range.clone(), edit.text.clone())),
        }
    }
    joined
}

/// Removed text as it can show inside one line.
fn shown_inline(text: &str) -> String {
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|paragraph| !paragraph.is_empty())
        .collect();
    paragraphs.join(PARAGRAPH_MARK)
}

/// A place where something can show near `at`: a blank line has no text to stand in, so it moves on to the next line
/// with text, or back to the end of the last one.
/// Removed words standing at `at` in `text`, with a space of their own where the text has none: before them after a
/// word, and after them before a word, but none before punctuation that closes.
fn with_own_space(text: &str, at: usize, gone: &str) -> String {
    let before = text[..at]
        .chars()
        .next_back()
        .is_some_and(|character| !character.is_whitespace());
    let after = text[at..].chars().next().is_some_and(|character| {
        !character.is_whitespace() && !matches!(character, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '…')
    });
    let lead = if before { " " } else { "" };
    let trail = if after { " " } else { "" };
    format!("{lead}{gone}{trail}")
}

fn readable_place(text: &str, at: usize) -> usize {
    let line_start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |found| at + found);
    if !text[line_start..line_end].trim().is_empty() {
        return at;
    }
    let rest = &text[at..];
    match rest.find(|character: char| !character.is_whitespace()) {
        Some(next) => at + next,
        None => text[..at].trim_end().len(),
    }
}

/// New lines cut into passages: each block with the blank lines after it, so a whole chapter is reviewed one passage after
/// another. A comment shows nothing to review by, so it stays with the block after it.
fn passages_of(text: &Text, range: Range<usize>) -> Vec<Range<usize>> {
    let blocks = Blocks::parse(text.text);
    let kinds = blocks.as_slice();
    let mut passages = Vec::new();
    let mut start = range.start;
    for at in range.clone() {
        let here = blocks.index_at(text.start_of(at));
        let separates = kinds[here].kind == BlockKind::Blank
            && here
                .checked_sub(1)
                .is_none_or(|before| kinds[before].kind != BlockKind::Comment);
        let more = text.lines[..range.end]
            .get(at + 1)
            .is_some_and(|next| !next.trim().is_empty());
        if separates && more {
            passages.push(start..at + 1);
            start = at + 1;
        }
    }
    if start < range.end {
        passages.push(start..range.end);
    }
    passages
}

/// The hunks that turn `base` into `theirs`, by whole lines. `step` is their change's place among the file's.
fn hunks_between(step: usize, base: &Arc<str>, theirs: &Arc<str>) -> Vec<Tracked> {
    let (old_text, new_text) = (Text::new(base), Text::new(theirs));
    let line = |mark, text: &&str| PatchLine {
        mark,
        text: (*text).to_owned(),
    };
    let on_disk = |old: Range<usize>, new: Range<usize>| OnDisk {
        step,
        base: base.clone(),
        theirs: theirs.clone(),
        old: old_text.start_of(old.start)..old_text.start_of(old.end),
        new: new_text.start_of(new.start)..new_text.start_of(new.end),
    };
    let mut hunks = Vec::new();
    for (old, new) in line_changes(base, theirs) {
        if old.is_empty() {
            // Text that only came in: its context is what stands around it now, the other passages included.
            for piece in passages_of(&new_text, new.clone()) {
                let before = piece.start.saturating_sub(CONTEXT)..piece.start;
                let after = piece.end..(piece.end + CONTEXT).min(new_text.lines.len());
                let lines = (new_text.lines[before.clone()]
                    .iter()
                    .map(|text| line(Mark::Context, text)))
                .chain(new_text.lines[piece.clone()].iter().map(|text| line(Mark::Added, text)))
                .chain(new_text.lines[after].iter().map(|text| line(Mark::Context, text)))
                .collect();
                hunks.push(Tracked {
                    id: next_id(),
                    near: piece.start,
                    hunk: Hunk {
                        old_start: old.start + 1,
                        new_start: before.start + 1,
                        lines,
                    },
                    on_disk: Some(on_disk(old.clone(), piece)),
                });
            }
            continue;
        }
        let before = old.start.saturating_sub(CONTEXT)..old.start;
        let after = old.end..(old.end + CONTEXT).min(old_text.lines.len());
        let lines = (old_text.lines[before.clone()]
            .iter()
            .map(|text| line(Mark::Context, text)))
        .chain(old_text.lines[old.clone()].iter().map(|text| line(Mark::Removed, text)))
        .chain(new_text.lines[new.clone()].iter().map(|text| line(Mark::Added, text)))
        .chain(old_text.lines[after].iter().map(|text| line(Mark::Context, text)))
        .collect();
        hunks.push(Tracked {
            id: next_id(),
            near: new.start,
            hunk: Hunk {
                old_start: before.start + 1,
                new_start: new.start - before.len() + 1,
                lines,
            },
            on_disk: Some(on_disk(old, new)),
        });
    }
    hunks
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
    /// The file did not exist before the turn, so taking back all of it removes it.
    pub created: bool,
    steps: Vec<Step>,
    pub hunks: Vec<Tracked>,
}

impl FileChanges {
    fn rebuild(&mut self) {
        self.steps.retain(|step| step.base != step.theirs);
        self.hunks = (self.steps.iter().enumerate())
            .flat_map(|(at, step)| hunks_between(at, &step.base, &step.theirs))
            .collect();
    }

    pub fn holds(&self, id: u64) -> bool {
        self.hunks.iter().any(|tracked| tracked.id == id)
    }

    /// Makes a hunk part of what its change started from. Returns whether it was here.
    pub fn accept(&mut self, id: u64) -> bool {
        let Some(at) = self.hunks.iter().position(|tracked| tracked.id == id) else {
            return false;
        };
        let Some(disk) = self.hunks[at].on_disk.clone() else {
            self.hunks.remove(at);
            return true;
        };
        let step = &mut self.steps[disk.step];
        step.base = format!(
            "{}{}{}",
            &step.base[..disk.old.start],
            &step.theirs[disk.new.clone()],
            &step.base[disk.old.end..]
        )
        .into();
        self.rebuild();
        true
    }

    /// Accepts everything of this file: all that is left of its changes is in the text.
    pub fn accept_all(&mut self) {
        self.steps.clear();
        self.hunks.clear();
    }

    /// What `mine` becomes with everything still under review taken back, newest first. A step the author did not touch
    /// goes back whole, so the bytes from before it return exactly.
    pub fn reject_all(&mut self, mine: &str) -> String {
        let mut text = mine.to_owned();
        for (step, change) in self.steps.iter().enumerate().rev() {
            if text == *change.theirs {
                text = change.base.to_string();
                continue;
            }
            for tracked in self.hunks.iter_mut().rev() {
                if tracked.on_disk.as_ref().is_some_and(|disk| disk.step == step)
                    && let Some(reverted) = tracked.revert(&text)
                {
                    text = reverted;
                }
            }
        }
        if self.steps.is_empty() {
            for tracked in self.hunks.iter_mut().rev() {
                if let Some(reverted) = tracked.revert(&text) {
                    text = reverted;
                }
            }
        }
        text
    }
}

/// Everything a turn changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnChanges {
    pub files: Vec<FileChanges>,
    /// Files a tool wrote that did not exist.
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
            // What was seen on disk tells more.
            Some(file) if !file.steps.is_empty() => {}
            Some(file) => file.hunks.extend(tracked),
            None => self.files.push(FileChanges {
                path,
                chapter: None,
                created: false,
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
                created: false,
                steps: Vec::new(),
                hunks: Vec::new(),
            });
            self.files.len() - 1
        });
        let file = &mut self.files[at];
        file.path = path;
        file.chapter = chapter;
        match file.steps.last_mut() {
            Some(last) if last.theirs == theirs => return false,
            Some(last) if last.theirs == base => last.theirs = theirs,
            _ => file.steps.push(Step { base, theirs }),
        }
        file.rebuild();
        true
    }

    /// Takes in a file that did not exist before the turn: all of its text is the change.
    pub fn observe_created(&mut self, path: PathBuf, chapter: Option<ChapterId>, text: Arc<str>) {
        self.created.retain(|created| *created != path);
        self.observe(path.clone(), chapter, "".into(), text);
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            file.created = true;
        }
    }
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init, reason = "a list of byte ranges that holds one")]
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
        let mut hunks = hunks_between(0, &base.into(), &theirs.into());
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
        let rewritten = "# Notes\n\nGood day, world.\n\nA second line.\n";
        assert_eq!(hunk.presence(rewritten), Presence::Moved);
        assert_eq!(hunk.revert(rewritten), None);
    }

    const LONG: &str = "One two three four five six seven eight nine ten.\n";

    #[test]
    fn typing_elsewhere_in_a_changed_line_keeps_the_passage_in_review() {
        let theirs = "One two three FOUR five six seven eight nine ten.\n";
        let mut hunk = only_hunk(LONG, theirs);
        let mine = "One two three FOUR five six seven eight, typed here, nine ten.\n";
        let Presence::Applied(place) = hunk.presence(mine) else {
            panic!("still in review")
        };
        let passage = hunk.passage(mine, &place, 0);
        let word = mine.find("FOUR").expect("in the text");
        assert_eq!(passage.added, [word..word + 4], "the author's words are not tinted");
        assert_eq!(passage.removed, [(word, "four ".to_owned())]);
        assert_eq!(
            hunk.revert(mine).as_deref(),
            Some("One two three four five six seven eight, typed here, nine ten.\n"),
            "a reject keeps what was typed"
        );
    }

    #[test]
    fn typing_in_front_of_a_changed_line_keeps_the_tint_on_the_agents_word() {
        let mut hunk = only_hunk(LONG, "One two three FOUR five six seven eight nine ten.\n");
        let mine = "Typed. One two three FOUR five six seven eight nine ten.\n";
        let Presence::Applied(place) = hunk.presence(mine) else {
            panic!("still in review")
        };
        let word = mine.find("FOUR").expect("in the text");
        assert_eq!(hunk.passage(mine, &place, 0).added, [word..word + 4]);
    }

    #[test]
    fn typing_over_the_changed_words_ends_the_review() {
        let mut hunk = only_hunk(LONG, "One two three FOUR five six seven eight nine ten.\n");
        let inside = "One two three FOURTH five six seven eight nine ten.\n";
        assert_eq!(hunk.presence(inside), Presence::Moved);
        assert_eq!(hunk.revert(inside), None);
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
        let mut hunks = hunks_between(0, &base.into(), &theirs.into());
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

    /// The author adds a sentence to a line while the agent rewrites the sentence before it: both stay, and the agent's
    /// words are still a passage to review.
    #[test]
    fn an_agent_change_beside_the_authors_sentence_merges_and_stays_in_review() {
        let folder = Folder::new("sentence");
        let mut session = Session::open(&folder.0).expect("opens");
        let id = session.book()[0];
        let at = session.buffer(id).text().find("Second paragraph.").expect("text") + "Second paragraph.".len();
        session
            .edit_chapter(id, vec![Edit::new(at..at, " Typed by the author.")], EditKind::Typing)
            .expect("edit");
        let mut turn = TurnChanges::default();
        let theirs = CHAPTER.replace("Second paragraph.", "Second paragraph, rewritten.");
        let kind = agent_writes(&mut session, &folder.chapter(), &theirs, &mut turn);
        assert_eq!(kind, ChangeKind::Merged);
        let expected = CHAPTER.replace("Second paragraph.", "Second paragraph, rewritten. Typed by the author.");
        assert_eq!(session.buffer(id).text(), expected);
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

    #[test]
    fn a_passage_shows_new_words_tinted_and_old_words_struck() {
        let theirs = "# Notes\n\nHowdy world.\n\nA second line.\n";
        let mut hunk = only_hunk(NOTES, theirs);
        let Presence::Applied(place) = hunk.presence(theirs) else {
            panic!("applied")
        };
        let passage = hunk.passage(theirs, &place, 0);
        let at = theirs.find("Howdy").expect("there");
        assert_eq!(passage.added, [at..at + "Howdy".len()]);
        assert_eq!(passage.removed, [(at, "Hello ".to_owned())]);
        assert_eq!(passage.end, at);
    }

    #[test]
    fn removed_words_take_their_own_space_where_the_text_has_none() {
        let text = "Licht, sonst alles dunkel. Neu";
        let comma = text.find(',').expect("there");
        assert_eq!(with_own_space(text, comma, "fällt"), " fällt");
        let word = text.find("sonst").expect("there");
        assert_eq!(with_own_space(text, word, "fällt"), "fällt ");
        assert_eq!(with_own_space(text, text.len(), "Alt"), " Alt");
    }

    #[test]
    fn changes_a_few_words_apart_read_as_one() {
        let changes = word_changes(
            "the lamps in silver lilies caught it",
            "the soft radiance in the lilies of silver caught it",
        );
        assert_eq!(changes.len(), 1, "{changes:?}");
    }

    #[test]
    fn a_removed_passage_stands_at_text_and_keeps_paragraphs_apart() {
        let base = "One.\n\nTwo.\n\nThree.\n";
        let theirs = "One.\n\nThree.\n";
        let mut hunk = only_hunk(base, theirs);
        let Presence::Applied(place) = hunk.presence(theirs) else {
            panic!("applied")
        };
        let passage = hunk.passage(theirs, &place, 0);
        assert!(passage.added.is_empty());
        let (at, text) = &passage.removed[0];
        assert!(
            theirs[*at..].starts_with("Three") || theirs[..*at].ends_with("One."),
            "at {at}"
        );
        assert_eq!(text, "Two. ");
        assert_eq!(shown_inline("One\nmore.\n\nTwo.\n"), "One more. \u{b6} Two.");
    }

    #[test]
    fn a_new_chapter_is_one_passage_after_another_and_goes_back_to_nothing() {
        let text = "# New\n\nFirst paragraph.\n\n***\n\nSecond paragraph.\n\n***\n\nThird.\n";
        let mut changes = TurnChanges::default();
        changes.observe_created("/book/manuscript/new.md".into(), None, text.into());
        let file = &mut changes.files[0];
        assert!(file.created);
        assert_eq!(file.hunks.len(), 6);
        assert_eq!(file.reject_all(text), "");
    }

    #[test]
    fn a_comment_stays_with_the_passage_after_it() {
        let text = "<!--\nA note\n\nwith a gap.\n-->\n\n# New\n\nFirst.\n\nSecond.\n";
        let mut changes = TurnChanges::default();
        changes.observe_created("/new.md".into(), None, text.into());
        let cores: Vec<String> = changes.files[0].hunks.iter().map(Tracked::new_core).collect();
        assert_eq!(
            cores,
            [
                "<!--\nA note\n\nwith a gap.\n-->\n\n# New\n\n",
                "First.\n\n",
                "Second.\n"
            ]
        );
    }

    #[test]
    fn passages_of_a_new_chapter_reject_one_by_one_down_to_nothing() {
        let text = "# New\n\nFirst.\n\n***\n\nSecond.\n\n***\n\nThird.\n";
        let mut changes = TurnChanges::default();
        changes.observe_created("/book/manuscript/new.md".into(), None, text.into());
        let mut mine = text.to_owned();
        let file = &mut changes.files[0];
        assert_eq!(file.hunks.len(), 6);
        for order in [2, 0, 4, 1, 3, 5] {
            mine = file.hunks[order].revert(&mine).expect("rejects");
        }
        assert_eq!(mine, "");
    }

    #[test]
    fn rejecting_everything_returns_the_exact_bytes() {
        // No final line break, and a line ending the diff must not normalize.
        let base = "One.\r\n\r\nTwo.\n\nThree";
        let theirs = "One changed.\r\n\r\nTwo.\n\nNew passage.\n\nThree, again";
        let mut changes = TurnChanges::default();
        changes.observe("/a.md".into(), None, base.into(), theirs.into());
        assert_eq!(changes.files[0].reject_all(theirs), base);
    }

    #[test]
    fn rejecting_everything_keeps_what_the_author_typed() {
        let base = "One.\n\nTwo.\n\nThree.\n";
        let theirs = "One changed.\n\nTwo.\n\nThree changed.\n";
        let mine = "One changed.\n\nTwo, typed by the author.\n\nThree changed.\n";
        let mut changes = TurnChanges::default();
        changes.observe("/a.md".into(), None, base.into(), theirs.into());
        assert_eq!(
            changes.files[0].reject_all(mine),
            "One.\n\nTwo, typed by the author.\n\nThree.\n"
        );
    }

    #[test]
    fn an_accepted_passage_leaves_review_and_a_reject_all_leaves_it_in_the_text() {
        let base = "One.\n\nTwo.\n\nThree.\n";
        let theirs = "One changed.\n\nTwo.\n\nThree changed.\n";
        let mut changes = TurnChanges::default();
        changes.observe("/a.md".into(), None, base.into(), theirs.into());
        let file = &mut changes.files[0];
        assert_eq!(file.hunks.len(), 2);
        let first = file.hunks[0].id;
        assert!(file.accept(first));
        assert_eq!(file.hunks.len(), 1);
        assert_eq!(file.reject_all(theirs), "One changed.\n\nTwo.\n\nThree.\n");
    }

    #[test]
    fn accepting_everything_empties_review_and_a_later_write_starts_anew() {
        let mut changes = TurnChanges::default();
        changes.observe("/a.md".into(), None, "One.\n".into(), "Two.\n".into());
        changes.files[0].accept_all();
        assert!(changes.files[0].hunks.is_empty());
        assert!(changes.observe("/a.md".into(), None, "Two.\n".into(), "Three.\n".into()));
        assert_eq!(changes.files[0].reject_all("Three.\n"), "Two.\n");
    }
}
