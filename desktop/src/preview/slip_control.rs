//! What the proof slip does to the text. The slip holds a copy of its paragraph, and the chapter is the truth, so these
//! carry its changes over and keep it on its paragraph when something else changes the chapter.

use gpui::{App, Entity, Window};
use serde_json::json;

use super::directives::{self, edit_between};
use super::job::BookType;
use super::proof::BlockAt;
use super::slip::{self, Directive, ProofSlip, StyleChoice};
use super::strings::Key;
use crate::editor::rich::styles;
use crate::session::ChapterId;
use crate::source::{Blocks, EditKind};
use crate::theme_file::editor::{Change, Outcome};
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
    parts
        .preview
        .update(cx, |preview, cx| preview.set_slip(Some(source), cx));
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
        let splice = (revision == opened.revision + 1)
            .then(|| buffer.last_splice())
            .flatten();
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

/// What the paragraph's list of styles offers, from the chapter and the theme as they stand. `None` for a block that takes
/// no `{.name}`, such as a quotation.
fn style_choice(parts: &Parts, cx: &App) -> Option<StyleChoice> {
    let opened = slip_of(parts, cx).read(cx).opened()?.clone();
    let session = parts.editor.read(cx).session();
    let text = session.chapter(opened.chapter)?.text();
    let blocks = Blocks::parse(text);
    let index = blocks.index_at(opened.range.start);
    let block = &blocks.as_slice()[index];
    let class = styles::class_of(block.kind, &text[block.range.clone()])?;
    let model = parts.inspector.read(cx).model().read(cx);
    Some(StyleChoice {
        current: styles::style_of(text, &blocks, index),
        base: styles::base_style(block.kind),
        styles: model
            .file()
            .map_or_else(|_| Vec::new(), |file| styles::styles_for(class, file.merged())),
    })
}

/// Opens or closes the paragraph's list of styles. A block with no list goes straight to its style's section.
pub fn toggle_styles(parts: &Parts, window: &mut Window, cx: &mut App) {
    let slip = slip_of(parts, cx);
    if slip.read(cx).choice().is_some() {
        slip.update(cx, |slip, cx| slip.set_choice(None, cx));
        return;
    }
    if !follow_chapter(parts, cx) {
        return;
    }
    match style_choice(parts, cx) {
        Some(choice) => slip.update(cx, |slip, cx| slip.set_choice(Some(choice), cx)),
        None => {
            let style = slip.read(cx).opened().and_then(|opened| opened.style.clone());
            if let Some(style) = style {
                parts
                    .inspector
                    .update(cx, |inspector, cx| inspector.open_style(&style, window, cx));
            }
        }
    }
}

/// Writes `{.name}` as an ordinary edit of the text, or takes it off, and the slip follows its paragraph.
pub fn pick_style(parts: &Parts, name: Option<&str>, window: &mut Window, cx: &mut App) {
    if !follow_chapter(parts, cx) {
        return;
    }
    let slip = slip_of(parts, cx);
    let Some(opened) = slip.read(cx).opened().cloned() else {
        return;
    };
    let editor = parts.editor.clone();
    let session = editor.read(cx).session();
    let text = session.buffer(opened.chapter).text();
    let blocks = Blocks::parse(text);
    let index = blocks.index_at(opened.range.start);
    let style = name
        .unwrap_or(styles::base_style(blocks.as_slice()[index].kind))
        .to_owned();
    let Some(edit) = styles::set_style(text, &blocks, index, name) else {
        slip.update(cx, |slip, cx| slip.set_choice(None, cx));
        return;
    };
    let range = slip::range_after_style(&opened.range, &edit);
    let made = editor.update(cx, |editor, cx| {
        editor.edit_outside(opened.chapter, vec![edit], EditKind::Discrete, window, cx)
    });
    if made.is_ok() {
        let buffer = editor.read(cx).session().buffer(opened.chapter);
        let written = buffer.text()[range.clone()].to_owned();
        let (state, revision) = (directives::read(buffer.text(), &range), buffer.revision());
        slip.update(cx, |slip, cx| slip.styled(style, range, written, revision, state, cx));
    }
}

/// Picks or makes the named style that sets the paragraph this way: `body-center`, based on the style it has now. There are
/// no per-block settings. A second press of the alignment it has takes it back off.
pub fn pick_alignment(parts: &Parts, align: &'static str, window: &mut Window, cx: &mut App) {
    let Some(choice) = style_choice(parts, cx) else {
        return;
    };
    let current = choice.current.as_deref();
    let base = current
        .map_or(choice.base, |name| styles::unaligned(name).unwrap_or(name))
        .to_owned();
    let name = styles::aligned_name(&base, align);
    if current == Some(name.as_str()) {
        let back = (base != choice.base).then_some(base.as_str());
        pick_style(parts, back, window, cx);
        return;
    }
    if choice.styles.contains(&name) {
        pick_style(parts, Some(&name), window, cx);
        return;
    }
    let slip = slip_of(parts, cx);
    let model = parts.inspector.read(cx).model().clone();
    if model.read(cx).file().is_err() {
        slip.update(cx, |slip, cx| {
            slip.set_style_problem(Some(crate::t!(Key::SlipNoTheme).to_owned()), cx)
        });
        return;
    }
    let change = Change::set(&["custom-styles", &name], json!({"based-on": base, "align": align}));
    match model.update(cx, |model, cx| model.change(change, cx)) {
        Ok(Outcome::Saved) => pick_style(parts, Some(&name), window, cx),
        Ok(Outcome::NeedsDecision) => slip.update(cx, |slip, cx| {
            slip.set_waiting_style(name);
            slip.set_style_problem(Some(crate::t!(Key::SlipStyleWaits).to_owned()), cx);
        }),
        Err(why) => slip.update(cx, |slip, cx| slip.set_style_problem(Some(why), cx)),
    }
}

/// `DRUCK_SLIP_ALIGN=center` presses that alignment, and `DRUCK_SLIP_STYLES=1` opens the list. For screenshots.
#[cfg(any(debug_assertions, feature = "measure"))]
pub fn try_from_gate(parts: &Parts, window: &mut Window, cx: &mut App) {
    if let Some(align) = crate::gate::var("DRUCK_SLIP_ALIGN")
        && let Some(align) = styles::ALIGNMENTS.into_iter().find(|known| *known == align)
    {
        pick_alignment(parts, align, window, cx);
    }
    if crate::gate::var("DRUCK_SLIP_STYLES").is_some() {
        toggle_styles(parts, window, cx);
    }
}

/// The theme changed. A style the question about a common theme held back is there now.
pub fn theme_changed(parts: &Parts, window: &mut Window, cx: &mut App) {
    let slip = slip_of(parts, cx);
    let Some(name) = slip.update(cx, |slip, _| slip.take_waiting_style()) else {
        return;
    };
    let there = style_choice(parts, cx).is_some_and(|choice| choice.styles.contains(&name));
    if there {
        pick_style(parts, Some(&name), window, cx);
    } else {
        slip.update(cx, |slip, _| slip.set_waiting_style(name));
    }
}
