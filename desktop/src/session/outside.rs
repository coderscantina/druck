//! A change from outside to a single file, as one undo step over all its sections.
//!
//! A single file is split into sections, each with its own buffer and undo history, so a change that touches several
//! of them, or moves the headings between them, cannot be one step of any one buffer. Instead the sections it changed
//! are set aside whole, with their histories, and new ones with its text take their place. Undo puts the set-aside
//! sections back and sets the new ones aside in turn, and redo swaps again. Sections the change left alone stay in the
//! book through both, with their own histories, and typing in them does not block the swap.

use std::time::Instant;

use super::joined::ChapterMap;
use super::{Chapter, ChapterId, Group, Read, Session};
use crate::persist::merge::{edits_between, map_offset};
use crate::source::buffer::{EditError, Restored};
use crate::source::{Blocks, Buffer, Edit, EditKind, Selection};

/// How many outside changes to a single file stay undoable. Each holds the text of the sections it replaced.
const KEPT: usize = 50;

/// One swap of sections, from the book as it is after the step to the book as it is on the other side. Every section
/// it holds is readable: a change from or to an unreadable file is no step.
pub(super) struct Turn {
    /// The book's sections where this turn starts, with each one's undo clock then.
    here: Vec<(ChapterId, u64)>,
    /// The sections of `here` that the turn sets aside, with the text they must still hold.
    leaving: Vec<(ChapterId, String)>,
    /// The book's sections on the other side.
    there: Vec<ChapterId>,
    /// The sections of `there` that are not in `here`, kept whole with their histories.
    away: Vec<(ChapterId, Chapter)>,
}

/// What one undo or redo changed.
#[derive(Debug)]
pub enum Undone {
    /// A change from outside to a single file, across its sections. The caret's section may be another one now.
    File,
    /// A step of the caret's section.
    Own(Restored),
    /// A change from outside to the caret's chapter, its own step.
    Outside(Restored),
}

/// The outside changes of a single file that undo and redo can reach.
#[derive(Default)]
pub(super) struct Outside {
    undo: Vec<Turn>,
    redo: Vec<Turn>,
}

impl Session {
    /// Applies a change from outside to a single file as one undo step over all its sections. The edits refer to the
    /// file's whole text. The caret stays with its text.
    pub(super) fn apply_outside(&mut self, edits: &[Edit]) -> Result<(), EditError> {
        let from = self.joined();
        let mut scratch = Buffer::new(from.text.as_str());
        let none = Selection::default();
        scratch.transact(EditKind::External, edits.to_vec(), none, none, Instant::now())?;
        let text = scratch.text();
        let before = self.revision();
        let follow = self.caret_place(&from.map);
        // An unreadable file's sections show text it does not hold, so they are nothing to go back to.
        let readable = (self.book.iter()).all(|id| self.known(*id).read == Read::Done);
        let mut old = self.book.clone();
        let mut book = Vec::new();
        let mut made = Vec::new();
        for range in super::sections(text) {
            let piece = &text[range];
            let kept = old.iter().position(|id| self.known(*id).buffer.text() == piece);
            let id = match kept {
                Some(at) => old.remove(at),
                None => {
                    let path = self.location().to_owned();
                    let id = self.add(Chapter::new(
                        path,
                        String::new(),
                        Group::Book,
                        Buffer::new(piece),
                        Read::Done,
                    ));
                    made.push((id, piece.to_owned()));
                    id
                }
            };
            book.push(id);
        }
        let there = std::mem::replace(&mut self.book, book);
        let away = (old.into_iter())
            .map(|id| (id, self.chapters.remove(&id).expect("a section of the book")))
            .collect::<Vec<_>>();
        self.resplit_unsaved |= !made.is_empty() || !away.is_empty();
        // The change's text is what the file holds, so every section of it is readable now.
        for id in &self.book {
            (self.chapters.get_mut(id).expect("a section of the book")).read = Read::Done;
        }
        self.outside.redo.clear();
        if readable {
            let turn = Turn {
                here: self.close_steps(),
                leaving: made,
                there,
                away,
            };
            self.outside.undo.push(turn);
            if self.outside.undo.len() > KEPT {
                self.outside.undo.remove(0);
            }
        }
        self.follow_caret(follow, edits);
        self.set_revision_above(before);
        self.refresh_details();
        Ok(())
    }

    /// Undo as every surface of the caret's text does it, the galley, its notes, and a table's cells: a change from
    /// outside to a single file first, unless the caret section's own step moved after it, and that step otherwise.
    pub fn undo(&mut self) -> Option<Undone> {
        if self.undo_outside_file_change() {
            return Some(Undone::File);
        }
        let buffer = self.active_buffer_mut();
        let outside = buffer.undo_is_outside_change();
        let restored = buffer.undo()?;
        Some(if outside {
            Undone::Outside(restored)
        } else {
            Undone::Own(restored)
        })
    }

    /// Redo in the same order as [`Self::undo`]. A redone change from outside to one buffer is [`Undone::Own`].
    pub fn redo(&mut self) -> Option<Undone> {
        if self.redo_outside_file_change() {
            return Some(Undone::File);
        }
        self.active_buffer_mut().redo().map(Undone::Own)
    }

    /// Whether the next undo takes back a change from outside to a single file: nothing typed since in its sections,
    /// and nothing typed since in the caret's section that undo would take first.
    pub fn undo_is_outside_file_change(&self) -> bool {
        (self.outside.undo.last()).is_some_and(|turn| self.can_turn(turn, true))
    }

    /// Takes back the last change from outside to a single file, in all its sections. See
    /// [`Self::undo_is_outside_file_change`].
    pub fn undo_outside_file_change(&mut self) -> bool {
        if !self.undo_is_outside_file_change() {
            return false;
        }
        let turn = self.outside.undo.pop().expect("a turn to undo");
        let back = self.turn(turn);
        self.outside.redo.push(back);
        true
    }

    /// Whether the next redo brings back a change from outside that undo took back.
    pub fn redo_is_outside_file_change(&self) -> bool {
        (self.outside.redo.last()).is_some_and(|turn| self.can_turn(turn, false))
    }

    pub fn redo_outside_file_change(&mut self) -> bool {
        if !self.redo_is_outside_file_change() {
            return false;
        }
        let turn = self.outside.redo.pop().expect("a turn to redo");
        let back = self.turn(turn);
        self.outside.undo.push(back);
        true
    }

    /// A turn applies while the book is as it left it, readable, and the sections it sets aside hold what it gave them.
    /// It goes before the caret's own history unless that history's next step to undo, or to redo, moved after the turn:
    /// typing since, or an undo since. A resplit since leaves other sections, and the turn unreachable.
    fn can_turn(&self, turn: &Turn, undo: bool) -> bool {
        let same_book = turn.here.len() == self.book.len()
            && (turn.here.iter().zip(&self.book)).all(|((recorded, _), id)| recorded == id);
        // Only the book's sections are known to the session, so the rest is read only once the book is the same.
        same_book
            && (self.book.iter()).all(|id| self.known(*id).read == Read::Done)
            && (turn.leaving.iter()).all(|(id, text)| self.known(*id).buffer.text() == text)
            && !(turn.here.iter())
                .find(|(id, _)| *id == self.active)
                .is_some_and(|(_, then)| self.active_buffer().moved_since(*then, undo))
    }

    /// Swaps the book to the other side of `turn`, and returns the turn that swaps it back.
    fn turn(&mut self, turn: Turn) -> Turn {
        let from = self.joined();
        let before = self.revision();
        let follow = self.caret_place(&from.map);
        let Turn { here, there, away, .. } = turn;
        let leaving = (here.iter())
            .filter(|(id, _)| !there.contains(id))
            .map(|(id, _)| (*id, self.chapters.remove(id).expect("a section of the book")))
            .collect::<Vec<_>>();
        for (id, chapter) in away {
            self.chapters.insert(id, chapter);
        }
        self.book = there;
        // Sections that come back can stand next to ones typed in since, which need not keep apart from them yet.
        for at in 1..self.book.len() {
            let (previous, next) = (self.book[at - 1], self.book[at]);
            let arrived = |id: ChapterId| !here.iter().any(|(kept, _)| *kept == id);
            if arrived(previous) || arrived(next) {
                self.keep_after(previous, next, at == 1);
            }
        }
        let back = Turn {
            here: self.close_steps(),
            leaving: (self.book.iter())
                .filter(|id| !here.iter().any(|(kept, _)| kept == *id))
                .map(|id| (*id, self.known(*id).buffer.text().to_owned()))
                .collect(),
            there: here.into_iter().map(|(id, _)| id).collect(),
            away: leaving,
        };
        // Neither side need be what the file holds: the next save compares.
        self.resplit_unsaved = true;
        let edits = edits_between(&from.text, &self.joined().text);
        self.follow_caret(follow, &edits);
        self.set_revision_above(before);
        self.refresh_details();
        back
    }

    /// Ends every section's open undo step, so typing after a turn never joins typing before it, and returns each one's
    /// undo clock.
    fn close_steps(&mut self) -> Vec<(ChapterId, u64)> {
        let book = self.book.clone();
        (book.into_iter())
            .map(|id| {
                let buffer = &mut self.known_mut(id).buffer;
                buffer.close_step();
                (id, buffer.clock())
            })
            .collect()
    }

    /// Where the caret and the top of the view stand in the file's whole text, before a change moves sections.
    fn caret_place(&self, map: &ChapterMap) -> CaretPlace {
        let chapter = self.known(self.active);
        let to_joined = |offset| map.to_joined(self.active, offset).unwrap_or(0);
        let scroll = chapter.view.scroll.and_then(|(block, offset)| {
            let start = Blocks::parse(chapter.buffer.text()).as_slice().get(block)?.range.start;
            Some((to_joined(start), offset))
        });
        CaretPlace {
            chapter: self.active,
            head: to_joined(chapter.view.selection.head),
            scroll,
        }
    }

    /// Puts the caret, and the view's top, in the section that holds its text now, if its section was set aside.
    fn follow_caret(&mut self, place: CaretPlace, edits: &[Edit]) {
        if self.book.contains(&place.chapter) {
            return;
        }
        let map = self.joined().map;
        let Some((id, offset)) = map.chapter_at(map_offset(place.head, edits)) else {
            self.active = self.book[0];
            return;
        };
        let start = map.to_joined(id, 0).unwrap_or(0);
        let chapter = self.known_mut(id);
        chapter.view.selection = Selection::caret(offset);
        chapter.view.scroll = place.scroll.and_then(|(at, offset)| {
            let local = map_offset(at, edits).checked_sub(start)?;
            Some((Blocks::parse(chapter.buffer.text()).index_at(local), offset))
        });
        self.active = id;
    }
}

/// The caret's section, and where its caret and its view's top stand in the whole text.
struct CaretPlace {
    chapter: ChapterId,
    head: usize,
    scroll: Option<(usize, f32)>,
}

#[cfg(test)]
mod tests;
