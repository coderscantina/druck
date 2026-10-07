//! The document's structure, found before layout: heading, figure, and table numbers, the anchors
//! that links and bookmarks go to, and the text each label is referenced by.
//!
//! Numbers do not depend on pages, so they are final before the first layout pass. Layout visits
//! headings, images, and tables in the same order as this pass and takes their entries in turn.

use std::collections::HashMap;

use crate::config::resolved::Config;
use crate::document::{Block, Document, Inline, Location};

/// Numbers, anchors, and labels of a document.
pub(super) struct Structure {
    /// Headings of the body in document order. Headings inside footnotes are not part of the structure.
    pub headings: Vec<Heading>,
    /// Each image of the body in document order: its number and anchor, or `None` without a caption.
    pub figures: Vec<Option<Numbered>>,
    /// Each table in document order, numbered like images.
    pub tables: Vec<Option<Numbered>>,
    /// Where each anchor is defined and what it marks, by anchor number, for diagnostics.
    pub anchors: Vec<(Location, String)>,
    /// The labels that page references point to.
    paged: Vec<String>,
    /// The anchor and reference text of each label.
    labels: HashMap<String, (usize, String)>,
}

pub(super) struct Heading {
    pub at: Location,
    pub level: u8,
    /// The number, such as `2.1`, if headings of this level are numbered.
    pub number: Option<String>,
    /// The heading text without its number.
    pub text: String,
    pub anchor: usize,
}

impl Heading {
    /// The number and the text, as in TOC entries, bookmarks, and running headers.
    pub fn title(&self) -> String {
        match &self.number {
            Some(number) => format!("{number} {}", self.text),
            None => self.text.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Numbered {
    pub number: usize,
    pub anchor: usize,
}

impl Structure {
    pub fn new(document: &Document, config: &Config) -> Self {
        let mut structure = Self {
            headings: Vec::new(),
            figures: Vec::new(),
            tables: Vec::new(),
            anchors: Vec::new(),
            paged: Vec::new(),
            labels: HashMap::new(),
        };
        let mut walk = Walk {
            structure: &mut structure,
            config,
            counters: [0; 6],
        };
        walk.blocks(&document.blocks);
        for footnote in &document.footnotes {
            walk.references(&footnote.blocks);
        }
        structure
    }

    /// The anchors whose page numbers the document shows: headings in the table of contents and the
    /// targets of page references. Running headers and footers do not count, since they never change
    /// the layout.
    pub fn shown_pages(&self, config: &Config) -> Vec<usize> {
        let document = &config.document;
        let contents = self
            .headings
            .iter()
            .filter(|heading| document.toc && heading.level <= document.toc_depth.get())
            .map(|heading| heading.anchor);
        let references = self.paged.iter().map(|label| self.labels[label].0);
        let mut anchors: Vec<usize> = contents.chain(references).collect();
        anchors.sort_unstable();
        anchors.dedup();
        anchors
    }

    /// The anchor of `label` and the text a reference to it shows: the label of its kind and its
    /// number, or the text of an unnumbered heading. The parser has checked that every label exists.
    pub fn reference(&self, label: &str) -> (usize, &str) {
        let (anchor, text) = &self.labels[label];
        (*anchor, text)
    }
}

struct Walk<'a> {
    structure: &'a mut Structure,
    config: &'a Config,
    /// The current number at each heading level.
    counters: [usize; 6],
}

impl Walk<'_> {
    fn anchor(&mut self, at: Location, what: String) -> usize {
        self.structure.anchors.push((at, what));
        self.structure.anchors.len() - 1
    }

    fn label(&mut self, label: Option<&String>, anchor: usize, text: String) {
        if let Some(label) = label {
            self.structure.labels.insert(label.clone(), (anchor, text));
        }
    }

    fn inlines(&mut self, content: &[Inline]) {
        for inline in content {
            if let Inline::Ref(reference) = inline
                && reference.page
            {
                self.structure.paged.push(reference.label.clone());
            }
        }
    }

    /// Notes only hold references; their headings, if any, are set unnumbered.
    fn references(&mut self, blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::Paragraph { content, .. } => self.inlines(content),
                Block::List { items, .. } => items.iter().for_each(|item| self.references(item)),
                Block::Quote { blocks, .. } => self.references(blocks),
                _ => {}
            }
        }
    }

    fn blocks(&mut self, blocks: &[Block]) {
        let labels = &self.config.labels;
        for block in blocks {
            match block {
                Block::Paragraph { content, .. } => self.inlines(content),
                Block::Heading {
                    at,
                    level,
                    content,
                    label,
                } => {
                    let level = *level;
                    let depth = usize::from(level);
                    self.counters[depth - 1] += 1;
                    self.counters[depth..].fill(0);
                    let document = &self.config.document;
                    let number = (document.numbered_headings && level <= document.numbering_depth.get()).then(|| {
                        let parts: Vec<String> = self.counters[..depth].iter().map(usize::to_string).collect();
                        parts.join(".")
                    });
                    let text = plain(content);
                    let anchor = self.anchor(*at, format!("the heading \"{text}\""));
                    let reference = match &number {
                        Some(number) => format!("{}\u{a0}{number}", labels.section),
                        None => text.clone(),
                    };
                    self.label(label.as_ref(), anchor, reference);
                    self.structure.headings.push(Heading {
                        at: *at,
                        level,
                        number,
                        text,
                        anchor,
                    });
                }
                Block::Image { at, caption, label, .. } => {
                    self.inlines(caption);
                    let figure = (!caption.is_empty()).then(|| {
                        let number = self.structure.figures.iter().flatten().count() + 1;
                        let text = format!("{}\u{a0}{number}", labels.figure);
                        let anchor = self.anchor(*at, text.clone());
                        self.label(label.as_ref(), anchor, text);
                        Numbered { number, anchor }
                    });
                    self.structure.figures.push(figure);
                }
                Block::Table {
                    at,
                    header,
                    rows,
                    caption,
                    label,
                    ..
                } => {
                    self.inlines(caption);
                    for cell in header.cells.iter().chain(rows.iter().flat_map(|row| &row.cells)) {
                        self.inlines(&cell.content);
                    }
                    let table = (!caption.is_empty()).then(|| {
                        let number = self.structure.tables.iter().flatten().count() + 1;
                        let text = format!("{}\u{a0}{number}", labels.table);
                        let anchor = self.anchor(*at, text.clone());
                        self.label(label.as_ref(), anchor, text);
                        Numbered { number, anchor }
                    });
                    self.structure.tables.push(table);
                }
                Block::List { items, .. } => items.iter().for_each(|item| self.blocks(item)),
                Block::Quote { blocks, .. }
                | Block::Keep { blocks, .. }
                | Block::Columns { blocks, .. }
                | Block::FullWidth { blocks, .. } => self.blocks(blocks),
                Block::Code { .. } | Block::PageBreak { .. } => {}
            }
        }
    }
}

/// The text of inline content without styles, notes, or line breaks.
pub(super) fn plain(content: &[Inline]) -> String {
    let mut text = String::new();
    for inline in content {
        match inline {
            Inline::Text { text: piece, .. } => text.push_str(piece),
            Inline::LineBreak => text.push(' '),
            Inline::FootnoteRef(_) | Inline::Ref(_) => {}
        }
    }
    text
}
