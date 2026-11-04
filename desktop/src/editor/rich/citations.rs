//! Citations and cross-references as text, and searching what they may name. The picker writes `[@key]` and `@sec:x`
//! here, and a group's other works and words stay byte for byte.

use std::ops::Range;

use druck::editor::rich::Work;
use druck::editor::syntax::{self, Citation, Form, Segment};

use super::facts::{LabelDef, caption_words};

/// The works whose key, authors, title, and year hold all words of `query`, in file order.
pub fn search_works<'a>(works: &'a [Work], query: &str) -> Vec<&'a Work> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    works
        .iter()
        .filter(|work| {
            let haystack = format!("{} {} {} {}", work.key, work.authors, work.title, work.year).to_lowercase();
            words.iter().all(|word| haystack.contains(word))
        })
        .collect()
}

/// The labels whose name or title holds all words of `query`.
pub fn search_labels<'a>(labels: &'a [LabelDef], query: &str) -> Vec<&'a LabelDef> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    labels
        .iter()
        .filter(|label| {
            let haystack = format!("{} {}", label.label, caption_words(&label.title)).to_lowercase();
            words.iter().all(|word| haystack.contains(word))
        })
        .collect()
}

/// The one citation `written` is, whole, or why it is not one.
pub fn read(written: &str) -> Result<Citation, String> {
    let mut found = None;
    for segment in syntax::find(written) {
        match segment {
            Segment::Citation(citation) if citation.range == (0..written.len()) => found = Some(citation),
            Segment::Invalid { message, .. } => return Err(message),
            _ => return Err(format!("`{written}` is not a citation")),
        }
    }
    found.ok_or_else(|| format!("`{written}` is not a citation"))
}

/// What is written after a key up to the next item or the closing bracket, without its leading comma: locator and suffix.
pub fn tail<'a>(written: &'a str, citation: &Citation, item: usize) -> &'a str {
    written[tail_range(written, citation, item)]
        .trim_start_matches(',')
        .trim()
}

/// Where the part after an item's key is written.
fn tail_range(written: &str, citation: &Citation, item: usize) -> Range<usize> {
    let start = citation.items[item].range.end;
    if citation.form == Form::Narrative {
        return start..written.len();
    }
    let end = written[start..]
        .find([';', ']'])
        .map_or(written.len(), |offset| start + offset);
    start..end
}

/// The text of a new citation of `key`, with a locator such as `p. 12` if one is given.
pub fn new_citation(key: &str, locator: &str, narrative: bool) -> String {
    match (narrative, locator.trim()) {
        (true, "") => format!("@{key}"),
        (true, locator) => format!("@{key} [{locator}]"),
        (false, "") => format!("[@{key}]"),
        (false, locator) => format!("[@{key}, {locator}]"),
    }
}

/// `written` with one item's key and tail changed. The other items and every other word stay as written.
pub fn with_item(written: &str, citation: &Citation, item: usize, key: &str, locator: &str) -> String {
    let locator = locator.trim();
    let mine = &citation.items[item];
    let tail = tail_range(written, citation, item);
    let mut result = String::with_capacity(written.len() + key.len());
    result.push_str(&written[..mine.range.start]);
    result.push('@');
    result.push_str(key);
    if citation.form == Form::Narrative {
        if !locator.is_empty() {
            result.push_str(" [");
            result.push_str(locator);
            result.push(']');
        }
    } else if !locator.is_empty() {
        result.push_str(", ");
        result.push_str(locator);
    }
    result.push_str(&written[tail.end..]);
    result
}

/// `written`, a bracketed citation, with another work added at its end.
pub fn with_added_item(written: &str, key: &str) -> Option<String> {
    let body = written.strip_suffix(']')?;
    Some(format!("{body}; @{key}]"))
}

/// `written`, a bracketed citation, without one item. The last item left keeps its brackets.
pub fn without_item(written: &str, citation: &Citation, item: usize) -> Option<String> {
    if citation.items.len() < 2 || citation.form != Form::Parenthetical {
        return None;
    }
    let tail = tail_range(written, citation, item);
    let start = if item == 0 {
        "[".len()
    } else {
        // The prior item's end, so its `;` goes with this one.
        tail_range(written, citation, item - 1).end
    };
    let end = if item == 0 {
        // Up to the next item's text, past `;` and spaces.
        tail.end
            + written[tail.end..]
                .bytes()
                .take_while(|byte| *byte == b';' || *byte == b' ')
                .count()
    } else {
        tail.end
    };
    Some(format!("{}{}", &written[..start], &written[end..]))
}

/// The text of a cross-reference: the short form where the sentence allows it, and always in brackets for a page.
pub fn reference_text(label: &str, page: bool, bracketed: bool) -> String {
    match (page, bracketed) {
        (true, _) => format!("[@{label}, page]"),
        (false, true) => format!("[@{label}]"),
        (false, false) => format!("@{label}"),
    }
}

/// The label and page form of a cross-reference as written.
pub fn read_reference(written: &str) -> Option<(&str, bool)> {
    if let Some(body) = written.strip_prefix("[@") {
        let body = body.strip_suffix(']')?;
        return Some(match body.strip_suffix(", page") {
            Some(label) => (label, true),
            None => (body, false),
        });
    }
    Some((written.strip_prefix('@')?, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(key: &str, authors: &str, title: &str, year: &str) -> Work {
        Work {
            key: key.to_owned(),
            authors: authors.to_owned(),
            title: title.to_owned(),
            year: year.to_owned(),
        }
    }

    #[test]
    fn a_search_looks_at_key_author_title_and_year_ignoring_case() {
        let works = [
            work(
                "bostrom2003",
                "Bostrom",
                "Are You Living in a Computer Simulation?",
                "2003",
            ),
            work("müller", "Müller und Groß", "Die Straße", "1999"),
        ];
        let keys = |query| {
            search_works(&works, query)
                .iter()
                .map(|work| work.key.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(keys("BOSTROM"), ["bostrom2003"]);
        assert_eq!(keys("simulation 2003"), ["bostrom2003"]);
        assert_eq!(keys("straße"), ["müller"]);
        assert_eq!(keys("grOß"), ["müller"]);
        assert_eq!(keys("").len(), 2);
        assert!(keys("nothing").is_empty());
    }

    #[test]
    fn a_new_citation_reads_back_as_one_citation() {
        for (text, key, locator) in [
            (new_citation("a", "", false), "a", ""),
            (new_citation("a", "p. 3", false), "a", "p. 3"),
            (new_citation("a", "S. 12–13", true), "a", "S. 12–13"),
            (new_citation("a", "", true), "a", ""),
        ] {
            let citation = read(&text).expect("a citation");
            assert_eq!(citation.items[0].key, key);
            assert_eq!(tail(&text, &citation, 0).trim_matches(['[', ']', ' ']), locator);
        }
    }

    #[test]
    fn changing_one_item_keeps_the_others_and_their_words() {
        let written = "[see @a, p. 3, emphasis added; also @b]";
        let citation = read(written).expect("a citation");
        assert_eq!(tail(written, &citation, 0), "p. 3, emphasis added");
        assert_eq!(
            with_item(written, &citation, 1, "c", "S. 9"),
            "[see @a, p. 3, emphasis added; also @c, S. 9]"
        );
        assert_eq!(with_item(written, &citation, 0, "z", ""), "[see @z; also @b]");
        assert_eq!(
            with_added_item(written, "d").as_deref(),
            Some("[see @a, p. 3, emphasis added; also @b; @d]")
        );
        assert_eq!(
            without_item(written, &citation, 1).as_deref(),
            Some("[see @a, p. 3, emphasis added]")
        );
        assert_eq!(without_item(written, &citation, 0).as_deref(), Some("[also @b]"));
    }

    #[test]
    fn a_narrative_citation_keeps_its_form() {
        let written = "@a [p. 3]";
        let citation = read(written).expect("a citation");
        assert_eq!(with_item(written, &citation, 0, "b", "p. 4"), "@b [p. 4]");
        assert_eq!(with_item(written, &citation, 0, "b", ""), "@b");
    }

    #[test]
    fn what_is_not_a_citation_says_so() {
        assert!(read("[@a see below]").is_err());
        assert!(read("plain").is_err());
    }

    #[test]
    fn references_read_and_write_each_form() {
        assert_eq!(reference_text("sec:a", false, false), "@sec:a");
        assert_eq!(reference_text("sec:a", false, true), "[@sec:a]");
        assert_eq!(reference_text("sec:a", true, false), "[@sec:a, page]");
        assert_eq!(read_reference("@fig:a"), Some(("fig:a", false)));
        assert_eq!(read_reference("[@fig:a]"), Some(("fig:a", false)));
        assert_eq!(read_reference("[@fig:a, page]"), Some(("fig:a", true)));
    }
}
