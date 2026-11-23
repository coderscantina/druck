//! Fenced code blocks without their fences: the language as a choice from a list, the fence lines as lines the caret
//! never stands on, and the way out of the block by a key.

use std::ops::Range;

use super::{Command, line_ending};
use crate::source::blocks::{Block, closes_fence};
use crate::source::{BlockKind, Blocks, Edit, Selection};

/// The languages the list offers, by name and as a fence writes them. The engine knows about 200 more, which the
/// source view takes.
pub const LANGUAGES: [(&str, &str); 22] = [
    ("Bash", "bash"),
    ("C", "c"),
    ("C++", "cpp"),
    ("CSS", "css"),
    ("Diff", "diff"),
    ("Go", "go"),
    ("HTML", "html"),
    ("Java", "java"),
    ("JavaScript", "js"),
    ("JSON", "json"),
    ("Kotlin", "kotlin"),
    ("Markdown", "markdown"),
    ("PHP", "php"),
    ("Python", "python"),
    ("Ruby", "ruby"),
    ("Rust", "rust"),
    ("SQL", "sql"),
    ("Swift", "swift"),
    ("TOML", "toml"),
    ("TypeScript", "ts"),
    ("XML", "xml"),
    ("YAML", "yaml"),
];

/// The fence lines of a code block, without their line endings, relative to the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fences {
    pub open: Range<usize>,
    /// Missing while the block runs to the end of the text unclosed.
    pub close: Option<Range<usize>>,
    /// The language word in the opening line. Empty, at the line's end, when none is written.
    pub language: Range<usize>,
}

fn fence_length(line: &str) -> Option<(u8, usize)> {
    let marker = *line.as_bytes().first()?;
    let length = line.bytes().take_while(|byte| *byte == marker).count();
    (matches!(marker, b'`' | b'~') && length >= 3).then_some((marker, length))
}

/// The fences of a block's text, if it is a fenced code block. An indented one has none.
pub fn fences(block: &str) -> Option<Fences> {
    let first_end = block.find('\n').unwrap_or(block.len());
    let first = block[..first_end].trim_end_matches('\r');
    let indent = first.len() - first.trim_start_matches(' ').len();
    let (marker, length) = fence_length(&first[indent..])?;
    let info_start = indent + length;
    let info = &first[info_start..];
    let word_start = info_start + (info.len() - info.trim_start().len());
    let word_len = first[word_start..]
        .find([' ', ',', '\t'])
        .unwrap_or(first.len() - word_start);
    let body = block.trim_end_matches(['\n', '\r']);
    let last_start = body.rfind('\n').map(|at| at + 1);
    let close = last_start.filter(|start| closes_fence(&body[*start..], marker, length));
    Some(Fences {
        open: 0..first.len(),
        close: close.map(|start| start..body.len()),
        language: word_start..word_start + word_len,
    })
}

/// The language a fenced block names, as written. Empty for none.
pub fn language(block: &str) -> Option<&str> {
    fences(block).map(|fences| &block[fences.language])
}

/// Sets the language of the fenced block `block`. An empty `word` takes it off. Only the word changes.
pub fn set_language(text: &str, block: &Block, word: &str) -> Option<Edit> {
    if block.kind != BlockKind::Code {
        return None;
    }
    let found = fences(&text[block.range.clone()])?;
    let range = block.range.start + found.language.start..block.range.start + found.language.end;
    (&text[range.clone()] != word).then(|| Edit::new(range, word))
}

/// Where the caret goes when `target` falls on a fence line of `block`, moving `forward` or back: past the line, so the
/// caret never stands on a fence. Anywhere else it is `target`.
pub fn past_fence(text: &str, block: &Block, target: usize, forward: bool) -> usize {
    if block.kind != BlockKind::Code {
        return target;
    }
    let written = &text[block.range.clone()];
    let Some(found) = fences(written) else {
        return target;
    };
    let start = block.range.start;
    let relative = target - start;
    let on_open = relative <= found.open.end;
    let on_close = found.close.as_ref().is_some_and(|close| relative >= close.start);
    if !on_open && !on_close {
        return target;
    }
    let before = start.checked_sub(1);
    let after = written.ends_with('\n').then_some(block.range.end);
    // The first code line's start and the last one's end, when the block has a line between its fences.
    let first = written.find('\n').map(|at| at + 1);
    let last = match &found.close {
        Some(close) => {
            let above = &written[..close.start];
            let above = above.strip_suffix('\n').unwrap_or(above);
            above.strip_suffix('\r').unwrap_or(above).len()
        }
        None => written.trim_end_matches(['\n', '\r']).len(),
    };
    let inside = first.filter(|first| *first <= last);
    let first = inside.map(|first| start + first);
    let last = inside.map(|_| start + last);
    let moved = match (on_open, forward) {
        (true, true) => first.or(after),
        (true, false) => before.or(first),
        (false, true) => after.or(last),
        (false, false) => last.or(before),
    };
    moved.unwrap_or(target)
}

/// Leaves the code block the caret is in: the caret goes to an empty line after it, which is written when the text has
/// none there. `None` outside a closed fenced block.
pub fn leave(text: &str, blocks: &Blocks, caret: usize) -> Option<Command> {
    let index = blocks.index_at(caret);
    let list = blocks.as_slice();
    let block = &list[index];
    if block.kind != BlockKind::Code || fences(&text[block.range.clone()])?.close.is_none() {
        return None;
    }
    let eol = line_ending(text);
    let end = block.range.end;
    let (edit, caret) = match (list.get(index + 1), list.get(index + 2)) {
        (None, _) => {
            let written = if text.ends_with('\n') {
                eol.to_owned()
            } else {
                format!("{eol}{eol}")
            };
            (Some(Edit::new(end..end, written.clone())), end + written.len())
        }
        (Some(blank), None) if blank.kind == BlockKind::Blank => (None, text.len()),
        (Some(blank), Some(next)) if blank.kind == BlockKind::Blank => {
            let at = next.range.start;
            (Some(Edit::new(at..at, format!("{eol}{eol}"))), at)
        }
        _ => (Some(Edit::new(end..end, format!("{eol}{eol}"))), end + eol.len()),
    };
    Some(Command {
        edits: edit.into_iter().collect(),
        selection: Selection::caret(caret),
    })
}

/// Down from the caret's last code line, aiming at `target`, in a closed block with only blank lines after it: there is
/// no line below to go to, so it leaves the block as [`leave`] does. `None` anywhere else.
pub fn leave_at_end(text: &str, blocks: &Blocks, caret: usize, target: usize) -> Option<Command> {
    let index = blocks.index_at(caret);
    let list = blocks.as_slice();
    let block = &list[index];
    if block.kind != BlockKind::Code || !list[index + 1..].iter().all(|after| after.kind == BlockKind::Blank) {
        return None;
    }
    let close = fences(&text[block.range.clone()])?.close?;
    (target >= block.range.start + close.start).then(|| leave(text, blocks, caret))?
}

#[cfg(test)]
mod tests {
    use super::super::applied;
    use super::*;

    fn left(text: &str) -> (String, usize) {
        let caret = text.find('|').expect("a caret");
        let text = text.replacen('|', "", 1);
        let blocks = Blocks::parse(&text);
        let command = leave(&text, &blocks, caret).expect("a way out");
        let mut after = applied(&text, &command.edits);
        after.insert(command.selection.head, '|');
        (after, command.edits.len())
    }

    #[test]
    fn the_language_is_the_first_word_after_the_fence_and_only_it_changes() {
        let text = "A\n\n```rust,ignore\nx\n```\n";
        let blocks = Blocks::parse(text);
        let block = &blocks.as_slice()[2];
        assert_eq!(language(&text[block.range.clone()]), Some("rust"));
        let edit = set_language(text, block, "python").expect("an edit");
        assert_eq!(applied(text, &[edit]), "A\n\n```python,ignore\nx\n```\n");
        let edit = set_language(text, block, "").expect("an edit");
        assert_eq!(applied(text, &[edit]), "A\n\n```,ignore\nx\n```\n");
        assert!(set_language(text, block, "rust").is_none());
        let plain = "~~~\nx\n~~~\n";
        let blocks = Blocks::parse(plain);
        let edit = set_language(plain, &blocks.as_slice()[0], "ts").expect("an edit");
        assert_eq!(applied(plain, &[edit]), "~~~ts\nx\n~~~\n");
    }

    #[test]
    fn an_indented_block_has_no_fences_and_an_unclosed_one_no_closing_line() {
        assert_eq!(fences("    x\n"), None);
        assert_eq!(fences("```\nx\n").expect("fenced").close, None);
        assert_eq!(fences("````\n```\n````\n").expect("fenced").close, Some(9..13));
    }

    #[test]
    fn the_caret_passes_the_fence_lines() {
        let text = "A\n\n```rust\nx\ny\n```\n\nB\n";
        let blocks = Blocks::parse(text);
        let block = &blocks.as_slice()[2];
        let open = text.find("```").unwrap();
        let x = text.find('x').unwrap();
        let y = text.find('y').unwrap();
        let close = text.rfind("```").unwrap();
        assert_eq!(past_fence(text, block, open + 2, true), x);
        assert_eq!(past_fence(text, block, open + 7, false), open - 1);
        assert_eq!(past_fence(text, block, close, true), close + 4);
        assert_eq!(past_fence(text, block, close + 3, false), y + 1);
        assert_eq!(past_fence(text, block, y, true), y);
        // With no line ending after it, there is nowhere after the block.
        let text = "```\nx\n```";
        let blocks = Blocks::parse(text);
        assert_eq!(past_fence(text, &blocks.as_slice()[0], 7, true), 5);
    }

    #[test]
    fn leaving_goes_to_an_empty_line_after_the_block_and_writes_one_when_missing() {
        assert_eq!(left("```\nx|\n```\n"), ("```\nx\n```\n\n|".to_owned(), 1));
        assert_eq!(left("```\nx|\n```"), ("```\nx\n```\n\n|".to_owned(), 1));
        assert_eq!(left("```\nx|\n```\n\n"), ("```\nx\n```\n\n|".to_owned(), 0));
        assert_eq!(left("```\n|x\n```\n\nB\n"), ("```\nx\n```\n\n|\n\nB\n".to_owned(), 1));
        assert_eq!(
            left("::: keep\n```\n|x\n```\n:::\n"),
            ("::: keep\n```\nx\n```\n\n|\n:::\n".to_owned(), 1)
        );
        let text = "Text\n";
        assert!(leave(text, &Blocks::parse(text), 2).is_none());
    }

    #[test]
    fn down_from_the_last_code_line_at_the_end_of_the_text_leaves_the_block() {
        let text = "A\n\n```\nx\ny\n```\n";
        let blocks = Blocks::parse(text);
        let y = text.find('y').unwrap();
        let close = text.rfind("```").unwrap();
        let command = leave_at_end(text, &blocks, y, text.len()).expect("a way out");
        assert_eq!(applied(text, &command.edits), "A\n\n```\nx\ny\n```\n\n");
        assert_eq!(command.selection.head, text.len() + 1);
        assert!(leave_at_end(text, &blocks, y, close).is_some());
        // Down to a code line, or with text after the block, moves as it does elsewhere.
        assert!(leave_at_end(text, &blocks, text.find('x').unwrap(), y).is_none());
        let text = "```\nx\n```\n\nB\n";
        let blocks = Blocks::parse(text);
        assert!(leave_at_end(text, &blocks, 4, text.len()).is_none());
    }
}
