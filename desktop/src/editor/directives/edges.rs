//! What an edit over a selection does at a region's edges. A region the selection holds whole goes whole. A selection
//! holding only one of a region's two fence lines takes the text around that line and leaves the line, so no edit breaks
//! a region the author cannot see the lines of. Holds no gpui types.

use std::ops::Range;

use super::structure::{self, Structure};
use crate::editor::commands::{Command, applied, line_ending};
use crate::source::{BlockKind, Blocks, Edit, Selection};

fn touches(range: &Range<usize>, line: &Range<usize>) -> bool {
    line.start < range.end && range.start < line.end
}

/// The parts of `range` an edit may take, in order and never empty unless nothing can go. `hidden` is where the text
/// starts that Write shows: what stands before it is not the selection's to take.
pub fn pieces(
    text: &str,
    blocks: &Blocks,
    structure: &Structure,
    range: Range<usize>,
    hidden: usize,
) -> Vec<Range<usize>> {
    let list = blocks.as_slice();
    let mut range = range.start.max(hidden)..range.end.max(hidden);
    // A fence line goes whole or stays whole, so a line the selection only reaches into counts as held.
    for region in &structure.regions {
        let (open, close) = (&list[region.open].range, &list[region.close].range);
        if touches(&range, open) && touches(&range, close) {
            range = range.start.min(open.start)..range.end.max(close.end);
        }
    }
    let mut kept: Vec<Range<usize>> = (structure.regions.iter())
        .filter_map(|region| {
            let (open, close) = (&list[region.open].range, &list[region.close].range);
            match (touches(&range, open), touches(&range, close)) {
                (true, false) => Some(open.clone()),
                (false, true) => Some(close.clone()),
                _ => None,
            }
        })
        .collect();
    kept.sort_by_key(|line| line.start);
    let after_kept = kept.last().map(|line| line.end);
    let mut parts = Vec::with_capacity(kept.len() + 1);
    let mut from = range.start;
    for line in kept {
        // The line ending before a kept line stays, or the line would join the text before it.
        let ending = line_ending_before(text, line.start);
        let end = (line.start - ending).max(from);
        parts.push(from..end);
        from = line.end.max(from);
        // So does the blank line after it, which keeps it from the text that follows.
        let blank = line_ending_at(text, from);
        if blank > 0 && from + blank <= range.end {
            from += blank;
        }
    }
    // A selection ending where a fence line starts leaves the line ending before it, or the line would join the text.
    let fence_follows = (structure.regions.iter()).any(|region| {
        [region.open, region.close]
            .iter()
            .any(|&line| list[line].range.start == range.end)
    });
    let end = if fence_follows {
        range.end - line_ending_before(text, range.end)
    } else {
        range.end
    };
    parts.push(from.min(end)..end.max(from.min(end)));
    parts.retain(|part| !part.is_empty());
    if parts.is_empty() {
        // Nothing here can go. What is written over it goes after the fence line, never into it.
        let at = after_kept.unwrap_or(range.start);
        parts.push(at..at);
    }
    parts
}

fn line_ending_before(text: &str, at: usize) -> usize {
    let before = &text[..at];
    if before.ends_with("\r\n") {
        2
    } else {
        usize::from(before.ends_with('\n'))
    }
}

fn line_ending_at(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    if rest.starts_with("\r\n") {
        2
    } else {
        usize::from(rest.starts_with('\n'))
    }
}

/// `new` written over `parts`, with the caret after it.
pub fn written_over(parts: &[Range<usize>], new: &str) -> (Vec<Edit>, Selection) {
    let start = parts.first().map_or(0, |part| part.start);
    let edits = parts
        .iter()
        .enumerate()
        .map(|(at, part)| Edit::new(part.clone(), if at == 0 { new } else { "" }))
        .collect();
    (edits, Selection::caret(start + new.len()))
}

/// What the clipboard takes of `parts`: the text an edit would take, so no lone fence line travels. Text from either
/// side of a fence line stays two paragraphs.
pub fn copied(text: &str, parts: &[Range<usize>]) -> String {
    let eol = line_ending(text);
    let pieces: Vec<&str> = parts.iter().map(|part| &text[part.clone()]).collect();
    pieces.join(&format!("{eol}{eol}"))
}

/// What a paste of text holding directive lines does.
#[derive(Debug, PartialEq, Eq)]
pub enum Paste {
    /// No directive line in it: it goes in as text.
    Text,
    /// It goes in as blocks of their own.
    Blocks(Command),
    /// The engine would refuse its regions here, so this much of it goes in as text.
    Refused(String),
}

/// How `pasted` goes in over `parts`. Regions stand on lines of their own, after the caret's block when that holds text.
pub fn paste(text: &str, parts: &[Range<usize>], pasted: &str) -> Paste {
    // In code, a comment, or settings a directive line is only text.
    let blocks = Blocks::parse(text);
    let at = parts.first().map_or(0, |part| part.start);
    let kind = blocks.as_slice()[blocks.index_at(at)].kind;
    if matches!(kind, BlockKind::Code | BlockKind::Comment | BlockKind::FrontMatter) {
        return Paste::Text;
    }
    let trimmed = pasted.trim_matches(['\n', '\r']);
    let held = Blocks::parse(trimmed);
    if !held.as_slice().iter().any(|block| block.kind == BlockKind::Directive) {
        return Paste::Text;
    }
    let refused = || Paste::Refused(words_of(trimmed));
    let own = structure::read(trimmed, &held);
    if !own.rejected.is_empty() {
        return refused();
    }
    let (cut, caret) = written_over(parts, "");
    let rest = applied(text, &cut);
    let rest_blocks = Blocks::parse(&rest);
    let at = place_for_blocks(&rest, &rest_blocks, caret.head);
    let eol = line_ending(text);
    let gap = format!("{eol}{eol}");
    let before = match &rest[..at] {
        "" => "",
        head if head.ends_with("\n\n") || head.ends_with("\r\n\r\n") => "",
        head if head.ends_with('\n') => eol,
        _ => &gap,
    };
    let after = match &rest[at..] {
        "" => eol,
        tail if tail.starts_with("\n\n") || tail.starts_with("\r\n\r\n") => "",
        tail if tail.starts_with('\n') || tail.starts_with("\r\n") => eol,
        _ => &gap,
    };
    let new = format!("{}{before}{trimmed}{after}{}", &rest[..at], &rest[at..]);
    let found = structure::read(&new, &Blocks::parse(&new));
    let had = structure::read(&rest, &rest_blocks);
    if found.regions.len() != had.regions.len() + own.regions.len() || found.rejected.len() > had.rejected.len() {
        return refused();
    }
    let end = at + before.len() + trimmed.len();
    Paste::Blocks(Command {
        edits: vec![smallest_edit(text, &new)],
        selection: Selection::caret(end),
    })
}

/// The text of `pasted` without its directive lines.
pub fn words_of(pasted: &str) -> String {
    let held = Blocks::parse(pasted);
    (held.as_slice().iter())
        .filter(|block| block.kind != BlockKind::Directive)
        .map(|block| &pasted[block.range.clone()])
        .collect()
}

/// Where blocks go in for a caret at `caret`: on its line when that is empty, otherwise after its block's text.
fn place_for_blocks(text: &str, blocks: &Blocks, caret: usize) -> usize {
    let block = &blocks.as_slice()[blocks.index_at(caret)];
    if block.kind == BlockKind::Blank {
        return caret;
    }
    block.range.start + text[block.range.clone()].trim_end_matches(['\n', '\r']).len()
}

/// The one edit that turns `old` into `new`, so undo and the blocks around it see only what changed.
fn smallest_edit(old: &str, new: &str) -> Edit {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let most = old.len().min(new.len()) - start;
    let same = |(a, b): &(u8, u8)| a == b;
    let mut tail = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(most)
        .take_while(same)
        .count();
    while !old.is_char_boundary(old.len() - tail) || !new.is_char_boundary(new.len() - tail) {
        tail -= 1;
    }
    Edit::new(start..old.len() - tail, &new[start..new.len() - tail])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn over(text: &str, range: Range<usize>, new: &str) -> String {
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let parts = pieces(text, &blocks, &structure, range, 0);
        applied(text, &written_over(&parts, new).0)
    }

    fn between(text: &str, from: &str, to: &str) -> Range<usize> {
        text.find(from).unwrap()..text.find(to).unwrap()
    }

    #[test]
    fn a_selection_over_one_fence_line_leaves_the_line() {
        let text = "::: keep\nAb cd\n:::\n\nEf gh\n\n::: columns\nx\n:::\n";
        let range = between(text, "cd", " gh");
        let result = over(text, range, "");
        assert_eq!(result, "::: keep\nAb \n:::\n\n gh\n\n::: columns\nx\n:::\n");
        let blocks = Blocks::parse(&result);
        assert_eq!(structure::read(&result, &blocks).regions.len(), 2);
    }

    #[test]
    fn typing_over_it_lands_at_the_selections_start() {
        let text = "Ab cd\n\n::: keep\nEf gh\n:::\n";
        assert_eq!(
            over(text, between(text, "cd", " gh"), "X"),
            "Ab X\n::: keep\n gh\n:::\n"
        );
    }

    #[test]
    fn a_region_held_whole_goes_whole() {
        let text = "Ab\n\n::: keep\nCd\n:::\n\nEf\n";
        assert_eq!(over(text, between(text, "b\n", "f"), ""), "Af\n");
    }

    #[test]
    fn a_marker_the_selection_reaches_into_goes_whole() {
        let text = "Ab\n\n::: toc\n\nCd\n";
        assert_eq!(over(text, 1..text.find("toc").unwrap(), ""), "A\nCd\n");
    }

    #[test]
    fn hidden_text_before_the_selection_stays() {
        let text = "---\ntitle: A\n---\n\nText\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let shown = text.find("Text").unwrap();
        let parts = pieces(text, &blocks, &structure, 0..text.len(), shown);
        assert_eq!(parts.first(), Some(&(shown..text.len())));
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn the_clipboard_takes_no_lone_fence_line() {
        let text = "::: keep\nAb\n:::\n\nCd\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let parts = pieces(text, &blocks, &structure, between(text, "Ab", "d\n"), 0);
        assert_eq!(copied(text, &parts), "Ab\n\nC");
    }

    fn pasted(text: &str, caret: usize, new: &str) -> Paste {
        paste(text, std::slice::from_ref(&(caret..caret)), new)
    }

    #[test]
    fn a_region_pasted_into_a_paragraph_follows_it() {
        let text = "Some text\n\nNext\n";
        let Paste::Blocks(command) = pasted(text, 4, "::: keep\nA\n:::\n") else {
            panic!("blocks");
        };
        let result = applied(text, &command.edits);
        assert_eq!(result, "Some text\n\n::: keep\nA\n:::\n\nNext\n");
        assert_eq!(&result[..command.selection.head], "Some text\n\n::: keep\nA\n:::");
    }

    #[test]
    fn a_region_the_engine_would_refuse_goes_in_as_its_text() {
        let text = "::: toc\n\nText\n";
        assert_eq!(
            pasted(text, text.len(), "::: toc\n\nMore\n"),
            Paste::Refused("\nMore".to_owned())
        );
        assert_eq!(pasted("a\n", 1, ":::\nb\n"), Paste::Refused("b".to_owned()));
        assert_eq!(pasted("a\n", 1, "plain\n"), Paste::Text);
    }

    #[test]
    fn a_selection_ending_at_a_mark_leaves_it_on_its_own_line() {
        let text = "Hello world\n\n::: toc\n";
        let end = text.find(":::").unwrap();
        assert_eq!(over(text, 6..end, ""), "Hello \n::: toc\n");
    }

    #[test]
    fn a_directive_pasted_into_code_is_text() {
        let text = "```\ncode\n```\n";
        assert_eq!(pasted(text, 5, "::: toc\n"), Paste::Text);
    }

    #[test]
    fn writing_over_only_a_fence_lines_word_leaves_the_line() {
        let text = "::: keep\nAb\n:::\n";
        assert_eq!(over(text, 4..8, "X"), "::: keep\nXAb\n:::\n");
    }
}
