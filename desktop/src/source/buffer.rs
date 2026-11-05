//! The text buffer: transactional edits, grouped undo and redo, and the offset maps the platform text input needs.
//!
//! Offsets are UTF-8 byte offsets into the text. The UTF-16 and line maps scan the text instead of keeping an index,
//! because counting bytes is fast enough at any document size this app opens and leaves nothing to invalidate.

use std::fmt;
use std::ops::Range;
use std::string::FromUtf8Error;
use std::time::{Duration, Instant};

/// A pause longer than this starts a new undo group.
const GROUP_PAUSE: Duration = Duration::from_secs(1);

/// One replacement. In a transaction all ranges refer to the text before the transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
}

impl Edit {
    pub fn new(range: Range<usize>, text: impl Into<String>) -> Self {
        Self {
            range,
            text: text.into(),
        }
    }
}

/// How an edit groups with the one before it for undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Deleting,
    /// Marked text of an input method. A composition is never split, however long it pauses.
    Composing,
    /// Always its own undo step: paste, cut, a new paragraph.
    Discrete,
    /// A change that came from outside the app, as its own undo step. See [`Buffer::undo`].
    External,
}

impl EditKind {
    fn joins(self, previous: Self, pause: Duration) -> bool {
        use EditKind::{Composing, Deleting, Discrete, External, Typing};
        match (previous, self) {
            (Composing, Composing) => true,
            (Typing | Composing, Typing | Composing) | (Deleting, Deleting) => pause < GROUP_PAUSE,
            (Discrete | External, _)
            | (_, Discrete | External)
            | (Typing | Composing, Deleting)
            | (Deleting, Typing | Composing) => false,
        }
    }
}

/// A selection: `anchor` stays when it extends, `head` is where the caret shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }

    pub fn reversed(self) -> bool {
        self.head < self.anchor
    }
}

/// The span of text one change touched: `start..old_end` before it, `start..new_end` after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Splice {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
}

/// A transaction rejected before any of it applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    OutOfBounds(Range<usize>),
    NotOnCharBoundary(Range<usize>),
    NotSorted(Range<usize>),
    Empty,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds(range) => write!(f, "edit {range:?} is outside the text"),
            Self::NotOnCharBoundary(range) => write!(f, "edit {range:?} splits a character"),
            Self::NotSorted(range) => write!(f, "edit {range:?} overlaps or precedes the edit before it"),
            Self::Empty => write!(f, "a transaction needs at least one edit"),
        }
    }
}

/// The result of undo or redo: what changed, and the selection to restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restored {
    pub splice: Splice,
    pub selection: Selection,
}

struct Step {
    forward: Vec<Edit>,
    backward: Vec<Edit>,
}

struct Group {
    steps: Vec<Step>,
    kind: EditKind,
    last_edit: Instant,
    before: Selection,
    after: Selection,
}

pub struct Buffer {
    text: String,
    undo: Vec<Group>,
    redo: Vec<Group>,
    revision: u64,
    /// What the change to this revision touched.
    last_splice: Option<Splice>,
    /// The last undo stopped at a change from outside, so the next one takes it back.
    undo_outside_armed: bool,
}

impl Buffer {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            last_splice: None,
            undo_outside_armed: false,
        }
    }

    /// Keeps the bytes exactly as they are. Text that is not UTF-8 cannot be edited and is an error.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, FromUtf8Error> {
        String::from_utf8(bytes).map(Self::new)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Changes with every edit, undo, and redo. Consumers compare it to see whether their copy is stale.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// What the change to the current revision touched, for a consumer one revision behind.
    pub fn last_splice(&self) -> Option<Splice> {
        self.last_splice
    }

    /// Applies `edits` as one atomic change, in increasing non-overlapping order, or nothing at all. `before` and
    /// `after` are the selections undo and redo restore.
    pub fn transact(
        &mut self,
        kind: EditKind,
        edits: Vec<Edit>,
        before: Selection,
        after: Selection,
        now: Instant,
    ) -> Result<Splice, EditError> {
        self.validate(&edits)?;
        let (splice, backward) = self.apply(&edits);
        let step = Step {
            forward: edits,
            backward,
        };
        self.redo.clear();
        self.revision += 1;
        self.last_splice = Some(splice);
        self.undo_outside_armed = false;
        match self.undo.last_mut() {
            Some(group) if group.after == before && kind.joins(group.kind, now - group.last_edit) => {
                group.steps.push(step);
                group.kind = kind;
                group.last_edit = now;
                group.after = after;
            }
            _ => self.undo.push(Group {
                steps: vec![step],
                kind,
                last_edit: now,
                before,
                after,
            }),
        }
        Ok(splice)
    }

    /// Adds edits to the last undo step, whatever its kind, so one undo takes both back. With no step to add to, they
    /// start one.
    pub fn amend(&mut self, edits: Vec<Edit>, after: Selection, now: Instant) -> Result<Splice, EditError> {
        self.validate(&edits)?;
        let (splice, backward) = self.apply(&edits);
        let step = Step {
            forward: edits,
            backward,
        };
        self.redo.clear();
        self.revision += 1;
        self.last_splice = Some(splice);
        self.undo_outside_armed = false;
        match self.undo.last_mut() {
            Some(group) => {
                group.steps.push(step);
                group.last_edit = now;
                group.after = after;
            }
            None => self.undo.push(Group {
                steps: vec![step],
                kind: EditKind::Discrete,
                last_edit: now,
                before: after,
                after,
            }),
        }
        Ok(splice)
    }

    /// Whether the next undo meets a change from outside.
    pub fn undo_is_outside_change(&self) -> bool {
        self.undo.last().is_some_and(|group| group.kind == EditKind::External)
    }

    /// Takes back the last undo step. A change from outside is one step too, but taking it back takes someone else's
    /// text off the disk at the next save. So the first undo that reaches one stops there and does nothing, and only
    /// the next one takes it back.
    pub fn undo(&mut self) -> Option<Restored> {
        if self.undo_is_outside_change() && !self.undo_outside_armed {
            self.undo_outside_armed = true;
            return None;
        }
        self.undo_outside_armed = false;
        let group = self.undo.pop()?;
        let splice = self.replay(group.steps.iter().rev().map(|step| &step.backward));
        let selection = group.before;
        self.redo.push(group);
        Some(Restored { splice, selection })
    }

    pub fn redo(&mut self) -> Option<Restored> {
        self.undo_outside_armed = false;
        let group = self.redo.pop()?;
        let splice = self.replay(group.steps.iter().map(|step| &step.forward));
        let selection = group.after;
        self.undo.push(group);
        Some(Restored { splice, selection })
    }

    fn replay<'a>(&mut self, transactions: impl Iterator<Item = &'a Vec<Edit>>) -> Splice {
        let mut total: Option<Splice> = None;
        for edits in transactions {
            let (splice, _) = self.apply(edits);
            total = Some(match total {
                Some(total) => merge(total, splice),
                None => splice,
            });
        }
        self.revision += 1;
        self.last_splice = total;
        total.unwrap_or(Splice {
            start: 0,
            old_end: 0,
            new_end: 0,
        })
    }

    fn validate(&self, edits: &[Edit]) -> Result<(), EditError> {
        if edits.is_empty() {
            return Err(EditError::Empty);
        }
        let mut last_end = 0;
        for Edit { range, .. } in edits {
            if range.start > range.end || range.end > self.text.len() {
                return Err(EditError::OutOfBounds(range.clone()));
            }
            if !self.text.is_char_boundary(range.start) || !self.text.is_char_boundary(range.end) {
                return Err(EditError::NotOnCharBoundary(range.clone()));
            }
            if range.start < last_end {
                return Err(EditError::NotSorted(range.clone()));
            }
            last_end = range.end;
        }
        Ok(())
    }

    /// Applies validated edits. Returns the covering splice and the edits that reverse them.
    fn apply(&mut self, edits: &[Edit]) -> (Splice, Vec<Edit>) {
        let mut backward = Vec::with_capacity(edits.len());
        let mut delta = 0isize;
        for edit in edits {
            let start = edit.range.start.wrapping_add_signed(delta);
            backward.push(Edit {
                range: start..start + edit.text.len(),
                text: self.text[edit.range.clone()].to_owned(),
            });
            delta += edit.text.len() as isize - edit.range.len() as isize;
        }
        for edit in edits.iter().rev() {
            self.text.replace_range(edit.range.clone(), &edit.text);
        }
        let first = &edits[0].range;
        let old_end = edits[edits.len() - 1].range.end;
        let splice = Splice {
            start: first.start,
            old_end,
            new_end: old_end.wrapping_add_signed(delta),
        };
        (splice, backward)
    }

    pub fn utf16_len(&self) -> usize {
        utf16_units(self.text.as_bytes())
    }

    /// The UTF-16 offset of a byte offset on a character boundary.
    pub fn utf16_of(&self, offset: usize) -> usize {
        utf16_units(&self.text.as_bytes()[..offset])
    }

    /// The byte offset of a UTF-16 offset. A position inside a surrogate pair moves to the end of its character, and
    /// one past the end clamps to the end.
    pub fn offset_of_utf16(&self, offset: usize) -> usize {
        const CHUNK: usize = 4096;
        let bytes = self.text.as_bytes();
        let mut units = 0;
        let mut at = 0;
        while at + CHUNK <= bytes.len() {
            let chunk_units = utf16_units(&bytes[at..at + CHUNK]);
            if units + chunk_units >= offset {
                break;
            }
            units += chunk_units;
            at += CHUNK;
        }
        while at < bytes.len() {
            if units >= offset && self.text.is_char_boundary(at) {
                return at;
            }
            units += byte_units(bytes[at]);
            at += 1;
        }
        bytes.len()
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_of(range.start)..self.utf16_of(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_of_utf16(range.start)..self.offset_of_utf16(range.end)
    }

    /// The 1-based source line of a byte offset, the numbering engine diagnostics use for a whole file.
    pub fn line_of(&self, offset: usize) -> usize {
        self.text.as_bytes()[..offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1
    }

    /// The byte offset where a 1-based source line starts, or the end of the text past the last line.
    pub fn offset_of_line(&self, line: usize) -> usize {
        if line <= 1 {
            return 0;
        }
        self.text
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .nth(line - 2)
            .map_or(self.text.len(), |(at, _)| at + 1)
    }
}

/// The hull of two successive splices, in terms of the text before the first and after the second.
fn merge(first: Splice, second: Splice) -> Splice {
    let first_delta = first.new_end as isize - first.old_end as isize;
    let second_delta = second.new_end as isize - second.old_end as isize;
    Splice {
        start: first.start.min(second.start),
        old_end: first
            .old_end
            .max((second.old_end as isize - first_delta).max(0) as usize),
        new_end: second
            .new_end
            .max((first.new_end as isize + second_delta).max(0) as usize),
    }
}

/// UTF-16 code units a byte adds: one per character start, one more for a 4-byte character.
fn byte_units(byte: u8) -> usize {
    usize::from(byte & 0xC0 != 0x80) + usize::from(byte >= 0xF0)
}

fn utf16_units(bytes: &[u8]) -> usize {
    bytes.iter().map(|byte| byte_units(*byte)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> (Buffer, Instant) {
        (Buffer::new(text), Instant::now())
    }

    fn insert(buffer: &mut Buffer, at: usize, text: &str, kind: EditKind, now: Instant) -> Splice {
        let after = Selection::caret(at + text.len());
        buffer
            .transact(kind, vec![Edit::new(at..at, text)], Selection::caret(at), after, now)
            .unwrap()
    }

    #[test]
    fn a_transaction_applies_all_edits_against_the_old_text() {
        let (mut buffer, now) = buffer("abc def ghi");
        let edits = vec![Edit::new(0..3, "X"), Edit::new(8..11, "YYYY")];
        let splice = buffer
            .transact(
                EditKind::Discrete,
                edits,
                Selection::default(),
                Selection::default(),
                now,
            )
            .unwrap();
        assert_eq!(buffer.text(), "X def YYYY");
        assert_eq!(
            splice,
            Splice {
                start: 0,
                old_end: 11,
                new_end: 10
            }
        );
    }

    #[test]
    fn a_rejected_transaction_changes_nothing() {
        let (mut buffer, now) = buffer("äb");
        let before = Selection::default();
        for edits in [
            vec![Edit::new(1..1, "x")],
            vec![Edit::new(0..9, "x")],
            vec![Edit::new(2..3, "x"), Edit::new(0..1, "y")],
            vec![],
        ] {
            assert!(buffer.transact(EditKind::Discrete, edits, before, before, now).is_err());
        }
        assert_eq!(buffer.text(), "äb");
        assert_eq!(buffer.revision(), 0);
        assert!(buffer.undo().is_none());
    }

    #[test]
    fn undo_and_redo_restore_text_and_selection() {
        let (mut buffer, now) = buffer("one two");
        let before = Selection { anchor: 0, head: 3 };
        let after = Selection::caret(1);
        let edits = vec![Edit::new(0..3, "1"), Edit::new(4..7, "2")];
        buffer.transact(EditKind::Discrete, edits, before, after, now).unwrap();
        assert_eq!(buffer.text(), "1 2");

        let undone = buffer.undo().unwrap();
        assert_eq!(buffer.text(), "one two");
        assert_eq!(undone.selection, before);

        let redone = buffer.redo().unwrap();
        assert_eq!(buffer.text(), "1 2");
        assert_eq!(redone.selection, after);
    }

    #[test]
    fn typing_groups_until_a_pause_or_a_caret_move() {
        let (mut buffer, now) = buffer("");
        insert(&mut buffer, 0, "a", EditKind::Typing, now);
        insert(&mut buffer, 1, "b", EditKind::Typing, now + Duration::from_millis(100));
        insert(&mut buffer, 2, "c", EditKind::Typing, now + Duration::from_secs(5));
        insert(&mut buffer, 0, "z", EditKind::Typing, now + Duration::from_millis(5100));
        assert_eq!(buffer.text(), "zabc");

        buffer.undo();
        assert_eq!(buffer.text(), "abc");
        buffer.undo();
        assert_eq!(buffer.text(), "ab");
        buffer.undo();
        assert_eq!(buffer.text(), "");
        assert!(buffer.undo().is_none());
    }

    #[test]
    fn a_composition_stays_one_undo_step_with_the_word_it_extends() {
        let (mut buffer, now) = buffer("");
        insert(&mut buffer, 0, "caf", EditKind::Typing, now);
        insert(
            &mut buffer,
            3,
            "´",
            EditKind::Composing,
            now + Duration::from_millis(50),
        );
        let composed = Instant::now() + Duration::from_secs(30);
        let edit = vec![Edit::new(3..5, "é")];
        let (before, after) = (Selection::caret(5), Selection::caret(5));
        buffer
            .transact(EditKind::Composing, edit, before, after, composed)
            .unwrap();
        assert_eq!(buffer.text(), "café");
        buffer.undo();
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn an_edit_clears_redo_and_discrete_edits_never_group() {
        let (mut buffer, now) = buffer("");
        insert(&mut buffer, 0, "a", EditKind::Discrete, now);
        insert(&mut buffer, 1, "b", EditKind::Discrete, now);
        buffer.undo();
        assert_eq!(buffer.text(), "a");
        insert(&mut buffer, 1, "c", EditKind::Typing, now);
        assert!(buffer.redo().is_none());
        assert_eq!(buffer.text(), "ac");
    }

    #[test]
    fn undo_stops_once_at_a_change_from_outside() {
        let (mut buffer, now) = buffer("");
        insert(&mut buffer, 0, "typed ", EditKind::Typing, now);
        insert(&mut buffer, 6, "outside ", EditKind::External, now);
        insert(&mut buffer, 14, "more", EditKind::Typing, now);

        buffer.undo();
        assert_eq!(buffer.text(), "typed outside ");
        assert!(buffer.undo().is_none());
        assert_eq!(buffer.text(), "typed outside ");

        assert!(buffer.undo().is_some());
        assert_eq!(buffer.text(), "typed ");
        buffer.redo();
        assert_eq!(buffer.text(), "typed outside ");

        assert!(buffer.undo().is_none(), "a new undo stops again");
        buffer.undo();
        buffer.undo();
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn utf16_offsets_map_both_ways_across_all_widths() {
        let text = "aä€😀b".repeat(2000);
        let buffer = Buffer::new(text.clone());
        assert_eq!(buffer.utf16_len(), text.encode_utf16().count());
        for (offset, _) in text.char_indices().step_by(97) {
            let utf16 = text[..offset].encode_utf16().count();
            assert_eq!(buffer.utf16_of(offset), utf16);
            assert_eq!(buffer.offset_of_utf16(utf16), offset);
        }
        assert_eq!(buffer.offset_of_utf16(usize::MAX), text.len());
    }

    #[test]
    fn a_utf16_position_inside_a_surrogate_pair_moves_past_the_character() {
        let buffer = Buffer::new("a😀b");
        assert_eq!(buffer.offset_of_utf16(2), 5);
    }

    #[test]
    fn lines_map_both_ways() {
        let buffer = Buffer::new("one\ntwo\n\nfour");
        assert_eq!(buffer.line_of(0), 1);
        assert_eq!(buffer.line_of(3), 1);
        assert_eq!(buffer.line_of(4), 2);
        assert_eq!(buffer.line_of(9), 4);
        assert_eq!(buffer.offset_of_line(1), 0);
        assert_eq!(buffer.offset_of_line(4), 9);
        assert_eq!(buffer.offset_of_line(9), buffer.len());
    }
}
