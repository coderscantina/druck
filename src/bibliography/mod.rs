//! BibTeX bibliographies and citations: the reader, the citation syntax in text, and the author-date and numeric
//! styles. Nothing here touches the filesystem or the document; the caller reads the `.bib` file, scans text with
//! [`syntax::find`], feeds citations to [`Citations`], and lays out the [`Reference`]s.
//!
//! Required fields per entry type (an entry type outside this list is an error):
//!
//! | Type | Required | Also used |
//! | --- | --- | --- |
//! | `article` | author, title, journal, year | volume, number, pages, doi, url |
//! | `book` | author or editor, title, publisher, year | edition, address, doi, url |
//! | `inproceedings`, `conference` | author, title, booktitle, year | pages, publisher, address, doi, url |
//! | `phdthesis`, `mastersthesis`, `thesis` | author, title, school (or institution), year | type, doi, url |
//! | `techreport`, `report` | title, institution, year | author, type, number, address, doi, url |
//! | `online`, `misc` | title, url | author, year, publisher (or organization), urldate |
//!
//! Other fields are ignored. `@string` macros and `crossref` are errors.

mod bibtex;
mod cite;
mod entry;
mod format;
mod latex;
mod names;
pub mod syntax;
mod words;

use std::collections::BTreeMap;
use std::path::Path;

use crate::config::source::Source;
use crate::diagnostic::Diagnostic;

pub use cite::{Citations, Part, Reference};
pub use entry::Entry;

use bibtex::Problem;
use entry::Kind;

/// The entries of one `.bib` file, in file order, with unique keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Bibliography {
    source: Source,
    entries: Vec<Entry>,
    index: BTreeMap<String, usize>,
}

impl Bibliography {
    /// Reads and validates a bibliography. All problems are reported together, in file order, each with the path and
    /// the line and column.
    pub fn parse(text: &str, path: &Path) -> Result<Self, Vec<Diagnostic>> {
        let (raw_entries, mut problems) = bibtex::read(text);
        let mut bibliography = Self {
            source: Source::Bibliography(path.to_path_buf()),
            entries: Vec::new(),
            index: BTreeMap::new(),
        };
        for raw in &raw_entries {
            let Some(kind) = Kind::from_type(&raw.kind) else {
                let message = format!("entry `{}` has the unsupported type `@{}`", raw.key, raw.kind);
                problems.push(Problem::new(message, raw.at));
                continue;
            };
            if bibliography.index.contains_key(&raw.key) {
                problems.push(Problem::new(format!("the key `{}` is defined twice", raw.key), raw.at));
                continue;
            }
            match Entry::from_raw(raw, kind) {
                Ok(entry) => {
                    bibliography.index.insert(entry.key.clone(), bibliography.entries.len());
                    bibliography.entries.push(entry);
                }
                Err(entry_problems) => problems.extend(entry_problems),
            }
        }
        if problems.is_empty() {
            return Ok(bibliography);
        }
        problems.sort_by_key(|problem| problem.at);
        let source = &bibliography.source;
        Err(problems
            .into_iter()
            .map(|problem| Diagnostic::new(Some(source.clone()), problem.message).at(problem.at.0, problem.at.1))
            .collect())
    }

    /// The `.bib` file, for diagnostics.
    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The position of a key in [`Self::entries`].
    pub fn index(&self, key: &str) -> Option<usize> {
        self.index.get(key).copied()
    }
}

#[cfg(test)]
mod tests;
