//! What one chapter's text says about footnotes, citations, labels, and images. Scanned from the text whenever it is
//! wanted and never written back, and small enough to keep for every chapter of a book.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;

use druck::editor::syntax::{self, Segment};

use super::footnotes::{Keeping, keeping};
use super::images;
use crate::source::inline::{SpanKind, inline_spans};
use crate::source::{BlockKind, Blocks};

/// A footnote reference in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRef {
    /// As written, without `[^` and `]`.
    pub label: String,
    pub range: Range<usize>,
}

/// A footnote definition: the whole block, without the line endings closing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub label: String,
    pub range: Range<usize>,
}

/// A label on a heading, figure, or table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelDef {
    /// With its prefix, such as `sec:intro`.
    pub label: String,
    /// The heading's words or the caption, as written.
    pub title: String,
    pub offset: usize,
}

/// An image as the text uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageUse {
    pub path: String,
    pub offset: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub refs: Vec<NoteRef>,
    pub definitions: Vec<Definition>,
    pub labels: Vec<LabelDef>,
    pub images: Vec<ImageUse>,
    /// The keys every citation names, as written.
    pub keys: Vec<String>,
    /// Where this text keeps its footnote definitions, if it has any.
    pub keeping: Option<Keeping>,
    /// Changes when anything does that a chip's text could depend on: citations, references, and what is numbered.
    pub signature: u64,
}

/// The label a definition block starts with.
pub fn definition_label(block_text: &str) -> Option<&str> {
    let rest = block_text.trim_start_matches(' ').strip_prefix("[^")?;
    let close = rest.find(']')?;
    (close > 0 && rest[close + 1..].starts_with(':')).then(|| &rest[..close])
}

/// The label a `[^label]` span names.
pub fn reference_label(span_text: &str) -> Option<&str> {
    span_text.strip_prefix("[^")?.strip_suffix(']')
}

/// The block's text after its heading markers, without a closing `{...}` group or closing `#`s.
fn heading_words(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let line = line.trim_start_matches('#').trim();
    let line = match line.rfind('{') {
        Some(open) if line.ends_with('}') => line[..open].trim_end(),
        _ => line,
    };
    line.trim_end_matches('#').trim_end().to_owned()
}

/// The `{#kind:name ...}` label in a `{...}` group at the end of `text`, with the prefix `kind:`.
fn trailing_label(text: &str, prefix: &str) -> Option<String> {
    let text = text.trim_end();
    let open = text.rfind('{').filter(|_| text.ends_with('}'))?;
    text[open + 1..text.len() - 1]
        .split_whitespace()
        .find_map(|word| word.strip_prefix('#').filter(|label| label.starts_with(prefix)))
        .map(str::to_owned)
}

/// A caption's words: without the leading `: ` and a closing `{...}`.
pub fn caption_words(text: &str) -> &str {
    let text = text.trim_end();
    let text = text.strip_prefix(':').unwrap_or(text).trim_start();
    match text.rfind('{') {
        Some(open) if text.ends_with('}') => text[..open].trim_end(),
        _ => text,
    }
}

/// Whether `text` may hold anything this scan looks for in running text.
fn may_hold_marks(text: &str) -> bool {
    text.contains("[^") || text.contains('@')
}

pub fn scan(text: &str) -> Facts {
    let blocks = Blocks::parse(text);
    let mut facts = Facts {
        keeping: keeping(&blocks),
        ..Facts::default()
    };
    let mut hasher = DefaultHasher::new();
    for block in blocks.as_slice() {
        let body = &text[block.range.clone()];
        let start = block.range.start;
        match block.kind {
            BlockKind::Heading { .. } => {
                body.hash(&mut hasher);
                if let Some(label) = trailing_label(body.lines().next().unwrap_or_default(), "sec:") {
                    facts.labels.push(LabelDef {
                        label,
                        title: heading_words(body),
                        offset: start,
                    });
                }
            }
            BlockKind::FootnoteDefinition => {
                if let Some(label) = definition_label(body) {
                    let trimmed = body.trim_end_matches(['\n', '\r']);
                    facts.definitions.push(Definition {
                        label: label.to_owned(),
                        range: start..start + trimmed.len(),
                    });
                }
            }
            BlockKind::Paragraph => {
                if let Some(figure) = images::Figure::parse(body) {
                    facts.images.push(ImageUse {
                        path: figure.path(body).to_owned(),
                        offset: start,
                    });
                    hasher.write_u8(!figure.caption(body).is_empty() as u8);
                    if let Some(label) = figure.label.as_ref().map(|range| &body[range.clone()]) {
                        facts.labels.push(LabelDef {
                            label: label.to_owned(),
                            title: figure.caption(body).to_owned(),
                            offset: start,
                        });
                    }
                    continue;
                }
                if body.starts_with(": ") {
                    hasher.write_u8(1);
                    if let Some(label) = trailing_label(body, "tbl:") {
                        facts.labels.push(LabelDef {
                            label,
                            title: caption_words(body).to_owned(),
                            offset: start,
                        });
                    }
                }
            }
            _ => {}
        }
        if !matches!(
            block.kind,
            BlockKind::Paragraph
                | BlockKind::List
                | BlockKind::Quote
                | BlockKind::FootnoteDefinition
                | BlockKind::Table
        ) || !may_hold_marks(body)
        {
            continue;
        }
        // A table's cells read as text, and its cells are where its marks are.
        let kind = if block.kind == BlockKind::Table {
            BlockKind::Paragraph
        } else {
            block.kind
        };
        for span in inline_spans(kind, body) {
            let written = &body[span.range.clone()];
            match span.kind {
                SpanKind::FootnoteReference => {
                    if let Some(label) = reference_label(written) {
                        facts.refs.push(NoteRef {
                            label: label.to_owned(),
                            range: start + span.range.start..start + span.range.end,
                        });
                    }
                }
                SpanKind::Citation => {
                    written.hash(&mut hasher);
                    for segment in syntax::find(written) {
                        if let Segment::Citation(citation) = segment {
                            facts.keys.extend(citation.items.into_iter().map(|item| item.key));
                        }
                    }
                }
                SpanKind::CrossReference => written.hash(&mut hasher),
                _ => {}
            }
        }
    }
    facts.signature = hasher.finish();
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_definitions_and_labels_come_from_the_text_not_from_code() {
        let text = "# Eins {#sec:eins}\n\nText[^a] und `[^x]` [@k, p. 3] @sec:eins.\n\n```\n[^y]\n```\n\n| a[^b] | c |\n|---|---|\n| 1 | 2 |\n\n: Größe {#tbl:maß}\n\n[^a]: Note [@l].\n\n[^b]: Other.\n\n![Bild](x.png){#fig:bild}\n";
        let facts = scan(text);
        let labels: Vec<&str> = facts.refs.iter().map(|note| note.label.as_str()).collect();
        assert_eq!(labels, ["a", "b"]);
        let defined: Vec<&str> = facts.definitions.iter().map(|note| note.label.as_str()).collect();
        assert_eq!(defined, ["a", "b"]);
        assert_eq!(facts.keys, ["k", "l"]);
        let named: Vec<(&str, &str)> = facts
            .labels
            .iter()
            .map(|label| (label.label.as_str(), label.title.as_str()))
            .collect();
        assert_eq!(
            named,
            [("sec:eins", "Eins"), ("tbl:maß", "Größe"), ("fig:bild", "Bild")]
        );
        assert_eq!(facts.images[0].path, "x.png");
    }

    #[test]
    fn the_signature_ignores_plain_words_and_follows_what_chips_depend_on() {
        let base = scan("# A\n\nWords [@k].\n").signature;
        assert_eq!(scan("# A\n\nOther words [@k].\n").signature, base);
        assert_ne!(scan("# A\n\nWords [@l].\n").signature, base);
        assert_ne!(scan("# B\n\nWords [@k].\n").signature, base);
    }
}
