//! What a prompt can carry along: the open chapter, the selected text, and what Druck reports as wrong. The author
//! sees each as a chip and removes the ones they do not want sent. Nothing here sends anything.

use std::collections::HashSet;
use std::ops::Range;

use crate::session::{ChapterId, Kind, Session};

/// The most of a selection that goes into a prompt.
const SELECTION_LIMIT: usize = 6_000;
/// The most diagnostics that go into a prompt.
const DIAGNOSTIC_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChipKind {
    Chapter,
    Selection,
    Diagnostics,
}

/// The selected text and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// First and last line, counting from 1.
    pub lines: (usize, usize),
    pub text: String,
}

impl Selected {
    /// The text in `range` of a chapter, with its lines counted in the chapter's file, which is what the agent reads:
    /// a later section of a single file starts below the sections before it.
    pub fn in_file(session: &Session, id: ChapterId, range: Range<usize>) -> Self {
        let buffer = session.buffer(id);
        let text = buffer.text();
        let last = range.end - usize::from(text[..range.end].ends_with('\n'));
        let before = lines_before(session, id);
        Self {
            lines: (
                before + buffer.line_of(range.start),
                before + buffer.line_of(last.max(range.start)),
            ),
            text: text[range].to_owned(),
        }
    }
}

/// The lines of a chapter's file above its text: in a single file, those of the sections before it.
fn lines_before(session: &Session, id: ChapterId) -> usize {
    let Kind::Single { .. } = session.kind() else {
        return 0;
    };
    let book = session.book();
    let at = book.iter().position(|other| *other == id).unwrap_or(0);
    (book[..at].iter())
        .map(|other| {
            session
                .buffer(*other)
                .text()
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
        })
        .sum()
}

/// The state of the document when the author began writing a prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    /// The open chapter's file, relative to the project folder, which is what a prompt names.
    pub chapter: Option<String>,
    /// Its title, which is what the author reads.
    pub chapter_title: String,
    pub selection: Option<Selected>,
    /// One line each, with the file and line they concern.
    pub diagnostics: Vec<String>,
}

impl Context {
    /// The chips to show, in order.
    pub fn chips(&self) -> Vec<ChipKind> {
        let mut chips = Vec::new();
        if self.chapter.is_some() {
            chips.push(ChipKind::Chapter);
        }
        if self.selection.is_some() {
            chips.push(ChipKind::Selection);
        }
        if !self.diagnostics.is_empty() {
            chips.push(ChipKind::Diagnostics);
        }
        chips
    }

    /// The prompt with the chips not in `removed` written above it, and the text that was added, if any.
    pub fn compose(&self, prompt: &str, removed: &HashSet<ChipKind>) -> (String, Option<String>) {
        let mut lines = Vec::new();
        let kept = |kind| self.chips().contains(&kind) && !removed.contains(&kind);
        if kept(ChipKind::Chapter) {
            lines.push(format!("Open chapter: {}", self.chapter.as_deref().unwrap_or_default()));
        }
        if let Some(selection) = self.selection.as_ref().filter(|_| kept(ChipKind::Selection)) {
            let cut = selection.text.char_indices().nth(SELECTION_LIMIT).map(|(end, _)| end);
            let shown = &selection.text[..cut.unwrap_or(selection.text.len())];
            let (first, last) = selection.lines;
            lines.push(format!("Selected text (lines {first} to {last}):"));
            lines.extend(shown.lines().map(|line| format!("> {line}")));
            if cut.is_some() {
                lines.push("> \u{2026} (the selection continues)".to_owned());
            }
        }
        if kept(ChipKind::Diagnostics) {
            lines.push("What Druck reports as wrong:".to_owned());
            lines.extend(
                self.diagnostics
                    .iter()
                    .take(DIAGNOSTIC_LIMIT)
                    .map(|line| format!("- {line}")),
            );
            if self.diagnostics.len() > DIAGNOSTIC_LIMIT {
                lines.push(format!("- and {} more", self.diagnostics.len() - DIAGNOSTIC_LIMIT));
            }
        }
        if lines.is_empty() {
            return (prompt.to_owned(), None);
        }
        let context = lines.join("\n");
        (
            format!("<druck-context>\n{context}\n</druck-context>\n\n{prompt}"),
            Some(context),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            chapter: Some("manuscript/03-weg.md".into()),
            chapter_title: "Der Weg".into(),
            selection: Some(Selected {
                lines: (12, 13),
                text: "Der erste Satz.\nDer zweite.".into(),
            }),
            diagnostics: vec!["manuscript/03-weg.md:12: warning: widow line".into()],
        }
    }

    #[test]
    fn chips_follow_what_the_document_has() {
        assert_eq!(
            context().chips(),
            [ChipKind::Chapter, ChipKind::Selection, ChipKind::Diagnostics]
        );
        assert_eq!(Context::default().chips(), []);
    }

    #[test]
    fn the_prompt_carries_the_chips_the_author_kept() {
        let removed = HashSet::from([ChipKind::Diagnostics]);
        let (message, sent) = context().compose("Tighten this.", &removed);
        assert_eq!(
            message,
            "<druck-context>\nOpen chapter: manuscript/03-weg.md\nSelected text (lines 12 to 13):\n> Der erste Satz.\n> Der zweite.\n</druck-context>\n\nTighten this."
        );
        assert!(!sent.expect("sent").contains("widow"));
    }

    #[test]
    fn with_every_chip_removed_the_prompt_goes_alone() {
        let removed = HashSet::from([ChipKind::Chapter, ChipKind::Selection, ChipKind::Diagnostics]);
        assert_eq!(context().compose("Hello", &removed), ("Hello".to_owned(), None));
    }

    #[test]
    fn a_long_selection_is_cut_and_says_so() {
        let mut long = context();
        long.selection = Some(Selected {
            lines: (1, 400),
            text: "word ".repeat(2_000),
        });
        let (_, sent) = long.compose("x", &HashSet::new());
        let sent = sent.expect("sent");
        assert!(sent.contains("(the selection continues)"));
        assert!(sent.len() < 7_000);
    }

    #[test]
    fn a_selection_in_a_later_section_of_a_single_file_counts_the_files_lines() {
        let file = std::env::temp_dir().join(format!("druck-selected-{}.md", std::process::id()));
        std::fs::write(&file, "# One\n\nThe same line.\n\n# Two\n\nThe same line.\n").expect("file");
        let session = Session::open(&file).expect("opens");
        std::fs::remove_file(&file).ok();
        let second = *session.book().last().expect("a second section");
        let text = session.buffer(second).text();
        let start = text.find("The same").expect("the line");
        let selected = Selected::in_file(&session, second, start..text.len());
        assert_eq!(selected.lines, (7, 7));
        assert_eq!(selected.text, "The same line.\n");
    }
}
