//! Where words may break: hyphenation points from the bundled TeX patterns, and explicit hyphens.
//!
//! The patterns come compiled into `hypher`: American English (`hyph-en-us`) and German in the
//! reformed orthography (`hyph-de-1996`). Fragment limits are the pattern authors' own: two letters
//! before and three after a break in English, two on each side in German.

use crate::config::theme::Lang;

/// Byte offsets inside `word` where a hyphen may be inserted, in increasing order.
///
/// Only runs of letters are hyphenated, so punctuation, digits, and explicit hyphens stay intact.
/// Runs with a capital after their first letter (acronyms, camel case) and words that look like
/// URLs or e-mail addresses get no points.
pub fn points(word: &str, lang: Lang) -> Vec<usize> {
    let mut points = Vec::new();
    if word.contains("://") || word.contains('@') || word.starts_with("www.") {
        return points;
    }
    let lang = match lang {
        Lang::En => hypher::Lang::English,
        Lang::De => hypher::Lang::German,
    };
    let mut start = 0;
    for run in word.split(|c: char| !c.is_alphabetic()) {
        let run_start = start;
        // The separator is one character; add its length for the next run.
        start += run.len() + word[run_start + run.len()..].chars().next().map_or(0, char::len_utf8);
        if run.chars().skip(1).any(char::is_uppercase) {
            continue;
        }
        let syllables = hypher::hyphenate(run, lang);
        let breaks = syllables.len().saturating_sub(1);
        let mut offset = run_start;
        for syllable in syllables.take(breaks) {
            offset += syllable.len();
            points.push(offset);
        }
    }
    points
}

/// Byte offsets just after hyphens and em dashes that join two words, as in "e-mail" or
/// "word—word". Breaking there adds nothing to the text.
pub fn explicit(word: &str) -> Vec<usize> {
    word.char_indices()
        .filter(|&(_, c)| matches!(c, '-' | '\u{2010}' | '—'))
        .filter_map(|(index, c)| {
            let end = index + c.len_utf8();
            let before = word[..index].chars().next_back()?;
            let after = word[end..].chars().next()?;
            (before.is_alphanumeric() && after.is_alphabetic()).then_some(end)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(word: &str, offsets: &[usize]) -> String {
        let mut parts = Vec::new();
        let mut start = 0;
        for &offset in offsets {
            parts.push(&word[start..offset]);
            start = offset;
        }
        parts.push(&word[start..]);
        parts.join("|")
    }

    #[test]
    fn hyphenates_english_words_and_keeps_punctuation_whole() {
        let word = "“typography,”";
        assert_eq!(split(word, &points(word, Lang::En)), "“ty|pog|ra|phy,”");
    }

    #[test]
    fn hyphenates_german_compounds() {
        let word = "Donaudampfschifffahrtsgesellschaft";
        let split = split(word, &points(word, Lang::De));
        assert!(split.contains("schiff|fahrts"), "{split}");
        assert!(split.contains("ge|sell|schaft"), "{split}");
    }

    #[test]
    fn leaves_acronyms_camel_case_and_urls_alone() {
        assert!(points("CommonMark", Lang::En).is_empty());
        assert!(points("https://spec.commonmark.org/", Lang::En).is_empty());
        assert!(points("ada@example.com", Lang::En).is_empty());
    }

    #[test]
    fn breaks_after_joining_hyphens_only() {
        let word = "CommonMark-Spezifikation";
        assert_eq!(split(word, &explicit(word)), "CommonMark-|Spezifikation");
        assert!(explicit("-5").is_empty());
        assert!(explicit("well-").is_empty());
    }
}
