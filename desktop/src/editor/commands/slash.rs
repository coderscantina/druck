//! The slash menu's list and what each entry writes.
//!
//! Typing `/` puts a slash in the text. What follows it is the query. Choosing an entry replaces the slash and the query,
//! as one undo step, and leaves the text as the writer typed it if the menu is dismissed.
//!
//! An entry is offered only where its result is valid Druck: a `toc` once in a book and outside containers, an image only
//! if the project has one to point at, a citation only if the bibliography has a key. Nothing can be typed into the menu
//! that the engine would then reject.

use std::ops::Range;

use super::convert::{BlockType, Callout, gapped_edit, rewritten, type_of};
use super::{Bias, Command, body_range, line_ending, moved, needs_gap, normalized};
use crate::editor::strings::{DE, EN, Strings};
use crate::session::TEMPLATES;
use crate::source::inline::line_markers;
use crate::source::{BlockKind, Blocks, Edit, Selection};

/// What an entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The current block becomes this type, or on an empty line, a new block of it.
    Turn(BlockType),
    /// `::: name` around the current block, or around placeholder text on an empty line.
    Wrap(&'static str),
    /// A block of its own after the current block.
    Insert(Standalone),
    /// A reference at the caret, with its definition below the block.
    Footnote(String),
    /// A citation of this key at the caret.
    Citation(String),
    /// An image at this path, as the engine reads it from the document's folder.
    Image(String),
    /// A new section from the template with this slug. The session carries this one out.
    Section(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standalone {
    SceneBreak,
    Table,
    PageBreak,
    Toc,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: &'static str,
    /// Also matched by the query, so a writer can use either language.
    pub other_label: &'static str,
    /// Shown quietly beside the label.
    pub group: Option<&'static str>,
    pub icon: &'static str,
    pub action: Action,
}

/// What the menu needs to know of the document to offer only what is valid.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The directives open at the caret, innermost last.
    pub open_directives: Vec<String>,
    /// Whether the book has a `::: toc` already.
    pub has_toc: bool,
    /// A label no chapter of the book uses yet.
    pub free_footnote: String,
    pub first_citation_key: Option<String>,
    /// An image of the document, as the engine reads its path.
    pub first_image: Option<String>,
    /// The kind of the block the slash stands in.
    pub block: Option<BlockKind>,
}

/// Where a slash opens the menu: in running text, at the start of a word.
pub fn opens_at(kind: BlockKind, text: &str, slash: usize) -> bool {
    let allowed = matches!(
        kind,
        BlockKind::Paragraph
            | BlockKind::Heading { .. }
            | BlockKind::List
            | BlockKind::Quote
            | BlockKind::FootnoteDefinition
            | BlockKind::Blank
    );
    allowed && text[..slash].chars().next_back().is_none_or(char::is_whitespace)
}

/// The icon of a block type, for the menus and the gutter.
pub fn type_icon(target: BlockType) -> &'static str {
    match target {
        BlockType::Paragraph => "icons/block-text.svg",
        BlockType::Heading(1) => "icons/block-h1.svg",
        BlockType::Heading(2) => "icons/block-h2.svg",
        BlockType::Heading(3) => "icons/block-h3.svg",
        BlockType::Heading(4) => "icons/block-h4.svg",
        BlockType::Heading(5) => "icons/block-h5.svg",
        BlockType::Heading(_) => "icons/block-h6.svg",
        BlockType::Bullets => "icons/block-bullets.svg",
        BlockType::Numbers => "icons/block-numbers.svg",
        BlockType::Quote => "icons/block-quote.svg",
        BlockType::Callout(_) => "icons/block-callout.svg",
        BlockType::Code => "icons/block-code.svg",
    }
}

/// The label of a block type in one language.
pub fn type_label(target: BlockType, strings: &'static Strings) -> &'static str {
    match target {
        BlockType::Paragraph => strings.paragraph,
        BlockType::Heading(1) => strings.heading_1,
        BlockType::Heading(2) => strings.heading_2,
        BlockType::Heading(3) => strings.heading_3,
        BlockType::Heading(4) => strings.heading_4,
        BlockType::Heading(5) => strings.heading_5,
        BlockType::Heading(_) => strings.heading_6,
        BlockType::Bullets => strings.bullets,
        BlockType::Numbers => strings.numbers,
        BlockType::Quote => strings.quote,
        BlockType::Callout(Callout::Note) => strings.note,
        BlockType::Callout(Callout::Tip) => strings.tip,
        BlockType::Callout(Callout::Important) => strings.important,
        BlockType::Callout(Callout::Warning) => strings.warning,
        BlockType::Callout(Callout::Caution) => strings.caution,
        BlockType::Code => strings.code,
    }
}

/// Every entry valid in `context`, in the menu's order, in the language of `strings`.
pub fn items(context: &Context, strings: &'static Strings) -> Vec<Item> {
    let other: &'static Strings = if std::ptr::eq(strings, &EN) { &DE } else { &EN };
    let mut items = Vec::new();
    let open = &context.open_directives;
    // Only a list belongs in a list table.
    if open.iter().any(|name| name == "table") {
        return items;
    }
    let held = open.iter().any(|name| name == "keep" || name == "bottom");
    let label = |of: fn(&Strings) -> &'static str, icon, action| Item {
        label: of(strings),
        other_label: of(other),
        group: None,
        icon,
        action,
    };
    for target in [
        BlockType::Heading(1),
        BlockType::Heading(2),
        BlockType::Heading(3),
        BlockType::Heading(4),
        BlockType::Bullets,
        BlockType::Numbers,
        BlockType::Quote,
        BlockType::Callout(Callout::Note),
        BlockType::Callout(Callout::Tip),
        BlockType::Callout(Callout::Important),
        BlockType::Callout(Callout::Warning),
        BlockType::Callout(Callout::Caution),
        BlockType::Code,
    ] {
        items.push(Item {
            label: type_label(target, strings),
            other_label: type_label(target, other),
            group: None,
            icon: type_icon(target),
            action: Action::Turn(target),
        });
    }
    items.push(label(
        |s| s.scene_break,
        "icons/block-break.svg",
        Action::Insert(Standalone::SceneBreak),
    ));
    items.push(label(
        |s| s.table,
        "icons/block-table.svg",
        Action::Insert(Standalone::Table),
    ));
    if let Some(path) = &context.first_image {
        items.push(label(|s| s.image, "icons/images.svg", Action::Image(path.clone())));
    }
    if !held {
        items.push(label(|s| s.keep, "icons/block-directive.svg", Action::Wrap("keep")));
    }
    if open.is_empty() {
        items.push(label(
            |s| s.columns,
            "icons/block-directive.svg",
            Action::Wrap("columns"),
        ));
        items.push(label(|s| s.bottom, "icons/block-directive.svg", Action::Wrap("bottom")));
    }
    if !held {
        items.push(label(
            |s| s.page_break,
            "icons/block-directive.svg",
            Action::Insert(Standalone::PageBreak),
        ));
    }
    if open.is_empty() && !context.has_toc {
        items.push(label(|s| s.toc, "icons/contents.svg", Action::Insert(Standalone::Toc)));
    }
    let in_note = context.block == Some(BlockKind::FootnoteDefinition);
    let in_heading = matches!(context.block, Some(BlockKind::Heading { .. }));
    if !in_note && !in_heading {
        items.push(label(
            |s| s.footnote,
            "icons/block-footnote.svg",
            Action::Footnote(context.free_footnote.clone()),
        ));
    }
    if let Some(key) = context.first_citation_key.as_ref().filter(|_| !in_heading) {
        items.push(label(|s| s.citation, "icons/book.svg", Action::Citation(key.clone())));
    }
    for template in TEMPLATES {
        items.push(Item {
            label: template.name,
            other_label: strings.new_section,
            group: Some(strings.new_section),
            icon: template.icon,
            action: Action::Section(template.slug),
        });
    }
    items
}

/// The entries matching a query: all its words in either language's label, the ones starting with it first.
pub fn filtered(items: Vec<Item>, query: &str) -> Vec<Item> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let (mut first, mut rest): (Vec<Item>, Vec<Item>) = items
        .into_iter()
        .filter(|item| {
            let label = item.label.to_lowercase();
            let other = item.other_label.to_lowercase();
            words.iter().all(|word| label.contains(word) || other.contains(word))
        })
        .partition(|item| {
            item.label.to_lowercase().starts_with(&query) || item.other_label.to_lowercase().starts_with(&query)
        });
    first.append(&mut rest);
    first
}

/// The directives open at `offset`, innermost last.
pub fn open_directives(text: &str, blocks: &Blocks, offset: usize) -> Vec<String> {
    let mut open: Vec<String> = Vec::new();
    for block in blocks.as_slice() {
        if block.range.start >= offset {
            break;
        }
        if block.kind != BlockKind::Directive {
            continue;
        }
        let name = text[block.range.clone()].trim_start_matches(':').trim();
        let name = name.split_whitespace().next().unwrap_or("");
        match name {
            "" => {
                open.pop();
            }
            "columns" | "full-width" | "keep" | "bottom" | "table" => open.push(name.to_owned()),
            _ => {}
        }
    }
    open
}

/// Whether this text has a table of contents.
pub fn holds_toc(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with(":::") && line.trim_start_matches(':').trim() == "toc")
}

/// A footnote label none of these texts uses.
pub fn free_footnote_label<'a>(texts: impl Iterator<Item = &'a str> + Clone) -> String {
    (1..)
        .map(|number| format!("note-{number}"))
        .find(|label| {
            let reference = format!("[^{label}]");
            texts.clone().all(|text| !text.contains(&reference))
        })
        .expect("some label is free")
}

/// The first citation key of a BibTeX text, skipping the entries that are not works.
pub fn first_citation_key(bibtex: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = bibtex[from..].find('@') {
        let start = from + at + 1;
        let rest = &bibtex[start..];
        from = start;
        let Some(open) = rest.find('{') else {
            break;
        };
        let kind = rest[..open].trim().to_ascii_lowercase();
        if kind.is_empty()
            || !kind.bytes().all(|byte| byte.is_ascii_alphabetic())
            || matches!(kind.as_str(), "comment" | "string" | "preamble")
        {
            continue;
        }
        let key = rest[open + 1..].split([',', '}']).next().unwrap_or("").trim();
        if !key.is_empty() {
            return Some(key.to_owned());
        }
    }
    None
}

/// What an entry writes on an empty line, without a final line ending, and the part of it the writer replaces first.
fn template(action: &Action, strings: &Strings, eol: &str) -> Option<(String, Range<usize>)> {
    let with_placeholder = |before: String, placeholder: &str, after: &str| {
        let start = before.len();
        (
            format!("{before}{placeholder}{after}"),
            start..start + placeholder.len(),
        )
    };
    Some(match action {
        Action::Turn(BlockType::Heading(level)) => with_placeholder(
            format!("{} ", "#".repeat(usize::from(*level))),
            strings.placeholder_heading,
            "",
        ),
        Action::Turn(BlockType::Bullets) => with_placeholder("- ".to_owned(), strings.placeholder_item, ""),
        Action::Turn(BlockType::Numbers) => with_placeholder("1. ".to_owned(), strings.placeholder_item, ""),
        Action::Turn(BlockType::Quote) => with_placeholder("> ".to_owned(), strings.placeholder_quote, ""),
        Action::Turn(BlockType::Callout(callout)) => {
            with_placeholder(format!("> {}{eol}> ", callout.marker()), strings.placeholder_text, "")
        }
        Action::Turn(BlockType::Code) => {
            with_placeholder(format!("```{eol}"), strings.placeholder_code, &format!("{eol}```"))
        }
        Action::Turn(BlockType::Paragraph) => with_placeholder(String::new(), strings.placeholder_text, ""),
        Action::Wrap(name) => with_placeholder(
            format!("::: {name}{eol}"),
            strings.placeholder_text,
            &format!("{eol}:::"),
        ),
        Action::Insert(Standalone::SceneBreak) => {
            let text = "***".to_owned();
            let end = text.len();
            (text, end..end)
        }
        Action::Insert(Standalone::PageBreak) => {
            let text = "::: page-break".to_owned();
            let end = text.len();
            (text, end..end)
        }
        Action::Insert(Standalone::Toc) => {
            let text = "::: toc".to_owned();
            let end = text.len();
            (text, end..end)
        }
        Action::Insert(Standalone::Table) => {
            let (column, cell) = (strings.placeholder_column, strings.placeholder_cell);
            with_placeholder(
                "| ".to_owned(),
                column,
                &format!(" | {column} |{eol}| --- | --- |{eol}| {cell} | {cell} |"),
            )
        }
        Action::Image(path) => {
            let path = if path.contains(' ') {
                format!("<{path}>")
            } else {
                path.clone()
            };
            with_placeholder("![".to_owned(), strings.placeholder_caption, &format!("]({path})"))
        }
        Action::Footnote(_) | Action::Citation(_) | Action::Section(_) => return None,
    })
}

/// The command for choosing `action` with `slash..caret` as the typed slash and query. A new section is the session's to
/// make, so here it only takes the typed text away.
pub fn apply(text: &str, blocks: &Blocks, slash: Range<usize>, action: &Action, strings: &Strings) -> Option<Command> {
    let eol = line_ending(text);
    let index = blocks.index_at(slash.start);
    let block = &blocks.as_slice()[index];
    let body = body_range(text, &block.range);
    let body = body.start..body.end.max(slash.end);
    match action {
        Action::Section(_) => {
            return finish(text, vec![Edit::new(slash.clone(), "")], Selection::caret(slash.start));
        }
        Action::Citation(key) => {
            let written = format!("[@{key}]");
            let key_start = slash.start + "[@".len();
            let selection = Selection {
                anchor: key_start,
                head: key_start + key.len(),
            };
            return finish(text, vec![Edit::new(slash, written)], selection);
        }
        Action::Footnote(label) => return footnote(text, blocks, index, slash, label, strings),
        _ => {}
    }
    let line = line_of(text, &body, slash.start);
    let lead = line_markers(block.kind, &text[block.range.clone()])
        .iter()
        .find(|marker| {
            let start = block.range.start + marker.range.start;
            line.start <= start && start <= line.end
        })
        .map_or(line.start, |marker| block.range.start + marker.range.end);
    let others_on_line =
        !(text[lead.min(slash.start)..slash.start].trim().is_empty() && text[slash.end..line.end].trim().is_empty());
    let only_line = line.start == body.start && line.end >= body.end;

    if !others_on_line && only_line {
        // The block is nothing but the slash: the entry takes its place.
        let (new, placeholder) = template(action, strings, eol)?;
        let edit = gapped_edit(text, blocks, index, body.clone(), new);
        return placed(text, edit, placeholder);
    }
    if !others_on_line {
        // A line of its own in a longer block: it goes, and the entry follows the block.
        let (new, placeholder) = template(action, strings, eol)?;
        let removed = if line.start == body.start {
            line.start..line.end + eol.len()
        } else {
            line.start - eol.len()..line.end
        };
        return after_block(text, blocks, index, removed, "", new, placeholder);
    }
    let cut = cut_out(text, &body, &slash);
    match action {
        Action::Turn(target) => {
            let kind = single_kind(&cut).unwrap_or(block.kind);
            if type_of(kind, &cut) == Some(*target) {
                return finish(text, vec![Edit::new(slash.clone(), "")], Selection::caret(slash.start));
            }
            if let Some(new) = rewritten(kind, &cut, *target, 1) {
                let edit = gapped_edit(text, blocks, index, body.clone(), new);
                let selection = after_edit_end(&edit);
                return finish(text, vec![edit], selection);
            }
            let (new, placeholder) = template(action, strings, eol)?;
            after_block(text, blocks, index, slash, "", new, placeholder)
        }
        Action::Wrap(name) => {
            let new = format!("::: {name}{eol}{cut}{eol}:::");
            let edit = gapped_edit(text, blocks, index, body.clone(), new);
            let selection = after_edit_end(&edit);
            finish(text, vec![edit], selection)
        }
        _ => {
            let (new, placeholder) = template(action, strings, eol)?;
            after_block(text, blocks, index, slash, "", new, placeholder)
        }
    }
}

/// `body` with the typed slash and query cut out, and nothing else changed.
fn cut_out(text: &str, body: &Range<usize>, slash: &Range<usize>) -> String {
    let mut cut = String::with_capacity(body.len());
    cut.push_str(text[body.start..slash.start].trim_end_matches(' '));
    let after = &text[slash.end..body.end];
    if cut.ends_with(char::is_alphanumeric) && after.starts_with(char::is_alphanumeric) {
        cut.push(' ');
    }
    cut.push_str(after);
    cut
}

fn single_kind(text: &str) -> Option<BlockKind> {
    match Blocks::parse(text).as_slice() {
        [only] => Some(only.kind),
        _ => None,
    }
}

fn line_of(text: &str, body: &Range<usize>, at: usize) -> Range<usize> {
    let start = text[body.start..at]
        .rfind('\n')
        .map_or(body.start, |line| body.start + line + 1);
    let end = text[at..body.end].find('\n').map_or(body.end, |line| at + line);
    let end = if end > start && text.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    };
    start..end
}

fn after_edit_end(edit: &Edit) -> Selection {
    let trailing = edit.text.len() - edit.text.trim_end_matches(['\n', '\r']).len();
    Selection::caret(edit.range.start + edit.text.len() - trailing)
}

fn finish(text: &str, edits: Vec<Edit>, selection: Selection) -> Option<Command> {
    let edits = normalized(edits, text);
    (!edits.is_empty()).then_some(Command { edits, selection })
}

/// A replacement of a whole block, with the placeholder of `new` selected wherever the gaps around it put it.
fn placed(text: &str, edit: Edit, placeholder: Range<usize>) -> Option<Command> {
    let edits = normalized(vec![edit], text);
    let edit = edits.first()?;
    // The smallest edit may keep what the old block and `new` share at the end, but `new` always starts in it.
    let lead = edit.text.len() - edit.text.trim_start_matches(['\n', '\r']).len();
    let start = edit.range.start + lead;
    let selection = Selection {
        anchor: start + placeholder.start,
        head: start + placeholder.end,
    };
    Some(Command { edits, selection })
}

/// `removed` goes away and `new` follows the block as a block of its own, with its placeholder selected.
fn after_block(
    text: &str,
    blocks: &Blocks,
    index: usize,
    removed: Range<usize>,
    filler: &str,
    new: String,
    placeholder: Range<usize>,
) -> Option<Command> {
    let eol = line_ending(text);
    let (insert, start_in_insert) = block_insert(text, blocks, index, &new, eol);
    let edits = vec![Edit::new(removed, filler), insert];
    let start = moved(edits[1].range.start, &edits, Bias::Before) + start_in_insert;
    let edits = normalized(edits, text);
    let selection = Selection {
        anchor: start + placeholder.start,
        head: start + placeholder.end,
    };
    Some(Command { edits, selection })
}

/// The insertion of a new block after block `index`, and how far into the insertion the block's text starts.
fn block_insert(text: &str, blocks: &Blocks, index: usize, new: &str, eol: &str) -> (Edit, usize) {
    let block = &blocks.as_slice()[index];
    let end = block.range.end;
    let ended = text[..end].ends_with('\n');
    let lead = if ended { eol.to_owned() } else { format!("{eol}{eol}") };
    let next_needs_gap = index + 1 < blocks.as_slice().len() && needs_gap(blocks, index + 1);
    let trail = if ended {
        format!("{eol}{}", if next_needs_gap { eol } else { "" })
    } else {
        String::new()
    };
    (Edit::new(end..end, format!("{lead}{new}{trail}")), lead.len())
}

fn footnote(
    text: &str,
    blocks: &Blocks,
    index: usize,
    slash: Range<usize>,
    label: &str,
    strings: &Strings,
) -> Option<Command> {
    let eol = line_ending(text);
    let definition = format!("[^{label}]: ");
    let new = format!("{definition}{}", strings.placeholder_footnote);
    let (insert, start_in_insert) = block_insert(text, blocks, index, &new, eol);
    let edits = vec![Edit::new(slash, format!("[^{label}]")), insert];
    let start = moved(edits[1].range.start, &edits, Bias::Before) + start_in_insert + definition.len();
    let edits = normalized(edits, text);
    let selection = Selection {
        anchor: start,
        head: start + strings.placeholder_footnote.len(),
    };
    Some(Command { edits, selection })
}

#[cfg(test)]
mod tests {
    use super::super::applied;
    use super::*;

    fn choose(text: &str, action: &Action) -> (String, String) {
        let slash = text.find('/').expect("a slash");
        let end = text[slash..]
            .find(['\r', '\n', '|'])
            .map_or(text.len(), |at| slash + at);
        let source = text.replace('|', "");
        let blocks = Blocks::parse(&source);
        let command = apply(&source, &blocks, slash..end, action, &EN).expect("a command");
        let result = applied(&source, &command.edits);
        let selected = result[command.selection.range()].to_owned();
        (result, selected)
    }

    #[test]
    fn on_an_empty_line_an_entry_writes_its_block_with_the_placeholder_selected() {
        for (action, expected, selected) in [
            (Action::Turn(BlockType::Heading(2)), "## Heading\n", "Heading"),
            (Action::Turn(BlockType::Bullets), "- Item\n", "Item"),
            (Action::Turn(BlockType::Numbers), "1. Item\n", "Item"),
            (Action::Turn(BlockType::Quote), "> Quotation\n", "Quotation"),
            (
                Action::Turn(BlockType::Callout(Callout::Warning)),
                "> [!WARNING]\n> Text\n",
                "Text",
            ),
            (Action::Turn(BlockType::Code), "```\ncode\n```\n", "code"),
            (Action::Wrap("keep"), "::: keep\nText\n:::\n", "Text"),
            (Action::Insert(Standalone::SceneBreak), "***\n", ""),
            (Action::Insert(Standalone::PageBreak), "::: page-break\n", ""),
            (Action::Insert(Standalone::Toc), "::: toc\n", ""),
            (
                Action::Insert(Standalone::Table),
                "| Column | Column |\n| --- | --- |\n| Cell | Cell |\n",
                "Column",
            ),
            (
                Action::Image("a b.png".to_owned()),
                "![Caption](<a b.png>)\n",
                "Caption",
            ),
        ] {
            let (result, chosen) = choose("/query\n", &action);
            assert_eq!(result, expected);
            assert_eq!(chosen, selected, "{action:?}");
        }
    }

    #[test]
    fn with_text_on_its_line_an_entry_turns_the_block_or_follows_it() {
        let (result, _) = choose("Some text /h|\n", &Action::Turn(BlockType::Heading(2)));
        assert_eq!(result, "## Some text\n");
        let (result, _) = choose("Some /q|text\n", &Action::Turn(BlockType::Quote));
        assert_eq!(result, "> Some text\n");
        let (result, _) = choose("Text /k|\n", &Action::Wrap("keep"));
        assert_eq!(result, "::: keep\nText\n:::\n");
        let (result, _) = choose("Text /s|\n\nNext\n", &Action::Insert(Standalone::SceneBreak));
        assert_eq!(result, "Text \n\n***\n\nNext\n");
        let (result, chosen) = choose("Text /t|", &Action::Insert(Standalone::Table));
        assert_eq!(result, "Text \n\n| Column | Column |\n| --- | --- |\n| Cell | Cell |");
        assert_eq!(chosen, "Column");
    }

    #[test]
    fn a_slash_alone_on_a_list_line_goes_and_the_entry_follows_the_list() {
        let (result, chosen) = choose("- a\n- /h|\n\nNext\n", &Action::Turn(BlockType::Heading(1)));
        assert_eq!(result, "- a\n\n# Heading\n\nNext\n");
        assert_eq!(chosen, "Heading");
    }

    #[test]
    fn a_footnote_writes_a_reference_and_a_definition_below_the_block() {
        let (result, chosen) = choose("Word /f| more.\n\nNext\n", &Action::Footnote("note-1".to_owned()));
        assert_eq!(result, "Word [^note-1] more.\n\n[^note-1]: Footnote text.\n\nNext\n");
        assert_eq!(chosen, "Footnote text.");
    }

    #[test]
    fn a_citation_selects_its_key() {
        let (result, chosen) = choose("See /c|.\n", &Action::Citation("smith".to_owned()));
        assert_eq!(result, "See [@smith].\n");
        assert_eq!(chosen, "smith");
    }

    #[test]
    fn line_endings_stay_as_written() {
        let (result, _) = choose("a\r\n\r\n/x\r\n", &Action::Turn(BlockType::Quote));
        assert_eq!(result, "a\r\n\r\n> Quotation\r\n");
    }

    #[test]
    fn dismissing_is_no_command_at_all_and_a_slash_inside_a_word_opens_nothing() {
        assert!(opens_at(BlockKind::Paragraph, "a /", 2));
        assert!(!opens_at(BlockKind::Paragraph, "and/or", 3));
        assert!(!opens_at(BlockKind::Code, "/", 0));
        assert!(!opens_at(BlockKind::Comment, "/", 0));
        assert!(opens_at(BlockKind::Blank, "/", 0));
    }

    #[test]
    fn the_list_offers_only_what_is_valid_here() {
        let labels = |context: &Context| -> Vec<&'static str> {
            items(context, &EN).into_iter().map(|item| item.label).collect()
        };
        let free = Context::default();
        let all = labels(&free);
        assert!(all.contains(&"Table of contents") && all.contains(&"Two columns"));
        assert!(!all.contains(&"Image") && !all.contains(&"Citation"));

        let kept = Context {
            open_directives: vec!["keep".to_owned()],
            ..Context::default()
        };
        let inside = labels(&kept);
        for absent in [
            "Table of contents",
            "Two columns",
            "Keep together",
            "Bottom of page",
            "Page break",
        ] {
            assert!(!inside.contains(&absent), "{absent}");
        }
        let toc = Context {
            has_toc: true,
            ..Context::default()
        };
        assert!(!labels(&toc).contains(&"Table of contents"));
        let list_table = Context {
            open_directives: vec!["table".to_owned()],
            ..Context::default()
        };
        assert!(labels(&list_table).is_empty());
        let note = Context {
            block: Some(BlockKind::FootnoteDefinition),
            ..Context::default()
        };
        assert!(!labels(&note).contains(&"Footnote"));
    }

    #[test]
    fn the_query_matches_words_in_either_language_and_prefixes_lead() {
        let all = items(&Context::default(), &EN);
        let labels = |query: &str| -> Vec<&'static str> {
            filtered(all.clone(), query)
                .into_iter()
                .map(|item| item.label)
                .collect()
        };
        assert_eq!(labels("heading")[0], "Heading 1");
        assert_eq!(labels("head 3"), ["Heading 3"]);
        assert_eq!(labels("überschrift 2"), ["Heading 2"]);
        assert_eq!(labels("TABLE")[0], "Table");
        assert!(labels("zzz").is_empty());
        assert_eq!(labels("new section").len(), TEMPLATES.len());
        assert_eq!(labels("").len(), all.len());
    }

    #[test]
    fn the_context_reads_directives_toc_labels_and_keys() {
        let text = "::: keep\na\n:::\n\n::: columns\nb\n\n::: full-width\nc\n";
        let blocks = Blocks::parse(text);
        assert_eq!(open_directives(text, &blocks, text.len()), ["columns", "full-width"]);
        assert!(open_directives(text, &blocks, 9).contains(&"keep".to_owned()));
        assert!(holds_toc("a\n\n::: toc\n"));
        assert!(!holds_toc("toc\n"));
        assert_eq!(free_footnote_label(["a[^note-1] b", "[^note-2]"].into_iter()), "note-3");
        assert_eq!(
            first_citation_key("% c\n@comment{x}\n@string{a=1}\n@book{ smith2020 ,\n title={x}}\n@misc{b,}"),
            Some("smith2020".to_owned())
        );
        assert_eq!(first_citation_key("no entries"), None);
    }
}
