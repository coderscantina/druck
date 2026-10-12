//! The back-of-book index: entries from the document's index marks, sorted and grouped under letters, and
//! the page locators of an entry, with runs of pages merged into ranges.
//!
//! Terms sort the same way in English and German, following DIN 5007-1: a letter with a diacritic sorts as
//! its base letter (ä as a, ö as o, ü as u, é as e), ß as ss, æ and œ as ae and oe, and case does not count.
//! Spaces and punctuation sort before letters, so entries sort word by word. Terms that are equal by these
//! rules put the one without diacritics first, then lowercase before capitals, then compare their characters,
//! so the order never depends on the input order. Terms that start with something other than a letter form a
//! group before the letters.

use std::collections::{BTreeMap, HashSet};

use crate::document::{Block, Document, Inline};

/// The entries of an index, grouped by their first letter.
#[derive(Debug, PartialEq)]
pub struct Index {
    pub groups: Vec<Group>,
}

#[derive(Debug, PartialEq)]
pub struct Group {
    /// The capital letter the group's terms start with; `None` for terms that start with a digit or symbol.
    pub letter: Option<String>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Entry {
    pub term: String,
    /// The marks that show a page for this entry, as indexes into [`Document::index_marks`], in reading order.
    pub marks: Vec<usize>,
    /// The entries this one refers to with `see` and with `see also`, each its terms joined by ", ".
    pub see: Vec<String>,
    pub see_also: Vec<String>,
    pub sub_entries: Vec<Entry>,
}

impl Index {
    /// The index of the marks in `document`, or `None` for a document without any.
    pub fn new(document: &Document) -> Option<Self> {
        if document.index_marks.is_empty() {
            return None;
        }
        let mut order = Vec::with_capacity(document.index_marks.len());
        reading_order(&document.blocks, document, &mut order);
        let mut root = Node::default();
        for index in order {
            let mark = &document.index_marks[index];
            let node = (mark.terms.iter()).fold(&mut root, |node, term| node.children.entry(term.clone()).or_default());
            match &mark.see {
                Some(see) => {
                    let target = see.terms.join(", ");
                    let list = if see.also { &mut node.see_also } else { &mut node.see };
                    if !list.contains(&target) {
                        list.push(target);
                    }
                }
                None => node.marks.push(index),
            }
        }
        let mut groups: Vec<Group> = Vec::new();
        for entry in entries(root.children) {
            let letter = letter(&entry.term);
            match groups.last_mut() {
                Some(group) if group.letter == letter => group.entries.push(entry),
                _ => groups.push(Group {
                    letter,
                    entries: vec![entry],
                }),
            }
        }
        Some(Self { groups })
    }
}

#[derive(Default)]
struct Node {
    marks: Vec<usize>,
    see: Vec<String>,
    see_also: Vec<String>,
    children: BTreeMap<String, Node>,
}

/// The entries of `nodes` in index order.
fn entries(nodes: BTreeMap<String, Node>) -> Vec<Entry> {
    let mut entries: Vec<(Key, Entry)> = nodes
        .into_iter()
        .map(|(term, node)| {
            let entry = Entry {
                marks: node.marks,
                see: node.see,
                see_also: node.see_also,
                sub_entries: self::entries(node.children),
                term,
            };
            (Key::new(&entry.term), entry)
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.into_iter().map(|(_, entry)| entry).collect()
}

/// The sort key of a term. Fields compare in order.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    /// Whether the term starts with a letter; terms that do not come first.
    letter: bool,
    /// The term in lowercase base letters.
    base: String,
    /// Which characters carry a diacritic or are a ligature.
    marked: Vec<bool>,
    /// Which characters are capitals.
    capitals: Vec<bool>,
    term: String,
}

impl Key {
    fn new(term: &str) -> Self {
        let mut base = String::with_capacity(term.len());
        let (mut marked, mut capitals) = (Vec::new(), Vec::new());
        for character in term.chars() {
            capitals.push(character.is_uppercase());
            let mut is_marked = false;
            for lower in character.to_lowercase() {
                match fold(lower) {
                    Some(folded) => {
                        base.push_str(folded);
                        is_marked = true;
                    }
                    None => base.push(lower),
                }
            }
            marked.push(is_marked);
        }
        Self {
            letter: base.starts_with(char::is_alphabetic),
            base,
            marked,
            capitals,
            term: term.to_owned(),
        }
    }
}

/// The capital a term is grouped under, or `None` if it does not start with a letter.
fn letter(term: &str) -> Option<String> {
    let first = term.chars().next()?.to_lowercase().next()?;
    let base = fold(first).and_then(|folded| folded.chars().next()).unwrap_or(first);
    base.is_alphabetic().then(|| base.to_uppercase().collect())
}

/// The base letters of a lowercase Latin letter with a diacritic, or of a ligature; `None` for others.
fn fold(character: char) -> Option<&'static str> {
    Some(match character {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'þ' => "th",
        _ => return None,
    })
}

/// A page or a range of pages of an entry, each with what its number links to.
#[derive(Debug, Clone, PartialEq)]
pub struct Locator<T> {
    pub first: (String, T),
    /// The last page of a range.
    pub last: Option<(String, T)>,
}

/// The locators of an entry's pages, given as the number each mark's page shows and its link target, in
/// reading order. A page counts once, with the target of its first mark. Runs of three or more consecutive
/// pages of the same numbering, as in 3, 4, 5 or iv, v, vi, become a range, as numeric citations do.
pub fn locators<T: Copy>(pages: &[(String, T)]) -> Vec<Locator<T>> {
    let mut seen = HashSet::new();
    let unique: Vec<&(String, T)> = pages.iter().filter(|(page, _)| seen.insert(page.as_str())).collect();
    let mut locators = Vec::new();
    let mut start = 0;
    while start < unique.len() {
        let mut end = start;
        while end + 1 < unique.len() && follows(&unique[end].0, &unique[end + 1].0) {
            end += 1;
        }
        if end - start >= 2 {
            locators.push(Locator {
                first: unique[start].clone(),
                last: Some(unique[end].clone()),
            });
        } else {
            locators.extend(unique[start..=end].iter().map(|&page| Locator {
                first: page.clone(),
                last: None,
            }));
        }
        start = end + 1;
    }
    locators
}

/// Whether page number `next` directly follows `page` in the same numbering.
fn follows(page: &str, next: &str) -> bool {
    match (value(page), value(next)) {
        (Some((kind, value)), Some((next_kind, next_value))) => kind == next_kind && next_value == value + 1,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Numerals {
    Decimal,
    LowerRoman,
    UpperRoman,
}

/// The numbering and value of a page number such as `12`, `012`, `iv`, or `XI`.
fn value(page: &str) -> Option<(Numerals, usize)> {
    if !page.is_empty() && page.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some((Numerals::Decimal, page.parse().ok()?));
    }
    let kind = if page.chars().all(|c| "ivxlcdm".contains(c)) {
        Numerals::LowerRoman
    } else if page.chars().all(|c| "IVXLCDM".contains(c)) {
        Numerals::UpperRoman
    } else {
        return None;
    };
    let digit = |c: char| match c.to_ascii_uppercase() {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        _ => 1000,
    };
    let digits: Vec<i64> = page.chars().map(digit).collect();
    let total = (digits.iter().enumerate()).fold(0, |total, (index, &digit)| match digits.get(index + 1) {
        Some(&next) if next > digit => total - digit,
        _ => total + digit,
    });
    let total = usize::try_from(total).ok()?;
    (!page.is_empty()).then_some((kind, total))
}

/// Adds the indexes of the marks in `blocks` in reading order, with a footnote's at its reference.
fn reading_order(blocks: &[Block], document: &Document, order: &mut Vec<usize>) {
    let inlines = |content: &[Inline], order: &mut Vec<usize>| {
        for inline in content {
            match inline {
                Inline::IndexMark(index) => order.push(*index),
                Inline::FootnoteRef(note) => reading_order(&document.footnotes[*note].blocks, document, order),
                _ => {}
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
                for cell in header.cells.iter().chain(rows.iter().flat_map(|row| &row.cells)) {
                    reading_order(&cell.blocks, document, order);
                }
                inlines(caption, order);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{IndexMark, Location, See};

    const AT: Location = Location { line: 1, column: 1 };

    fn mark(terms: &[&str], see: Option<(bool, &str)>) -> IndexMark {
        IndexMark {
            at: AT,
            terms: terms.iter().map(|term| (*term).to_owned()).collect(),
            see: see.map(|(also, target)| See {
                also,
                terms: target.split('!').map(str::to_owned).collect(),
            }),
        }
    }

    /// A document with one paragraph holding `marks` in order.
    fn document(marks: Vec<IndexMark>) -> Document {
        let content = (0..marks.len()).map(Inline::IndexMark).collect();
        Document {
            blocks: vec![Block::Paragraph {
                at: AT,
                content,
                class: None,
            }],
            index_marks: marks,
            ..Document::default()
        }
    }

    fn sorted(terms: &[&str]) -> Vec<String> {
        let mut terms: Vec<String> = terms.iter().map(|term| (*term).to_owned()).collect();
        terms.sort_by_key(|term| Key::new(term));
        terms
    }

    #[test]
    fn sorts_german_terms_per_din_5007_1_case_insensitive_with_a_fixed_tie_break() {
        assert_eq!(
            sorted(&[
                "Zug", "Öl", "Muße", "Müller", "Muller", "Mus", "mueller", "Abend", "Ähre", "abend", "Ofen"
            ]),
            [
                "abend", "Abend", "Ähre", "mueller", "Muller", "Müller", "Mus", "Muße", "Ofen", "Öl", "Zug"
            ]
        );
    }

    #[test]
    fn sorts_english_terms_word_by_word_with_symbols_first() {
        assert_eq!(
            sorted(&[
                "New York",
                "Newark",
                "new",
                "élan",
                "Eve",
                "3D printing",
                "@media",
                "Zeta"
            ]),
            [
                "3D printing",
                "@media",
                "élan",
                "Eve",
                "new",
                "New York",
                "Newark",
                "Zeta"
            ]
        );
    }

    #[test]
    fn groups_entries_under_letters_with_sub_entries_and_see_references() {
        let index = Index::new(&document(vec![
            mark(&["Hyphenation", "patterns"], None),
            mark(&["Äther"], None),
            mark(&["Breaking"], Some((false, "Line breaking"))),
            mark(&["Hyphenation"], None),
            mark(&["Hyphenation"], Some((true, "Line breaking!optimal"))),
            mark(&["Line breaking", "optimal"], None),
            mark(&["1st edition"], None),
            mark(&["Hyphenation", "exceptions"], None),
        ]))
        .expect("marks make an index");
        let letters: Vec<_> = index.groups.iter().map(|group| group.letter.as_deref()).collect();
        assert_eq!(letters, [None, Some("A"), Some("B"), Some("H"), Some("L")]);
        let hyphenation = &index.groups[3].entries[0];
        assert_eq!(hyphenation.marks, [3]);
        assert_eq!(hyphenation.see_also, ["Line breaking, optimal"]);
        let subs: Vec<_> = (hyphenation.sub_entries.iter())
            .map(|entry| (entry.term.as_str(), entry.marks.clone()))
            .collect();
        assert_eq!(subs, [("exceptions", vec![7]), ("patterns", vec![0])]);
        let breaking = &index.groups[2].entries[0];
        assert_eq!(
            (breaking.marks.len(), breaking.see.as_slice()),
            (0, ["Line breaking".to_owned()].as_slice())
        );
    }

    #[test]
    fn counts_marks_in_footnotes_at_their_reference() {
        let mut document = document(vec![mark(&["a"], None), mark(&["a"], None), mark(&["a"], None)]);
        document.footnotes.push(crate::document::Footnote {
            at: AT,
            blocks: vec![Block::Paragraph {
                at: AT,
                content: vec![Inline::IndexMark(2)],
                class: None,
            }],
        });
        if let Block::Paragraph { content, .. } = &mut document.blocks[0] {
            content.truncate(2);
            content.insert(1, Inline::FootnoteRef(0));
        }
        let index = Index::new(&document).expect("marks make an index");
        assert_eq!(index.groups[0].entries[0].marks, [0, 2, 1]);
    }

    fn shown(pages: &[&str]) -> String {
        let pages: Vec<(String, usize)> = pages
            .iter()
            .enumerate()
            .map(|(i, page)| ((*page).to_owned(), i))
            .collect();
        let text: Vec<String> = locators(&pages)
            .into_iter()
            .map(|locator| match locator.last {
                Some(last) => format!("{}({})–{}({})", locator.first.0, locator.first.1, last.0, last.1),
                None => format!("{}({})", locator.first.0, locator.first.1),
            })
            .collect();
        text.join(", ")
    }

    #[test]
    fn merges_runs_of_three_pages_into_ranges_and_counts_each_page_once() {
        assert_eq!(
            shown(&["3", "3", "4", "5", "7", "8", "10", "9"]),
            "3(0)–5(3), 7(4), 8(5), 10(6), 9(7)"
        );
        assert_eq!(shown(&["ii", "iii", "iv", "v", "1", "2", "3"]), "ii(0)–v(3), 1(4)–3(6)");
        assert_eq!(shown(&["IX", "X", "XI", "09", "10", "11"]), "IX(0)–XI(2), 09(3)–11(5)");
        assert_eq!(shown(&["iv", "v", "6"]), "iv(0), v(1), 6(2)");
    }
}
