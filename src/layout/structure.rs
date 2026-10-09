//! The document's structure, found before layout: heading, figure, and table numbers, the anchors
//! that links and bookmarks go to, and the text each label is referenced by.
//!
//! Numbers do not depend on pages, so they are final before the first layout pass. Headings marked
//! unnumbered and the headings of front and back matter have no number and do not count. The chapter part of
//! a number, from level 1 headings, is written in `chapters.number-format`. Layout visits
//! headings, images, and tables in the same order as this pass and takes their entries in turn.
//!
//! The bibliography's heading is an unnumbered level 1 heading with the `references` label, so it is in
//! the table of contents, the outline, and running headers like any other. Each entry has an anchor.

use std::collections::HashMap;

use crate::bibliography::Reference;
use crate::config::resolved::{Config, CustomStyle};
use crate::document::{Block, Document, Inline, Location, Matter};

/// Numbers, anchors, and labels of a document.
pub(crate) struct Structure {
    /// Headings of the body in document order.
    pub headings: Vec<Heading>,
    /// Each image of the body in document order: its number and anchor, or `None` without a caption.
    pub figures: Vec<Option<Numbered>>,
    /// Each table in document order, numbered like images.
    pub tables: Vec<Option<Numbered>>,
    /// Where each anchor is defined and what it marks, by anchor number, for diagnostics.
    pub anchors: Vec<(Location, String)>,
    /// The anchor of each bibliography entry, in bibliography order.
    pub entries: Vec<usize>,
    /// Whether `::: toc` places the table of contents.
    pub contents: bool,
    /// The labels that page references point to.
    paged: Vec<String>,
    /// The anchor and reference text of each label.
    labels: HashMap<String, (usize, String)>,
}

pub(crate) struct Heading {
    pub at: Location,
    pub level: u8,
    /// The number, such as `2.1`, if headings of this level are numbered.
    pub number: Option<String>,
    /// The heading text without its number.
    pub text: String,
    pub anchor: usize,
    /// Whether the heading is in the table of contents.
    pub listed: bool,
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
pub(crate) struct Numbered {
    pub number: usize,
    pub anchor: usize,
}

impl Structure {
    /// The structure of `document` with its bibliography section of `references`, if it cites.
    pub fn new(document: &Document, config: &Config, references: &[Reference]) -> Self {
        let mut structure = Self {
            headings: Vec::new(),
            figures: Vec::new(),
            tables: Vec::new(),
            anchors: Vec::new(),
            entries: Vec::new(),
            contents: false,
            paged: Vec::new(),
            labels: HashMap::new(),
        };
        let mut walk = Walk {
            structure: &mut structure,
            config,
            references,
            counters: [0; 6],
            matter: None,
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
            .filter(|heading| self.has_contents(config) && heading.listed && heading.level <= document.toc_depth.get())
            .map(|heading| heading.anchor);
        let references = self.paged();
        let mut anchors: Vec<usize> = contents.chain(references).collect();
        anchors.sort_unstable();
        anchors.dedup();
        anchors
    }

    /// Whether the document has a table of contents, from `toc` or `::: toc`.
    pub fn has_contents(&self, config: &Config) -> bool {
        config.document.toc || self.contents
    }

    /// The anchors that page references point to.
    pub fn paged(&self) -> impl Iterator<Item = usize> {
        self.paged.iter().map(|label| self.labels[label].0)
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
    references: &'a [Reference],
    /// The current number at each heading level.
    counters: [usize; 6],
    /// The part of the book the blocks are in.
    matter: Option<Matter>,
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

    /// Notes only hold references.
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
                    class,
                    unnumbered,
                    unlisted,
                } => {
                    let level = *level;
                    let depth = usize::from(level);
                    let document = &self.config.document;
                    // A heading in a style with a number gap carries the number its author typed.
                    let typed = class.as_ref().is_some_and(|class| {
                        matches!(
                            self.config.custom_styles.get(&class.name),
                            Some(CustomStyle::Heading {
                                number_gap: Some(_),
                                ..
                            })
                        )
                    });
                    let counted = !typed && !unnumbered && matches!(self.matter, None | Some(Matter::Main));
                    let numbered = counted && document.numbered_headings && level <= document.numbering_depth.get();
                    if counted {
                        self.counters[depth - 1] += 1;
                        self.counters[depth..].fill(0);
                    }
                    let format = self.config.chapters.number_format;
                    let number = numbered.then(|| {
                        let parts: Vec<String> = (self.counters[..depth].iter().enumerate())
                            .map(|(index, &count)| match index {
                                0 if count > 0 => format.format(count),
                                _ => count.to_string(),
                            })
                            .collect();
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
                        listed: !unlisted,
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
                        self.blocks(&cell.blocks);
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
                | Block::Bottom { blocks, .. }
                | Block::Columns { blocks, .. }
                | Block::FullWidth { blocks, .. } => self.blocks(blocks),
                Block::Bibliography { at } => {
                    // Without citations there is no section, also where `::: bibliography` asks for one.
                    if self.references.is_empty() {
                        continue;
                    }
                    let text = labels.references.clone();
                    let anchor = self.anchor(*at, format!("the heading \"{text}\""));
                    self.structure.headings.push(Heading {
                        at: *at,
                        level: 1,
                        number: None,
                        text,
                        anchor,
                        listed: true,
                    });
                    for reference in self.references {
                        let anchor = self.anchor(*at, format!("the bibliography entry `{}`", reference.key));
                        self.structure.entries.push(anchor);
                    }
                }
                Block::Matter { matter, .. } => self.matter = Some(*matter),
                Block::Contents { .. } => self.structure.contents = true,
                Block::Code { .. } | Block::PageBreak { .. } | Block::SceneBreak { .. } => {}
            }
        }
    }
}

/// The text of inline content without styles, notes, or line breaks.
pub(crate) fn plain(content: &[Inline]) -> String {
    let mut text = String::new();
    for inline in content {
        match inline {
            Inline::Text { text: piece, .. } => text.push_str(piece),
            Inline::LineBreak => text.push(' '),
            Inline::FootnoteRef(_) | Inline::Ref(_) | Inline::Citation { .. } | Inline::Field { .. } => {}
        }
    }
    text
}
