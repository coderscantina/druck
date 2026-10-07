//! The citations of a parsed document, formatted with its bibliography in the configured style.
//!
//! Citations are added in reading order: the body in order, with a footnote's content at its reference.
//! Numeric labels and the order of author-date suffixes follow it.

use crate::bibliography::{Bibliography, Citations, Part, Reference};
use crate::config::resolved::Config;
use crate::config::source::Source;
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Document, Inline, Location};

/// The formatted citations of a document and the works they cite.
#[derive(Debug, Default)]
pub struct Cited {
    /// The text of each of [`Document::citations`] in pieces. A piece links to the entry of the work it shows,
    /// given as an index into the section's references.
    pub citations: Vec<Vec<Part>>,
    /// `None` for a document without citations.
    pub section: Option<Section>,
}

/// The cited works in bibliography order, with their `.bib` file for diagnostics.
#[derive(Debug)]
pub struct Section {
    pub source: Source,
    pub references: Vec<Reference>,
}

impl Cited {
    pub fn references(&self) -> &[Reference] {
        self.section.as_ref().map_or(&[], |section| &section.references)
    }
}

/// Formats the citations of `document`, read from `source`. Citations without a bibliography and keys the
/// bibliography lacks are errors at the citation and the key.
pub fn resolve(
    document: &Document,
    bibliography: Option<&Bibliography>,
    config: &Config,
    source: &Source,
) -> Result<Cited, Vec<Diagnostic>> {
    let Some(first) = document.citations.first() else {
        return Ok(Cited::default());
    };
    let error = |at: Location, message: String| Diagnostic::new(Some(source.clone()), message).at(at.line, at.column);
    let Some(bibliography) = bibliography else {
        let message = "a citation needs a bibliography; set `bibliography` to a BibTeX file in the front matter";
        return Err(vec![error(first.at, message.to_owned())]);
    };

    let mut order = Vec::with_capacity(document.citations.len());
    reading_order(&document.blocks, document, &mut order);
    let mut citations = Citations::new(bibliography);
    let mut ids = vec![None; document.citations.len()];
    let mut errors = Vec::new();
    for index in order {
        let citation = &document.citations[index];
        match citations.add(&citation.syntax) {
            Ok(id) => ids[index] = Some(id),
            Err(missing) => errors.extend(missing.into_iter().map(|missing| {
                let message = format!("no entry in {} has the key `{}`", bibliography.source(), missing.key);
                error(citation.keys[missing.item], message)
            })),
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|diagnostic| diagnostic.location);
        return Err(errors);
    }

    let rendered = citations.finish(config.document.citation_style, config.document.lang);
    let parts = ids
        .into_iter()
        .map(|id| {
            rendered
                .citation(id.expect("every citation is in the reading order"))
                .to_vec()
        })
        .collect();
    Ok(Cited {
        citations: parts,
        section: Some(Section {
            source: bibliography.source().clone(),
            references: rendered.into_references(),
        }),
    })
}

/// Adds the indexes of the citations in `blocks` in reading order, with a footnote's at its reference.
fn reading_order(blocks: &[Block], document: &Document, order: &mut Vec<usize>) {
    let inlines = |content: &[Inline], order: &mut Vec<usize>| {
        for inline in content {
            match inline {
                Inline::Citation { index, .. } => order.push(*index),
                Inline::FootnoteRef(note) => reading_order(&document.footnotes[*note].blocks, document, order),
                Inline::Text { .. } | Inline::LineBreak | Inline::Ref(_) => {}
            }
        }
    };
    for block in blocks {
        match block {
            Block::Paragraph { content, .. } | Block::Heading { content, .. } => inlines(content, order),
            Block::Image { caption, .. } => inlines(caption, order),
            Block::Table {
                header, rows, caption, ..
            } => {
                inlines(caption, order);
                for cell in header.cells.iter().chain(rows.iter().flat_map(|row| &row.cells)) {
                    reading_order(&cell.blocks, document, order);
                }
            }
            Block::List { items, .. } => items.iter().for_each(|item| reading_order(item, document, order)),
            Block::Quote { blocks, .. }
            | Block::Keep { blocks, .. }
            | Block::Columns { blocks, .. }
            | Block::FullWidth { blocks, .. } => reading_order(blocks, document, order),
            Block::Code { .. } | Block::PageBreak { .. } | Block::Bibliography { .. } => {}
        }
    }
}
