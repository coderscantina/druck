//! Text statistics for `{chars}`, `{words}`, and the other count placeholders.
//!
//! Counts cover the text the author wrote in the body and the footnotes: paragraphs, list items,
//! quotations, headings, table cells, and captions. Code blocks, generated text (heading numbers,
//! citations, cross-references, footnote markers), and placeholders count as no text, so a count never
//! depends on the values it shows. Each block's text is counted on its own, with runs of white space
//! read as one space and none at its ends.
//!
//! - Characters are user-perceived characters, extended grapheme clusters, so "é" written as "e" with a
//!   combining accent and a flag emoji count once each. Without spaces leaves out white space.
//! - Words are [UAX #29](https://www.unicode.org/reports/tr29/) words that hold a letter or digit, with
//!   words joined by a hyphen counted as one, as in "well-known". "can't", "3.5", and "12,480" are one
//!   word each; "word—word" with a dash is two.
//! - Sentences are UAX #29 sentences that hold a word. A full stop after a single letter ("J. R. R.",
//!   "z. B."), or a common abbreviation of the document language ("Dr.", "vgl.") does not end a
//!   sentence, nor in German does a number before a month or "Jahrhundert", which is an ordinal, as in
//!   "am 3. Oktober". Every block ends a sentence.
//! - Paragraphs are paragraph blocks that hold a word, including those in lists, quotations, and
//!   footnotes. Headings, table cells, and captions count toward characters and words only.
//! - Figures and tables are those with a caption, which are the numbered ones.

use unicode_segmentation::UnicodeSegmentation;

use crate::config::theme::Lang;
use crate::document::{Block, Document, Inline};

/// Abbreviations that do not end a sentence, without their full stop.
const ABBREVIATIONS_EN: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "Prof", "Sr", "Jr", "St", "Mt", "vs", "cf", "e.g", "i.e", "approx", "al", "Fig", "Figs",
    "Eq", "Eqs", "Vol", "Vols", "No", "Nos", "pp", "p", "Ch", "Sec", "ed", "eds",
];
const ABBREVIATIONS_DE: &[&str] = &[
    "z.B", "d.h", "u.a", "o.ä", "bzw", "ca", "vgl", "Nr", "S", "Dr", "Prof", "Hr", "Fr", "evtl", "ggf", "inkl", "zzgl",
    "Abb", "Tab", "Kap", "Bd", "Hrsg", "Aufl", "Jh", "Mio", "Mrd", "St", "Str", "geb", "sog", "bspw", "allg", "Anm",
    "Tel", "max", "min", "gem", "lt", "urspr",
];

/// German words an ordinal number commonly precedes, written with a full stop.
const ORDINAL_NOUNS_DE: &[&str] = &[
    "Januar",
    "Jänner",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
    "Jahrhundert",
    "Jahrhunderts",
];

/// Silent reading speeds in words per minute, from the same study for both languages:
/// Trauzettel-Klosinski and Dietz (2012), "Standardized Assessment of Reading Performance: The New
/// International Reading Speed Texts IReST".
const WORDS_PER_MINUTE_EN: usize = 228;
const WORDS_PER_MINUTE_DE: usize = 179;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Statistics {
    pub chars: usize,
    pub chars_no_spaces: usize,
    pub words: usize,
    pub sentences: usize,
    pub paragraphs: usize,
    pub figures: usize,
    pub tables: usize,
}

impl Statistics {
    /// The statistics of `document`, whose sentences follow the conventions of `lang`.
    pub fn count(document: &Document, lang: Lang) -> Self {
        let mut counter = Counter {
            statistics: Self::default(),
            lang,
        };
        counter.blocks(&document.blocks);
        for footnote in &document.footnotes {
            counter.blocks(&footnote.blocks);
        }
        counter.statistics
    }

    /// The reading time in whole minutes, rounded up, at least 1 for a document with words.
    pub fn reading_time(&self, lang: Lang) -> usize {
        let speed = match lang {
            Lang::En => WORDS_PER_MINUTE_EN,
            Lang::De => WORDS_PER_MINUTE_DE,
        };
        self.words.div_ceil(speed)
    }
}

struct Counter {
    statistics: Statistics,
    lang: Lang,
}

impl Counter {
    fn blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::Paragraph { content, .. } => self.text(content, true),
                Block::Heading { content, .. } => self.text(content, false),
                Block::List { items, .. } => items.iter().for_each(|item| self.blocks(item)),
                Block::Quote { blocks, .. }
                | Block::Keep { blocks, .. }
                | Block::Columns { blocks, .. }
                | Block::FullWidth { blocks, .. } => self.blocks(blocks),
                Block::Image { caption, .. } => {
                    self.statistics.figures += usize::from(!caption.is_empty());
                    self.text(caption, false);
                }
                Block::Table {
                    header, rows, caption, ..
                } => {
                    self.statistics.tables += usize::from(!caption.is_empty());
                    for cell in std::iter::once(header).chain(rows).flat_map(|row| &row.cells) {
                        self.cell(&cell.blocks);
                    }
                    self.text(caption, false);
                }
                Block::Code { .. }
                | Block::PageBreak { .. }
                | Block::Matter { .. }
                | Block::Contents { .. }
                | Block::SceneBreak { .. }
                | Block::Bibliography { .. } => {}
            }
        }
    }

    /// A table cell's blocks, whose paragraphs count toward characters and words only.
    fn cell(&mut self, blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::Paragraph { content, .. } => self.text(content, false),
                Block::List { items, .. } => items.iter().for_each(|item| self.cell(item)),
                other => self.blocks(std::slice::from_ref(other)),
            }
        }
    }

    /// Counts a block's text, and with `prose` its sentences and the paragraph.
    fn text(&mut self, content: &[Inline], prose: bool) {
        let text = written(content);
        let words = words(&text);
        if words == 0 && text.is_empty() {
            return;
        }
        let statistics = &mut self.statistics;
        statistics.chars += text.graphemes(true).count();
        statistics.chars_no_spaces += text
            .graphemes(true)
            .filter(|g| !g.chars().all(char::is_whitespace))
            .count();
        statistics.words += words;
        if prose && words > 0 {
            statistics.sentences += sentences(&text, self.lang);
            statistics.paragraphs += 1;
        }
    }
}

/// The text the author wrote in `content`, with white space runs as one space and none at the ends.
fn written(content: &[Inline]) -> String {
    let mut raw = String::new();
    for inline in content {
        match inline {
            Inline::Text { text, .. } => raw.push_str(text),
            Inline::LineBreak => raw.push(' '),
            Inline::FootnoteRef(_) | Inline::Ref(_) | Inline::Citation { .. } | Inline::Field { .. } => {}
        }
    }
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// UAX #29 words, with words joined by a single hyphen counted as one.
fn words(text: &str) -> usize {
    let mut count = 0;
    let mut previous_end = None;
    for (start, word) in text.unicode_word_indices() {
        let joined = previous_end.is_some_and(|end| matches!(&text[end..start], "-" | "\u{2010}" | "\u{2011}"));
        count += usize::from(!joined);
        previous_end = Some(start + word.len());
    }
    count
}

/// UAX #29 sentences holding a word, without breaks after abbreviations.
fn sentences(text: &str, lang: Lang) -> usize {
    let bounds: Vec<&str> = text.split_sentence_bounds().collect();
    let mut count = 0;
    let mut open = false;
    for (index, sentence) in bounds.iter().enumerate() {
        open |= words(sentence) > 0;
        let next = bounds.get(index + 1);
        if open && !next.is_some_and(|next| continues(sentence, next, lang)) {
            count += 1;
            open = false;
        }
    }
    count
}

/// Whether the sentence UAX #29 ends after `sentence` goes on in `next`, since its full stop ends an
/// abbreviation or an ordinal number.
fn continues(sentence: &str, next: &str, lang: Lang) -> bool {
    let Some(before) = sentence.trim_end().strip_suffix('.') else {
        return false;
    };
    let token = before
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or_default()
        .trim_start_matches(['(', '[', '"', '\u{201c}', '\u{201e}', '\u{2018}', '\u{201a}']);
    let mut chars = token.chars();
    let single_letter = chars.next().is_some_and(char::is_alphabetic) && chars.next().is_none();
    let (abbreviations, ordinal) = match lang {
        Lang::En => (ABBREVIATIONS_EN, false),
        Lang::De => {
            let number = !token.is_empty() && token.chars().all(|c| c.is_ascii_digit());
            let noun = next
                .unicode_words()
                .next()
                .is_some_and(|word| ORDINAL_NOUNS_DE.contains(&word));
            (ABBREVIATIONS_DE, number && noun)
        }
    };
    single_letter || ordinal || abbreviations.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::template::Placeholder;
    use crate::document::{Cell, Footnote, InlineStyle, Location, Row};

    const AT: Location = Location { line: 1, column: 1 };

    fn text(text: &str) -> Inline {
        Inline::Text {
            text: text.to_owned(),
            style: InlineStyle::default(),
        }
    }

    fn paragraph(content: Vec<Inline>) -> Block {
        Block::Paragraph {
            at: AT,
            content,
            class: None,
        }
    }

    fn count(blocks: Vec<Block>, lang: Lang) -> Statistics {
        let document = Document {
            blocks,
            ..Document::default()
        };
        Statistics::count(&document, lang)
    }

    #[test]
    fn counts_grapheme_clusters_with_and_without_collapsed_spaces() {
        let statistics = count(vec![paragraph(vec![text("  Cafe\u{301}   au\nlait 🇦🇹 ")])], Lang::En);
        // "Café au lait 🇦🇹": 14 characters, 11 without the three spaces.
        assert_eq!((statistics.chars, statistics.chars_no_spaces), (14, 11));
    }

    #[test]
    fn counts_hyphenated_compounds_as_one_word_and_dashes_as_separators() {
        let sample = "A well-known fact: can't, 3.5 and 12,480 \u{2014} word\u{2014}word, e-mail. (Yes!)";
        assert_eq!(words(sample), 11);
        assert_eq!(words("Ein- und Ausgang"), 3);
    }

    #[test]
    fn abbreviations_initials_and_german_ordinals_do_not_end_sentences() {
        let english = "Dr. Smith met J. R. R. Tolkien in 1950. He was late, e.g. by an hour. Was he? Yes";
        assert_eq!(sentences(english, Lang::En), 4);
        let german = "Wir treffen uns am 3. Oktober, vgl. S. 12. Das ist z. B. sinnvoll. Nr. 5 folgt im 20. \
            Jahrhundert.";
        assert_eq!(sentences(german, Lang::De), 3);
        // In English a number can end a sentence.
        assert_eq!(sentences("It began in 2024. Then it ended.", Lang::En), 2);
    }

    #[test]
    fn generated_text_and_placeholders_count_as_nothing() {
        let field = Inline::Field {
            placeholder: Placeholder::Words,
            at: AT,
            style: InlineStyle::default(),
        };
        let content = vec![
            text("This text has "),
            field,
            text(" words"),
            Inline::FootnoteRef(0),
            text("."),
        ];
        let document = Document {
            blocks: vec![paragraph(content)],
            footnotes: vec![Footnote {
                at: AT,
                blocks: vec![paragraph(vec![text("A note.")])],
            }],
            ..Document::default()
        };
        let statistics = Statistics::count(&document, Lang::En);
        assert_eq!(
            (statistics.words, statistics.sentences, statistics.paragraphs),
            (6, 2, 2)
        );
        assert_eq!(statistics.chars, "This text has words.".len() + "A note.".len());
    }

    #[test]
    fn headings_cells_and_captions_count_words_but_not_sentences_or_paragraphs() {
        let row = |words: &str| Row {
            at: AT,
            cells: vec![Cell {
                at: AT,
                blocks: vec![paragraph(vec![text(words)])],
                span: 1,
                class: None,
            }],
            class: None,
        };
        let blocks = vec![
            Block::Heading {
                at: AT,
                level: 1,
                content: vec![text("Two words")],
                label: None,
                class: None,
                unnumbered: false,
                unlisted: false,
            },
            Block::Table {
                at: AT,
                columns: Vec::new(),
                header: row("Name"),
                rows: vec![row("Druck")],
                caption: vec![text("A caption.")],
                label: None,
            },
            Block::Image {
                at: AT,
                image: 0,
                caption: Vec::new(),
                label: None,
            },
            Block::Code {
                at: AT,
                first_line: 1,
                lines: vec!["let ignored = true;".to_owned()],
            },
        ];
        let statistics = count(blocks, Lang::En);
        assert_eq!(
            (statistics.words, statistics.sentences, statistics.paragraphs),
            (6, 0, 0)
        );
        assert_eq!((statistics.figures, statistics.tables), (0, 1));
    }

    #[test]
    fn reading_time_rounds_up_by_language() {
        let statistics = Statistics {
            words: 229,
            ..Statistics::default()
        };
        assert_eq!(statistics.reading_time(Lang::En), 2);
        assert_eq!(statistics.reading_time(Lang::De), 2);
        assert_eq!(Statistics::default().reading_time(Lang::En), 0);
    }
}
