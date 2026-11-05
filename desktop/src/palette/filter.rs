//! What the palette's field finds, without gpui: every letter of the query, in order, with what reads most like the query
//! first.

/// How well `text` holds `query`, lower being better, or `None` if it does not. Case does not count.
pub fn score(query: &str, text: &str) -> Option<usize> {
    let (query, text) = (query.to_lowercase(), text.to_lowercase());
    if let Some(at) = text.find(&query) {
        let at_word_start = at == 0 || !text[..at].chars().next_back().is_some_and(char::is_alphanumeric);
        return Some(if at == 0 {
            0
        } else if at_word_start {
            1
        } else {
            2
        });
    }
    let mut letters = text.char_indices();
    let mut spread = 0;
    let mut last = None;
    for wanted in query.chars() {
        let (at, _) = letters.find(|(_, letter)| *letter == wanted)?;
        spread += last.map_or(0, |last| at - last);
        last = Some(at);
    }
    Some(3 + spread)
}

/// The indices of the texts that hold `query`, best first. Equal ones keep their order, so an empty query keeps them all.
pub fn ranked<'a>(query: &str, texts: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    let mut found: Vec<(usize, usize)> = texts
        .into_iter()
        .enumerate()
        .filter_map(|(index, text)| Some((score(query, text)?, index)))
        .collect();
    found.sort();
    found.into_iter().map(|(_, index)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAPTERS: [&str; 4] = ["Prolog", "Die Suchanfrage", "Zwischenspiel I", "Teil II: Antwort"];

    #[test]
    fn an_empty_query_keeps_every_row_in_order() {
        assert_eq!(ranked("", CHAPTERS), [0, 1, 2, 3]);
    }

    #[test]
    fn the_start_of_a_title_beats_a_word_inside_it_and_that_beats_the_middle_of_a_word() {
        assert_eq!(ranked("su", ["Absurd", "Die Suche", "Suche"]), [2, 1, 0]);
    }

    #[test]
    fn letters_in_order_are_enough_and_tighter_is_better() {
        assert_eq!(ranked("zsp", CHAPTERS), [2]);
        assert_eq!(ranked("ab", ["a..b", "a......b"]), [0, 1]);
        assert!(score("xyz", "Prolog").is_none());
    }

    #[test]
    fn case_does_not_count() {
        assert_eq!(score("PROLOG", "Prolog"), Some(0));
        assert_eq!(score("änd", "Ändern"), Some(0));
    }
}
