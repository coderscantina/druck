//! Index marks as text: `{^entry!sub-entry}` after the words it is about. The card shows the entry and the
//! sub-entry, and what else a mark holds, a third level or a `|see` reference, stays as written.

use std::ops::Range;

use super::Key;
use crate::editor::display::{Chip, ChipLook};
use crate::source::inline::{InlineSpan, SpanKind, has_inline, inline_spans};
use crate::source::{BlockKind, Blocks, Edit, Selection, motion};

/// What stands in the text for a mark at rest.
pub const SIGN: &str = "\u{2038}";

/// A mark's terms as the card shows them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Terms {
    pub entry: String,
    pub sub: String,
    /// A third level and a reference, with their `!` and `|`, which the card keeps as they are.
    rest: String,
}

/// A mark in a block's text, as ranges relative to the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `{^...}`
    pub braces: Range<usize>,
    /// The `[text]` before it that the mark takes an empty entry from.
    pub brackets: Option<Range<usize>>,
}

impl Terms {
    /// Reads `{^entry!sub|see other}`. An entry left empty is the text in brackets before the mark.
    pub fn read(written: &str, bracketed: Option<&str>) -> Self {
        let inner = written
            .strip_prefix("{^")
            .and_then(|inner| inner.strip_suffix('}'))
            .unwrap_or("");
        let (terms, see) = match inner.find('|') {
            Some(at) => inner.split_at(at),
            None => (inner, ""),
        };
        let mut levels = terms.splitn(3, '!');
        let entry = levels.next().unwrap_or("").trim();
        let sub = levels.next().unwrap_or("").trim();
        let detail = levels.next().map(|detail| format!("!{detail}")).unwrap_or_default();
        Self {
            entry: if entry.is_empty() {
                bracketed.unwrap_or("").trim().to_owned()
            } else {
                entry.to_owned()
            },
            sub: sub.to_owned(),
            rest: format!("{detail}{see}"),
        }
    }

    /// The mark as text. With `bracketed` the same as the entry, the entry stays unwritten, as `[text]{^}` has it.
    fn written(&self, bracketed: Option<&str>) -> String {
        let entry = match bracketed {
            Some(text) if text.trim() == self.entry => "",
            _ => &self.entry,
        };
        if self.sub.is_empty() {
            // A third level cannot stand without the second, so it goes with it.
            let see = self.rest.find('|').map_or("", |at| &self.rest[at..]);
            format!("{{^{entry}{see}}}")
        } else {
            format!("{{^{entry}!{}{}}}", self.sub, self.rest)
        }
    }
}

/// Why a term cannot be written, in a mark where `!`, `|`, and `}` have a meaning.
pub fn problem(entry: &str, sub: &str) -> Option<Key> {
    if entry.trim().is_empty() {
        Some(Key::IndexNeedsEntry)
    } else if [entry, sub].iter().any(|term| term.contains(['!', '|', '}', '\n'])) {
        Some(Key::IndexForbidden)
    } else {
        None
    }
}

/// Whether a mark may stand at `offset`: in running text, outside a heading.
pub fn allowed(blocks: &Blocks, offset: usize) -> bool {
    let kind = blocks.as_slice()[blocks.index_at(offset)].kind;
    has_inline(kind) && !matches!(kind, BlockKind::Heading { .. })
}

/// The marks among a block's spans.
fn marks(spans: &[InlineSpan]) -> impl Iterator<Item = Found> + '_ {
    spans
        .iter()
        .filter(|span| span.kind == SpanKind::Index && span.open.is_empty())
        .map(|span| Found {
            braces: span.range.clone(),
            brackets: spans
                .iter()
                .find(|other| other.kind == SpanKind::Index && other.range.end == span.range.start)
                .map(|other| other.range.clone()),
        })
}

/// Every mark's main entry in `text`, with where its braces start.
pub fn entries(text: &str) -> Vec<(usize, String)> {
    let blocks = Blocks::parse(text);
    let mut found = Vec::new();
    for block in blocks.as_slice() {
        let written = &text[block.range.clone()];
        let spans = inline_spans(block.kind, written);
        for mark in marks(&spans) {
            let bracketed = (mark.brackets.as_ref()).map(|brackets| &written[brackets.start + 1..brackets.end - 1]);
            let terms = Terms::read(&written[mark.braces.clone()], bracketed);
            if !terms.entry.is_empty() {
                found.push((block.range.start + mark.braces.start, terms.entry));
            }
        }
    }
    found
}

/// The mark the caret stands at or in, with where its block starts.
pub fn mark_at(text: &str, blocks: &Blocks, offset: usize) -> Option<(usize, Found)> {
    let block = &blocks.as_slice()[blocks.index_at(offset)];
    let relative = offset - block.range.start;
    let spans = inline_spans(block.kind, &text[block.range.clone()]);
    let found = marks(&spans).find(|found| {
        let start = found
            .brackets
            .as_ref()
            .map_or(found.braces.start, |brackets| brackets.start);
        start <= relative && relative <= found.braces.end
    })?;
    Some((block.range.start, found))
}

/// The entry a new mark starts with: the selected words, or the word at the caret.
pub fn entry_for(text: &str, selection: Selection) -> String {
    let range = if selection.is_empty() {
        motion::word_range(text, selection.head)
    } else {
        selection.range()
    };
    text[range].split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where a new mark for the selection goes: right after its last word, so the mark stays on that word's page.
pub fn place_for(text: &str, selection: Selection) -> usize {
    if selection.is_empty() {
        return motion::word_range(text, selection.head).end;
    }
    let range = selection.range();
    range.start + text[range].trim_end().len()
}

/// Writes a new mark at `at`.
pub fn insert(at: usize, entry: &str, sub: &str) -> Edit {
    let terms = Terms {
        entry: entry.trim().to_owned(),
        sub: sub.trim().to_owned(),
        rest: String::new(),
    };
    Edit::new(at..at, terms.written(None))
}

/// Gives the mark `found`, in the block starting at `start`, a new entry and sub-entry.
pub fn change(text: &str, start: usize, found: &Found, entry: &str, sub: &str) -> Edit {
    let braces = start + found.braces.start..start + found.braces.end;
    let bracketed = found
        .brackets
        .as_ref()
        .map(|brackets| &text[start + brackets.start + 1..start + brackets.end - 1]);
    let terms = Terms {
        entry: entry.trim().to_owned(),
        sub: sub.trim().to_owned(),
        ..Terms::read(&text[braces.clone()], bracketed)
    };
    Edit::new(braces, terms.written(bracketed))
}

/// Takes the mark out. Text in brackets stays, without the brackets.
pub fn remove(start: usize, found: &Found) -> Vec<Edit> {
    let mut edits = Vec::new();
    if let Some(brackets) = &found.brackets {
        edits.push(Edit::new(start + brackets.start..start + brackets.start + 1, ""));
        edits.push(Edit::new(start + brackets.end - 1..start + brackets.end, ""));
    }
    edits.push(Edit::new(start + found.braces.start..start + found.braces.end, ""));
    edits
}

/// The quiet sign that stands for each mark of a block. A mark the caret is inside shows its source.
pub fn chips(text: &str, spans: &[InlineSpan], caret: Option<usize>) -> Vec<Chip> {
    marks(spans)
        .filter(|found| !text[found.braces.clone()].contains('\n'))
        .filter(|found| !caret.is_some_and(|caret| found.braces.start < caret && caret < found.braces.end))
        .map(|found| Chip {
            range: found.braces,
            text: SIGN.to_owned(),
            look: ChipLook::Index,
            atomic: true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands::applied;

    fn at(text: &str, word: &str) -> (Blocks, usize, Found) {
        let blocks = Blocks::parse(text);
        let (start, found) = mark_at(text, &blocks, text.find(word).expect("the place")).expect("a mark");
        (blocks, start, found)
    }

    #[test]
    fn a_selection_is_marked_after_its_last_word_with_its_words_as_the_entry() {
        let text = "Paragraphs are broken as a whole. More.\n";
        let start = text.find("broken").unwrap();
        let selection = Selection {
            anchor: start,
            head: start + "broken as a whole. ".len(),
        };
        assert_eq!(entry_for(text, selection), "broken as a whole.");
        let place = place_for(text, selection);
        let edit = insert(place, "line breaking", " optimal ");
        assert_eq!(
            applied(text, &[edit]),
            "Paragraphs are broken as a whole.{^line breaking!optimal} More.\n"
        );
        // A caret marks the word it touches.
        let caret = Selection::caret(text.find("roken").unwrap());
        assert_eq!(entry_for(text, caret), "broken");
        let edit = insert(place_for(text, caret), "broken", "");
        assert!(applied(text, &[edit]).contains("broken{^broken} as"));
    }

    #[test]
    fn changing_a_mark_keeps_what_the_card_does_not_show() {
        let text = "A hyphen{^hyphenation!rules!German|see also line breaking} here.\n";
        let (_, start, found) = at(text, "{^");
        let terms = Terms::read(&text[found.braces.clone()], None);
        assert_eq!((terms.entry.as_str(), terms.sub.as_str()), ("hyphenation", "rules"));
        let edit = change(text, start, &found, "Hyphens", "rules");
        assert_eq!(
            applied(text, &[edit]),
            "A hyphen{^Hyphens!rules!German|see also line breaking} here.\n"
        );
        let edit = change(text, start, &found, "Hyphens", "");
        assert_eq!(
            applied(text, &[edit]),
            "A hyphen{^Hyphens|see also line breaking} here.\n"
        );
    }

    #[test]
    fn text_in_brackets_is_the_entry_until_another_is_given() {
        let text = "The [Knuth-Plass algorithm]{^} weighs.\n";
        let (_, start, found) = at(text, "Plass");
        let bracketed = &text[found.brackets.clone().unwrap()];
        let terms = Terms::read(&text[found.braces.clone()], Some(&bracketed[1..bracketed.len() - 1]));
        assert_eq!(terms.entry, "Knuth-Plass algorithm");
        let edit = change(text, start, &found, "Knuth-Plass algorithm", "cost");
        assert_eq!(applied(text, &[edit]), "The [Knuth-Plass algorithm]{^!cost} weighs.\n");
        let edit = change(text, start, &found, "Line breaking", "");
        assert_eq!(
            applied(text, &[edit]),
            "The [Knuth-Plass algorithm]{^Line breaking} weighs.\n"
        );
    }

    #[test]
    fn removing_a_mark_leaves_the_sentence_as_it_was() {
        let text = "One{^a!b} and [two words]{^} three.\n";
        let (_, start, found) = at(text, "{^a");
        assert_eq!(applied(text, &remove(start, &found)), "One and [two words]{^} three.\n");
        let (_, start, found) = at(text, "two");
        assert_eq!(
            applied(text, &remove(start, &found)),
            "One{^a!b} and two words three.\n"
        );
    }

    #[test]
    fn a_term_cannot_hold_what_a_mark_reads_as_syntax_and_a_heading_takes_no_mark() {
        assert_eq!(problem("", "x"), Some(Key::IndexNeedsEntry));
        assert_eq!(problem("a|b", ""), Some(Key::IndexForbidden));
        assert_eq!(problem("a", "b!c"), Some(Key::IndexForbidden));
        assert_eq!(problem("Knuth, Donald", "early work"), None);
        let blocks = Blocks::parse("# Title\n\nText.\n");
        assert!(!allowed(&blocks, 3));
        assert!(allowed(&blocks, 10));
    }

    #[test]
    fn a_mark_shows_as_its_sign_until_the_caret_is_inside_it() {
        let text = "One{^a} two.\n";
        let spans = inline_spans(BlockKind::Paragraph, text);
        let shown = chips(text, &spans, Some(0));
        assert_eq!(shown.len(), 1);
        assert_eq!((shown[0].range.clone(), shown[0].text.as_str()), (3..7, SIGN));
        assert!(chips(text, &spans, Some(5)).is_empty());
    }
}
