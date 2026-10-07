//! The citation syntax in running text. A pure scan over a text string; the integrator calls it on text outside code
//! and links and maps byte offsets to document locations.
//!
//! - `[@key]`, `[@a; @b]`: parenthetical citations with one or more keys.
//! - `[@key, p. 12]`, `[@key, pp. 3-5]`, `[@key, S. 12]`: with a locator after a comma.
//! - `@key` and `@key [p. 12]`: narrative citations, only at a word start so that `a@b.de` stays text.
//!
//! A locator is `p.`, `pp.`, or `S.`, then a page or a range of two pages, each made of letters and digits ("12",
//! "xiv", "A3"). The range separator is `-` or an en dash and is normalized to an en dash. Keys with the prefixes of
//! cross-references (`sec:`, `fig:`, `tbl:`) are not citations, and a bracket group holding one is left as text.

use std::ops::Range;

use crate::config::theme::Lang;

use super::words::words;

/// Key prefixes that belong to cross-references.
const RESERVED_PREFIXES: [&str; 3] = ["sec:", "fig:", "tbl:"];

/// A page or page range, normalized, for example "12" or "3–5".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locator(String);

impl Locator {
    /// The text with the language's label: "p. 12", "pp. 3–5", or "S. 12", "S. 3–5".
    pub fn text(&self, lang: Lang) -> String {
        let words = words(lang);
        let label = if self.0.contains('–') {
            words.pages
        } else {
            words.page
        };
        format!("{label} {}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    /// Where the key is written, including the `@`.
    pub range: Range<usize>,
    pub locator: Option<Locator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `[@a; @b]`
    Parenthetical,
    /// `@a`
    Narrative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// The whole citation in the text, brackets included.
    pub range: Range<usize>,
    pub form: Form,
    /// One or more items; a narrative citation has exactly one.
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<'a> {
    Text {
        range: Range<usize>,
        text: &'a str,
    },
    Citation(Citation),
    /// A bracket group that starts like a citation but cannot be read, for example with an unknown locator.
    Invalid {
        range: Range<usize>,
        message: String,
    },
}

/// Splits text into plain text and citations, in order. The ranges of all segments cover the text without gaps.
pub fn find(text: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let (mut plain_start, mut pos) = (0, 0);
    while pos < text.len() {
        let found = match text.as_bytes()[pos] {
            b'[' => bracketed(text, pos),
            b'@' => narrative(text, pos),
            _ => None,
        };
        let Some(segment) = found else {
            pos += 1;
            continue;
        };
        let end = match &segment {
            Segment::Citation(citation) => citation.range.end,
            Segment::Invalid { range, .. } | Segment::Text { range, .. } => range.end,
        };
        if matches!(segment, Segment::Text { .. }) {
            // Skipped as plain text, so the scan does not look inside it.
            pos = end;
            continue;
        }
        if plain_start < pos {
            segments.push(Segment::Text {
                range: plain_start..pos,
                text: &text[plain_start..pos],
            });
        }
        segments.push(segment);
        (plain_start, pos) = (end, end);
    }
    if plain_start < text.len() {
        segments.push(Segment::Text {
            range: plain_start..text.len(),
            text: &text[plain_start..],
        });
    }
    segments
}

fn is_reserved(key: &str) -> bool {
    RESERVED_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

/// The end of the key that starts at `start`: letters, digits, and `_`, plus `- : . / +` between them.
fn key_end(text: &str, start: usize) -> usize {
    let is_key_char = |c: char| c.is_alphanumeric() || c == '_';
    let mut end = start;
    let mut chars = text[start..].char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let internal =
            matches!(c, '-' | ':' | '.' | '/' | '+') && chars.peek().is_some_and(|&(_, next)| is_key_char(next));
        if !(is_key_char(c) || internal) {
            break;
        }
        end = start + index + c.len_utf8();
    }
    end
}

fn parse_locator(text: &str) -> Option<Locator> {
    let text = text.trim();
    let value = ["pp.", "p.", "S."].iter().find_map(|label| text.strip_prefix(label))?;
    let is_page = |page: &str| !page.is_empty() && page.chars().all(|c| c.is_ascii_alphanumeric());
    let normalized = match value.split_once(['-', '–']) {
        Some((from, to)) if is_page(from.trim()) && is_page(to.trim()) => format!("{}–{}", from.trim(), to.trim()),
        None if is_page(value.trim()) => value.trim().to_owned(),
        _ => return None,
    };
    Some(Locator(normalized))
}

/// Parses `[@a, p. 3; @b]` at `open`. `None` means the text is not a citation; a group with a cross-reference key is
/// returned as text so that its keys are not read as narrative citations.
fn bracketed(text: &str, open: usize) -> Option<Segment<'_>> {
    let inner_start = open + 1;
    let close = inner_start + text[inner_start..].find(']')?;
    let inner = &text[inner_start..close];
    if !inner.trim_start().starts_with('@') {
        return None;
    }
    let mut items = Vec::new();
    let mut error = None;
    let mut offset = inner_start;
    for part in inner.split(';') {
        match item(text, offset, part) {
            Ok(item) => items.push(item),
            Err(message) => error = error.or(Some(message)),
        }
        offset += part.len() + 1;
    }
    let range = open..close + 1;
    if items.iter().any(|item| is_reserved(&item.key)) {
        return Some(Segment::Text {
            range: range.clone(),
            text: &text[range],
        });
    }
    Some(match error {
        Some(message) => Segment::Invalid { range, message },
        None => Segment::Citation(Citation {
            range,
            form: Form::Parenthetical,
            items,
        }),
    })
}

/// One `@key` or `@key, locator` between the separators, where `offset` is where `part` starts in `text`.
fn item(text: &str, offset: usize, part: &str) -> Result<Item, String> {
    let leading = part.len() - part.trim_start().len();
    let start = offset + leading;
    if !text[start..offset + part.len()].starts_with('@') {
        return Err(format!("expected `@key` in the citation, found `{}`", part.trim()));
    }
    let end = key_end(text, start + 1);
    if end == start + 1 {
        return Err("expected a citation key after `@`".to_owned());
    }
    let key = text[start + 1..end].to_owned();
    let rest = text[end..offset + part.len()].trim();
    let locator = match rest.strip_prefix(',') {
        _ if rest.is_empty() => None,
        Some(locator) => Some(parse_locator(locator).ok_or_else(|| unknown_locator(locator.trim()))?),
        None => {
            return Err(format!(
                "unexpected `{rest}` after the key `{key}`; a locator follows a comma"
            ));
        }
    };
    Ok(Item {
        key,
        range: start..end,
        locator,
    })
}

fn unknown_locator(locator: &str) -> String {
    format!("unsupported locator `{locator}`; use `p. 12`, `pp. 3-5`, or `S. 12`")
}

/// Parses `@key` or `@key [p. 12]` at `at`. `None` means the text is not a citation.
fn narrative(text: &str, at: usize) -> Option<Segment<'_>> {
    let at_word_start = text[..at]
        .chars()
        .next_back()
        .is_none_or(|c| c.is_whitespace() || "([{\"'“„«»".contains(c));
    let end = key_end(text, at + 1);
    let key = &text[at + 1..end];
    if !at_word_start || key.is_empty() || is_reserved(key) {
        return None;
    }
    let locator_end = text[end..]
        .strip_prefix(' ')
        .filter(|rest| rest.starts_with('['))
        .and_then(|rest| Some((rest.find(']')?, rest)))
        .and_then(|(close, rest)| Some((parse_locator(&rest[1..close])?, end + 1 + close + 1)));
    let (locator, range_end) = match locator_end {
        Some((locator, range_end)) => (Some(locator), range_end),
        None => (None, end),
    };
    Some(Segment::Citation(Citation {
        range: at..range_end,
        form: Form::Narrative,
        items: vec![Item {
            key: key.to_owned(),
            range: at..end,
            locator,
        }],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn citations(text: &str) -> Vec<Citation> {
        find(text)
            .into_iter()
            .filter_map(|segment| match segment {
                Segment::Citation(citation) => Some(citation),
                _ => None,
            })
            .collect()
    }

    fn keys(citation: &Citation) -> Vec<&str> {
        citation.items.iter().map(|item| item.key.as_str()).collect()
    }

    #[test]
    fn finds_parenthetical_citations_with_ranges_around_text() {
        let text = "As shown [@smith2024] here.";
        let segments = find(text);
        assert_eq!(segments.len(), 3);
        let Segment::Citation(citation) = &segments[1] else {
            panic!("expected a citation")
        };
        assert_eq!(&text[citation.range.clone()], "[@smith2024]");
        assert_eq!(&text[citation.items[0].range.clone()], "@smith2024");
        assert_eq!(citation.form, Form::Parenthetical);
        assert_eq!(
            segments[2],
            Segment::Text {
                range: 21..27,
                text: " here."
            }
        );
    }

    #[test]
    fn finds_multiple_keys_and_locators() {
        let found = citations("[@a; @b, p. 12; @c,pp. 3-5; @d, S. 7–9]");
        assert_eq!(keys(&found[0]), ["a", "b", "c", "d"]);
        let locators: Vec<_> = found[0].items.iter().map(|item| item.locator.clone()).collect();
        assert_eq!(locators[0], None);
        assert_eq!(locators[1].as_ref().unwrap().text(Lang::En), "p. 12");
        assert_eq!(locators[2].as_ref().unwrap().text(Lang::En), "pp. 3–5");
        assert_eq!(locators[3].as_ref().unwrap().text(Lang::De), "S. 7–9");
    }

    #[test]
    fn finds_narrative_citations_with_an_optional_locator() {
        let text = "Per @smith2024 [p. 12] and @jones.2020, but @k [see].";
        let found = citations(text);
        assert_eq!(keys(&found[0]), ["smith2024"]);
        assert_eq!(&text[found[0].range.clone()], "@smith2024 [p. 12]");
        assert_eq!(found[0].form, Form::Narrative);
        assert!(found[0].items[0].locator.is_some());
        assert_eq!(keys(&found[1]), ["jones.2020"]);
        assert_eq!(&text[found[2].range.clone()], "@k");
    }

    #[test]
    fn leaves_emails_and_lone_at_signs_alone() {
        assert!(citations("write to ada@example.org or @ here").is_empty());
    }

    #[test]
    fn leaves_cross_reference_keys_alone() {
        let found = citations("see [@sec:intro] and @fig:one and [@tbl:x; @a] and [@a]");
        assert_eq!(found.len(), 1);
        assert_eq!(keys(&found[0]), ["a"]);
    }

    #[test]
    fn reports_unreadable_groups() {
        let segments = find("x [@a, see below] y");
        let Segment::Invalid { range, message } = &segments[1] else {
            panic!("expected invalid")
        };
        assert_eq!(*range, 2..17);
        assert!(message.contains("unsupported locator `see below`"));
        assert!(matches!(&find("[@a; b]")[0], Segment::Invalid { .. }));
        assert!(
            find("[a link](http://x.org) and [^1]")
                .iter()
                .all(|s| matches!(s, Segment::Text { .. }))
        );
    }

    #[test]
    fn segments_cover_the_text_without_gaps() {
        let text = "ä @a [@b] ö";
        let end = find(text).iter().fold(0, |end, segment| {
            let range = match segment {
                Segment::Text { range, .. } | Segment::Invalid { range, .. } => range,
                Segment::Citation(citation) => &citation.range,
            };
            assert_eq!(range.start, end);
            range.end
        });
        assert_eq!(end, text.len());
    }
}
