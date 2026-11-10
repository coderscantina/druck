//! What the options of a directive write. Each rewrites only fence lines, and the blank lines a removed fence would leave
//! doubled, never the text between them. A change the engine would reject is not offered: every one is read again from
//! the text it would make, and refused if any directive stops being one.

use std::ops::Range;

use super::structure::{self, Kind, Structure};
use crate::editor::commands::{Bias, Command, applied, line_ending, moved, needs_gap, normalized, selection_after};
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

/// Where `region`'s lines are, from the start of its opening line to the end of its closing line.
pub fn region_range(blocks: &Blocks, structure: &Structure, region: usize) -> Range<usize> {
    let found = &structure.regions[region];
    let list = blocks.as_slice();
    list[found.open].range.start..list[found.close].range.end
}

/// Puts blocks `first` to `last` in a new directive of `kind`, leaving out blank lines at either end. The selection stays
/// on its text.
pub fn wrap(
    text: &str,
    blocks: &Blocks,
    structure: &Structure,
    blocks_range: Range<usize>,
    kind: Kind,
    selection: Selection,
) -> Option<Command> {
    if !kind.has_body() || kind == Kind::Table {
        return None;
    }
    let list = blocks.as_slice();
    let shown = |at: &usize| list[*at].kind != BlockKind::Blank;
    let first = blocks_range.clone().find(shown)?;
    let last = blocks_range.rev().find(shown)?;
    // A directive with only one of its lines inside would close another's instead.
    let held = |at: usize| (first..=last).contains(&at);
    if (structure.regions.iter()).any(|region| held(region.open) != held(region.close)) {
        return None;
    }
    let eol = line_ending(text);
    let start = list[first].range.start;
    let end = list[last].range.end;
    let close = if text[..end].ends_with('\n') {
        format!(":::{eol}")
    } else {
        format!("{eol}:::")
    };
    let edits = vec![
        Edit::new(start..start, format!("::: {}{eol}", kind.name())),
        Edit::new(end..end, close),
    ];
    let after = applied(text, &edits);
    (structure::read(&after, &Blocks::parse(&after)).regions.len() == structure.regions.len() + 1).then(|| Command {
        selection: selection_after(selection, &edits),
        edits,
    })
}

/// The kinds blocks `blocks_range` can be wrapped in, in [`BODY_KINDS`]'s order.
pub fn wraps_offered(text: &str, blocks: &Blocks, structure: &Structure, blocks_range: Range<usize>) -> Vec<Kind> {
    BODY_KINDS
        .into_iter()
        .filter(|kind| {
            wrap(
                text,
                blocks,
                structure,
                blocks_range.clone(),
                *kind,
                Selection::default(),
            )
            .is_some()
        })
        .collect()
}

/// Takes `region` away with all it holds, and the blank line that kept it from the next block. The caret goes where it
/// stood.
pub fn delete_region(text: &str, blocks: &Blocks, structure: &Structure, region: usize) -> Option<Command> {
    let found = &structure.regions[region];
    if !found.kind.has_body() {
        return None;
    }
    let list = blocks.as_slice();
    let (open, close) = (found.open, found.close);
    let mut removed = region_range(blocks, structure, region);
    let mut filler = "";
    if list.get(close + 1).is_some_and(|next| next.kind == BlockKind::Blank) {
        removed.end = list[close + 1].range.end;
    } else if open > 0 && list[open - 1].kind == BlockKind::Blank && close + 1 == list.len() {
        removed.start = list[open - 1].range.start;
    } else if open > 0 && close + 1 < list.len() && needs_gap(blocks, open - 1) && needs_gap(blocks, close + 1) {
        filler = line_ending(text);
    }
    let caret = removed.start;
    let edits = vec![Edit::new(removed, filler)];
    let after = applied(text, &edits);
    let held = structure
        .regions
        .iter()
        .filter(|other| (open..=close).contains(&other.open))
        .count();
    (structure::read(&after, &Blocks::parse(&after)).regions.len() + held == structure.regions.len()).then(|| Command {
        edits: normalized(edits, text),
        selection: Selection::caret(caret),
    })
}

/// A copy of `region` after it. The caret goes to the copy's opening line.
pub fn duplicate_region(text: &str, blocks: &Blocks, structure: &Structure, region: usize) -> Option<Command> {
    let found = &structure.regions[region];
    if !found.kind.has_body() {
        return None;
    }
    let list = blocks.as_slice();
    let range = region_range(blocks, structure, region);
    let body = text[range.clone()].trim_end_matches(['\n', '\r']);
    let eol = line_ending(text);
    let end = range.end;
    let (mut insert, start) = if text[..end].ends_with('\n') {
        (format!("{eol}{body}{eol}"), end + eol.len())
    } else {
        (format!("{eol}{eol}{body}"), end + eol.len() * 2)
    };
    if list
        .get(found.close + 1)
        .is_some_and(|next| next.kind != BlockKind::Blank)
    {
        insert.push_str(eol);
    }
    let edits = vec![Edit::new(end..end, insert)];
    let after = applied(text, &edits);
    let held = (structure.regions.iter())
        .filter(|other| (found.open..=found.close).contains(&other.open))
        .count();
    (structure::read(&after, &Blocks::parse(&after)).regions.len() == structure.regions.len() + held).then(|| Command {
        edits,
        selection: Selection::caret(start),
    })
}

/// Return on an empty line that ends a directive's text: the line goes after the closing line, with the caret on it.
pub fn leave_region(text: &str, blocks: &Blocks, structure: &Structure, caret: usize) -> Option<Command> {
    let empty_line = (caret == 0 || text[..caret].ends_with('\n'))
        && (caret == text.len() || text[caret..].starts_with('\n') || text[caret..].starts_with("\r\n"));
    if !empty_line {
        return None;
    }
    let list = blocks.as_slice();
    let found = (structure.regions.iter())
        .filter(|region| region.kind.has_body() && region.kind != Kind::Table)
        .filter(|region| list[region.open].range.end <= caret && caret <= list[region.close].range.start)
        .filter(|region| text[caret..list[region.close].range.start].trim().is_empty())
        .max_by_key(|region| region.depth)?;
    let content_end = (found.open + 1..found.close)
        .rev()
        .find(|&at| list[at].kind != BlockKind::Blank)
        .map_or(list[found.open].range.end, |at| list[at].range.end);
    let eol = line_ending(text);
    let fence = list[found.close].range.clone();
    let line = text[fence.clone()].trim_end_matches(['\n', '\r']);
    let rest = &text[fence.end..];
    let blank_lead = rest.len() - rest.trim_start_matches(['\n', '\r']).len();
    let mut new = format!("{line}{eol}{eol}");
    let at = content_end + new.len();
    if blank_lead < rest.len() {
        new.push_str(eol);
        new.push_str(eol);
    }
    let edits = vec![Edit::new(content_end..fence.end + blank_lead, new)];
    Some(Command {
        edits: normalized(edits, text),
        selection: Selection::caret(at),
    })
}

/// Backspace at the very start of a directive's first block: the directive goes, and its text stays.
pub fn unwrap_at_start(text: &str, blocks: &Blocks, structure: &Structure, caret: usize) -> Option<Command> {
    let list = blocks.as_slice();
    let starts_text = |at: usize| {
        let range = &list[at].range;
        let line = &text[range.clone()];
        let hashes = if matches!(list[at].kind, BlockKind::Heading { .. }) {
            let count = line.bytes().take_while(|byte| *byte == b'#').count();
            count + line[count..].bytes().take_while(|byte| *byte == b' ').count()
        } else {
            0
        };
        (range.start..=range.start + hashes).contains(&caret)
    };
    let region = (structure.regions.iter().enumerate())
        .filter(|(_, region)| region.kind.has_body() && region.kind != Kind::Table)
        .filter(|(_, region)| {
            (region.open + 1..region.close)
                .find(|&at| list[at].kind != BlockKind::Blank)
                .is_some_and(starts_text)
        })
        .max_by_key(|(_, region)| region.depth)?
        .0;
    let mut command = unwrap(text, blocks, structure, region)?;
    command.selection = Selection::caret(moved(caret, &command.edits, Bias::After));
    Some(command)
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
    fn wrapping_takes_the_blocks_between_their_blank_lines() {
        let text = "a\n\nb\n\nc\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let command = wrap(text, &blocks, &structure, 0..4, Kind::Keep, Selection::caret(0)).expect("a wrap");
        assert_eq!(applied(text, &command.edits), "::: keep\na\n\nb\n:::\n\nc\n");
        let offered = wraps_offered(text, &blocks, &structure, 0..4);
        assert_eq!(offered, [Kind::Columns, Kind::Keep, Kind::Bottom]);
        let text = "::: columns\na\n\nb\n:::\n";
        let (blocks, structure, _) = region_of(text, Kind::Columns);
        let b = blocks.index_at(text.find('b').unwrap());
        assert_eq!(
            wraps_offered(text, &blocks, &structure, b..b + 1),
            [Kind::FullWidth, Kind::Keep]
        );
        // A selection from inside the columns to after them holds only their closing line.
        let text = "::: columns\na\n\nb\n:::\n\nc\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let b = blocks.index_at(text.find('b').unwrap());
        let c = blocks.index_at(text.find('c').unwrap());
        assert!(wraps_offered(text, &blocks, &structure, b..c + 1).is_empty());
    }

    #[test]
    fn a_region_is_deleted_and_duplicated_whole() {
        let text = "a\n\n::: keep\nb\n:::\n\nc\n";
        let (blocks, structure, keep) = region_of(text, Kind::Keep);
        let command = delete_region(text, &blocks, &structure, keep).expect("a delete");
        assert_eq!(applied(text, &command.edits), "a\n\nc\n");
        let command = duplicate_region(text, &blocks, &structure, keep).expect("a copy");
        let result = applied(text, &command.edits);
        assert_eq!(result, "a\n\n::: keep\nb\n:::\n\n::: keep\nb\n:::\n\nc\n");
        assert!(result[command.selection.head..].starts_with("::: keep\nb\n:::\n\nc"));
        let text = "a\n::: keep\nb\n:::\nc\n";
        let (blocks, structure, keep) = region_of(text, Kind::Keep);
        let command = delete_region(text, &blocks, &structure, keep).expect("a delete");
        assert_eq!(applied(text, &command.edits), "a\n\nc\n");
    }

    #[test]
    fn return_on_an_empty_last_line_leaves_the_region() {
        let text = "::: keep\nA\n\n\n:::\n\nNext\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let caret = "::: keep\nA\n\n".len();
        let command = leave_region(text, &blocks, &structure, caret).expect("out");
        let result = applied(text, &command.edits);
        assert_eq!(result, "::: keep\nA\n:::\n\n\n\nNext\n");
        assert_eq!(command.selection.head, "::: keep\nA\n:::\n\n".len());
        // Text after the caret keeps it in.
        let text = "::: keep\nA\n\n\nB\n:::\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        assert!(leave_region(text, &blocks, &structure, "::: keep\nA\n\n".len()).is_none());
        let text = "::: keep\nA\n\n\n:::";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let command = leave_region(text, &blocks, &structure, "::: keep\nA\n\n".len()).expect("out");
        assert_eq!(applied(text, &command.edits), "::: keep\nA\n:::\n\n");
    }

    #[test]
    fn backspace_at_the_start_of_the_first_block_unwraps() {
        let text = "Before\n\n::: keep\n### Head\n\nText\n:::\n";
        let blocks = Blocks::parse(text);
        let structure = structure::read(text, &blocks);
        let caret = text.find("Head").unwrap();
        let command = unwrap_at_start(text, &blocks, &structure, caret).expect("an unwrap");
        let result = applied(text, &command.edits);
        assert_eq!(result, "Before\n\n### Head\n\nText\n");
        assert_eq!(command.selection.head, result.find("Head").unwrap());
        assert!(unwrap_at_start(text, &blocks, &structure, text.find("Text").unwrap()).is_none());
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
