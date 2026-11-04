//! Comments as the galley treats them: the ones that open a chapter are its notes, and the panel edits their text where
//! it stands in the buffer.

use std::ops::Range;

use crate::source::{BlockKind, Blocks, Edit};

/// A comment that opens a chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// All of it, with `<!--` and `-->`.
    pub comment: Range<usize>,
    /// What the panel shows: inside the markers, without the whitespace at either end. Every other byte stays as written.
    pub shown: Range<usize>,
}

/// Whether a comment block has its `-->`. One without it is a mistake to show, never a note to hide.
pub fn is_closed(block_text: &str) -> bool {
    block_text
        .find("<!--")
        .is_some_and(|open| block_text[open + "<!--".len()..].contains("-->"))
}

/// The comments in a comment block, relative to the text.
fn comments_in(text: &str, range: &Range<usize>) -> Vec<Range<usize>> {
    let mut comments = Vec::new();
    let mut from = range.start;
    while let Some(open) = text[from..range.end].find("<!--") {
        let open = from + open;
        let body = open + "<!--".len();
        let Some(close) = text[body..range.end].find("-->") else {
            break;
        };
        let end = body + close + "-->".len();
        comments.push(open..end);
        from = end;
    }
    comments
}

/// The notes of a chapter: the comments before any other content, after front matter.
pub fn leading_notes(text: &str, blocks: &Blocks) -> Vec<Note> {
    let mut notes = Vec::new();
    for block in blocks.as_slice() {
        match block.kind {
            BlockKind::FrontMatter | BlockKind::Blank => {}
            BlockKind::Comment if is_closed(&text[block.range.clone()]) => {
                notes.extend(comments_in(text, &block.range).into_iter().map(|comment| Note {
                    shown: shown_range(text, &comment),
                    comment,
                }));
            }
            _ => break,
        }
    }
    notes
}

fn shown_range(text: &str, comment: &Range<usize>) -> Range<usize> {
    let inner = comment.start + "<!--".len()..comment.end - "-->".len();
    let written = &text[inner.clone()];
    let trimmed = written.trim();
    if trimmed.is_empty() {
        // New text goes after one space, if there is one.
        let at = inner.start + written.chars().next().map_or(0, char::len_utf8);
        return at..at;
    }
    let start = inner.start + written.len() - written.trim_start().len();
    start..start + trimmed.len()
}

/// The edit for a change in the panel: `range` of the note's shown text becomes `new`. Text that would close the comment
/// early is refused.
pub fn note_edit(text: &str, note: &Note, range: Range<usize>, new: &str) -> Option<Edit> {
    let start = note.shown.start + range.start;
    let end = note.shown.start + range.end;
    if start > note.shown.end || end > note.shown.end {
        return None;
    }
    let kept_before = &text[note.shown.start..start];
    let kept_after = &text[end..note.shown.end];
    let result = format!("{kept_before}{new}{kept_after}");
    (!result.contains("-->")).then(|| Edit::new(start..end, new))
}

/// A block range that can be hidden together with the blank lines after it, for caret movement: the comment blocks of the
/// chapter's start and the blank lines around them.
pub fn leading_region(text: &str, blocks: &Blocks) -> Option<Range<usize>> {
    let list = blocks.as_slice();
    let first = list
        .iter()
        .position(|block| block.kind != BlockKind::FrontMatter && block.kind != BlockKind::Blank)?;
    if list[first].kind != BlockKind::Comment || !is_closed(&text[list[first].range.clone()]) {
        return None;
    }
    let start = list[first].range.start;
    let mut end = list[first].range.end;
    for block in &list[first + 1..] {
        let hidden = block.kind == BlockKind::Blank
            || (block.kind == BlockKind::Comment && is_closed(&text[block.range.clone()]));
        if !hidden {
            break;
        }
        end = block.range.end;
    }
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Buffer;

    fn notes_of(text: &str) -> Vec<Note> {
        leading_notes(text, &Blocks::parse(text))
    }

    #[test]
    fn notes_are_the_comments_before_any_other_content() {
        let text = "---\na: b\n---\n\n<!-- one -->\n\n<!--\ntwo\n\nlines\n-->\n\n# T\n\n<!-- later -->\n";
        let shown: Vec<&str> = notes_of(text).iter().map(|note| &text[note.shown.clone()]).collect();
        assert_eq!(shown, ["one", "two\n\nlines"]);
        assert!(notes_of("# T\n\n<!-- later -->\n").is_empty());
        assert!(notes_of("<!-- c --> text\n").is_empty());
    }

    #[test]
    fn a_comment_never_closed_is_not_a_note() {
        assert!(notes_of("<!-- open\n\ntext\n").is_empty());
        assert_eq!(
            leading_region("<!-- open\n\ntext\n", &Blocks::parse("<!-- open\n\ntext\n")),
            None
        );
    }

    #[test]
    fn editing_a_note_changes_only_its_text() {
        let text = "<!--\n  a note  \n-->\n\n# T\n";
        let note = &notes_of(text)[0];
        let edit = note_edit(text, note, 2..6, "line").expect("allowed");
        let mut result = text.to_owned();
        result.replace_range(edit.range, &edit.text);
        assert_eq!(result, "<!--\n  a line  \n-->\n\n# T\n");
    }

    #[test]
    fn an_empty_comment_takes_text_after_its_space() {
        for (text, expected) in [
            ("<!-- -->\n", "<!-- x-->\n"),
            ("<!---->\n", "<!--x-->\n"),
            ("<!--  -->\n", "<!-- x -->\n"),
        ] {
            let note = &notes_of(text)[0];
            let edit = note_edit(text, note, 0..0, "x").expect("allowed");
            let mut result = text.to_owned();
            result.replace_range(edit.range, &edit.text);
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn text_that_would_close_the_comment_is_refused() {
        let text = "<!-- a- -->\n";
        let note = &notes_of(text)[0];
        assert!(note_edit(text, note, 0..0, "-->").is_none());
        assert!(note_edit(text, note, 2..2, "->").is_none());
        assert!(note_edit(text, note, 0..1, "b").is_some());
    }

    #[test]
    fn an_edit_and_its_undo_keep_every_byte() {
        let text = "<!--  a -->\n\nb\n";
        let mut buffer = Buffer::new(text);
        let note = &notes_of(text)[0];
        let edit = note_edit(text, note, 1..1, "x").expect("allowed");
        let caret = crate::source::Selection::caret(0);
        buffer
            .transact(
                crate::source::EditKind::Typing,
                vec![edit],
                caret,
                caret,
                std::time::Instant::now(),
            )
            .expect("valid");
        assert_eq!(buffer.text(), "<!--  ax -->\n\nb\n");
        buffer.undo();
        assert_eq!(buffer.text(), text);
    }

    #[test]
    fn the_leading_region_ends_at_the_first_visible_block() {
        let text = "<!-- a -->\n\n<!-- b -->\n\n# T\n";
        assert_eq!(leading_region(text, &Blocks::parse(text)), Some(0..24));
        assert_eq!(leading_region("# T\n", &Blocks::parse("# T\n")), None);
    }
}
