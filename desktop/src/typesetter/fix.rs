//! What a typesetting fix may be: a directive that exists, written at a block, or one value of the theme, which holds
//! for the whole book. Nothing here is a setting of one paragraph. Holds no gpui types.

use std::ops::Range;

use serde_json::Value;

use crate::editor::rich::images::{Figure, Placement};
use crate::editor::rich::table;
use crate::preview::directives;
use crate::source::{BlockKind, Blocks, Edit};

/// How far a fix may move a style's tracking from zero, in em. More than this shows as spaced or squeezed letters.
const TRACKING_LIMIT: f64 = 0.02;

/// A block of the book, by where it starts in the text the list of warnings was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The chapter's file, relative to the project folder.
    pub file: String,
    /// The line the block starts on, from 1, as a warning names it.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fix {
    /// `::: page-break` before the block.
    PageBreak(Place),
    /// `::: keep` around the block.
    Keep(Place),
    /// Where the figure at the block goes on its page.
    Placement(Place, Placement),
    /// Where the table at the block goes on its page, written in its caption or its `::: table` line.
    TablePlacement(Place, Placement),
    /// One value of the theme, by its dotted property.
    Theme { property: String, value: Value },
}

impl Fix {
    /// A fix as the agent names it. The reason it is none is for the agent, so it can try again.
    pub fn from_json(input: &Value) -> Result<Self, String> {
        let kind = input["kind"].as_str().ok_or("a fix needs a kind.")?;
        if kind == "theme" {
            let property = input["property"].as_str().ok_or("a theme fix needs a property.")?;
            let value = input.get("value").ok_or("a theme fix needs a value.")?.clone();
            check_theme_value(property, &value)?;
            return Ok(Self::Theme {
                property: property.to_owned(),
                value,
            });
        }
        let file = input["file"].as_str().ok_or("a fix at a block needs its file.")?;
        let line = (input["line"].as_u64())
            .filter(|line| *line > 0)
            .ok_or("a fix at a block needs its line, counted from 1.")?;
        let place = Place {
            file: file.to_owned(),
            line: line as usize,
        };
        let placement = || match input["value"].as_str() {
            Some("top") => Ok(Placement::Top),
            Some("bottom") => Ok(Placement::Bottom),
            Some("here") => Ok(Placement::Here),
            _ => Err("a placement is top, bottom, or here.".to_owned()),
        };
        match kind {
            "page-break" => Ok(Self::PageBreak(place)),
            "keep" => Ok(Self::Keep(place)),
            "placement" => Ok(Self::Placement(place, placement()?)),
            "table-placement" => Ok(Self::TablePlacement(place, placement()?)),
            other => Err(format!(
                "{other} is no kind of fix. The kinds are page-break, keep, placement, table-placement, and theme."
            )),
        }
    }

    /// The block the fix is written at. A theme value has none.
    pub fn place(&self) -> Option<&Place> {
        match self {
            Self::PageBreak(place) | Self::Keep(place) | Self::Placement(place, _) | Self::TablePlacement(place, _) => {
                Some(place)
            }
            Self::Theme { .. } => None,
        }
    }

    /// The same fix at another line of its file.
    pub fn at_line(&self, line: usize) -> Self {
        let mut moved = self.clone();
        match &mut moved {
            Self::PageBreak(place) | Self::Keep(place) | Self::Placement(place, _) | Self::TablePlacement(place, _) => {
                place.line = line
            }
            Self::Theme { .. } => {}
        }
        moved
    }

    /// Among edits at one place, which goes first: a page break stands before a keep's opening line, and a figure's or
    /// table's attributes before a keep's closing line.
    fn rank(&self) -> u8 {
        match self {
            Self::PageBreak(_) | Self::Placement(..) | Self::TablePlacement(..) => 0,
            Self::Keep(_) | Self::Theme { .. } => 1,
        }
    }
}

/// The theme values a fix may set. The limits a warning is measured against, such as `typesetting.heading-lines`, are
/// not among them: changing one clears its warnings without moving a line.
fn check_theme_value(property: &str, value: &Value) -> Result<(), String> {
    let parts: Vec<&str> = property.split('.').collect();
    match parts.as_slice() {
        ["typesetting", "hyphen-fragments"] => Ok(()),
        ["styles", _, "hyphenate"] if value.is_boolean() => Ok(()),
        ["styles", _, "tracking"] => match value.as_f64() {
            Some(tracking) if tracking.abs() <= TRACKING_LIMIT => Ok(()),
            _ => Err(format!("tracking as a fix stays within {TRACKING_LIMIT} em of zero.")),
        },
        _ => Err(format!(
            "{property} is not a value a fix may set. Those are typesetting.hyphen-fragments, styles.<style>.tracking, \
             and styles.<style>.hyphenate."
        )),
    }
}

/// The block that holds `line`, without the line ending of its last line.
pub fn block_at(text: &str, line: usize) -> Result<Range<usize>, String> {
    let start = line_start(text, line).ok_or_else(|| format!("the file has no line {line}."))?;
    let blocks = Blocks::parse(text);
    let block = &blocks.as_slice()[blocks.index_at(start)];
    let stands = matches!(
        block.kind,
        BlockKind::Heading { .. }
            | BlockKind::Paragraph
            | BlockKind::List
            | BlockKind::Quote
            | BlockKind::Code
            | BlockKind::Table
    );
    if !stands {
        return Err(format!("line {line} is in no block a directive can stand at."));
    }
    let written = text[block.range.clone()].trim_end_matches(['\n', '\r']);
    Ok(block.range.start..block.range.start + written.len())
}

/// The table that holds `line`, with the block of its rows, which its placement does not change: a pipe table, or the
/// list or pipe table inside a `::: table`. The line may be any of the table's, its directive's or caption's too.
fn table_at(text: &str, line: usize) -> Result<(table::Grid, Range<usize>), String> {
    let start = line_start(text, line).ok_or_else(|| format!("the file has no line {line}."))?;
    let blocks = Blocks::parse(text);
    let index = blocks.index_at(start);
    let no_table = || format!("line {line} is in no table.");
    let region = (table::regions(text, &blocks).into_iter())
        .find(|region| region.first <= index && index <= region.last)
        .ok_or_else(no_table)?;
    let grid = region.grid.map_err(|_| no_table())?;
    let rows = (blocks.as_slice()[region.first..=region.last].iter())
        .find(|block| matches!(block.kind, BlockKind::Table | BlockKind::List))
        .ok_or_else(no_table)?;
    let written = text[rows.range.clone()].trim_end_matches(['\n', '\r']);
    Ok((grid, rows.range.start..rows.range.start + written.len()))
}

/// The block a fix stands at: for a table's placement the block of its rows, for any other the block at its line.
pub fn block_of(text: &str, fix: &Fix) -> Result<Range<usize>, String> {
    match fix {
        Fix::TablePlacement(place, _) => table_at(text, place.line).map(|(_, rows)| rows),
        Fix::PageBreak(place) | Fix::Keep(place) | Fix::Placement(place, _) => block_at(text, place.line),
        Fix::Theme { .. } => Err("a theme value stands at no block.".to_owned()),
    }
}

fn line_start(text: &str, line: usize) -> Option<usize> {
    let mut start = 0;
    for _ in 1..line {
        start += text[start..].find('\n')? + 1;
    }
    (start < text.len()).then_some(start)
}

/// The line an offset is on, from 1.
pub fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// What a directive fix writes into its chapter, as edits of `text`.
fn edits_of(text: &str, fix: &Fix, block: &Range<usize>) -> Result<Vec<Edit>, String> {
    let has = directives::read(text, block);
    match fix {
        Fix::PageBreak(_) if has.page_break => Err("a page break stands there already.".to_owned()),
        Fix::PageBreak(_) => Ok(directives::toggle_page_break(text, block).edits),
        Fix::Keep(_) if has.keep => Err("the block is kept together already.".to_owned()),
        Fix::Keep(_) => Ok(directives::toggle_keep(text, block).edits),
        Fix::Placement(_, placement) => {
            let written = &text[block.clone()];
            let figure = Figure::parse(written).ok_or("the block is no figure, so it has no placement.")?;
            let (caption, label) = (figure.caption(written), figure.label_name(written));
            (figure.edit(written, block.start, caption, label, *placement))
                .map(|edit| vec![edit])
                .ok_or_else(|| "the figure is placed there already.".to_owned())
        }
        Fix::TablePlacement(place, placement) => {
            let (grid, _) = table_at(text, place.line)?;
            let word = match placement {
                Placement::Top => "top",
                Placement::Bottom => "bottom",
                Placement::Here => "",
            };
            if grid.placement(text) == word {
                return Err("the table is placed there already.".to_owned());
            }
            grid.set_placement(text, word)
                .map_err(|_| "a table needs a caption for a placement, or must be a ::: table.".to_owned())
        }
        Fix::Theme { .. } => Ok(Vec::new()),
    }
}

/// The edits that write `fixes` into one chapter, in the order the buffer takes them. Every fix names its block by its
/// line in `text`, so fixes do not move each other's places.
pub fn edits(text: &str, fixes: &[&Fix]) -> Result<Vec<Edit>, String> {
    let mut all: Vec<(u8, Edit)> = Vec::new();
    for fix in fixes {
        if fix.place().is_none() {
            continue;
        }
        let block = block_of(text, fix)?;
        all.extend(edits_of(text, fix, &block)?.into_iter().map(|edit| (fix.rank(), edit)));
    }
    all.sort_by_key(|(rank, edit)| (edit.range.start, *rank));
    let edits: Vec<Edit> = all.into_iter().map(|(_, edit)| edit).collect();
    if edits.windows(2).any(|pair| pair[0].range.end > pair[1].range.start) {
        return Err("two fixes change the same words.".to_owned());
    }
    Ok(edits)
}

/// `text` with `edits`, which are sorted and do not overlap.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut result = String::with_capacity(text.len() + edits.iter().map(|edit| edit.text.len()).sum::<usize>());
    let mut copied = 0;
    for edit in edits {
        result.push_str(&text[copied..edit.range.start]);
        result.push_str(&edit.text);
        copied = edit.range.end;
    }
    result.push_str(&text[copied..]);
    result
}

/// The line of the text before `edits` that a line of the text after them came from. A line the edits added counts as
/// the line it was added before.
pub fn line_before(text: &str, edits: &[Edit], line_after: usize) -> usize {
    let after = apply(text, edits);
    let Some(offset) = line_start(&after, line_after) else {
        return line_after;
    };
    // How much longer the text after the edits is, up to the edit being looked at.
    let mut grown: isize = 0;
    for edit in edits {
        let start = (edit.range.start as isize + grown) as usize;
        if offset < start {
            break;
        }
        if offset < start + edit.text.len() {
            return line_of(text, edit.range.start);
        }
        grown += edit.text.len() as isize - edit.range.len() as isize;
    }
    line_of(text, (offset as isize - grown) as usize)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const TEXT: &str = "# One\n\nFirst paragraph.\n\n![A chart](chart.svg)\n\nLast paragraph.\n";

    fn fix(input: Value) -> Fix {
        Fix::from_json(&input).expect("a fix")
    }

    fn written(fixes: &[Fix]) -> String {
        let fixes: Vec<&Fix> = fixes.iter().collect();
        apply(TEXT, &edits(TEXT, &fixes).expect("the fixes can be written"))
    }

    #[test]
    fn a_page_break_and_a_keep_go_around_the_block_at_a_line() {
        let at = json!({"file": "a.md", "line": 3});
        let page_break = fix(json!({"kind": "page-break", "file": "a.md", "line": 3}));
        let keep = fix(json!({"kind": "keep", "file": at["file"], "line": at["line"]}));
        assert_eq!(
            written(std::slice::from_ref(&page_break)),
            "# One\n\n::: page-break\n\nFirst paragraph.\n\n![A chart](chart.svg)\n\nLast paragraph.\n"
        );
        assert_eq!(
            written(&[keep, page_break]),
            "# One\n\n::: page-break\n\n::: keep\nFirst paragraph.\n:::\n\n![A chart](chart.svg)\n\nLast paragraph.\n"
        );
    }

    #[test]
    fn a_placement_is_written_on_a_figure_only() {
        let top = fix(json!({"kind": "placement", "file": "a.md", "line": 5, "value": "top"}));
        assert!(written(&[top]).contains("![A chart](chart.svg){placement=top}\n"));
        let on_text = fix(json!({"kind": "placement", "file": "a.md", "line": 3, "value": "top"}));
        assert!(edits(TEXT, &[&on_text]).is_err());
    }

    #[test]
    fn a_tables_placement_goes_in_its_caption_or_its_table_line() {
        let pipe = "Text.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n: Sizes {#tbl:sizes}\n\nMore.\n";
        let bottom = fix(json!({"kind": "table-placement", "file": "a.md", "line": 4, "value": "bottom"}));
        let placed = apply(
            pipe,
            &edits(pipe, &[&bottom]).expect("a captioned table takes a placement"),
        );
        assert!(placed.contains(": Sizes {#tbl:sizes placement=bottom}\n"), "{placed}");
        assert_eq!(
            block_of(pipe, &bottom).map(|block| &pipe[block]),
            Ok("| A | B |\n|---|---|\n| 1 | 2 |")
        );
        assert!(edits(&placed, &[&bottom]).is_err(), "placed there already");

        let list = "::: table\n- - a\n  - b\n:::\n";
        let top = fix(json!({"kind": "table-placement", "file": "a.md", "line": 1, "value": "top"}));
        let placed = apply(list, &edits(list, &[&top]).expect("a list table needs no caption"));
        assert!(placed.starts_with("::: table {placement=\"top\"}\n"), "{placed}");

        let bare = "| A |\n|---|\n| 1 |\n";
        let on_bare = fix(json!({"kind": "table-placement", "file": "a.md", "line": 1, "value": "top"}));
        assert!(
            edits(bare, &[&on_bare]).is_err(),
            "a pipe table without a caption has no place for it"
        );
        assert!(edits(TEXT, &[&on_bare]).is_err(), "no table there");
    }

    #[test]
    fn a_fix_is_refused_where_nothing_can_take_it() {
        let blank = fix(json!({"kind": "page-break", "file": "a.md", "line": 2}));
        assert!(edits(TEXT, &[&blank]).is_err());
        let past = fix(json!({"kind": "keep", "file": "a.md", "line": 40}));
        assert!(edits(TEXT, &[&past]).is_err());
        let broken = "::: page-break\n\nText.\n";
        let again = fix(json!({"kind": "page-break", "file": "a.md", "line": 3}));
        assert!(edits(broken, &[&again]).is_err(), "a second page break at one block");
    }

    #[test]
    fn only_book_wide_values_that_move_lines_are_theme_fixes() {
        let theme = |property: &str, value: Value| {
            Fix::from_json(&json!({"kind": "theme", "property": property, "value": value}))
        };
        assert!(theme("typesetting.hyphen-fragments", json!([2, 2])).is_ok());
        assert!(theme("styles.body.tracking", json!(0.01)).is_ok());
        assert!(theme("styles.body.tracking", json!(0.2)).is_err());
        assert!(theme("styles.body.hyphenate", json!(true)).is_ok());
        assert!(
            theme("typesetting.heading-lines", json!(2)).is_err(),
            "it moves the bar, not a line"
        );
        assert!(theme("styles.body.size", json!("9pt")).is_err());
        assert!(Fix::from_json(&json!({"kind": "tighten", "file": "a.md", "line": 3})).is_err());
    }

    #[test]
    fn a_line_after_the_fixes_is_traced_to_the_line_it_was() {
        let page_break = fix(json!({"kind": "page-break", "file": "a.md", "line": 3}));
        let keep = fix(json!({"kind": "keep", "file": "a.md", "line": 3}));
        let edits = edits(TEXT, &[&page_break, &keep]).expect("edits");
        // After: 1 heading, 3 page break, 5 keep, 6 the paragraph, 7 the close, 9 the figure, 11 the last paragraph.
        assert_eq!(line_before(TEXT, &edits, 1), 1);
        assert_eq!(line_before(TEXT, &edits, 6), 3);
        assert_eq!(line_before(TEXT, &edits, 9), 5);
        assert_eq!(line_before(TEXT, &edits, 11), 7);
    }
}
