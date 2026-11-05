//! What the timeline draws for a version: the blocks of the text, in the book's kinds, with Markdown's markers hidden and
//! the word differences against the text as it is now. Holds no gpui types.

use std::ops::Range;

use super::words::{Hunk, hunks, passages};
use crate::source::inline::{SpanKind, inline_spans, line_markers};
use crate::source::{BlockKind, Blocks, Edit};

/// How a stretch of a shown block differs from now. The version holds what is `Removed`, and now holds what is `Added`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub range: Range<usize>,
    pub tint: Option<Tint>,
    pub passage: Option<usize>,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownBlock {
    pub kind: BlockKind,
    pub text: String,
    pub runs: Vec<Run>,
    /// The first passage in this block.
    pub passage: Option<usize>,
}

/// A version against the text as it is now.
pub struct Shown {
    pub blocks: Vec<ShownBlock>,
    hunks: Vec<Hunk>,
    passages: Vec<Range<usize>>,
}

impl Shown {
    pub fn passage_count(&self) -> usize {
        self.passages.len()
    }

    /// The edits that make one passage of `current` what `version` holds.
    pub fn restore_passage(&self, passage: usize, version: &str) -> Vec<Edit> {
        let range = self.passages.get(passage).cloned().unwrap_or(0..0);
        (self.hunks[range].iter())
            .map(|hunk| Edit::new(hunk.new.clone(), &version[hunk.old.clone()]))
            .collect()
    }
}

/// A stretch of the text with both versions in it.
struct Stretch {
    range: Range<usize>,
    tint: Tint,
    passage: usize,
}

/// Lays `version` out with what differs from `current`.
pub fn build(version: &str, current: &str) -> Shown {
    let hunks = hunks(version, current);
    let passages = passages(&hunks, version);
    let passage_of = |at: usize| passages.iter().position(|passage| passage.contains(&at)).unwrap_or(0);
    let mut merged = String::with_capacity(version.len() + 64);
    let mut stretches = Vec::new();
    let mut read = 0;
    for (at, hunk) in hunks.iter().enumerate() {
        merged.push_str(&version[read..hunk.old.start]);
        let mut stretch = |text: &str, tint| {
            if !text.is_empty() {
                let start = merged.len();
                merged.push_str(text);
                stretches.push(Stretch {
                    range: start..merged.len(),
                    tint,
                    passage: passage_of(at),
                });
            }
        };
        stretch(&version[hunk.old.clone()], Tint::Removed);
        stretch(&current[hunk.new.clone()], Tint::Added);
        read = hunk.old.end;
    }
    merged.push_str(&version[read..]);
    let parsed = Blocks::parse(&merged);
    let blocks = (parsed.as_slice().iter())
        .filter(|block| block.kind != BlockKind::Blank)
        .filter_map(|block| shown_block(block.kind, block.range.clone(), &merged, &stretches))
        .collect();
    Shown {
        blocks,
        hunks,
        passages,
    }
}

/// One block's text with its markers taken out. A comment or front matter nobody changed stays hidden, as in the text.
fn shown_block(kind: BlockKind, range: Range<usize>, merged: &str, stretches: &[Stretch]) -> Option<ShownBlock> {
    let inside: Vec<&Stretch> = (stretches.iter())
        .filter(|stretch| stretch.range.start < range.end && range.start < stretch.range.end)
        .collect();
    if inside.is_empty() && matches!(kind, BlockKind::Comment | BlockKind::FrontMatter) {
        return None;
    }
    let source = merged[range.clone()].trim_end_matches(['\n', '\r']);
    let spans = inline_spans(kind, source);
    let mut hidden: Vec<Range<usize>> = line_markers(kind, source)
        .into_iter()
        .map(|marker| marker.range)
        .collect();
    let mut bold = Vec::new();
    let mut italic = Vec::new();
    for span in &spans {
        match span.kind {
            SpanKind::Strong => bold.push(span.range.clone()),
            SpanKind::Emphasis => italic.push(span.range.clone()),
            _ => continue,
        }
        hidden.push(span.open.clone());
        hidden.push(span.close.clone());
    }
    hidden.sort_by_key(|range| range.start);
    // Every place a style can change.
    let mut cuts = vec![0, source.len()];
    for range in hidden.iter().chain(&bold).chain(&italic) {
        cuts.extend([range.start, range.end]);
    }
    for stretch in &inside {
        cuts.push(stretch.range.start.saturating_sub(range.start).min(source.len()));
        cuts.push(stretch.range.end.saturating_sub(range.start).min(source.len()));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut text = String::with_capacity(source.len());
    let mut runs: Vec<Run> = Vec::new();
    let mut first_passage = None;
    for pair in cuts.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if from >= to || hidden.iter().any(|hide| hide.start <= from && to <= hide.end) {
            continue;
        }
        let stretch = (inside.iter())
            .find(|stretch| stretch.range.start <= range.start + from && range.start + to <= stretch.range.end);
        let start = text.len();
        text.push_str(&source[from..to]);
        let run = Run {
            range: start..text.len(),
            tint: stretch.map(|stretch| stretch.tint),
            passage: stretch.map(|stretch| stretch.passage),
            bold: bold.iter().any(|span| span.start <= from && to <= span.end),
            italic: italic.iter().any(|span| span.start <= from && to <= span.end),
        };
        first_passage = first_passage.or(run.passage);
        runs.push(run);
    }
    Some(ShownBlock {
        kind,
        text,
        runs,
        passage: first_passage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tinted(block: &ShownBlock, tint: Tint) -> Vec<&str> {
        (block.runs.iter())
            .filter(|run| run.tint == Some(tint))
            .map(|run| &block.text[run.range.clone()])
            .collect()
    }

    #[test]
    fn differences_show_as_words_in_the_blocks_kinds() {
        let version = "# The title\n\nThe fire burnt very brightly.\n";
        let current = "# The title\n\nThe fire burnt quite brightly.\n";
        let shown = build(version, current);
        assert_eq!(shown.blocks.len(), 2);
        assert_eq!(shown.blocks[0].kind, BlockKind::Heading { level: 1 });
        assert_eq!(shown.blocks[0].text, "The title");
        assert_eq!(shown.blocks[0].passage, None);
        let body = &shown.blocks[1];
        assert_eq!(tinted(body, Tint::Removed), ["very"]);
        assert_eq!(tinted(body, Tint::Added), ["quite"]);
        assert_eq!(body.passage, Some(0));
        assert_eq!(shown.passage_count(), 1);
    }

    #[test]
    fn emphasis_shows_without_its_marks() {
        let shown = build(
            "Ein *schönes* und **großes** Wort.\n",
            "Ein *schönes* und **großes** Wort.\n",
        );
        assert_eq!(shown.blocks[0].text, "Ein schönes und großes Wort.");
        let run = |word: &str| {
            (shown.blocks[0].runs.iter())
                .find(|run| &shown.blocks[0].text[run.range.clone()] == word)
                .unwrap()
                .clone()
        };
        assert!(run("schönes").italic && !run("schönes").bold);
        assert!(run("großes").bold);
        assert_eq!(shown.passage_count(), 0);
    }

    #[test]
    fn a_hidden_comment_shows_only_if_it_changed() {
        let same = "Text.\n\n<!-- a note -->\n\nMore.\n";
        assert_eq!(build(same, same).blocks.len(), 2);
        let changed = "Text.\n\n<!-- a better note -->\n\nMore.\n";
        assert_eq!(build(same, changed).blocks.len(), 3);
    }

    #[test]
    fn restoring_a_passage_changes_only_that_passage_and_is_exact() {
        let version = "One two three.\n\nFour five six.\n\nSeven.\n";
        let current = "One 2 three.\n\nFour five six.\n\nSeven and eight.\n";
        let shown = build(version, current);
        assert_eq!(shown.passage_count(), 2);
        let mut text = current.to_owned();
        for edit in shown.restore_passage(0, version).iter().rev() {
            text.replace_range(edit.range.clone(), &edit.text);
        }
        assert_eq!(text, "One two three.\n\nFour five six.\n\nSeven and eight.\n");
    }
}
