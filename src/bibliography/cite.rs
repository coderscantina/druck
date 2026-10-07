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
use std::ops::Range;

use crate::config::theme::{CitationStyle, Lang};
use crate::document::Inline;

use super::entry::{Entry, Lead};
use super::names::fold;
use super::syntax::{Citation, Form, Locator};
use super::words::words;
use super::{Bibliography, format};

/// A citation key that is not in the bibliography.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingKey {
    pub key: String,
    /// Where the key is written, as in [`super::syntax::Item::range`].
    pub range: Range<usize>,
}

/// Identifies an added citation, in the order of [`Citations::add`] calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CitationId(usize);

/// One bibliography entry, ready to lay out.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub key: String,
    /// "[1]" in the numeric style.
    pub label: Option<String>,
    pub content: Vec<Inline>,
}

/// The formatted result for a whole document.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    texts: Vec<String>,
    references: Vec<Reference>,
}

impl Rendered {
    pub fn citation(&self, id: CitationId) -> &str {
        &self.texts[id.0]
    }

    /// The cited works in bibliography order.
    pub fn references(&self) -> &[Reference] {
        &self.references
    }
}

struct Use {
    form: Form,
    items: Vec<(usize, Option<Locator>)>,
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
        for item in &citation.items {
            match self.bibliography.index(&item.key) {
                Some(index) => items.push((index, item.locator.clone())),
                None => missing.push(MissingKey {
                    key: item.key.clone(),
                    range: item.range.clone(),
                }),
            }
        }
        if !missing.is_empty() {
            return Err(missing);
        }
        for &(index, _) in &items {
            if !self.first_cited.contains(&index) {
                self.first_cited.push(index);
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
        let texts = self
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
                label: (style == CitationStyle::Numeric).then(|| format!("[{}]", numbers[index])),
                content: format::reference(&entries[index], lang, &suffixes[index]),
            })
            .collect();
        Rendered { texts, references }
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
    fn locator(&self, locator: &Option<Locator>) -> Option<String> {
        locator.as_ref().map(|locator| locator.text(self.lang))
    }

    fn author_date(&self, usage: &Use) -> String {
        let no_date = words(self.lang).no_date;
        let item = |&(index, ref locator): &(usize, Option<Locator>)| {
            let entry = &self.entries[index];
            let year = format!("{}{}", entry.year.as_deref().unwrap_or(no_date), self.suffixes[index]);
            let year_and_locator = [Some(year.clone()), self.locator(locator)]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            match usage.form {
                Form::Parenthetical => {
                    let parts = [
                        Some(format!("{} {year}", label(entry, self.lang).0)),
                        self.locator(locator),
                    ];
                    parts.into_iter().flatten().collect::<Vec<_>>().join(", ")
                }
                Form::Narrative => format!("{} ({})", label(entry, self.lang).0, year_and_locator.join(", ")),
            }
        };
        let items: Vec<String> = usage.items.iter().map(item).collect();
        match usage.form {
            Form::Parenthetical => format!("({})", items.join("; ")),
            Form::Narrative => items.join(", "),
        }
    }

    fn numeric(&self, usage: &Use) -> String {
        let mut items: Vec<(usize, &Option<Locator>)> = usage
            .items
            .iter()
            .map(|(index, locator)| (self.numbers[*index], locator))
            .collect();
        items.sort_by_key(|&(number, _)| number);
        let number_and_locator = |&(number, locator): &(usize, &Option<Locator>)| match self.locator(locator) {
            Some(locator) => format!("{number}, {locator}"),
            None => number.to_string(),
        };
        match usage.form {
            Form::Parenthetical if items.iter().all(|(_, locator)| locator.is_none()) => {
                let numbers: Vec<usize> = items.iter().map(|&(number, _)| number).collect();
                format!("[{}]", group_numbers(&numbers))
            }
            Form::Parenthetical => {
                let items: Vec<String> = items.iter().map(number_and_locator).collect();
                format!("[{}]", items.join("; "))
            }
            Form::Narrative => {
                let items: Vec<String> = usage
                    .items
                    .iter()
                    .map(|(index, locator)| {
                        let label = label(&self.entries[*index], self.lang).0;
                        format!("{label} [{}]", number_and_locator(&(self.numbers[*index], locator)))
                    })
                    .collect();
                items.join(", ")
            }
        }
    }
}

/// Joins sorted numbers, collapsing runs of three or more into a range: 1, 2, 3, 5 becomes "1–3, 5".
pub(super) fn group_numbers(numbers: &[usize]) -> String {
    let mut unique = numbers.to_vec();
    unique.dedup();
    let mut parts = Vec::new();
    let mut start = 0;
    while start < unique.len() {
        let mut end = start;
        while end + 1 < unique.len() && unique[end + 1] == unique[end] + 1 {
            end += 1;
        }
        if end - start >= 2 {
            parts.push(format!("{}–{}", unique[start], unique[end]));
        } else {
            parts.extend(unique[start..=end].iter().map(usize::to_string));
        }
        start = end + 1;
    }
    parts.join(", ")
}
