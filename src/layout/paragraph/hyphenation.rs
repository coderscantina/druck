//! Where words may break: hyphenation points from the bundled TeX patterns, explicit hyphens, and
//! slashes in URLs.
//!
//! The patterns come compiled into `hypher`: American English (`hyph-en-us`) and German in the
//! reformed orthography (`hyph-de-1996`). Fragment limits default to two letters before and three after
//! a break in English and three on each side in German, and a theme may override both.

use crate::config::theme::Lang;

/// The fewest letters before and after a hyphenation point: `custom` if given, else the language's
/// own. English reads fine with a short head, German compounds want three on both sides.
pub fn fragments(lang: Lang, custom: Option<(usize, usize)>) -> (usize, usize) {
    custom.unwrap_or(match lang {
        Lang::En => (2, 3),
        Lang::De => (3, 3),
    })
}

/// Byte offsets inside `word` where a hyphen may be inserted, in increasing order, with at least
/// `fragments` letters before and after each.
///
/// Only runs of letters are hyphenated, so punctuation, digits, and explicit hyphens stay intact.
/// Runs with a capital after their first letter (acronyms, camel case) and words that look like
/// URLs or e-mail addresses get no points.
pub fn points(word: &str, lang: Lang, (left, right): (usize, usize)) -> Vec<usize> {
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
        let syllables = hypher::hyphenate_bounded(run, lang, left, right);
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

/// Byte offsets just after slashes in a URL, as in "doi.org/|10.1093/|comjnl", where a line may end
/// without a hyphen. The slashes of "://" and a slash at the end stay with their neighbours.
pub fn url(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    (1..bytes.len().saturating_sub(1))
        .filter(|&index| bytes[index] == b'/' && bytes[index - 1] != b'/' && bytes[index + 1] != b'/')
        .map(|index| index + 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: (usize, usize) = (2, 3);
    const DE: (usize, usize) = (3, 3);

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
        assert_eq!(split(word, &points(word, Lang::En, EN)), "“ty|pog|ra|phy,”");
    }

    #[test]
    fn hyphenates_german_compounds() {
        let word = "Donaudampfschifffahrtsgesellschaft";
        let split = split(word, &points(word, Lang::De, DE));
        assert!(split.contains("schiff|fahrts"), "{split}");
        assert!(split.contains("ge|sell|schaft"), "{split}");
    }

    #[test]
    fn leaves_acronyms_camel_case_and_urls_alone() {
        assert!(points("CommonMark", Lang::En, EN).is_empty());
        assert!(points("https://spec.commonmark.org/", Lang::En, EN).is_empty());
        assert!(points("ada@example.com", Lang::En, EN).is_empty());
    }

    #[test]
    fn bounds_fragments_by_language_and_lets_a_theme_override_them() {
        assert_eq!(fragments(Lang::En, None), (2, 3));
        assert_eq!(fragments(Lang::De, None), (3, 3));
        assert_eq!(fragments(Lang::De, Some((1, 2))), (1, 2));

        // English may break after two letters, German needs three.
        assert_eq!(split("ability", &points("ability", Lang::En, EN)), "abil|ity");
        let strict = points("aberration", Lang::En, (4, 4));
        assert!(
            strict
                .iter()
                .all(|&point| (4..="aberration".len() - 4).contains(&point)),
            "{strict:?}"
        );
        let word = "Abenteuer";
        let german = split(word, &points(word, Lang::De, DE));
        let loose = split(word, &points(word, Lang::De, (1, 1)));
        assert!(!german.starts_with("A|") && !german.starts_with("Ab|"), "{german}");
        assert!(loose.len() > german.len(), "{loose} vs {german}");
    }

    #[test]
    fn breaks_urls_after_single_slashes() {
        let link = "https://doi.org/10.1093/comjnl/";
        assert_eq!(split(link, &url(link)), "https://doi.org/|10.1093/|comjnl/");
    }

    #[test]
    fn breaks_after_joining_hyphens_only() {
        let word = "CommonMark-Spezifikation";
        assert_eq!(split(word, &explicit(word)), "CommonMark-|Spezifikation");
        assert!(explicit("-5").is_empty());
        assert!(explicit("well-").is_empty());
    }
}
