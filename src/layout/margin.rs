//! Margin notes and sidenotes: notes set in a column beside the prose, on the outer side of the page.
//!
//! A margin note is a footnote whose definition starts with `{-}`. With `margin-notes.sidenotes`, numbered
//! footnotes go to the same column instead of the foot of the page. Notes are set at the column width in
//! the style `margin-notes.style` names, and [the composer](super::pages) places them beside the line
//! that references them.

use super::pages::{Margin, Note};
use super::{FlowLine, Frame, NoteRef};
use crate::config::resolved::Config;
use crate::config::source::InputSource;
use crate::diagnostic::Diagnostic;
use crate::document::{Document, Footnote};
use crate::page::NoteKind;
use crate::tags::Kind;

/// Reports a note column that runs off the page, or one inside the margin frame where wide blocks would
/// run under it. Only documents with notes in the margin are checked.
pub(super) fn check(document: &Document, config: &Config, source: &InputSource) -> Result<(), Vec<Diagnostic>> {
    let notes = &config.margin_notes;
    let sidenotes = notes.sidenotes && !document.footnotes.is_empty();
    if document.margin_notes.is_empty() && !sidenotes {
        return Ok(());
    }
    let page = &config.page;
    let need = notes.gap.0 + notes.width.0;
    let room = page.outer_room();
    let free = page.text_width().0 - page.prose_width.0;
    let message = if need > room + 1e-6 {
        format!(
            "the margin note column needs {need:.1}pt beside the prose (margin-notes.gap and margin-notes.width), but \
             only {room:.1}pt are left to the edge of the page"
        )
    } else if notes.gap.0 < free - 1e-6 && !page.wide.is_empty() {
        format!(
            "the margin note column starts {:.1}pt inside the margin frame, where the blocks in page.wide would run \
             under it; set page.wide to [] or margin-notes.gap to at least {free:.1}pt",
            free - notes.gap.0
        )
    } else {
        return Ok(());
    };
    Err(vec![Diagnostic::new(Some(source.clone()), message)])
}

impl<'a> super::Flow<'a> {
    /// Sets the margin notes, and the `side` footnotes as sidenotes, at the width of the note column, in
    /// the order the `body` lines reference them.
    pub(super) fn margin(&mut self, body: &[FlowLine], side: &[Footnote], notes: &[Footnote]) -> Margin {
        if side.is_empty() && notes.is_empty() {
            return Margin::default();
        }
        let config: &'a Config = self.config;
        let frame = Frame {
            left: 0.0,
            right: config.page.text_width().0 - config.margin_notes.width.0,
            style: config.named_style(&config.margin_notes.style),
            list_depth: 0,
            widen: 0.0,
            align: Some(config.margin_notes.align),
        };
        let set = |lines: Vec<FlowLine>, note: &Footnote, kind: NoteKind| {
            let height = lines.iter().skip(1).map(|line| line.space_before).sum::<f64>()
                + lines.iter().map(|line| line.line.height).sum::<f64>();
            Some(Note {
                baseline: lines.first().map_or(0.0, |line| line.line.baseline),
                lines,
                height,
                at: note.at,
                kind,
            })
        };
        let mut side: Vec<Option<Note>> = side
            .iter()
            .enumerate()
            .map(|(index, note)| set(self.note(index, note, frame), note, NoteKind::Side(index)))
            .collect();
        let mut notes: Vec<Option<Note>> = notes
            .iter()
            .enumerate()
            .map(|(index, note)| {
                let element = self
                    .tags
                    .margin_note(index)
                    .unwrap_or_else(|| self.tags.add(Kind::Note, None));
                let scope = self.enter(element, false);
                let lines = self.unmarked(&note.blocks, frame);
                self.close(scope);
                set(lines, note, NoteKind::Margin(index))
            })
            .collect();
        let mut margin = Margin::default();
        for (index, line) in body.iter().enumerate() {
            for &(mark, baseline) in &line.line.marks {
                let note = match mark {
                    NoteRef::Footnote(note) => side.get_mut(note),
                    NoteRef::Margin(note) => notes.get_mut(note),
                };
                if let Some(note) = note.and_then(Option::take) {
                    margin.notes.push(note);
                    margin.references.push((index, baseline));
                }
            }
        }
        margin
    }
}
