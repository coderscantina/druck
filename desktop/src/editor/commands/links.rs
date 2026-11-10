//! Reference links, `[text][label]`, whose address stands in a definition elsewhere in the file. The link field shows
//! and changes that address where it stands, so the text keeps its form.

use std::ops::Range;

use super::marks::link_destination;
use super::{Command, line_ending, selection_after};
use crate::source::inline::{SpanKind, inline_spans};
use crate::source::{BlockKind, Blocks, Edit, Selection};

/// The reference link a selection stands in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The label as the link writes it.
    pub label: String,
    /// Where its address stands in the text, when the file defines the label.
    pub address: Option<Range<usize>>,
}

/// A label as Markdown compares it: without case, and with runs of white space as one space.
fn folded(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The label and the address of a definition line, as ranges in it.
fn definition_line(line: &str) -> Option<(Range<usize>, Range<usize>)> {
    let content = line.trim_start_matches(' ');
    let indent = line.len() - content.len();
    let rest = content.strip_prefix('[')?;
    let close = rest.find(']')?;
    let after = rest[close + 1..].strip_prefix(':')?;
    let address = after.trim_start();
    let address_start = line.len() - address.len();
    let length = if address.starts_with('<') {
        address.find('>').map(|end| end + 1)?
    } else {
        address.find(char::is_whitespace).unwrap_or(address.len())
    };
    (indent <= 3 && close > 0 && length > 0)
        .then(|| (indent + 1..indent + 1 + close, address_start..address_start + length))
}

/// Whether a block holds link definitions and nothing else, so it prints nothing.
pub fn is_definitions(block: &str) -> bool {
    let mut lines = block.lines().filter(|line| !line.trim().is_empty()).peekable();
    lines.peek().is_some() && lines.all(|line| definition_line(line).is_some())
}

/// Where the address of `label` stands in the text, in its first definition.
fn address_of(text: &str, blocks: &Blocks, label: &str) -> Option<Range<usize>> {
    let wanted = folded(label);
    (blocks.as_slice().iter())
        .filter(|block| block.kind == BlockKind::Raw)
        .find_map(|block| {
            let mut start = block.range.start;
            text[block.range.clone()].split_inclusive('\n').find_map(|line| {
                let at = start;
                start += line.len();
                let (found, address) = definition_line(line.trim_end_matches(['\n', '\r']))?;
                (folded(&line[found]) == wanted).then(|| at + address.start..at + address.end)
            })
        })
}

/// The reference link the selection stands in or touches.
pub fn reference_at(text: &str, blocks: &Blocks, selection: Selection) -> Option<Reference> {
    let range = selection.range();
    let block = &blocks.as_slice()[blocks.index_at(range.start)];
    let written = &text[block.range.clone()];
    let relative = range.start - block.range.start..range.end.min(block.range.end) - block.range.start;
    let span = inline_spans(block.kind, written).into_iter().find(|span| {
        span.kind == SpanKind::Link
            && written[span.close.clone()].starts_with("][")
            && span.range.start <= relative.end
            && relative.start <= span.range.end
    })?;
    let named = &written[span.close.start + 2..span.close.end - 1];
    let label = if named.is_empty() {
        &written[span.open.end..span.close.start]
    } else {
        named
    };
    Some(Reference {
        label: label.to_owned(),
        address: address_of(text, blocks, label),
    })
}

/// The address as the link field shows it, without the angle brackets an address with spaces is written in.
pub fn shown_address(written: &str) -> &str {
    written
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(written)
}

/// Gives `reference` the address `url`: in its definition where that stands, or in a new definition at the end of the
/// text. The link itself is not touched. An address Markdown cannot hold is `None`.
pub fn set_address(text: &str, reference: &Reference, url: &str, selection: Selection) -> Option<Command> {
    let destination = link_destination(url)?;
    let edit = match &reference.address {
        Some(range) if text[range.clone()] == destination => return None,
        Some(range) => Edit::new(range.clone(), destination),
        None => {
            let eol = line_ending(text);
            let gap = if text.is_empty() || text.ends_with(&format!("{eol}{eol}")) {
                ""
            } else if text.ends_with('\n') {
                eol
            } else {
                &format!("{eol}{eol}")
            };
            Edit::new(
                text.len()..text.len(),
                format!("{gap}[{}]: {destination}{eol}", reference.label),
            )
        }
    };
    let edits = vec![edit];
    Some(Command {
        selection: selection_after(selection, &edits),
        edits,
    })
}

#[cfg(test)]
mod tests {
    use super::super::applied;
    use super::*;

    fn found(text: &str, at: &str) -> (Blocks, Reference) {
        let blocks = Blocks::parse(text);
        let caret = Selection::caret(text.find(at).expect("the place") + 1);
        let reference = reference_at(text, &blocks, caret).expect("a reference link");
        (blocks, reference)
    }

    #[test]
    fn a_reference_link_reads_its_address_from_the_definition() {
        let text = "See [the docs][Docs] and [site][].\n\n[docs]: <http://x y> \"T\"\n[Site]: http://s\n";
        let (_, reference) = found(text, "the docs");
        assert_eq!(reference.label, "Docs");
        let address = reference.address.expect("defined");
        assert_eq!(shown_address(&text[address]), "http://x y");
        let (_, reference) = found(text, "site");
        assert_eq!(&text[reference.address.expect("defined")], "http://s");
        let blocks = Blocks::parse(text);
        assert_eq!(reference_at(text, &blocks, Selection::caret(1)), None);
    }

    #[test]
    fn a_new_address_changes_the_definition_where_it_stands_and_nothing_else() {
        let text = "See [the docs][d].\n\n[d]: http://old \"T\"\n\nMore.\n";
        let (_, reference) = found(text, "the docs");
        let command = set_address(text, &reference, "http://new", Selection::caret(6)).expect("a command");
        assert_eq!(
            applied(text, &command.edits),
            "See [the docs][d].\n\n[d]: http://new \"T\"\n\nMore.\n"
        );
        assert_eq!(command.selection, Selection::caret(6));
        assert!(set_address(text, &reference, "http://old", Selection::caret(6)).is_none());
        assert!(set_address(text, &reference, "<bad>", Selection::caret(6)).is_none());
    }

    #[test]
    fn a_label_without_a_definition_gets_one_at_the_end_of_the_text() {
        let text = "See [the docs][d].\n";
        let (_, reference) = found(text, "the docs");
        assert_eq!(reference.address, None);
        let command = set_address(text, &reference, "my file.md", Selection::caret(6)).expect("a command");
        assert_eq!(
            applied(text, &command.edits),
            "See [the docs][d].\n\n[d]: <my file.md>\n"
        );
    }

    #[test]
    fn a_block_of_definitions_prints_nothing() {
        assert!(is_definitions("[a]: http://x\n[b]: <y z>\n"));
        assert!(!is_definitions("[a]: http://x\ntext\n"));
        assert!(!is_definitions("<div>\n"));
    }
}
