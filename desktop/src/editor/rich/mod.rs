//! The rich blocks: footnotes, citations and cross-references, tables, and images. Each edits the Markdown through the
//! buffer as one undo step, and writes nothing from a model. The logic is here without gpui, one module per kind of
//! block, and the views are in [`view`].
//!
//! [`Rich`] holds what the views ask of the whole book: what each chapter's text says, read again after a pause, and the
//! engine's text for each chip, read on another thread when anything it depends on has changed. So a key costs a lookup
//! in a table, and plain typing runs no engine at all.

pub mod check;
pub mod citations;
pub mod facts;
pub mod footnotes;
pub mod images;
mod strings;
pub mod styles;
pub mod table;
#[cfg(test)]
mod tests;
pub mod view;

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use druck::editor::rich::{Labels, Work, citation_key};

use self::facts::{Facts, LabelDef};
use self::footnotes::{Keeping, folded};
use self::images::Figure;
pub use self::strings::Key;
use self::table::Region;
use crate::editor::commands::slash::block_insert;
use crate::editor::commands::{Bias, Command, body_range, line_ending, moved};
use crate::editor::display::{Chip, ChipLook};
use crate::session::{ChapterId, Session};
use crate::source::inline::{InlineSpan, SpanKind};
use crate::source::{BlockKind, Blocks, Edit, Selection};

/// A run of blocks that shows as one thing in the galley.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomKind {
    Table(Result<Rc<table::Grid>, Key>),
    Figure(Figure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atom {
    pub first: usize,
    pub last: usize,
    pub kind: AtomKind,
}

impl Atom {
    /// Whether it shows as one thing, which a table the grid cannot read does not.
    pub fn is_shown_whole(&self) -> bool {
        !matches!(self.kind, AtomKind::Table(Err(_)))
    }
}

/// The tables and figures of the active chapter, sorted. Read from the blocks at every change, which costs little: only
/// tables and paragraphs starting with an image are looked into.
pub fn atoms(text: &str, blocks: &Blocks) -> Vec<Atom> {
    let mut atoms: Vec<Atom> = table_atoms(text, blocks);
    for (index, block) in blocks.as_slice().iter().enumerate() {
        if block.kind == BlockKind::Paragraph
            && text[block.range.clone()].starts_with("![")
            && let Some(figure) = Figure::parse(&text[block.range.clone()])
        {
            atoms.push(Atom {
                first: index,
                last: index,
                kind: AtomKind::Figure(figure),
            });
        }
    }
    atoms.sort_by_key(|atom| atom.first);
    atoms
}

fn table_atoms(text: &str, blocks: &Blocks) -> Vec<Atom> {
    if !blocks
        .as_slice()
        .iter()
        .any(|block| matches!(block.kind, BlockKind::Table | BlockKind::Directive))
    {
        return Vec::new();
    }
    table::regions(text, blocks)
        .into_iter()
        .map(|Region { first, last, grid }| Atom {
            first,
            last,
            kind: AtomKind::Table(grid.map(Rc::new)),
        })
        .collect()
}

/// What the book says about footnotes and labels, across its chapters.
#[derive(Debug, Default, Clone)]
pub struct BookIndex {
    /// By folded label: the number a note gets, in the order of the references.
    pub numbers: HashMap<String, usize>,
    /// Folded labels that a reference names.
    pub referenced: HashSet<String>,
    /// By folded label: the chapter and the definition's range.
    pub definitions: HashMap<String, (ChapterId, Range<usize>)>,
    /// Every label written anywhere, as written, so a new one stays unique.
    pub note_labels: Vec<String>,
    pub labels: Vec<LabelDef>,
    pub label_set: HashSet<String>,
    pub keeping: Option<Keeping>,
    pub signature: u64,
}

/// What the galley asks of the book, without gpui.
#[derive(Default)]
pub struct Rich {
    facts: HashMap<ChapterId, (u64, Rc<Facts>)>,
    pub book: BookIndex,
    /// The bibliography's works, and whether the document names one that can be read.
    pub works: Vec<Work>,
    pub has_bibliography: bool,
    bibliography_hash: u64,
    /// The engine's text for chips, from the last time it could read the book.
    pub labels: Labels,
    /// The signature [`Self::labels`] was read for, or is being read for.
    labels_for: Option<u64>,
    /// The chapters of the book when it was last read, since moving one changes every number after it.
    order: Vec<ChapterId>,
    /// The tables and figures of the active chapter.
    pub atoms: Vec<Atom>,
    sizes: HashMap<PathBuf, Result<(f32, f32), String>>,
}

impl Rich {
    /// Reads again what changed in the book. Returns the labels of definitions that lost their last reference.
    pub fn refresh(&mut self, session: &Session) -> Vec<String> {
        let mut changed = self.order != session.book();
        if changed {
            self.order = session.book().to_vec();
        }
        for id in session.book().iter().copied().chain(std::iter::once(session.active())) {
            let Some(chapter) = session.chapter(id) else {
                continue;
            };
            let revision = session.buffer(id).revision();
            if self.facts.get(&id).is_none_or(|(seen, _)| *seen != revision) {
                self.facts.insert(id, (revision, Rc::new(facts::scan(chapter.text()))));
                changed = true;
            }
        }
        let bibliography = session.bibliography_text();
        let mut hasher = DefaultHasher::new();
        bibliography.hash(&mut hasher);
        let bibliography_hash = hasher.finish();
        if bibliography_hash != self.bibliography_hash || (bibliography.is_some() && !self.has_bibliography) {
            self.bibliography_hash = bibliography_hash;
            self.has_bibliography = bibliography.is_some();
            self.works = bibliography.map_or(Vec::new(), |text| druck::editor::rich::works(&text));
            changed = true;
        }
        if !changed && !self.book.numbers.is_empty() {
            return Vec::new();
        }
        let before = std::mem::take(&mut self.book);
        self.book = self.index(session);
        // A definition orphaned by this change: referenced before, defined still, and referenced no more.
        before
            .referenced
            .iter()
            .filter(|label| !self.book.referenced.contains(*label) && self.book.definitions.contains_key(*label))
            .cloned()
            .collect()
    }

    fn index(&self, session: &Session) -> BookIndex {
        let mut index = BookIndex::default();
        let mut hasher = DefaultHasher::new();
        let mut margin = HashSet::new();
        let mut order = session.book().to_vec();
        if !order.contains(&session.active()) {
            order.push(session.active());
        }
        for id in &order {
            let Some((_, facts)) = self.facts.get(id) else {
                continue;
            };
            let text = session.buffer(*id).text();
            for definition in &facts.definitions {
                let label = folded(&definition.label);
                if text[definition.range.clone()].contains("]: {-}") {
                    margin.insert(label.clone());
                }
                index.note_labels.push(definition.label.clone());
                index.definitions.insert(label, (*id, definition.range.clone()));
            }
        }
        let mut keepings = Vec::new();
        for id in &order {
            let Some((_, facts)) = self.facts.get(id) else {
                continue;
            };
            facts.signature.hash(&mut hasher);
            for note in &facts.refs {
                let label = folded(&note.label);
                index.note_labels.push(note.label.clone());
                if index.referenced.insert(label.clone()) && !margin.contains(&label) {
                    let number = index.numbers.len() + 1;
                    index.numbers.insert(label, number);
                }
            }
            for label in &facts.labels {
                index.label_set.insert(label.label.clone());
                index.labels.push(label.clone());
            }
            keepings.push(facts.keeping);
        }
        index.keeping = footnotes::usual_keeping(keepings.into_iter());
        self.bibliography_hash.hash(&mut hasher);
        index.signature = hasher.finish();
        index
    }

    /// Every use of an image in the book, as written.
    pub fn image_uses(&self, session: &Session) -> Vec<(ChapterId, String, usize)> {
        session
            .book()
            .iter()
            .filter_map(|id| self.facts.get(id).map(|(_, facts)| (*id, facts)))
            .flat_map(|(id, facts)| {
                facts
                    .images
                    .iter()
                    .map(move |image| (id, image.path.clone(), image.offset))
            })
            .collect()
    }

    /// Whether the labels need reading again for the book as it stands, and the signature to read them for. Asking
    /// counts as having it: a read under way is not started twice, and one that fails is not retried at every pause.
    pub fn labels_wanted(&mut self) -> Option<u64> {
        let wanted = (self.labels_for != Some(self.book.signature)).then_some(self.book.signature);
        self.labels_for = Some(self.book.signature);
        wanted
    }

    /// Takes the engine's text, and returns whether what chips show may have changed.
    pub fn set_labels(&mut self, labels: Option<Labels>) -> bool {
        match labels {
            Some(labels) if labels != self.labels => {
                self.labels = labels;
                true
            }
            _ => false,
        }
    }

    /// Whether a definition has a reference somewhere in the book, so it needs no block of its own.
    pub fn is_referenced(&self, block_text: &str) -> bool {
        facts::definition_label(block_text).is_some_and(|label| self.book.referenced.contains(&folded(label)))
    }

    /// The chips of a block's spans. A span the caret stands inside shows its source, so it can be typed in. So does a
    /// form without brackets with the caret at its end, since typing it makes one at every key.
    pub fn chips(&self, text: &str, spans: &[InlineSpan], caret: Option<usize>) -> Vec<Chip> {
        let mut chips = Vec::new();
        for span in spans {
            if !matches!(
                span.kind,
                SpanKind::FootnoteReference | SpanKind::Citation | SpanKind::CrossReference
            ) {
                continue;
            }
            let written = &text[span.range.clone()];
            if written.contains('\n') {
                continue;
            }
            let bracketed = written.starts_with('[');
            let inside = caret.is_some_and(|caret| span.range.start < caret && caret < span.range.end);
            let at_end = !bracketed && caret == Some(span.range.end);
            if inside || at_end {
                continue;
            }
            let (shown, look) = match span.kind {
                SpanKind::FootnoteReference => self.note_chip(written),
                SpanKind::Citation => self.citation_chip(written),
                _ => self.reference_chip(written),
            };
            chips.push(Chip {
                range: span.range.clone(),
                // The chip stays on one row.
                text: shown.replace(' ', "\u{a0}"),
                look,
                atomic: bracketed,
            });
        }
        chips
    }

    fn note_chip(&self, written: &str) -> (String, ChipLook) {
        let label = facts::reference_label(written).map(folded).unwrap_or_default();
        if !self.book.definitions.contains_key(&label) {
            return (written.to_owned(), ChipLook::Error);
        }
        let shown = self
            .book
            .numbers
            .get(&label)
            .map_or("*".to_owned(), |number| superscript(*number));
        (shown, ChipLook::Mark)
    }

    fn citation_chip(&self, written: &str) -> (String, ChipLook) {
        let Ok(citation) = citations::read(written) else {
            return (written.to_owned(), ChipLook::Error);
        };
        let known = |key: &str| self.works.iter().any(|work| work.key == key);
        if !self.has_bibliography || citation.items.iter().any(|item| !known(&item.key)) {
            return (written.to_owned(), ChipLook::Error);
        }
        if let Some(label) = self.labels.citations.get(&citation_key(&citation)) {
            return (label.clone(), ChipLook::Label);
        }
        // Until the engine has read it: who and when, from the bibliography.
        let parts: Vec<String> = citation
            .items
            .iter()
            .filter_map(|item| self.works.iter().find(|work| work.key == item.key))
            .map(|work| format!("{} {}", work.authors, work.year).trim().to_owned())
            .collect();
        (format!("({})", parts.join("; ")), ChipLook::Label)
    }

    fn reference_chip(&self, written: &str) -> (String, ChipLook) {
        let Some((label, page)) = citations::read_reference(written) else {
            return (written.to_owned(), ChipLook::Error);
        };
        if !self.book.label_set.contains(label) {
            return (written.to_owned(), ChipLook::Error);
        }
        let target = self
            .labels
            .references
            .get(label)
            .cloned()
            .unwrap_or_else(|| label.to_owned());
        if page {
            (crate::t!(Key::PageOf, target = target), ChipLook::Label)
        } else {
            (target, ChipLook::Label)
        }
    }

    /// What is wrong with a chip's source, for the picker to say.
    pub fn problem_of(&self, written: &str) -> Option<String> {
        if let Some(label) = facts::reference_label(written) {
            return (!self.book.definitions.contains_key(&folded(label)))
                .then(|| crate::t!(Key::UnknownNote, label = label));
        }
        if let Some((label, _)) = citations::read_reference(written).filter(|(label, _)| is_label_form(label)) {
            return (!self.book.label_set.contains(label)).then(|| crate::t!(Key::UnknownLabel, label = label));
        }
        let citation = match citations::read(written) {
            Ok(citation) => citation,
            Err(why) => return Some(why),
        };
        if !self.has_bibliography {
            return Some(crate::t!(Key::NoBibliography).to_owned());
        }
        citation
            .items
            .iter()
            .find(|item| !self.works.iter().any(|work| work.key == item.key))
            .map(|item| crate::t!(Key::UnknownKey, key = item.key))
    }

    /// An image's size for showing it, kept until [`Self::forget_sizes`].
    pub fn image_size(&mut self, file: &Path) -> Result<(f32, f32), String> {
        self.sizes
            .entry(file.to_owned())
            .or_insert_with(|| {
                if file.is_file() {
                    images::size_of(file)
                } else {
                    Err(String::new())
                }
            })
            .clone()
    }

    pub fn forget_sizes(&mut self) {
        self.sizes.clear();
    }
}

fn is_label_form(label: &str) -> bool {
    ["sec:", "fig:", "tbl:"].iter().any(|prefix| label.starts_with(prefix))
}

/// The command that takes `at` out and writes a figure as a block of its own: in place of a block `at` empties, or after
/// the block it stands in. The caret goes to the figure's start.
pub fn image_command(text: &str, blocks: &Blocks, at: Range<usize>, new: &str) -> Command {
    let eol = line_ending(text);
    let index = blocks.index_at(at.start);
    let block = &blocks.as_slice()[index];
    let body = body_range(text, &block.range);
    let rest_empty =
        text[body.start..at.start].trim().is_empty() && text[at.end.min(body.end)..body.end].trim().is_empty();
    if block.kind == BlockKind::Blank || rest_empty {
        let range = if block.kind == BlockKind::Blank {
            at
        } else {
            body.start..body.end.max(at.end)
        };
        let lead = if block.kind == BlockKind::Blank
            && !text[..range.start].is_empty()
            && !text[..range.start].ends_with(&format!("{eol}{eol}"))
        {
            eol
        } else {
            ""
        };
        let trail = if block.kind == BlockKind::Blank && range.end < text.len() {
            eol
        } else {
            ""
        };
        let start = range.start + lead.len();
        return Command {
            edits: vec![Edit::new(range, format!("{lead}{new}{trail}"))],
            selection: Selection::caret(start),
        };
    }
    let (insert, in_insert) = block_insert(text, blocks, index, new, eol);
    let edits = vec![Edit::new(at, ""), insert];
    let start = moved(edits[1].range.start, &edits, Bias::Before) + in_insert;
    Command {
        edits,
        selection: Selection::caret(start),
    }
}

/// A number in superscript digits.
pub fn superscript(number: usize) -> String {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    number
        .to_string()
        .bytes()
        .map(|digit| DIGITS[usize::from(digit - b'0')])
        .collect()
}
