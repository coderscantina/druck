//! What the proof slip does to the text. The slip holds a copy of its paragraph, and the chapter is the truth, so these
//! carry its changes over and keep it on its paragraph when something else changes the chapter.

use gpui::{App, Entity, Window};

use super::job::BookType;
use super::proof::BlockAt;
use super::slip::{self, Directive, ProofSlip};
use super::directives::{self, edit_between};
use crate::session::ChapterId;
use crate::source::EditKind;
use crate::tokens;
use crate::workspace::Parts;

fn slip_of(parts: &Parts, cx: &App) -> Entity<ProofSlip> {
    parts.preview.read(cx).slip().clone()
}

/// Opens the slip on a block, unless the pages are behind the text and so have other offsets. The text goes to the same
/// place, so Write opens there.
pub fn open(parts: &Parts, block: &BlockAt, book_type: Option<&BookType>, window: &mut Window, cx: &mut App) {
    let editor = parts.editor.clone();
    if !parts.preview.read(cx).shows(editor.read(cx).revision()) {
        return;
    }
    let session = editor.read(cx).session();
    let Some(opening) = slip::opening(session, block) else {
        return;
    };
    let state = directives::read(session.buffer(opening.chapter).text(), &opening.range);
    let (id, at) = (opening.chapter, opening.range.start);
    let stand_in = tokens::stand_in_type();
    let face = (book_type.unwrap_or(&stand_in))
        .face_of(opening.style.as_deref().unwrap_or("body"))
        .clone();
    slip_of(parts, cx).update(cx, |slip, cx| slip.open(opening, state, &face, window, cx));
    editor.update(cx, |editor, cx| editor.show_place(id, at, window, cx));
    sync(parts, cx);
}

pub fn close(parts: &Parts, cx: &mut App) {
    slip_of(parts, cx).update(cx, |slip, cx| slip.close(cx));
    parts.preview.update(cx, |preview, cx| preview.set_slip(None, cx));
}

/// Shows the slip at its paragraph on the pages, once they are set from the text it is in.
pub fn sync(parts: &Parts, cx: &mut App) {
    let Some(opened) = slip_of(parts, cx).read(cx).opened().cloned() else {
        return;
    };
    let editor = parts.editor.read(cx);
    if !parts.preview.read(cx).shows(editor.revision()) {
        return;
    }
    let Some((file, start)) = editor.session().file_position(opened.chapter, opened.range.start) else {
        return;
    };
    let source = (file, start..start + opened.range.len());
    parts.preview.update(cx, |preview, cx| preview.set_slip(Some(source), cx));
}

/// Keeps the slip on its paragraph when something else changed the chapter. While the field composes this waits: the end
/// of the composition reaches [`edited`], which looks first.
pub fn keep(parts: &Parts, cx: &mut App) {
    let composing = slip_of(parts, cx).read(cx).input().read(cx).is_composing();
    if !composing {
        follow_chapter(parts, cx);
    }
}

/// Finds the slip's paragraph in the chapter as it stands now, or closes the slip if it is gone. Says whether it is open.
fn follow_chapter(parts: &Parts, cx: &mut App) -> bool {
    let slip = slip_of(parts, cx);
    let Some(opened) = slip.read(cx).opened().cloned() else {
        return false;
    };
    let session = parts.editor.read(cx).session();
    let found = session.chapter(opened.chapter).and_then(|chapter| {
        let buffer = session.buffer(opened.chapter);
        let revision = buffer.revision();
        if revision == opened.revision {
            return Some(None);
        }
        let splice = (revision == opened.revision + 1).then(|| buffer.last_splice()).flatten();
        let text = chapter.text();
        let range = slip::relocated(text, &opened.synced, &opened.range, splice)?;
        Some(Some((directives::read(text, &range), range, revision)))
    });
    match found {
        Some(None) => true,
        Some(Some((state, range, revision))) => {
            slip.update(cx, |slip, cx| slip.synced(range, opened.synced, revision, state, cx));
            true
        }
        None => {
            close(parts, cx);
            false
        }
    }
}

/// Carries what changed in the slip over to the chapter. What the chapter holds at the slip's place may not be the
/// paragraph any more, so the slip finds it first and never writes over anything else.
pub fn edited(parts: &Parts, window: &mut Window, cx: &mut App) {
    if !follow_chapter(parts, cx) {
        return;
    }
    let slip = slip_of(parts, cx);
    let Some(opened) = slip.read(cx).opened().cloned() else {
        return;
    };
    let new = slip.read(cx).text(cx).to_owned();
    let Some(edit) = edit_between(opened.range.start, &opened.synced, &new) else {
        return;
    };
    let kind = if new.len() < opened.synced.len() {
        EditKind::Deleting
    } else {
        EditKind::Typing
    };
    let range = opened.range.start..opened.range.start + new.len();
    let made = parts.editor.update(cx, |editor, cx| {
        editor.edit_outside(opened.chapter, vec![edit], kind, window, cx)
    });
    if made.is_ok() {
        mark_synced(parts, opened.chapter, range, new, cx);
    }
}

/// Adds or takes away a directive around the slip's paragraph.
pub fn toggled(parts: &Parts, directive: Directive, window: &mut Window, cx: &mut App) {
    if !follow_chapter(parts, cx) {
        return;
    }
    let Some(opened) = slip_of(parts, cx).read(cx).opened().cloned() else {
        return;
    };
    let editor = parts.editor.clone();
    let text = editor.read(cx).session().buffer(opened.chapter).text();
    let change = match directive {
        Directive::PageBreak => directives::toggle_page_break(text, &opened.range),
        Directive::Keep => directives::toggle_keep(text, &opened.range),
    };
    let range = change.range.clone();
    let made = editor.update(cx, |editor, cx| {
        editor.edit_outside(opened.chapter, change.edits, EditKind::Discrete, window, cx)
    });
    if made.is_ok() {
        mark_synced(parts, opened.chapter, range, opened.synced, cx);
    }
}

fn mark_synced(parts: &Parts, chapter: ChapterId, range: std::ops::Range<usize>, synced: String, cx: &mut App) {
    let buffer = parts.editor.read(cx).session().buffer(chapter);
    let (state, revision) = (directives::read(buffer.text(), &range), buffer.revision());
    slip_of(parts, cx).update(cx, |slip, cx| slip.synced(range, synced, revision, state, cx));
}
