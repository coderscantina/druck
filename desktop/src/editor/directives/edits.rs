//! What the options of a directive write. Each rewrites only fence lines, and the blank lines a removed fence would leave
//! doubled, never the text between them. A change the engine would reject is not offered: every one is read again from
//! the text it would make, and refused if any directive stops being one.

use std::ops::Range;

use super::structure::{self, Kind, Structure};
use crate::editor::commands::{Bias, Command, applied, line_ending, moved, normalized, selection_after};
use crate::source::{BlockKind, Blocks, Edit, Selection};

/// The kinds the options offer a body directive, in their order.
pub const BODY_KINDS: [Kind; 4] = [Kind::Columns, Kind::FullWidth, Kind::Keep, Kind::Bottom];

/// Whether the text after `edits` keeps every directive but the `removed` ones.
fn keeps_the_rest(text: &str, structure: &Structure, edits: &[Edit], removed: usize) -> bool {
    let after = applied(text, edits);
    let blocks = Blocks::parse(&after);
    structure::read(&after, &blocks).regions.len() + removed == structure.regions.len()
}

/// The range of the name in a directive line starting at `start`.
fn name_range(text: &str, line: &Range<usize>) -> Range<usize> {
    let written = &text[line.clone()];
    let after_colons = written.len() - written.trim_start_matches(':').len();
    let lead = written[after_colons..].len() - written[after_colons..].trim_start().len();
    let start = line.start + after_colons + lead;
    let (name, _) = structure::name_of(written);
    start..start + name.len()
}

/// The opening line of `region` names `kind` instead.
pub fn change_kind(
    text: &str,
    blocks: &Blocks,
    structure: &Structure,
    region: usize,
    kind: Kind,
    selection: Selection,
) -> Option<Command> {
    let found = &structure.regions[region];
    if found.kind == kind || !found.kind.has_body() || !kind.has_body() || kind == Kind::Table {
        return None;
    }
    let line = blocks.as_slice()[found.open].range.clone();
    let edits = vec![Edit::new(name_range(text, &line), kind.name())];
    keeps_the_rest(text, structure, &edits, 0).then(|| Command {
        selection: selection_after(selection, &edits),
        edits,
    })
}

/// The kinds a body directive can become here, its own first among them in [`BODY_KINDS`]'s order.
pub fn kinds_offered(text: &str, blocks: &Blocks, structure: &Structure, region: usize) -> Vec<Kind> {
    let own = structure.regions[region].kind;
    if !own.has_body() || own == Kind::Table {
        return Vec::new();
    }
    BODY_KINDS
        .into_iter()
        .filter(|kind| {
            *kind == own || change_kind(text, blocks, structure, region, *kind, Selection::default()).is_some()
        })
        .collect()
}

/// Takes `region`'s lines away and keeps its text. Columns take their full-width parts' lines with them, which cannot
/// stand without them. The caret goes to the start of what they held.
pub fn unwrap(text: &str, blocks: &Blocks, structure: &Structure, region: usize) -> Option<Command> {
    let found = &structure.regions[region];
    if !found.kind.has_body() {
        return None;
    }
    let mut taken = vec![region];
    if found.kind == Kind::Columns {
        taken.extend(
            structure
                .children(region)
                .filter(|&child| structure.regions[child].kind == Kind::FullWidth),
        );
    }
    let list = blocks.as_slice();
    let mut removed = vec![false; list.len()];
    for &index in &taken {
        removed[structure.regions[index].open] = true;
        removed[structure.regions[index].close] = true;
    }
    // A blank line that would follow another, or open the text, goes too.
    let mut last_kept: Option<BlockKind> = None;
    for (at, block) in list.iter().enumerate() {
        if removed[at] {
            continue;
        }
        let doubled = block.kind == BlockKind::Blank && matches!(last_kept, None | Some(BlockKind::Blank));
        if doubled && at > 0 && removed[at - 1] {
            removed[at] = true;
            continue;
        }
        last_kept = Some(block.kind);
    }
    let mut edits: Vec<Edit> = Vec::new();
    for (at, block) in list.iter().enumerate() {
        if !removed[at] {
            continue;
        }
        match edits.last_mut() {
            Some(last) if last.range.end == block.range.start => last.range.end = block.range.end,
            _ => edits.push(Edit::new(block.range.clone(), "")),
        }
    }
    // A closing line without a line ending of its own takes the one before it.
    if let Some(last) = edits.last_mut()
        && last.range.end == text.len()
        && !text.ends_with('\n')
        && last.range.start > 0
        && text[..last.range.start].ends_with('\n')
    {
        last.range.start -= line_ending(text).len().min(last.range.start);
    }
    if !keeps_the_rest(text, structure, &edits, taken.len()) {
        return None;
    }
    let first_text = (found.open + 1..found.close)
        .find(|&at| !removed[at] && list[at].kind != BlockKind::Blank)
        .map_or(list[found.open].range.start, |at| list[at].range.start);
    let caret = moved(first_text, &edits, Bias::After);
    let edits = normalized(edits, text);
    Some(Command {
        edits,
        selection: Selection::caret(caret),
    })
}

/// Wraps the part of a two-column run at `block` in `::: full-width`, so the columns close before it and open again
/// after it. A nested group goes whole. Nothing when `block` is not in columns.
pub fn full_width_at(
    text: &str,
    blocks: &Blocks,
    structure: &Structure,
    block: usize,
    selection: Selection,
) -> Option<Command> {
    let runs = structure.column_runs(blocks);
    let run = runs.iter().find(|run| run.contains(&block))?;
    let unit = structure
        .units(run)
        .into_iter()
        .filter(|unit| blocks.as_slice()[unit.start].kind != BlockKind::Blank)
        .find(|unit| unit.end > block)?;
    let list = blocks.as_slice();
    let eol = line_ending(text);
    let start = list[unit.start].range.start;
    let end = list[unit.end - 1].range.end;
    let close = if text[..end].ends_with('\n') {
        format!(":::{eol}")
    } else {
        format!("{eol}:::")
    };
    let edits = vec![
        Edit::new(start..start, format!("::: full-width{eol}")),
        Edit::new(end..end, close),
    ];
    // One region more: the full-width part.
    let after = applied(text, &edits);
    (structure::read(&after, &Blocks::parse(&after)).regions.len() == structure.regions.len() + 1).then(|| Command {
        selection: selection_after(selection, &edits),
        edits,
    })
}

/// Where the first full-width part of a columns region starts its text.
pub fn full_width_part(blocks: &Blocks, structure: &Structure, region: usize) -> Option<usize> {
    let part = structure
        .children(region)
        .find(|&child| structure.regions[child].kind == Kind::FullWidth)?;
    let found = &structure.regions[part];
    let list = blocks.as_slice();
    let first = (found.open + 1..found.close)
        .find(|&at| list[at].kind != BlockKind::Blank)
        .unwrap_or(found.open);
    Some(list[first].range.start)
}

/// A column's alignment as `align` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    pub fn word(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    /// The next one, for a key that steps through them.
    pub fn next(self) -> Self {
        match self {
            Self::Left => Self::Center,
            Self::Center => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// A new list table's `align` and `widths`, one entry per column. A `widths` entry is whether the column fills, `*`,
/// or fits its content, `auto`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableOptions {
    pub align: Vec<Align>,
    pub fill: Vec<bool>,
}

impl TableOptions {
    fn align_value(&self) -> String {
        self.align
            .iter()
            .map(|align| align.word())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn widths_value(&self) -> String {
        let words: Vec<&str> = self.fill.iter().map(|fill| if *fill { "*" } else { "auto" }).collect();
        words.join(" ")
    }

    /// The opening line of a list table with these options.
    pub fn opening_line(&self) -> String {
        format!(
            "::: table {{align=\"{}\" widths=\"{}\"}}",
            self.align_value(),
            self.widths_value()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region_of(text: &str, kind: Kind) -> (Blocks, Structure, usize) {
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let region = structure
            .regions
            .iter()
            .position(|region| region.kind == kind)
            .expect("the region");
        (blocks, structure, region)
    }

    #[test]
    fn a_new_kind_rewrites_only_the_name_and_keeps_the_caret_on_its_line() {
        let text = "::: keep\nText\n:::\n";
        let (blocks, structure, region) = region_of(text, Kind::Keep);
        let caret = Selection::caret("::: keep".len());
        let command = change_kind(text, &blocks, &structure, region, Kind::Columns, caret).expect("a change");
        assert_eq!(applied(text, &command.edits), "::: columns\nText\n:::\n");
        assert_eq!(command.selection, Selection::caret("::: columns".len()));
    }

    #[test]
    fn only_kinds_the_engine_takes_are_offered() {
        let text = "::: columns\na\n\n::: full-width\nb\n:::\n\n::: keep\nc\n:::\n:::\n";
        let (blocks, structure, columns) = region_of(text, Kind::Columns);
        // Keep or bottom would leave the full-width part outside columns.
        assert_eq!(kinds_offered(text, &blocks, &structure, columns), [Kind::Columns]);
        let keep = structure
            .regions
            .iter()
            .position(|region| region.kind == Kind::Keep)
            .unwrap();
        assert_eq!(
            kinds_offered(text, &blocks, &structure, keep),
            [Kind::FullWidth, Kind::Keep]
        );
        let text = "::: keep\na\n:::\n";
        let (blocks, structure, keep) = region_of(text, Kind::Keep);
        assert_eq!(
            kinds_offered(text, &blocks, &structure, keep),
            [Kind::Columns, Kind::Keep, Kind::Bottom]
        );
    }

    #[test]
    fn unwrapping_keeps_the_text_and_takes_the_full_width_lines_with_the_columns() {
        let text = "Before\n\n::: columns\n\na\n\n::: full-width\nb\n:::\n\nc\n:::\n\nAfter\n";
        let (blocks, structure, columns) = region_of(text, Kind::Columns);
        let command = unwrap(text, &blocks, &structure, columns).expect("an unwrap");
        let result = applied(text, &command.edits);
        assert_eq!(result, "Before\n\na\n\nb\n\nc\n\nAfter\n");
        assert_eq!(&result[command.selection.head..command.selection.head + 1], "a");
        let text = "::: keep\nText\n:::";
        let (blocks, structure, keep) = region_of(text, Kind::Keep);
        let command = unwrap(text, &blocks, &structure, keep).expect("an unwrap");
        assert_eq!(applied(text, &command.edits), "Text");
    }

    #[test]
    fn full_width_wraps_the_part_at_the_caret_and_only_inside_columns() {
        let text = "::: columns\na\n\nb\n\nc\n:::\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let b = blocks.index_at(text.find('b').unwrap());
        let command = full_width_at(text, &blocks, &structure, b, Selection::caret(0)).expect("a change");
        assert_eq!(
            applied(text, &command.edits),
            "::: columns\na\n\n::: full-width\nb\n:::\n\nc\n:::\n"
        );
        let text = "a\n\nb\n";
        let blocks = Blocks::parse(text);
        assert!(full_width_at(text, &blocks, &structure::read(text, &blocks), 0, Selection::caret(0)).is_none());
    }

    #[test]
    fn table_options_write_align_and_widths_for_every_column() {
        let options = TableOptions {
            align: vec![Align::Left, Align::Right],
            fill: vec![true, false],
        };
        assert_eq!(
            options.opening_line(),
            "::: table {align=\"left right\" widths=\"* auto\"}"
        );
        let text = format!("{}\n- - a\n  - b\n:::\n", options.opening_line());
        let blocks = Blocks::parse(&text);
        assert_eq!(structure::read(&text, &blocks).regions[0].kind, Kind::Table);
    }
}
