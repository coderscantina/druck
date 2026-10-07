//! Formats citations and orders the bibliography for one document.
//!
//! Collect every citation in document order with [`Citations::add`], then call [`Citations::finish`]. Two passes are
//! needed because an author-date suffix ("2024a") depends on which works the whole document cites.
//!
//! Author-date, English (German in brackets):
//! - parenthetical: `(Smith 2024, p. 12; Jones and Lee 2023)` (`Jones und Lee`, `S. 12`)
//! - narrative: `Smith (2024, p. 12)`
//! - one author "Smith", two "Smith and Jones", three or more "Smith et al."; without authors the editors, a report's
//!   institution, or the title stand in
//! - two works with the same label and year get a, b, c after the year, in bibliography order
//! - the bibliography sorts by lead (family names with umlauts folded), then year, then title, then key
//!
//! Numeric:
//! - parenthetical: `[1, p. 12]`, grouped `[1–3, 5]`; with locators `[1, p. 12; 3]`
//! - narrative: `Smith [1]`, `Smith and Jones [1, p. 12]`
//! - numbers follow the first citation, and the bibliography keeps that order with the label `[1]`

use std::collections::BTreeMap;

use crate::config::theme::{CitationStyle, Lang};
use crate::document::Inline;

use super::bibtex::Position;
use super::entry::{Entry, Lead};
use super::names::fold;
use super::syntax::{Citation, Form, Locator};
use super::words::words;
use super::{Bibliography, format};

/// A citation key that is not in the bibliography.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingKey {
    pub key: String,
    /// The position of the key's item in [`Citation::items`].
    pub item: usize,
}

/// Identifies an added citation, in the order of [`Citations::add`] calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CitationId(usize);

/// One bibliography entry, ready to lay out.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub key: String,
    /// Where the entry starts in the `.bib` file.
    pub at: Position,
    /// "[1]" in the numeric style.
    pub label: Option<String>,
    pub content: Vec<Inline>,
}

/// A piece of citation text. A linked piece points to the work it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub text: String,
    /// The position of the cited work in [`Rendered::references`].
    pub target: Option<usize>,
}

/// The formatted result for a whole document.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    citations: Vec<Vec<Part>>,
    references: Vec<Reference>,
}

impl Rendered {
    /// The text of a citation in pieces, one linked piece per work it shows.
    pub fn citation(&self, id: CitationId) -> &[Part] {
        &self.citations[id.0]
    }

    /// The cited works in bibliography order.
    #[cfg(test)]
    pub fn references(&self) -> &[Reference] {
        &self.references
    }

    pub fn into_references(self) -> Vec<Reference> {
        self.references
    }
}

/// One cited work with the text around it in the citation.
struct Work {
    /// The entry's index in the bibliography.
    index: usize,
    locator: Option<Locator>,
    prefix: Option<String>,
    suffix: Option<String>,
}

struct Use {
    form: Form,
    items: Vec<Work>,
}

pub struct Citations<'a> {
    bibliography: &'a Bibliography,
    uses: Vec<Use>,
    /// Entry indexes in order of first citation.
    first_cited: Vec<usize>,
}

impl<'a> Citations<'a> {
    pub fn new(bibliography: &'a Bibliography) -> Self {
        Self {
            bibliography,
            uses: Vec::new(),
            first_cited: Vec::new(),
        }
    }

    /// Adds the next citation in document order. Fails, adding nothing, if any key is unknown.
    pub fn add(&mut self, citation: &Citation) -> Result<CitationId, Vec<MissingKey>> {
        let mut items = Vec::new();
        let mut missing = Vec::new();
        for (position, item) in citation.items.iter().enumerate() {
            match self.bibliography.index(&item.key) {
                Some(index) => items.push(Work {
                    index,
                    locator: item.locator.clone(),
                    prefix: item.prefix.clone(),
                    suffix: item.suffix.clone(),
                }),
                None => missing.push(MissingKey {
                    key: item.key.clone(),
                    item: position,
                }),
            }
        }
        if !missing.is_empty() {
            return Err(missing);
        }
        for work in &items {
            if !self.first_cited.contains(&work.index) {
                self.first_cited.push(work.index);
            }
        }
        self.uses.push(Use {
            form: citation.form,
            items,
        });
        Ok(CitationId(self.uses.len() - 1))
    }

    pub fn finish(self, style: CitationStyle, lang: Lang) -> Rendered {
        let entries = self.bibliography.entries();
        let (order, suffixes) = match style {
            CitationStyle::AuthorDate => author_date_order(entries, &self.first_cited, lang),
            CitationStyle::Numeric => (self.first_cited.clone(), vec![String::new(); entries.len()]),
        };
        let mut numbers = vec![0; entries.len()];
        for (position, &index) in order.iter().enumerate() {
            numbers[index] = position + 1;
        }
        let context = Context {
            entries,
            numbers: &numbers,
            suffixes: &suffixes,
            lang,
        };
        let citations = self
            .uses
            .iter()
            .map(|usage| match style {
                CitationStyle::AuthorDate => context.author_date(usage),
                CitationStyle::Numeric => context.numeric(usage),
            })
            .collect();
        let references = order
            .iter()
            .map(|&index| Reference {
                key: entries[index].key.clone(),
                at: entries[index].at,
                label: (style == CitationStyle::Numeric).then(|| format!("[{}]", numbers[index])),
                content: format::reference(&entries[index], lang, &suffixes[index]),
            })
            .collect();
        Rendered { citations, references }
    }
}

/// The text a work is cited by in author-date style, and the text it sorts by.
fn label(entry: &Entry, lang: Lang) -> (String, String) {
    match entry.lead() {
        Lead::Authors(names) | Lead::Editors(names) => (names.short(lang), names.sort_key()),
        Lead::Institution(text) => (text.to_owned(), fold(text)),
        Lead::Title => (entry.title.clone(), fold(&entry.title)),
    }
}

/// The cited entries in bibliography order and each entry's disambiguation suffix, indexed by entry.
fn author_date_order(entries: &[Entry], cited: &[usize], lang: Lang) -> (Vec<usize>, Vec<String>) {
    let sort_key = |index: usize| {
        let entry = &entries[index];
        (
            label(entry, lang).1,
            entry.year.clone(),
            fold(&entry.title),
            entry.key.clone(),
        )
    };
    let mut order = cited.to_vec();
    order.sort_by_key(|&index| sort_key(index));
    let mut groups: BTreeMap<(String, Option<String>), Vec<usize>> = BTreeMap::new();
    for &index in &order {
        groups
            .entry((label(&entries[index], lang).0, entries[index].year.clone()))
            .or_default()
            .push(index);
    }
    let mut suffixes = vec![String::new(); entries.len()];
    for group in groups.values().filter(|group| group.len() > 1) {
        for (n, &index) in group.iter().enumerate() {
            suffixes[index] = letters(n);
        }
    }
    (order, suffixes)
}

/// a, b, ... z, aa, ab, ...
fn letters(mut n: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'a' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII letters")
}

struct Context<'a> {
    entries: &'a [Entry],
    numbers: &'a [usize],
    suffixes: &'a [String],
    lang: Lang,
}

impl Context<'_> {
    /// What follows a work in its citation: the locator and the suffix, joined by a comma.
    fn after(&self, work: &Work) -> Option<String> {
        let locator = work.locator.as_ref().map(|locator| locator.text(self.lang));
        let parts: Vec<_> = locator.into_iter().chain(work.suffix.clone()).collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    fn author_date(&self, usage: &Use) -> Vec<Part> {
        let no_date = words(self.lang).no_date;
        let item = |work: &Work| {
            let entry = &self.entries[work.index];
            let year = format!(
                "{}{}",
                entry.year.as_deref().unwrap_or(no_date),
                self.suffixes[work.index]
            );
            match usage.form {
                Form::Parenthetical => {
                    let parts = [Some(format!("{} {year}", label(entry, self.lang).0)), self.after(work)];
                    parts.into_iter().flatten().collect::<Vec<_>>().join(", ")
                }
                Form::Narrative => {
                    let inner = [Some(year), self.after(work)]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{} ({inner})", label(entry, self.lang).0)
                }
            }
        };
        let parenthetical = usage.form == Form::Parenthetical;
        let mut parts = Parts::default();
        if parenthetical {
            parts.text("(");
        }
        for (position, work) in usage.items.iter().enumerate() {
            if position > 0 {
                parts.text(if parenthetical { "; " } else { ", " });
            }
            parts.prefix(work);
            parts.link(item(work), self.numbers[work.index] - 1);
        }
        if parenthetical {
            parts.text(")");
        }
        parts.0
    }

    fn numeric(&self, usage: &Use) -> Vec<Part> {
        let mut items: Vec<&Work> = usage.items.iter().collect();
        // Prefixes such as "see" and "also" refer to the order the author wrote.
        if items.iter().all(|work| work.prefix.is_none()) {
            items.sort_by_key(|work| self.numbers[work.index]);
        }
        let number_and_after = |work: &Work| match self.after(work) {
            Some(after) => format!("{}, {after}", self.numbers[work.index]),
            None => self.numbers[work.index].to_string(),
        };
        let mut parts = Parts::default();
        match usage.form {
            Form::Parenthetical
                if items
                    .iter()
                    .all(|work| work.locator.is_none() && work.prefix.is_none() && work.suffix.is_none()) =>
            {
                let numbers: Vec<usize> = items.iter().map(|work| self.numbers[work.index]).collect();
                parts.text("[");
                for (position, (first, last)) in runs(&numbers).into_iter().enumerate() {
                    if position > 0 {
                        parts.text(", ");
                    }
                    parts.link(first.to_string(), first - 1);
                    if last > first {
                        parts.text("–");
                        parts.link(last.to_string(), last - 1);
                    }
                }
                parts.text("]");
            }
            Form::Parenthetical => {
                parts.text("[");
                for (position, work) in items.iter().enumerate() {
                    if position > 0 {
                        parts.text("; ");
                    }
                    parts.prefix(work);
                    parts.link(number_and_after(work), self.numbers[work.index] - 1);
                }
                parts.text("]");
            }
            Form::Narrative => {
                for (position, work) in usage.items.iter().enumerate() {
                    if position > 0 {
                        parts.text(", ");
                    }
                    let label = label(&self.entries[work.index], self.lang).0;
                    let text = format!("{label} [{}]", number_and_after(work));
                    parts.link(text, self.numbers[work.index] - 1);
                }
            }
        }
        parts.0
    }
}

/// Pieces of citation text under construction.
#[derive(Default)]
struct Parts(Vec<Part>);

impl Parts {
    fn text(&mut self, text: &str) {
        self.0.push(Part {
            text: text.to_owned(),
            target: None,
        });
    }

    /// The text before a work, if the author wrote one, and a space.
    fn prefix(&mut self, work: &Work) {
        if let Some(prefix) = &work.prefix {
            self.text(&format!("{prefix} "));
        }
    }

    fn link(&mut self, text: String, target: usize) {
        self.0.push(Part {
            text,
            target: Some(target),
        });
    }
}

/// Groups sorted numbers into `(first, last)` runs, collapsing three or more consecutive numbers into one
/// run and keeping the others single: 1, 2, 3, 5 becomes `(1, 3), (5, 5)`.
pub(super) fn runs(numbers: &[usize]) -> Vec<(usize, usize)> {
    let mut unique = numbers.to_vec();
    unique.dedup();
    let mut runs = Vec::new();
    let mut start = 0;
    while start < unique.len() {
        let mut end = start;
        while end + 1 < unique.len() && unique[end + 1] == unique[end] + 1 {
            end += 1;
        }
        if end - start >= 2 {
            runs.push((unique[start], unique[end]));
        } else {
            runs.extend(unique[start..=end].iter().map(|&number| (number, number)));
        }
        start = end + 1;
    }
    runs
}
