//! The citations of a parsed document, formatted with its bibliography in the configured style.
//!
//! Citations are added in reading order: the body in order, with a footnote's content at its reference.
//! Numeric labels and the order of author-date suffixes follow it.

use crate::bibliography::{Bibliography, Citations, Part, Reference};
use crate::cancel::{Cancel, CheckPoint};
use crate::config::resolved::Config;
use crate::config::source::InputSource;
use crate::diagnostic::{Code, Diagnostic, Severity};
use crate::document::{Block, Document, Inline, Location};
use crate::renderer::{Failure, Missing};

/// The formatted citations of a document and the works they cite.
#[derive(Debug, Default)]
pub struct Cited {
    /// The text of each of [`Document::citations`] in pieces. A piece links to the entry of the work it shows,
    /// given as an index into the section's references.
    pub citations: Vec<Vec<Part>>,
    /// `None` for a document without citations.
    pub section: Option<Section>,
    /// With [`Missing::StandIn`], a warning for each key the bibliography lacks, at the key.
    pub warnings: Vec<Diagnostic>,
}

/// The cited works in bibliography order, with their `.bib` file for diagnostics.
#[derive(Debug)]
pub struct Section {
    pub source: InputSource,
    pub references: Vec<Reference>,
}

impl Cited {
    pub fn references(&self) -> &[Reference] {
        self.section.as_ref().map_or(&[], |section| &section.references)
    }
}

/// Formats the citations of `document`, read from `source`. Citations without a bibliography and keys the
/// bibliography lacks are errors at the citation and the key. With `unresolved`, the bibliography is named but its file
/// is not there, which was reported as it was read, so each citation shows its keys and links nowhere. With
/// [`Missing::StandIn`], a citation with a key the bibliography lacks does the same and warns at the key.
pub fn resolve(
    document: &Document,
    bibliography: Option<&Bibliography>,
    unresolved: bool,
    missing: Missing,
    config: &Config,
    source: &InputSource,
    cancel: &Cancel,
) -> Result<Cited, Failure> {
    let Some(first) = document.citations.first() else {
        return Ok(Cited::default());
    };
    let error = |at: Location, message: String| Diagnostic::new(Some(source.clone()), message).at(at.line, at.column);
    if bibliography.is_none() && unresolved {
        return Ok(Cited {
            citations: document.citations.iter().map(keys_only).collect(),
            section: None,
            warnings: Vec::new(),
        });
    }
    let Some(bibliography) = bibliography else {
        let message = "a citation needs a bibliography; set `bibliography` to a BibTeX file in the front matter";
        return Err(vec![error(first.at, message.to_owned()).coded(Code::NoBibliography)].into());
    };

    let mut order = Vec::with_capacity(document.citations.len());
    reading_order(&document.blocks, document, &mut order);
    let mut citations = Citations::new(bibliography);
    let mut ids = vec![None; document.citations.len()];
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for index in order {
        cancel.check(CheckPoint::Citation)?;
        let citation = &document.citations[index];
        match citations.add(&citation.syntax) {
            Ok(id) => ids[index] = Some(id),
            Err(lacking) => {
                for lacking in lacking {
                    let file = bibliography.path();
                    let message = format!("no entry in {} has the key `{}`", file.display(), lacking.key);
                    let code = Code::MissingCitation {
                        file: file.to_owned(),
                        key: lacking.key,
                    };
                    let mut diagnostic = error(citation.keys[lacking.item], message).coded(code).naming(file);
                    match missing {
                        Missing::Error => errors.push(diagnostic),
                        Missing::StandIn => {
                            diagnostic.severity = Severity::Warning;
                            warnings.push(diagnostic);
                        }
                    }
                }
            }
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|diagnostic| diagnostic.position);
        return Err(errors.into());
    }

    let rendered = citations.finish(config.document.citation_style, config.document.lang);
    warnings.sort_by_key(|diagnostic| diagnostic.position);
    let parts = (ids.into_iter().zip(&document.citations))
        .map(|(id, citation)| match id {
            Some(id) => rendered.citation(id).to_vec(),
            None => keys_only(citation),
        })
        .collect();
    Ok(Cited {
        citations: parts,
        section: Some(Section {
            source: bibliography.source(),
            references: rendered.into_references(),
        }),
        warnings,
    })
}

/// A citation as its keys alone, such as `[?smith2020; ?jones]`, for a bibliography that is not there.
fn keys_only(citation: &crate::document::Citation) -> Vec<Part> {
    let keys: Vec<String> = (citation.syntax.items.iter())
        .map(|item| format!("?{}", item.key))
        .collect();
    vec![Part {
        text: format!("[{}]", keys.join("; ")),
        target: None,
    }]
}

/// Adds the indexes of the citations in `blocks` in reading order, with a footnote's at its reference.
pub(crate) fn reading_order(blocks: &[Block], document: &Document, order: &mut Vec<usize>) {
    let inlines = |content: &[Inline], order: &mut Vec<usize>| {
        for inline in content {
            match inline {
                Inline::Citation { index, .. } => order.push(*index),
                Inline::FootnoteRef(note) => reading_order(&document.footnotes[*note].blocks, document, order),
                Inline::MarginNote(note) => reading_order(&document.margin_notes[*note].blocks, document, order),
                Inline::Text { .. }
                | Inline::LineBreak
                | Inline::Ref(_)
                | Inline::Field { .. }
                | Inline::IndexMark(_)
                | Inline::Anchor(_) => {}
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
            | Block::Callout { blocks, .. }
            | Block::Keep { blocks, .. }
            | Block::Bottom { blocks, .. }
            | Block::Columns { blocks, .. }
            | Block::FullWidth { blocks, .. } => reading_order(blocks, document, order),
            Block::Code { .. }
            | Block::PageBreak { .. }
            | Block::Matter { .. }
            | Block::Contents { .. }
            | Block::SceneBreak { .. }
            | Block::Bibliography { .. }
            | Block::Index { .. } => {}
        }
    }
}
