//! Searching and replacing in text. Holds no gpui types.
//!
//! A [`Matcher`] is a query with its options. [`search`] finds every match in a list of texts, for the search panel, and
//! [`Matcher::edits`] turns the matches of one text into the edits that replace them, which the session applies as one
//! undo step per chapter.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

use crate::session::ChapterId;
use crate::source::Edit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Read the query as a regular expression, with `$1` in the replacement for a group.
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// A query ready to run.
#[derive(Debug, Clone)]
pub struct Matcher {
    pattern: Regex,
    regex: bool,
}

impl Matcher {
    /// `Ok(None)` for an empty query, and the reason for a query that is not a valid expression.
    pub fn new(query: &str, options: Options) -> Result<Option<Self>, String> {
        if query.is_empty() {
            return Ok(None);
        }
        let mut source = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        if options.whole_word {
            source = format!(r"\b(?:{source})\b");
        }
        let pattern = RegexBuilder::new(&source)
            .case_insensitive(!options.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|error| match error {
                regex::Error::Syntax(message) => message.lines().last().unwrap_or_default().trim().to_owned(),
                other => other.to_string(),
            })?;
        Ok(Some(Self {
            pattern,
            regex: options.regex,
        }))
    }

    /// Every non-empty match in `text`.
    pub fn find(&self, text: &str) -> Vec<Range<usize>> {
        self.pattern
            .find_iter(text)
            .filter(|found| !found.is_empty())
            .map(|found| found.range())
            .collect()
    }

    /// The text that replaces each match in `text`, with its range.
    pub fn replacements(&self, text: &str, replacement: &str) -> Vec<(Range<usize>, String)> {
        let mut out = Vec::new();
        for captures in self.pattern.captures_iter(text) {
            let Some(whole) = captures.get(0).filter(|whole| !whole.is_empty()) else {
                continue;
            };
            let mut new = String::new();
            if self.regex {
                captures.expand(replacement, &mut new);
            } else {
                new.push_str(replacement);
            }
            out.push((whole.range(), new));
        }
        out
    }

    /// The edits that replace every match in `text`, as one transaction.
    pub fn edits(&self, text: &str, replacement: &str) -> Vec<Edit> {
        self.replacements(text, replacement)
            .into_iter()
            .map(|(range, new)| Edit::new(range, new))
            .collect()
    }
}

/// A text to search, with the chapter it is.
#[derive(Debug, Clone)]
pub struct Source {
    pub id: ChapterId,
    pub title: String,
    pub text: String,
}

/// One match, with the line it is in as it reads now and as it would read after the replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub range: Range<usize>,
    /// 1-based.
    pub line: usize,
    pub before: String,
    pub after: String,
    /// Where the match stands in `before`, and its replacement in `after`.
    pub before_mark: Range<usize>,
    pub after_mark: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHits {
    pub id: ChapterId,
    pub title: String,
    pub hits: Vec<Hit>,
}

/// How many characters of the line a hit shows before its match, so a row starts near it.
const LEAD: usize = 20;
/// The longest stretch of a line a hit shows after its match.
const CONTEXT: usize = 120;

/// The line holding `range`, shortened to start about [`LEAD`] characters before it at a word, and `replacement` put in
/// place of the match, each with where the match stands in it.
fn line_of(text: &str, range: &Range<usize>, replacement: &str) -> (String, String, Range<usize>, Range<usize>) {
    let start = text[..range.start].rfind('\n').map_or(0, |at| at + 1);
    let end = text[range.end..].find('\n').map_or(text.len(), |at| range.end + at);
    let lead = &text[start..range.start];
    let from = match lead.char_indices().rev().nth(LEAD) {
        Some((cut, _)) => {
            let word = lead[cut..].find(' ').map_or(cut, |space| cut + space + 1);
            start + word.min(lead.len())
        }
        None => start,
    };
    let mut to = end.min(range.end + CONTEXT);
    while !text.is_char_boundary(to) {
        to -= 1;
    }
    let lead = if from > start { "\u{2026}" } else { "" };
    let tail = if to < end { "\u{2026}" } else { "" };
    let mark_start = lead.len() + range.start - from;
    let before = format!("{lead}{}{tail}", &text[from..to]);
    let after = format!(
        "{lead}{}{replacement}{}{tail}",
        &text[from..range.start],
        &text[range.end..to]
    );
    let before_mark = mark_start..mark_start + range.len();
    let after_mark = mark_start..mark_start + replacement.len();
    (before, after, before_mark, after_mark)
}

/// Every match in every source, in the order of the sources. A source without a match is left out. `replacement` shows
/// in each hit's `after`.
pub fn search(sources: &[Source], matcher: &Matcher, replacement: &str) -> Vec<FileHits> {
    sources
        .iter()
        .filter_map(|source| {
            let found = matcher.replacements(&source.text, replacement);
            if found.is_empty() {
                return None;
            }
            let mut line = 1;
            let mut counted = 0;
            let hits = found
                .into_iter()
                .map(|(range, new)| {
                    line += source.text[counted..range.start].matches('\n').count();
                    counted = range.start;
                    let (before, after, before_mark, after_mark) = line_of(&source.text, &range, &new);
                    Hit {
                        range,
                        line,
                        before,
                        after,
                        before_mark,
                        after_mark,
                    }
                })
                .collect();
            Some(FileHits {
                id: source.id,
                title: source.title.clone(),
                hits,
            })
        })
        .collect()
}

/// Where the next match after `from` starts, or the first one when none follows, for the find bar.
pub fn next_match(matches: &[Range<usize>], from: usize, backwards: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    let at = if backwards {
        matches
            .iter()
            .rposition(|found| found.end < from)
            .unwrap_or(matches.len() - 1)
    } else {
        matches.iter().position(|found| found.start >= from).unwrap_or(0)
    };
    Some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chapter ids come only from a session.
    fn a_chapter() -> ChapterId {
        let file = std::env::temp_dir().join(format!("druck-search-{}.md", std::process::id()));
        std::fs::write(&file, "# A\n").expect("file");
        let session = crate::session::Session::open(&file).expect("opens");
        std::fs::remove_file(&file).ok();
        session.active()
    }

    fn matcher(query: &str, options: Options) -> Matcher {
        Matcher::new(query, options).expect("valid").expect("not empty")
    }

    #[test]
    fn a_plain_query_is_literal_and_ignores_case_by_default() {
        let found = matcher("a.b", Options::default()).find("A.B axb a.b");
        assert_eq!(found, [0..3, 8..11]);
    }

    #[test]
    fn whole_word_and_case_narrow_the_matches() {
        let options = Options {
            whole_word: true,
            case_sensitive: true,
            ..Options::default()
        };
        assert_eq!(matcher("cat", options).find("cat Cat concat cat."), [0..3, 15..18]);
    }

    #[test]
    fn a_regex_replaces_with_its_groups_and_a_plain_query_does_not() {
        let regex = Options {
            regex: true,
            ..Options::default()
        };
        let edits = matcher(r"(\w+)@(\w+)", regex).edits("ann@home", "$2:$1");
        assert_eq!(edits, [Edit::new(0..8, "home:ann")]);
        let edits = matcher("a", Options::default()).edits("a", "$1");
        assert_eq!(edits, [Edit::new(0..1, "$1")]);
    }

    #[test]
    fn a_broken_expression_says_why_and_an_empty_query_finds_nothing() {
        let regex = Options {
            regex: true,
            ..Options::default()
        };
        assert!(Matcher::new("(", regex).is_err());
        assert!(Matcher::new("", regex).expect("fine").is_none());
        assert!(matcher("a*", regex).find("bbb").is_empty());
    }

    #[test]
    fn hits_carry_their_line_and_the_line_after_the_replacement() {
        let sources = [Source {
            id: a_chapter(),
            title: "One".into(),
            text: "first\nthe cat sat\nthe cat ran\n".into(),
        }];
        let found = search(&sources, &matcher("cat", Options::default()), "dog");
        let hits = &found[0].hits;
        assert_eq!((hits[0].line, hits[1].line), (2, 3));
        assert_eq!(hits[1].before, "the cat ran");
        assert_eq!(hits[1].after, "the dog ran");
        assert_eq!((hits[1].before_mark.clone(), hits[1].after_mark.clone()), (4..7, 4..7));
    }

    #[test]
    fn a_hit_far_into_its_line_starts_near_it_at_a_word() {
        let text = "Und so fragten sie nach der Gnade, die keiner verdient, und blieben.";
        let sources = [Source {
            id: a_chapter(),
            title: "One".into(),
            text: text.into(),
        }];
        let hit = &search(&sources, &matcher("Gnade", Options::default()), "")[0].hits[0];
        assert_eq!(
            hit.before,
            "\u{2026}sie nach der Gnade, die keiner verdient, und blieben."
        );
        assert_eq!(&hit.before[hit.before_mark.clone()], "Gnade");
    }

    #[test]
    fn the_next_match_wraps_around_in_both_directions() {
        let matches = [2..4, 10..12];
        assert_eq!(next_match(&matches, 5, false), Some(1));
        assert_eq!(next_match(&matches, 13, false), Some(0));
        assert_eq!(next_match(&matches, 10, true), Some(0));
        assert_eq!(next_match(&matches, 1, true), Some(1));
        assert_eq!(next_match(&[], 0, false), None);
    }
}
