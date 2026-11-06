//! The slash menu's list and what each entry writes.
//!
//! Typing `/` puts a slash in the text. What follows it is the query. Choosing an entry replaces the slash and the query,
//! as one undo step, and leaves the text as the writer typed it if the menu is dismissed.
//!
//! An entry is offered only where its result is valid Druck: a `toc` once in a book and outside containers, a citation only
//! if the bibliography has a key, a cross-reference only if the book has a label. Nothing can be typed into the menu that
//! the engine would then reject. Footnotes, citations, cross-references, and images open their editors in [`crate::editor::rich`].

use std::ops::Range;

use super::convert::{BlockType, Callout, gapped_edit, rewritten, type_of};
use super::{Bias, Command, body_range, line_ending, moved, needs_gap, normalized};
use crate::editor::strings::Key;
use crate::session::TEMPLATES;
use crate::shell::strings::{Language, Table};
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
    /// A new footnote at the caret, with its text in the note's popover. The view carries this one out.
    OpenFootnote,
    /// The citation picker. The view carries this one out.
    OpenCitation,
    /// The cross-reference picker. The view carries this one out.
    OpenReference,
    /// The file dialog for an image. The view carries this one out.
    OpenImage,
    /// A new section from the template with this slug. The session carries this one out.
    Section(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standalone {
    SceneBreak,
    Table,
    PageBreak,
    Toc,
    Bibliography,
    /// A `::: table` of rows from a list.
    ListTable,
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
    /// Whether the book places its bibliography already.
    pub has_bibliography: bool,
    /// Whether the bibliography holds a work to cite.
    pub has_works: bool,
    /// Whether the book has a label to refer to.
    pub has_labels: bool,
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
pub fn type_label(target: BlockType, language: Language) -> &'static str {
    let key = match target {
        BlockType::Paragraph => Key::Paragraph,
        BlockType::Heading(1) => Key::Heading1,
        BlockType::Heading(2) => Key::Heading2,
        BlockType::Heading(3) => Key::Heading3,
        BlockType::Heading(4) => Key::Heading4,
        BlockType::Heading(5) => Key::Heading5,
        BlockType::Heading(_) => Key::Heading6,
        BlockType::Bullets => Key::Bullets,
        BlockType::Numbers => Key::Numbers,
        BlockType::Quote => Key::Quote,
        BlockType::Callout(Callout::Note) => Key::Note,
        BlockType::Callout(Callout::Tip) => Key::Tip,
        BlockType::Callout(Callout::Important) => Key::Important,
        BlockType::Callout(Callout::Warning) => Key::Warning,
        BlockType::Callout(Callout::Caution) => Key::Caution,
        BlockType::Code => Key::Code,
    };
    key.text(language)
}

/// Every entry valid in `context`, in the menu's order, in `language`.
pub fn items(context: &Context, language: Language) -> Vec<Item> {
    let other = match language {
        Language::English => Language::German,
        Language::German => Language::English,
    };
    let mut items = Vec::new();
    let open = &context.open_directives;
    // Only a list belongs in a list table.
    if open.iter().any(|name| name == "table") {
        return items;
    }
    let held = open.iter().any(|name| name == "keep" || name == "bottom");
    let label = |key: Key, icon, action| Item {
        label: key.text(language),
        other_label: key.text(other),
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
            label: type_label(target, language),
            other_label: type_label(target, other),
            group: None,
            icon: type_icon(target),
            action: Action::Turn(target),
        });
    }
    items.push(label(
        Key::SceneBreak,
        "icons/block-break.svg",
        Action::Insert(Standalone::SceneBreak),
    ));
    items.push(label(
        Key::Table,
        "icons/block-table.svg",
        Action::Insert(Standalone::Table),
    ));
    // Written after a list, its rows would read as that list's.
    if !held && context.block != Some(BlockKind::List) {
        items.push(label(
            Key::ListTable,
            "icons/block-table.svg",
            Action::Insert(Standalone::ListTable),
        ));
    }
    items.push(label(Key::Image, "icons/images.svg", Action::OpenImage));
    if open.is_empty() {
        items.push(label(
            Key::Columns,
            "icons/directive-columns.svg",
            Action::Wrap("columns"),
        ));
    }
    if open.last().is_some_and(|name| name == "columns") {
        items.push(label(
            Key::FullWidth,
            "icons/directive-full-width.svg",
            Action::Wrap("full-width"),
        ));
    }
    if !held {
        items.push(label(Key::Keep, "icons/directive-keep.svg", Action::Wrap("keep")));
    }
    if open.is_empty() {
        items.push(label(Key::Bottom, "icons/directive-bottom.svg", Action::Wrap("bottom")));
    }
    if !held {
        items.push(label(
            Key::PageBreak,
            "icons/directive-page-break.svg",
            Action::Insert(Standalone::PageBreak),
        ));
    }
    if open.is_empty() && !context.has_toc {
        items.push(label(Key::Toc, "icons/contents.svg", Action::Insert(Standalone::Toc)));
    }
    if !held && context.has_works && !context.has_bibliography {
        items.push(label(
            Key::Bibliography,
            "icons/book.svg",
            Action::Insert(Standalone::Bibliography),
        ));
    }
    let in_note = context.block == Some(BlockKind::FootnoteDefinition);
    let in_heading = matches!(context.block, Some(BlockKind::Heading { .. }));
    if !in_note && !in_heading {
        items.push(label(Key::Footnote, "icons/block-footnote.svg", Action::OpenFootnote));
    }
    if context.has_works && !in_heading {
        items.push(label(Key::Citation, "icons/book.svg", Action::OpenCitation));
    }
    if context.has_labels && !in_heading {
        items.push(label(
            Key::CrossReference,
            "icons/block-reference.svg",
            Action::OpenReference,
        ));
    }
    for template in TEMPLATES {
        items.push(Item {
            label: template.name.text(language),
            other_label: template.name.text(other),
            group: Some(Key::NewSection.text(language)),
            icon: template.icon,
            action: Action::Section(template.slug),
        });
    }
    items
}

/// The entries matching a query: all its words in either language's label or the group, the ones starting with it first.
pub fn filtered(items: Vec<Item>, query: &str) -> Vec<Item> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let (mut first, mut rest): (Vec<Item>, Vec<Item>) = items
        .into_iter()
        .filter(|item| {
            let label = item.label.to_lowercase();
            let other = item.other_label.to_lowercase();
            let group = item.group.unwrap_or_default().to_lowercase();
            words
                .iter()
                .all(|word| label.contains(word) || other.contains(word) || group.contains(word))
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
    holds_directive(text, "toc")
}

/// Whether this text has a directive line naming `name` alone.
pub fn holds_directive(text: &str, name: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with(":::") && line.trim_start_matches(':').trim() == name)
}

/// Whether choosing a wrapping entry for `slash` wraps the block it stands in, because its line holds other text, rather
/// than writing a new block with a placeholder.
pub fn wraps_block(text: &str, blocks: &Blocks, slash: Range<usize>) -> bool {
    let index = blocks.index_at(slash.start);
    let block = &blocks.as_slice()[index];
    let body = body_range(text, &block.range);
    let body = body.start..body.end.max(slash.end);
    let line = line_of(text, &body, slash.start);
    let lead = line_markers(block.kind, &text[block.range.clone()])
        .iter()
        .find(|marker| {
            let start = block.range.start + marker.range.start;
            line.start <= start && start <= line.end
        })
        .map_or(line.start, |marker| block.range.start + marker.range.end);
    !(text[lead.min(slash.start)..slash.start].trim().is_empty() && text[slash.end..line.end].trim().is_empty())
}

/// What an entry writes on an empty line, without a final line ending, and the part of it the writer replaces first.
fn template(action: &Action, language: Language, eol: &str) -> Option<(String, Range<usize>)> {
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
            Key::PlaceholderHeading.text(language),
            "",
        ),
        Action::Turn(BlockType::Bullets) => with_placeholder("- ".to_owned(), Key::PlaceholderItem.text(language), ""),
        Action::Turn(BlockType::Numbers) => with_placeholder("1. ".to_owned(), Key::PlaceholderItem.text(language), ""),
        Action::Turn(BlockType::Quote) => with_placeholder("> ".to_owned(), Key::PlaceholderQuote.text(language), ""),
        Action::Turn(BlockType::Callout(callout)) => with_placeholder(
            format!("> {}{eol}> ", callout.marker()),
            Key::PlaceholderText.text(language),
            "",
        ),
        Action::Turn(BlockType::Code) => with_placeholder(
            format!("```{eol}"),
            Key::PlaceholderCode.text(language),
            &format!("{eol}```"),
        ),
        Action::Turn(BlockType::Paragraph) => with_placeholder(String::new(), Key::PlaceholderText.text(language), ""),
        Action::Wrap(name) => with_placeholder(
            format!("::: {name}{eol}"),
            Key::PlaceholderText.text(language),
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
        Action::Insert(Standalone::Bibliography) => {
            let text = "::: bibliography".to_owned();
            let end = text.len();
            (text, end..end)
        }
        Action::Insert(Standalone::ListTable) => list_table("::: table", eol, language),
        Action::Insert(Standalone::Table) => {
            let (column, cell) = (
                Key::PlaceholderColumn.text(language),
                Key::PlaceholderCell.text(language),
            );
            with_placeholder(
                "| ".to_owned(),
                column,
                &format!(" | {column} |{eol}| --- | --- |{eol}| {cell} | {cell} |"),
            )
        }
        Action::OpenFootnote
        | Action::OpenCitation
        | Action::OpenReference
        | Action::OpenImage
        | Action::Section(_) => return None,
    })
}

/// A list table of two columns, its header and one row, under the directive line `opening`, with its first placeholder.
pub fn list_table(opening: &str, eol: &str, language: Language) -> (String, Range<usize>) {
    let column = Key::PlaceholderColumn.text(language);
    let cell = Key::PlaceholderCell.text(language);
    let before = format!("{opening}{eol}- - ");
    let start = before.len();
    (
        format!("{before}{column}{eol}  - {column}{eol}- - {cell}{eol}  - {cell}{eol}:::"),
        start..start + column.len(),
    )
}

/// The command for choosing `action` with `slash..caret` as the typed slash and query. A new section is the session's to
/// make, so here it only takes the typed text away.
pub fn apply(text: &str, blocks: &Blocks, slash: Range<usize>, action: &Action, language: Language) -> Option<Command> {
    let eol = line_ending(text);
    let index = blocks.index_at(slash.start);
    let block = &blocks.as_slice()[index];
    let body = body_range(text, &block.range);
    let body = body.start..body.end.max(slash.end);
    if let Action::Section(_) = action {
        return finish(text, vec![Edit::new(slash.clone(), "")], Selection::caret(slash.start));
    }
    let line = line_of(text, &body, slash.start);
    let others_on_line = wraps_block(text, blocks, slash.clone());
    let only_line = line.start == body.start && line.end >= body.end;

    if !others_on_line && only_line {
        // The block is nothing but the slash: the entry takes its place.
        let (new, placeholder) = template(action, language, eol)?;
        let edit = gapped_edit(text, blocks, index, body.clone(), new);
        return placed(text, edit, placeholder);
    }
    if !others_on_line {
        // A line of its own in a longer block: it goes, and the entry follows the block.
        let (new, placeholder) = template(action, language, eol)?;
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
            let (new, placeholder) = template(action, language, eol)?;
            after_block(text, blocks, index, slash, "", new, placeholder)
        }
        Action::Wrap(name) => {
            let new = format!("::: {name}{eol}{cut}{eol}:::");
            let edit = gapped_edit(text, blocks, index, body.clone(), new);
            let selection = after_edit_end(&edit);
            finish(text, vec![edit], selection)
        }
        _ => {
            let (new, placeholder) = template(action, language, eol)?;
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
pub(crate) fn block_insert(text: &str, blocks: &Blocks, index: usize, new: &str, eol: &str) -> (Edit, usize) {
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
        let command = apply(&source, &blocks, slash..end, action, Language::English).expect("a command");
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
            items(context, Language::English)
                .into_iter()
                .map(|item| item.label)
                .collect()
        };
        let free = Context::default();
        let all = labels(&free);
        assert!(all.contains(&"Table of contents") && all.contains(&"Two columns"));
        assert!(all.contains(&"Image") && !all.contains(&"Citation") && !all.contains(&"Cross-reference"));

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
        let all = items(&Context::default(), Language::English);
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
    }
}
