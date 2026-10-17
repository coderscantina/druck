//! Automatic no-break spaces in author text.
//!
//! A single ordinary space becomes U+00A0 between the words of an abbreviation such as "z. B.", between
//! a number and its unit or sign, and between a reference abbreviation and its number, such as "S. 12".
//! The decision looks at the two words around each space, in one pass over the text.

use std::borrow::Cow;

const NO_BREAK_SPACE: char = '\u{a0}';

/// Units and signs that follow a number.
const UNITS: &[&str] = &[
    "%", "‰", "€", "$", "°C", "°F", "°", "km", "km/h", "m", "cm", "mm", "kg", "g", "mg", "l", "ml", "h", "min", "s",
    "ms", "pt", "px", "KB", "kB", "MB", "GB", "TB", "Hz", "kHz", "MHz", "GHz",
];

/// Abbreviations that are followed by a number.
const REFERENCES: &[&str] = &[
    "S.", "Nr.", "No.", "p.", "pp.", "§", "§§", "Abs.", "Art.", "Kap.", "Bd.", "Abb.", "Tab.", "Fig.", "Vol.", "vol.",
];

const OPENERS: &[char] = &['(', '[', '"', '\'', '„', '“', '‚', '‘', '»', '«'];
const CLOSERS: &[char] = &[')', ']', '"', '\'', '”', '’', '«', '»', ',', ';', ':', '!', '?'];

/// Replaces the spaces that must not break with no-break spaces. Borrows the text when none does.
pub fn join(text: &str) -> Cow<'_, str> {
    let mut words = text.split(' ');
    let Some(mut before) = words.next() else {
        return Cow::Borrowed(text);
    };
    let mut joined: Option<String> = None;
    let (mut start, mut copied) = (0, 0);
    for after in words {
        let space = start + before.len();
        if !before.is_empty() && !after.is_empty() && binds(before, after) {
            let out = joined.get_or_insert_with(|| String::with_capacity(text.len() + 4));
            out.push_str(&text[copied..space]);
            out.push(NO_BREAK_SPACE);
            copied = space + 1;
        }
        start = space + 1;
        before = after;
    }
    match joined {
        Some(mut out) => {
            out.push_str(&text[copied..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(text),
    }
}

/// Whether the space between two words must not break.
fn binds(before: &str, after: &str) -> bool {
    let opened = before.trim_start_matches(OPENERS);
    letter_period(opened, true) && letter_period(after.trim_start_matches(OPENERS), false)
        || is_number(before) && is_unit(after)
        || matches!(before, "€" | "$") && after.starts_with(|c: char| c.is_ascii_digit())
        || REFERENCES.contains(&opened) && after.starts_with(|c: char| c.is_ascii_digit())
}

/// A single letter and a period, such as "z." or "B.". Only the word after the space may carry closing
/// punctuation, as in "B.," or "B.)".
fn letter_period(word: &str, exact: bool) -> bool {
    let word = if exact { word } else { word.trim_end_matches(CLOSERS) };
    let mut chars = word.chars();
    matches!((chars.next(), chars.next(), chars.next()), (Some(letter), Some('.'), None) if letter.is_alphabetic())
}

/// Digits with separators and an optional sign or range dash, ending in a digit.
fn is_number(word: &str) -> bool {
    word.ends_with(|c: char| c.is_ascii_digit())
        && word
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '\'' | '’' | '+' | '-' | '−' | '–' | '/'))
}

/// A whole unit token, with the punctuation of the sentence after it removed.
fn is_unit(word: &str) -> bool {
    let word = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '"', '”', '’']);
    UNITS.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::join;

    #[test]
    fn joins_abbreviations_of_single_letters() {
        assert_eq!(
            join("also z. B. so, i. d. R. nie"),
            "also z.\u{a0}B. so, i.\u{a0}d.\u{a0}R. nie"
        );
        assert_eq!(join("(e. g. this)"), "(e.\u{a0}g. this)");
        assert_eq!(join("Plan B. Then more"), "Plan B. Then more");
    }

    #[test]
    fn joins_numbers_with_their_units_and_currency_signs() {
        assert_eq!(
            join("5 min, 20 %, 3,5 kg und 21 °C"),
            "5\u{a0}min, 20\u{a0}%, 3,5\u{a0}kg und 21\u{a0}°C"
        );
        assert_eq!(join("€ 5 or 5 €."), "€\u{a0}5 or 5\u{a0}€.");
        assert_eq!(join("5 minutes and 5 apples"), "5 minutes and 5 apples");
    }

    #[test]
    fn joins_reference_abbreviations_with_their_numbers() {
        assert_eq!(
            join("siehe S. 12, § 3 und Abb. 4"),
            "siehe S.\u{a0}12, §\u{a0}3 und Abb.\u{a0}4"
        );
        assert_eq!(join("see Fig. one"), "see Fig. one");
    }

    #[test]
    fn borrows_text_without_a_match() {
        assert!(matches!(join("nothing to do here"), std::borrow::Cow::Borrowed(_)));
    }
}
