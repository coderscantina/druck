//! Footnotes as text: which label comes next, where a definition goes, and how its text is read and written. A note's
//! text is whatever stands after `[^label]: `, with four spaces of indent on the lines after the first.

use std::ops::Range;

use super::facts::{Definition, Facts};
use crate::editor::commands::{Command, body_range, line_ending, needs_gap, smallest_edit};
use crate::source::{BlockKind, Blocks, Edit, Selection};

const CONTINUATION: &str = "    ";

/// Case does not matter to a label.
pub fn folded(label: &str) -> String {
    label.to_lowercase()
}

/// A label's stem and number: `k15-3` is `k15-` and 3.
fn numbered(label: &str) -> Option<(&str, u64)> {
    let digits = label.bytes().rev().take_while(u8::is_ascii_digit).count();
    let (stem, number) = label.split_at(label.len() - digits);
    Some((stem, number.parse().ok()?))
}

/// The stem most of these labels use. Ties go to the one seen last, which is nearest the text being written.
fn usual_stem<'a>(labels: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for label in labels {
        let Some((stem, _)) = numbered(label) else {
            continue;
        };
        match counts.iter_mut().find(|(known, _)| known == stem) {
            Some((_, count)) => *count += 1,
            None => counts.push((stem.to_owned(), 1)),
        }
    }
    let mut best: Option<(String, usize)> = None;
    for (stem, count) in counts {
        if best.as_ref().is_none_or(|(_, top)| count >= *top) {
            best = Some((stem, count));
        }
    }
    best.map(|(stem, _)| stem)
}

/// Letters then digits, such as `k15`, the kind of word a book names its chapter labels after.
fn is_chapter_word(word: &str) -> bool {
    let letters = word.bytes().take_while(u8::is_ascii_alphabetic).count();
    letters > 0 && letters < word.len() && word.bytes().skip(letters).all(|byte| byte.is_ascii_digit())
}

/// The next label for a note in this chapter, following the pattern its notes use, or the book's if it has none, and
/// unique across `book`, which holds every label of every chapter.
pub fn next_label(file_stem: &str, chapter: &Facts, book: &[String]) -> String {
    let own = chapter
        .refs
        .iter()
        .map(|note| note.label.as_str())
        .chain(chapter.definitions.iter().map(|note| note.label.as_str()));
    let stem = usual_stem(own).or_else(|| {
        let usual = usual_stem(book.iter().map(String::as_str))?;
        // `k01-` in a book means `k15-` in the file `46-k15.md`.
        let word = usual.trim_end_matches(['-', '_', '.']);
        let separator = &usual[word.len()..];
        if is_chapter_word(word)
            && let Some(mine) = file_stem.split(['-', '_', '.', ' ']).find(|part| is_chapter_word(part))
        {
            return Some(format!("{}{separator}", mine.to_lowercase()));
        }
        Some(usual)
    });
    let stem = stem.unwrap_or_else(|| "note-".to_owned());
    let taken: std::collections::HashSet<String> = book.iter().map(|label| folded(label)).collect();
    let highest = book
        .iter()
        .filter_map(|label| numbered(label))
        .filter(|(known, _)| folded(known) == folded(&stem))
        .map(|(_, number)| number)
        .max()
        .unwrap_or(0);
    (highest + 1..)
        .map(|number| format!("{stem}{number}"))
        .find(|label| !taken.contains(&folded(label)))
        .expect("some label is free")
}

/// Where a document keeps its definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keeping {
    /// Together, after the text they belong to. Something such as a closing line may still follow them.
    AtEnd,
    /// Each below the block that refers to it.
    BelowBlock,
}

/// Where this text keeps its definitions, if it has any.
pub fn keeping(blocks: &Blocks) -> Option<Keeping> {
    let list = blocks.as_slice();
    let first = list
        .iter()
        .position(|block| block.kind == BlockKind::FootnoteDefinition)?;
    let last = list
        .iter()
        .rposition(|block| block.kind == BlockKind::FootnoteDefinition)?;
    let apart = list[first..=last].iter().any(|block| {
        !matches!(
            block.kind,
            BlockKind::FootnoteDefinition | BlockKind::Blank | BlockKind::Comment
        )
    });
    // One definition with text after it says nothing of being kept together.
    let text_after = list[last..].iter().any(|block| {
        !matches!(
            block.kind,
            BlockKind::FootnoteDefinition | BlockKind::Blank | BlockKind::Comment
        )
    });
    Some(if apart || (first == last && text_after) {
        Keeping::BelowBlock
    } else {
        Keeping::AtEnd
    })
}

/// What keeping most chapters of a book use, for a chapter with no definitions yet.
pub fn usual_keeping(chapters: impl Iterator<Item = Option<Keeping>>) -> Option<Keeping> {
    let (mut at_end, mut below) = (0, 0);
    for keeping in chapters.flatten() {
        match keeping {
            Keeping::AtEnd => at_end += 1,
            Keeping::BelowBlock => below += 1,
        }
    }
    match (at_end, below) {
        (0, 0) => None,
        _ if at_end >= below => Some(Keeping::AtEnd),
        _ => Some(Keeping::BelowBlock),
    }
}

/// The edit that adds a definition `new` after the others, with what comments stand with the last of them, or at the end of
/// the text if it has none. Definitions kept with no blank line between them keep to that.
fn at_end_edit(text: &str, blocks: &Blocks, facts: &Facts, new: &str) -> Edit {
    let eol = line_ending(text);
    let end = match facts.definitions.last() {
        Some(last) => {
            let list = blocks.as_slice();
            let mut index = blocks.index_at(last.range.start);
            let mut at = index;
            while at + 1 < list.len() && matches!(list[at + 1].kind, BlockKind::Blank | BlockKind::Comment) {
                at += 1;
                if list[at].kind == BlockKind::Comment {
                    index = at;
                }
            }
            body_range(text, &list[index].range).end
        }
        None => text.trim_end_matches(['\n', '\r']).len(),
    };
    let tight = facts
        .definitions
        .windows(2)
        .last()
        .is_some_and(|pair| !text[pair[0].range.end..pair[1].range.start].contains(&format!("{eol}{eol}")));
    let gap = if end == 0 {
        String::new()
    } else if tight {
        eol.to_owned()
    } else {
        format!("{eol}{eol}")
    };
    Edit::new(end..end, format!("{gap}{new}"))
}

/// The edit that adds definition `new` for a reference at `at`.
fn definition_edit(text: &str, blocks: &Blocks, facts: &Facts, at: usize, new: &str, keeping: Keeping) -> Edit {
    let eol = line_ending(text);
    match keeping {
        Keeping::AtEnd => {
            // In reference order: before the definition of the next reference that has one here.
            let next = facts
                .refs
                .iter()
                .filter(|note| note.range.start >= at)
                .find_map(|note| {
                    facts
                        .definitions
                        .iter()
                        .find(|definition| folded(&definition.label) == folded(&note.label))
                });
            match next {
                Some(next) => {
                    let gap = text[..next.range.start]
                        .rfind(|character: char| character != '\n' && character != '\r')
                        .map_or("", |last| &text[last + 1..next.range.start]);
                    let gap = if gap.is_empty() { eol } else { gap };
                    Edit::new(next.range.start..next.range.start, format!("{new}{gap}"))
                }
                None => at_end_edit(text, blocks, facts, new),
            }
        }
        Keeping::BelowBlock => {
            let index = blocks.index_at(at);
            let block = &blocks.as_slice()[index];
            let end = body_range(text, &block.range).end;
            let next_needs_gap = index + 1 < blocks.as_slice().len() && needs_gap(blocks, index + 1);
            let ended = text[end..].starts_with(['\n', '\r']);
            let trail = if next_needs_gap && !ended {
                format!("{eol}{eol}")
            } else {
                String::new()
            };
            Edit::new(end..end, format!("{eol}{eol}{new}{trail}"))
        }
    }
}

/// The new note's reference at `replaced`, with its definition where the text keeps them. Returns the command, as one
/// undo step, and where the note's text lands in the text after it.
pub fn insert(
    text: &str,
    blocks: &Blocks,
    facts: &Facts,
    replaced: Range<usize>,
    label: &str,
    body: &str,
    book_keeping: Option<Keeping>,
) -> (Command, Range<usize>) {
    let reference = format!("[^{label}]");
    let keeping = keeping(blocks).or(book_keeping).unwrap_or(Keeping::BelowBlock);
    let lead = format!("[^{label}]: ");
    let new = format!("{lead}{body}");
    let definition = definition_edit(text, blocks, facts, replaced.start, &new, keeping);
    // The reference's edit comes first in the text unless the definition was placed before it, which a reference in a
    // definition's own block cannot do.
    let selection = Selection::caret(replaced.start + reference.len());
    let definition_at = definition.range.start;
    let shift = reference.len() as isize - replaced.len() as isize;
    let in_edit = definition.text.find(&lead).unwrap_or(0) + lead.len();
    let body_start = definition_at.wrapping_add_signed(shift) + in_edit;
    let edits = vec![Edit::new(replaced, reference), definition];
    (Command { edits, selection }, body_start..body_start + body.len())
}

/// The edit that adds a definition for a reference that has none.
pub fn add_definition(
    text: &str,
    blocks: &Blocks,
    facts: &Facts,
    at: usize,
    label: &str,
    body: &str,
    book_keeping: Option<Keeping>,
) -> Edit {
    let keeping = keeping(blocks).or(book_keeping).unwrap_or(Keeping::BelowBlock);
    definition_edit(text, blocks, facts, at, &format!("[^{label}]: {body}"), keeping)
}

/// The text of a note, without its label and with the indent of its later lines taken off. Line ends come out as `\n`.
pub fn read_body(text: &str, definition: &Range<usize>) -> String {
    let (_, lines) = body_lines(text, definition);
    let parts: Vec<&str> = lines.iter().map(|line| dedented(&text[line.clone()])).collect();
    parts.join("\n")
}

fn dedented(line: &str) -> &str {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let spaces = line
        .bytes()
        .take(CONTINUATION.len())
        .take_while(|byte| *byte == b' ')
        .count();
    if line.as_bytes().first() == Some(&b'\t') {
        &line[1..]
    } else {
        &line[spaces..]
    }
}

/// Where a note's text starts, and the range of each of its lines.
fn body_lines(text: &str, definition: &Range<usize>) -> (usize, Vec<Range<usize>>) {
    let written = &text[definition.clone()];
    let colon = written.find("]:").map_or(0, |at| at + "]:".len());
    let gap = written[colon..]
        .bytes()
        .take_while(|byte| *byte == b' ' || *byte == b'\t')
        .count();
    let start = definition.start + colon + gap;
    let mut lines = Vec::new();
    let mut at = start;
    for line in text[start..definition.end].split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        lines.push(at..at + content.len());
        at += line.len();
    }
    if lines.is_empty() {
        lines.push(start..start);
    }
    (start, lines)
}

/// The edit that makes a note's text `new`. Lines that stay the same stay as written, however they were indented, and
/// `None` means there is nothing to change.
pub fn write_body(text: &str, definition: &Range<usize>, new: &str) -> Option<Edit> {
    let eol = line_ending(text);
    let (start, lines) = body_lines(text, definition);
    let old: Vec<&str> = lines.iter().map(|line| &text[line.clone()]).collect();
    let mut written = String::with_capacity(new.len());
    for (index, line) in new.split('\n').enumerate() {
        if index > 0 {
            written.push_str(eol);
        }
        let kept = old.get(index).filter(|raw| dedented(raw) == line);
        match kept {
            Some(raw) => written.push_str(raw.strip_suffix('\r').unwrap_or(raw)),
            None if index > 0 && !line.is_empty() => {
                written.push_str(CONTINUATION);
                written.push_str(line);
            }
            None => written.push_str(line),
        }
    }
    if text[start..definition.end] == written {
        return None;
    }
    Some(smallest_edit(start, &text[start..definition.end], &written))
}

/// The edit that takes a definition out with the blank line after it, or before it at the end of the text, so the blocks
/// around it stay as they were written.
pub fn remove_definition(text: &str, blocks: &Blocks, definition: &Definition) -> Edit {
    let index = blocks.index_at(definition.range.start);
    let list = blocks.as_slice();
    let block = &list[index];
    let after = list.get(index + 1).filter(|next| next.kind == BlockKind::Blank);
    if let Some(after) = after.filter(|after| after.range.end < text.len()) {
        return Edit::new(block.range.start..after.range.end, "");
    }
    if block.range.end < text.len() {
        return Edit::new(block.range.clone(), "");
    }
    // The last block: the blank line before it goes, and the text keeps how it ends.
    let start = text[..block.range.start].trim_end_matches(['\n', '\r']).len();
    Edit::new(start..definition.range.end, "")
}

#[cfg(test)]
mod tests {
    use super::super::facts::scan;
    use super::*;
    use crate::editor::commands::applied;

    fn labels(list: &[&str]) -> Vec<String> {
        list.iter().map(|label| (*label).to_owned()).collect()
    }

    #[test]
    fn a_label_follows_the_chapter_then_the_book_and_stays_unique() {
        let chapter = scan("A[^k15-1] b[^k15-7] c[^k15-named].\n\n[^k15-1]: a\n\n[^k15-7]: b\n\n[^k15-named]: c\n");
        let book = labels(&["k15-1", "k15-7", "k15-named", "k01-1", "k01-2"]);
        assert_eq!(next_label("46-k15", &chapter, &book), "k15-8");
        let empty = scan("No notes.\n");
        assert_eq!(
            next_label("46-k15", &empty, &labels(&["k01-1", "k01-2", "k02-1"])),
            "k15-1"
        );
        assert_eq!(next_label("intro", &empty, &labels(&["1", "2", "5"])), "6");
        assert_eq!(next_label("intro", &empty, &[]), "note-1");
        assert_eq!(next_label("intro", &empty, &labels(&["NOTE-1"])), "NOTE-2");
    }

    #[test]
    fn a_definition_goes_where_the_chapter_keeps_them() {
        let insert_at = |text: &str, at: usize, usual| {
            let blocks = Blocks::parse(text);
            let facts = scan(text);
            let (command, body) = insert(text, &blocks, &facts, at..at, "n", "Größe", usual);
            let result = applied(text, &command.edits);
            assert_eq!(&result[body], "Größe");
            result
        };
        let tail = "One[^a].\n\nTwo.\n\nThree[^b].\n\n[^a]: A.\n\n[^b]: B.\n";
        assert_eq!(
            insert_at(tail, tail.find("Two").expect("found") + 3, None),
            "One[^a].\n\nTwo[^n].\n\nThree[^b].\n\n[^a]: A.\n\n[^n]: Größe\n\n[^b]: B.\n"
        );
        assert_eq!(
            insert_at(tail, tail.find("Three").expect("found") + 10, None),
            "One[^a].\n\nTwo.\n\nThree[^b].[^n]\n\n[^a]: A.\n\n[^b]: B.\n\n[^n]: Größe\n"
        );
        let below = "One[^a].\n\n[^a]: A.\n\nTwo.\n\nThree.\n";
        assert_eq!(
            insert_at(below, below.find("Two").expect("found") + 4, None),
            "One[^a].\n\n[^a]: A.\n\nTwo.[^n]\n\n[^n]: Größe\n\nThree.\n"
        );
        assert_eq!(
            insert_at("A.\n\nB.\n", 2, Some(Keeping::AtEnd)),
            "A.[^n]\n\nB.\n\n[^n]: Größe\n"
        );
        assert_eq!(insert_at("A.\n\nB.\n", 2, None), "A.[^n]\n\n[^n]: Größe\n\nB.\n");
        assert_eq!(insert_at("A.", 2, None), "A.[^n]\n\n[^n]: Größe");
        // A closing line after the definitions, and a comment that belongs to the last of them.
        let closed = "One[^a].\n\nTwo.\n\n[^a]: A.\n\n<!-- checked -->\n\n[^b]: B.\n\n<!-- too -->\n\n_The end._\n";
        assert_eq!(
            insert_at(closed, closed.find("Two").expect("found") + 4, None),
            "One[^a].\n\nTwo.[^n]\n\n[^a]: A.\n\n<!-- checked -->\n\n[^b]: B.\n\n<!-- too -->\n\n[^n]: Größe\n\n_The end._\n"
        );
    }

    #[test]
    fn a_note_reads_without_indent_and_writes_only_what_changed() {
        let text = "[^a]: First »ß« line\nlazy line\n\n    Second paragraph.\n\nNext.\n";
        let blocks = Blocks::parse(text);
        let facts = scan(text);
        let definition = &facts.definitions[0];
        assert_eq!(blocks.as_slice()[0].kind, BlockKind::FootnoteDefinition);
        let body = read_body(text, &definition.range);
        assert_eq!(body, "First »ß« line\nlazy line\n\nSecond paragraph.");
        assert_eq!(write_body(text, &definition.range, &body), None);
        let edit = write_body(
            text,
            &definition.range,
            "First »ß« line\nlazy line\n\nSecond.\n\nThird.",
        )
        .expect("an edit");
        assert_eq!(
            applied(text, &[edit]),
            "[^a]: First »ß« line\nlazy line\n\n    Second.\n\n    Third.\n\nNext.\n"
        );
    }

    #[test]
    fn line_endings_stay_as_written_in_a_note() {
        let text = "[^a]: One\r\n\r\n    Two\r\n\r\nNext\r\n";
        let facts = scan(text);
        let definition = &facts.definitions[0];
        assert_eq!(read_body(text, &definition.range), "One\n\nTwo");
        let edit = write_body(text, &definition.range, "One\n\nThree").expect("an edit");
        assert_eq!(applied(text, &[edit]), "[^a]: One\r\n\r\n    Three\r\n\r\nNext\r\n");
    }

    #[test]
    fn removing_a_definition_leaves_the_rest_as_written() {
        let remove = |text: &str, label: &str| {
            let blocks = Blocks::parse(text);
            let facts = scan(text);
            let definition = facts
                .definitions
                .iter()
                .find(|definition| definition.label == label)
                .expect("defined");
            applied(text, &[remove_definition(text, &blocks, definition)])
        };
        assert_eq!(remove("A\n\n[^a]: x\n\n[^b]: y\n", "a"), "A\n\n[^b]: y\n");
        assert_eq!(remove("A\n\n[^a]: x\n\n[^b]: y\n", "b"), "A\n\n[^a]: x\n");
        assert_eq!(remove("A\n\n[^a]: x\n\nB\n", "a"), "A\n\nB\n");
        assert_eq!(remove("A\n\n[^a]: x", "a"), "A");
    }
}
