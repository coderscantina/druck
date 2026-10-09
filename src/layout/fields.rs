//! Placeholder values that do not depend on the page: metadata, text statistics, the build date, and
//! the draft label. Slots and the body share them; bands add the page values.
//!
//! Counts are grouped by thousands for the document language, as in "12,480" and "12.480".

use crate::config::front_matter::MetaValue;
use crate::config::resolved::Config;
use crate::config::source::Source;
use crate::config::template::{Placeholder, Value};
use crate::config::theme::Lang;
use crate::date::Date;
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Document, Inline};
use crate::statistics::Statistics;

pub struct Fields<'a> {
    config: &'a Config,
    statistics: Statistics,
    today: Date,
}

impl<'a> Fields<'a> {
    /// The values for `document`, built on `today`.
    pub fn new(document: &Document, config: &'a Config, today: Date) -> Self {
        Self {
            config,
            statistics: Statistics::count(document, config.document.lang),
            today,
        }
    }

    /// The value of a placeholder that does not depend on the page. Authors are joined with commas.
    /// Blank values, page placeholders, and `{draft}` outside drafts have none.
    pub fn value(&self, placeholder: &Placeholder) -> Option<Value> {
        let metadata = &self.config.metadata;
        let lang = self.config.document.lang;
        let count = |n: usize| Some(grouped(n, lang));
        let text = match placeholder {
            Placeholder::Title => metadata.title.clone(),
            Placeholder::Subtitle => metadata.subtitle.clone(),
            Placeholder::Author => Some(metadata.authors.join(", ")),
            Placeholder::Date => metadata.date.clone(),
            Placeholder::Abstract => metadata.abstract_.clone(),
            Placeholder::Meta(key) => match metadata.meta.get(key)? {
                MetaValue::Text(text) => Some(text.clone()),
                MetaValue::Lines(lines) => return Some(Value::Lines(lines.clone())),
            },
            Placeholder::Chars => count(self.statistics.chars),
            Placeholder::CharsNoSpaces => count(self.statistics.chars_no_spaces),
            Placeholder::Words => count(self.statistics.words),
            Placeholder::Sentences => count(self.statistics.sentences),
            Placeholder::Paragraphs => count(self.statistics.paragraphs),
            Placeholder::ReadingTime => count(self.statistics.reading_time(lang)),
            Placeholder::Figures => count(self.statistics.figures),
            Placeholder::Tables => count(self.statistics.tables),
            Placeholder::BuildDate => Some(self.today.long(lang)),
            Placeholder::Year => Some(self.today.year.to_string()),
            Placeholder::Draft => self.config.document.draft.then(|| self.config.labels.draft.clone()),
            Placeholder::Section
            | Placeholder::Subsection
            | Placeholder::Page
            | Placeholder::Pages
            | Placeholder::SectionPage
            | Placeholder::SectionPages => None,
        };
        text.filter(|text| !text.trim().is_empty()).map(Value::Text)
    }

    /// The text a placeholder in the body shows: its value, with list entries joined by commas.
    pub fn text(&self, placeholder: &Placeholder) -> Option<String> {
        match self.value(placeholder)? {
            Value::Text(text) => Some(text),
            Value::Lines(lines) => {
                let entries: Vec<_> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
                (!entries.is_empty()).then(|| entries.join(", "))
            }
        }
    }

    /// Reports each placeholder in the body and footnotes without a value, at the placeholder.
    pub fn check(&self, document: &Document, source: &Source) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        let mut visit = |content: &[Inline]| {
            for inline in content {
                if let Inline::Field { placeholder, at, .. } = inline
                    && self.text(placeholder).is_none()
                {
                    let message = match placeholder {
                        Placeholder::Draft => "{draft} has a value only in drafts; set draft: true".to_owned(),
                        Placeholder::Meta(key) => {
                            format!("{{meta.{key}}} has no value; set meta.{key} in the front matter")
                        }
                        other => format!("{{{other}}} has no value; set {other} in the front matter"),
                    };
                    errors.push(Diagnostic::new(Some(source.clone()), message).at(at.line, at.column));
                }
            }
        };
        walk(&document.blocks, &mut visit);
        for footnote in &document.footnotes {
            walk(&footnote.blocks, &mut visit);
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

/// Calls `visit` with the inline content of every block in `blocks`.
fn walk(blocks: &[Block], visit: &mut impl FnMut(&[Inline])) {
    for block in blocks {
        match block {
            Block::Paragraph { content, .. } | Block::Heading { content, .. } => visit(content),
            Block::Image { caption, .. } => visit(caption),
            Block::Table {
                header, rows, caption, ..
            } => {
                for cell in std::iter::once(header).chain(rows).flat_map(|row| &row.cells) {
                    walk(&cell.blocks, visit);
                }
                visit(caption);
            }
            Block::List { items, .. } => items.iter().for_each(|item| walk(item, visit)),
            Block::Quote { blocks, .. }
            | Block::Keep { blocks, .. }
            | Block::Bottom { blocks, .. }
            | Block::Columns { blocks, .. }
            | Block::FullWidth { blocks, .. } => walk(blocks, visit),
            Block::Code { .. }
            | Block::PageBreak { .. }
            | Block::Matter { .. }
            | Block::Contents { .. }
            | Block::SceneBreak { .. }
            | Block::Bibliography { .. } => {}
        }
    }
}

/// `n` with its digits grouped by three from 1,000 on: a comma in English, a full stop in German.
fn grouped(n: usize, lang: Lang) -> String {
    let separator = match lang {
        Lang::En => ',',
        Lang::De => '.',
    };
    let digits = n.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(separator);
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_digits_by_language() {
        assert_eq!(grouped(999, Lang::En), "999");
        assert_eq!(grouped(1240, Lang::En), "1,240");
        assert_eq!(grouped(1_234_567, Lang::De), "1.234.567");
    }
}
