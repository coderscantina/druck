//! What the shell reads out of a chapter's text: its open `{todo:}` placeholders and its word count. Holds no gpui types.
//!
//! Reading a chapter costs a parse, so each result is kept with the buffer revision it came from, and only chapters
//! that changed since are read again. [`compute`] is plain, so a caller can run it on a background thread.

use std::collections::HashMap;

use druck::editor::resolved_config::split_front_matter;
use druck::editor::statistics::{self, Statistics};

use crate::session::{ChapterId, Session};
use crate::source::Blocks;
use crate::source::inline::{SpanKind, has_inline, inline_spans};

/// An open placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    /// Where its `{` is, in the chapter's text.
    pub offset: usize,
    /// What it says, without its braces.
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub todos: Vec<Todo>,
    /// `None` when the chapter's text alone does not parse.
    pub statistics: Option<Statistics>,
}

/// The placeholders written in running text. Code, comments, and front matter hold none.
pub fn todos(text: &str) -> Vec<Todo> {
    let mut found = Vec::new();
    for block in Blocks::parse(text).as_slice() {
        if !has_inline(block.kind) {
            continue;
        }
        let body = &text[block.range.clone()];
        for span in inline_spans(block.kind, body) {
            if span.kind != SpanKind::Todo {
                continue;
            }
            let written = &body[span.range.clone()];
            let note = written
                .strip_prefix("{todo:")
                .and_then(|rest| rest.strip_suffix('}'))
                .unwrap_or(written);
            found.push(Todo {
                offset: block.range.start + span.range.start,
                text: note.split_whitespace().collect::<Vec<_>>().join(" "),
            });
        }
    }
    found
}

/// The placeholders and statistics of a chapter, read as `lang` (`"en"` or `"de"`).
pub fn compute(text: &str, lang: &str) -> Facts {
    let body = match split_front_matter(text) {
        Ok(Some(split)) => split.body,
        _ => text,
    };
    Facts {
        todos: todos(text),
        statistics: statistics::count(body, lang),
    }
}

/// A chapter whose facts are out of date, with what a thread needs to read it.
#[derive(Debug, Clone)]
pub struct Stale {
    pub id: ChapterId,
    pub revision: u64,
    pub text: String,
}

/// The facts of chapters, each with the revision they were read at.
#[derive(Default)]
pub struct Cache {
    read: HashMap<ChapterId, (u64, Facts)>,
}

impl Cache {
    /// The chapters of the book and the open one whose text changed since they were read.
    pub fn stale(&self, session: &Session) -> Vec<Stale> {
        let wanted = session.book().iter().copied().chain([session.active()]);
        let mut stale: Vec<Stale> = Vec::new();
        for id in wanted {
            let buffer = session.buffer(id);
            let current = self
                .read
                .get(&id)
                .is_some_and(|(revision, _)| *revision == buffer.revision());
            if !current && !stale.iter().any(|other| other.id == id) {
                stale.push(Stale {
                    id,
                    revision: buffer.revision(),
                    text: buffer.text().to_owned(),
                });
            }
        }
        stale
    }

    /// Keeps a result unless a newer one is already kept.
    pub fn store(&mut self, id: ChapterId, revision: u64, facts: Facts) {
        if self.read.get(&id).is_none_or(|(kept, _)| *kept <= revision) {
            self.read.insert(id, (revision, facts));
        }
    }

    pub fn get(&self, id: ChapterId) -> Option<&Facts> {
        self.read.get(&id).map(|(_, facts)| facts)
    }

    /// The facts as a plain map, for a report.
    pub fn all(&self) -> HashMap<ChapterId, Facts> {
        self.read.iter().map(|(id, (_, facts))| (*id, facts.clone())).collect()
    }

    /// The words of the whole book, once every chapter of it has been counted.
    pub fn book_words(&self, session: &Session) -> Option<usize> {
        session
            .book()
            .iter()
            .map(|id| self.get(*id)?.statistics.map(|statistics| statistics.words))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_found_in_text_but_not_in_code_or_comments() {
        let text = "# Title\n\nSome {todo: check\nthis} text.\n\n```\n{todo: code}\n```\n\n<!-- {todo: hidden} -->\n";
        let found = todos(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "check this");
        assert_eq!(&text[found[0].offset..found[0].offset + 6], "{todo:");
    }

    #[test]
    fn words_come_from_the_engine_and_skip_the_placeholder() {
        let facts = compute("# One\n\nA well-known fact {todo: more words here}.\n", "en");
        assert_eq!(facts.statistics.map(|s| s.words), Some(4));
    }

    #[test]
    fn front_matter_is_not_counted() {
        let facts = compute("---\ntitle: A title of words\n---\n\nTwo words.\n", "en");
        assert_eq!(facts.statistics.map(|s| s.words), Some(2));
    }
}
