//! What the palette's field finds, without gpui: the query as one piece, or in pieces that each start a word, in order,
//! with what reads most like the query first. Letters scattered through a word never count.

/// `text` as the palette compares it: lower case, with ß as ss and umlauts as their base letters, so a German title is
/// found from a keyboard without them.
fn folded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for letter in text.chars().flat_map(char::to_lowercase) {
        match letter {
            '\u{df}' => out.push_str("ss"),
            '\u{e4}' => out.push('a'),
            '\u{f6}' => out.push('o'),
            '\u{fc}' => out.push('u'),
            other => out.push(other),
        }
    }
    out
}

/// How well `text` holds `query`, lower being better, or `None` if it does not. Case does not count, and neither do ß and
/// umlauts.
pub fn score(query: &str, text: &str) -> Option<usize> {
    let (query, text) = (folded(query), folded(text));
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
    let words: Vec<&str> = (text.split(|letter: char| !letter.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect();
    let query: String = query.chars().filter(|letter| !letter.is_whitespace()).collect();
    word_starts(&query, &words).map(|used| 3 + used)
}

/// How few words it takes for `query`'s pieces to start successive words, in order, or `None` if they cannot.
fn word_starts(query: &str, words: &[&str]) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    (0..words.len()).find_map(|first| {
        let word = words[first];
        let shared = (query.char_indices().zip(word.chars()))
            .take_while(|((_, wanted), letter)| wanted == letter)
            .map(|((at, wanted), _)| at + wanted.len_utf8())
            .last()?;
        // The longest piece first, so the fewest words are used.
        (1..=shared)
            .rev()
            .filter(|end| query.is_char_boundary(*end))
            .find_map(|end| word_starts(&query[end..], &words[first + 1..]))
            .map(|used| used + 1)
    })
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
    fn sharp_s_and_umlauts_fold_on_either_side() {
        assert_eq!(score("fuss", "Fußnote"), Some(0));
        assert_eq!(score("fuß", "Fussnote"), Some(0));
        assert_eq!(score("uber", "Über uns"), Some(0));
        assert_eq!(score("übe", "Uber uns"), Some(0));
    }

    #[test]
    fn pieces_must_start_words_and_scattered_letters_do_not_count() {
        assert_eq!(ranked("tant", CHAPTERS), [3]);
        assert_eq!(ranked("teil ant", CHAPTERS), [3]);
        assert_eq!(ranked("zi", CHAPTERS), [2]);
        assert!(score("zsp", "Zwischenspiel I").is_none());
        assert!(score("bug", "archiv/agentischer-ueberarbeitungsplan").is_none());
        assert_eq!(score("aup", "archiv/agentischer-ueberarbeitungsplan"), None);
        assert_eq!(score("agu", "archiv/agentischer-ueberarbeitungsplan"), Some(5));
        assert_eq!(score("grube", "Größe über"), Some(5));
    }

    #[test]
    fn case_does_not_count() {
        assert_eq!(score("PROLOG", "Prolog"), Some(0));
        assert_eq!(score("änd", "Ändern"), Some(0));
    }
}
