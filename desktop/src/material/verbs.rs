//! What Shift+Return writes into the text for each kind of item, without gpui. Every insert is one edit, so one undo.

use crate::source::{Blocks, Edit};

use super::model::{Form, Item, Kind, csv_rows, pipe_table};

/// What Shift+Return does with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert {
    /// A passage in quotation marks with a citation chip, at the caret. Without a key, the quotation alone.
    Quote { cited: bool },
    /// A figure of the image, after the caret's paragraph, as the image command writes one.
    Figure,
    /// The CSV as a pipe table, after the caret's paragraph.
    Table,
    /// A note's or probe's text as paragraphs, after the caret's paragraph.
    Passage,
    /// A citation of a bibliography entry, at the caret.
    Citation,
}

impl Insert {
    pub fn of(item: &Item) -> Self {
        match (item.form, item.kind) {
            (Form::Image, _) => Self::Figure,
            (Form::Table, _) => Self::Table,
            (Form::Source, _) => Self::Citation,
            (Form::Text, Kind::Research) => Self::Quote {
                cited: item.key.is_some(),
            },
            (Form::Text, _) => Self::Passage,
        }
    }
}

/// A quotation of `passage` in the book's language's marks, with a citation of `key` after it.
pub fn quotation(passage: &str, key: Option<&str>, language: &str) -> String {
    let (open, close) = if language.starts_with("de") {
        ('\u{201e}', '\u{201c}')
    } else {
        ('\u{201c}', '\u{201d}')
    };
    let passage = passage.trim().trim_matches(['"', '\u{201c}', '\u{201d}', '\u{201e}']);
    match key {
        Some(key) => format!("{open}{passage}{close} [@{key}]"),
        None => format!("{open}{passage}{close}"),
    }
}

/// `written` as it goes in at `at`: after a space unless it starts a line or follows one, or an opening bracket.
pub fn spaced(text: &str, at: usize, written: String) -> String {
    let before = text[..at].chars().next_back();
    match before {
        None => written,
        Some(char) if char.is_whitespace() || matches!(char, '(' | '[' | '\u{201e}' | '\u{201c}') => written,
        Some(_) => format!(" {written}"),
    }
}

/// What a table or passage insert writes for `item`: the CSV as a table, or the chosen passage, else the whole text.
pub fn block_text(item: &Item, csv: Option<&str>, passage: Option<&str>) -> Option<String> {
    match Insert::of(item) {
        Insert::Table => pipe_table(&csv_rows(csv?)),
        Insert::Passage => passage
            .map(str::to_owned)
            .or_else(|| item.text.as_deref().map(|text| text.trim().to_owned()))
            .filter(|text| !text.is_empty()),
        _ => None,
    }
}

/// An edit that puts `content` as blocks of their own after the block holding `caret`, and where the caret goes after it.
pub fn after_block(text: &str, caret: usize, content: &str) -> (Edit, usize) {
    let blocks = Blocks::parse(text);
    let at = blocks
        .as_slice()
        .get(blocks.index_at(caret.min(text.len())))
        .map_or(text.len(), |block| block.range.end);
    let before = if at == 0 || text[..at].ends_with("\n\n") {
        ""
    } else if text[..at].ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let after = if at == text.len() || text[at..].starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let written = format!("{before}{content}{after}");
    let end = at + before.len() + content.len();
    (Edit::new(at..at, written), end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quotation_takes_the_books_marks_and_a_citation() {
        assert_eq!(
            quotation(" They rounded. ", Some("s24"), "en"),
            "\u{201c}They rounded.\u{201d} [@s24]"
        );
        assert_eq!(
            quotation("\"Sie rundeten.\"", None, "de-AT"),
            "\u{201e}Sie rundeten.\u{201c}"
        );
    }

    #[test]
    fn an_inline_insert_after_a_word_takes_a_space() {
        assert_eq!(spaced("four.", 5, "[@k]".into()), " [@k]");
        assert_eq!(spaced("four. ", 6, "[@k]".into()), "[@k]");
        assert_eq!(spaced("(", 1, "[@k]".into()), "[@k]");
        assert_eq!(spaced("", 0, "[@k]".into()), "[@k]");
    }

    #[test]
    fn a_block_goes_after_the_carets_paragraph_with_blank_lines_around_it() {
        let text = "One.\n\nTwo.\n";
        let (edit, caret) = after_block(text, 1, "| a |");
        let mut written = text.to_owned();
        written.replace_range(edit.range.clone(), &edit.text);
        assert_eq!(written, "One.\n\n| a |\n\nTwo.\n");
        assert_eq!(&written[caret - 5..caret], "| a |");
        let (edit, _) = after_block("Last", 2, "B");
        assert_eq!(edit.text, "\n\nB\n");
    }
}
