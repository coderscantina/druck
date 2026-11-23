//! Which layout directives the text holds, read from its blocks as the engine reads them (`src/markdown.rs`), so Write
//! draws the effect of exactly those the pages will set. A directive the engine rejects is left out, and its lines stay
//! raw text beside the engine's own message. Holds no gpui types.

use std::ops::Range;

use crate::source::{BlockKind, Blocks};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Columns,
    FullWidth,
    Keep,
    Bottom,
    Table,
    PageBreak,
    Toc,
    Bibliography,
    Index,
    FrontMatter,
    MainMatter,
    BackMatter,
}

impl Kind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "columns" => Self::Columns,
            "full-width" => Self::FullWidth,
            "keep" => Self::Keep,
            "bottom" => Self::Bottom,
            "table" => Self::Table,
            "page-break" => Self::PageBreak,
            "toc" => Self::Toc,
            "bibliography" => Self::Bibliography,
            "index" => Self::Index,
            "front-matter" => Self::FrontMatter,
            "main-matter" => Self::MainMatter,
            "back-matter" => Self::BackMatter,
            _ => return None,
        })
    }

    /// The name as the text writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::FullWidth => "full-width",
            Self::Keep => "keep",
            Self::Bottom => "bottom",
            Self::Table => "table",
            Self::PageBreak => "page-break",
            Self::Toc => "toc",
            Self::Bibliography => "bibliography",
            Self::Index => "index",
            Self::FrontMatter => "front-matter",
            Self::MainMatter => "main-matter",
            Self::BackMatter => "back-matter",
        }
    }

    /// Whether it holds text up to a closing line, rather than standing alone.
    pub fn has_body(self) -> bool {
        matches!(
            self,
            Self::Columns | Self::FullWidth | Self::Keep | Self::Bottom | Self::Table
        )
    }

    /// Whether it opens a part of the book. The Binder writes these, and Write never shows them.
    pub fn is_part(self) -> bool {
        self.matter().is_some()
    }

    fn matter(self) -> Option<u8> {
        match self {
            Self::FrontMatter => Some(0),
            Self::MainMatter => Some(1),
            Self::BackMatter => Some(2),
            _ => None,
        }
    }
}

/// A directive the engine accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub kind: Kind,
    /// The block of its opening line.
    pub open: usize,
    /// The block of its closing line, or the opening line for one that stands alone.
    pub close: usize,
    /// The region holding it, as an index into [`Structure::regions`].
    pub parent: Option<usize>,
    /// How many regions hold it.
    pub depth: usize,
}

/// Why the engine will not take a directive line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// It names no directive the engine knows.
    Unknown,
    /// A known directive where the engine refuses it, such as a second table of contents.
    Refused,
    /// It opens a region that no line closes.
    Unclosed,
    /// It closes nothing.
    Stray,
}

/// A directive line the engine will not take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejected {
    pub block: usize,
    pub fault: Fault,
    /// The block of the line that closes it, which goes with it.
    pub close: Option<usize>,
}

/// The accepted directives of a text, in the order they open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Structure {
    pub regions: Vec<Region>,
    /// The directive lines the engine will not take, in text order. A rejected region's closing line is not listed.
    pub rejected: Vec<Rejected>,
    /// Each accepted directive line's block, with the region it opens or closes, sorted.
    fences: Vec<(usize, usize)>,
    /// The text is a page Druck makes and nothing else, so a new page before it is the page's own edge.
    made_page: bool,
}

/// One opening line still waiting for its closing line.
struct Open {
    kind: Option<Kind>,
    block: usize,
    valid: bool,
}

/// The name of a directive line and what follows it, as the engine splits them.
pub fn name_of(line: &str) -> (&str, &str) {
    let rest = line.trim_start_matches(':').trim();
    let (name, attributes) = rest.split_at(rest.find([' ', '\t', '{']).unwrap_or(rest.len()));
    (name, attributes.trim_start())
}

/// Reads the directives of `text`, whose blocks are `blocks`.
pub fn read(text: &str, blocks: &Blocks) -> Structure {
    let mut stack: Vec<Open> = Vec::new();
    let mut found: Vec<(Kind, usize, usize)> = Vec::new();
    let mut rejected: Vec<Rejected> = Vec::new();
    let fault_of = |kind: Option<Kind>| if kind.is_some() { Fault::Refused } else { Fault::Unknown };
    let (mut toc, mut bibliography, mut index) = (false, false, false);
    let mut matter: Option<u8> = None;
    for (at, block) in blocks.as_slice().iter().enumerate() {
        if block.kind != BlockKind::Directive {
            continue;
        }
        let line = text[block.range.clone()].trim_end();
        let (name, attributes) = name_of(line);
        if name.is_empty() {
            match stack.pop() {
                Some(Open {
                    kind: Some(kind),
                    block,
                    valid: true,
                }) => found.push((kind, block, at)),
                Some(open) => rejected.push(Rejected {
                    block: open.block,
                    fault: fault_of(open.kind),
                    close: Some(at),
                }),
                None => rejected.push(Rejected {
                    block: at,
                    fault: Fault::Stray,
                    close: None,
                }),
            }
            continue;
        }
        let kind = Kind::from_name(name);
        let open: Vec<Kind> = stack.iter().filter_map(|open| open.kind).collect();
        let kept = open.iter().any(|kind| matches!(kind, Kind::Keep | Kind::Bottom));
        let attributes_allowed = attributes.is_empty() || kind == Some(Kind::Table);
        let valid = attributes_allowed
            && match kind {
                None | Some(Kind::Keep | Kind::Table) => kind.is_some(),
                Some(Kind::Bottom) => stack.is_empty(),
                Some(Kind::Columns) => !open.contains(&Kind::Columns) && !kept,
                Some(Kind::FullWidth) => stack.last().and_then(|open| open.kind) == Some(Kind::Columns),
                Some(Kind::PageBreak) => !kept,
                Some(Kind::Bibliography) => !kept && !std::mem::replace(&mut bibliography, true),
                Some(Kind::Index) => !kept && !std::mem::replace(&mut index, true),
                Some(Kind::Toc) => !kept && !open.contains(&Kind::Columns) && !std::mem::replace(&mut toc, true),
                Some(matter_kind @ (Kind::FrontMatter | Kind::MainMatter | Kind::BackMatter)) => {
                    let order = matter_kind.matter();
                    let in_order = stack.is_empty() && matter < order;
                    if in_order {
                        matter = order;
                    }
                    in_order
                }
            };
        match kind {
            Some(kind) if !kind.has_body() => {
                if valid {
                    found.push((kind, at, at));
                } else {
                    rejected.push(Rejected {
                        block: at,
                        fault: Fault::Refused,
                        close: None,
                    });
                }
            }
            // An unknown name still opens a container, which its closing line closes.
            _ => stack.push(Open { kind, block: at, valid }),
        }
    }
    rejected.extend(stack.into_iter().map(|open| Rejected {
        block: open.block,
        fault: if open.valid {
            Fault::Unclosed
        } else {
            fault_of(open.kind)
        },
        close: None,
    }));
    rejected.sort_by_key(|rejected| rejected.block);
    found.sort_by_key(|(_, open, _)| *open);
    let mut regions: Vec<Region> = Vec::with_capacity(found.len());
    for (kind, open, close) in found {
        let parent = (0..regions.len()).rev().find(|&holder| {
            regions[holder].kind.has_body() && regions[holder].open < open && close < regions[holder].close
        });
        let depth = parent.map_or(0, |parent| regions[parent].depth + 1);
        regions.push(Region {
            kind,
            open,
            close,
            parent,
            depth,
        });
    }
    let mut fences: Vec<(usize, usize)> = regions
        .iter()
        .enumerate()
        .flat_map(|(index, region)| [(region.open, index), (region.close, index)])
        .collect();
    fences.sort_unstable();
    fences.dedup();
    let only_directives = (blocks.as_slice().iter()).all(|block| {
        matches!(
            block.kind,
            BlockKind::Directive | BlockKind::Blank | BlockKind::Comment | BlockKind::FrontMatter
        )
    });
    let made_page = only_directives
        && (regions.iter()).any(|region| matches!(region.kind, Kind::Toc | Kind::Index | Kind::Bibliography));
    Structure {
        regions,
        rejected,
        fences,
        made_page,
    }
}

/// How many closing lines of `text` close nothing in it, and how many regions it leaves open at its end.
pub fn balance(text: &str) -> (usize, usize) {
    let blocks = Blocks::parse(text);
    let mut open = 0_usize;
    let mut strays = 0;
    for block in blocks
        .as_slice()
        .iter()
        .filter(|block| block.kind == BlockKind::Directive)
    {
        let (name, _) = name_of(text[block.range.clone()].trim_end());
        if name.is_empty() {
            match open.checked_sub(1) {
                Some(left) => open = left,
                None => strays += 1,
            }
        } else if Kind::from_name(name).is_none_or(Kind::has_body) {
            open += 1;
        }
    }
    (strays, open)
}

impl Structure {
    /// The region whose opening or closing line is block `block`.
    pub fn fence(&self, block: usize) -> Option<usize> {
        let at = self.fences.partition_point(|(fence, _)| *fence < block);
        self.fences
            .get(at)
            .filter(|(fence, _)| *fence == block)
            .map(|(_, region)| *region)
    }

    /// Whether block `block` is a directive line that takes no room and no caret in Write: a fence line of a region
    /// whose text shows, or a line that is no mark. A table shows whole in its place.
    pub fn hides_line(&self, block: usize) -> bool {
        self.fence(block).is_some_and(|region| {
            let kind = self.regions[region].kind;
            kind != Kind::Table && (kind.has_body() || self.is_no_mark(region))
        })
    }

    /// Whether the text is a page Druck makes and nothing else, which then stands as the whole page.
    pub fn is_made_page(&self) -> bool {
        self.made_page
    }

    /// Whether `region` shows nothing in Write: a part's line, which the Binder keeps, or a new page before a page
    /// Druck makes that stands as its own section, whose edge the Binder's row already is.
    pub fn is_no_mark(&self, region: usize) -> bool {
        let kind = self.regions[region].kind;
        kind.is_part() || (kind == Kind::PageBreak && self.made_page)
    }

    /// The regions directly inside `region`.
    pub fn children(&self, region: usize) -> impl Iterator<Item = usize> + '_ {
        (0..self.regions.len()).filter(move |&index| self.regions[index].parent == Some(region))
    }

    /// The runs of blocks set in two columns: each `columns` region's body between its `full-width` parts, without the
    /// blank lines at either end. Sorted.
    pub fn column_runs(&self, blocks: &Blocks) -> Vec<Range<usize>> {
        let list = blocks.as_slice();
        let mut runs = Vec::new();
        for (index, region) in self.regions.iter().enumerate() {
            if region.kind != Kind::Columns {
                continue;
            }
            let mut start = region.open + 1;
            let parts = self
                .children(index)
                .filter(|&child| self.regions[child].kind == Kind::FullWidth)
                .map(|child| self.regions[child].open..self.regions[child].close + 1);
            for part in parts.chain(std::iter::once(region.close..region.close)) {
                let mut run = start..part.start;
                while run.start < run.end && list[run.start].kind == BlockKind::Blank {
                    run.start += 1;
                }
                while run.end > run.start && list[run.end - 1].kind == BlockKind::Blank {
                    run.end -= 1;
                }
                if !run.is_empty() {
                    runs.push(run);
                }
                start = part.end;
            }
        }
        runs
    }

    /// Where the blocks of a run may part between the columns: the blocks a nested directive holds go together.
    pub fn units(&self, run: &Range<usize>) -> Vec<Range<usize>> {
        let mut units = Vec::new();
        let mut at = run.start;
        while at < run.end {
            let end = self
                .fence(at)
                .map(|region| &self.regions[region])
                .filter(|region| region.open == at)
                .map_or(at + 1, |region| region.close + 1);
            units.push(at..end.min(run.end));
            at = end;
        }
        units
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, usize)> {
        read(text, &Blocks::parse(text))
            .regions
            .iter()
            .map(|region| (region.kind, region.depth))
            .collect()
    }

    #[test]
    fn part_lines_and_fence_lines_are_hidden_and_marks_are_not() {
        let text = "::: front-matter\n\n::: toc\n\n::: main-matter\n\n::: keep\nText\n:::\n\n::: back-matter\n";
        let blocks = Blocks::parse(text);
        let structure = read(text, &blocks);
        let hidden: Vec<&str> = (0..blocks.as_slice().len())
            .filter(|&block| structure.hides_line(block))
            .map(|block| text[blocks.as_slice()[block].range.clone()].trim_end())
            .collect();
        assert_eq!(
            hidden,
            [
                "::: front-matter",
                "::: main-matter",
                "::: keep",
                ":::",
                "::: back-matter"
            ]
        );
    }

    #[test]
    fn reads_nested_directives_with_their_depth() {
        let text = "::: columns\na\n\n::: full-width\nb\n:::\n\n::: keep\nc\n:::\n:::\n\n::: page-break\n";
        assert_eq!(
            kinds(text),
            [
                (Kind::Columns, 0),
                (Kind::FullWidth, 1),
                (Kind::Keep, 1),
                (Kind::PageBreak, 0)
            ]
        );
    }

    #[test]
    fn leaves_out_what_the_engine_rejects() {
        use Kind::*;
        for (text, expected) in [
            ("::: columns\n::: columns\na\n:::\n:::\n", vec![(Columns, 0)]),
            ("::: full-width\na\n:::\n", vec![]),
            ("::: keep\n::: bottom\na\n:::\n:::\n", vec![(Keep, 0)]),
            ("::: keep\n::: page-break\n:::\n", vec![(Keep, 0)]),
            ("::: columns\n::: toc\n:::\n", vec![(Columns, 0)]),
            ("::: keep {x}\na\n:::\n", vec![]),
            ("::: aside\na\n:::\n", vec![]),
            ("::: keep\na\n", vec![]),
            (":::\n", vec![]),
            ("::: toc\n::: toc\n", vec![(Toc, 0)]),
            ("::: main-matter\n::: front-matter\n", vec![(MainMatter, 0)]),
            ("::: table {align=\"left\"}\n- - a\n:::\n", vec![(Table, 0)]),
        ] {
            assert_eq!(kinds(text), expected, "{text:?}");
        }
    }

    #[test]
    fn says_why_a_line_is_left_out() {
        use Fault::*;
        let faults = |text: &str| -> Vec<(Fault, bool)> {
            (read(text, &Blocks::parse(text)).rejected.iter())
                .map(|rejected| (rejected.fault, rejected.close.is_some()))
                .collect()
        };
        assert_eq!(faults("::: aside\na\n:::\n"), [(Unknown, true)]);
        assert_eq!(faults("::: keep\na\n"), [(Unclosed, false)]);
        assert_eq!(faults("a\n\n:::\n"), [(Stray, false)]);
        assert_eq!(faults("::: toc\n\n::: toc\n"), [(Refused, false)]);
        assert_eq!(faults("::: full-width\na\n:::\n"), [(Refused, true)]);
        assert_eq!(faults("::: keep\na\n:::\n"), []);
    }

    #[test]
    fn balance_counts_what_runs_over_the_ends_of_a_text() {
        assert_eq!(balance("a\n\n:::\n\n::: keep\nb\n"), (1, 1));
        assert_eq!(balance("::: toc\n\n::: keep\nb\n:::\n"), (0, 0));
    }

    #[test]
    fn an_unknown_name_still_takes_its_closing_line() {
        let text = "::: keep\n::: aside\na\n:::\nb\n:::\n";
        let structure = read(text, &Blocks::parse(text));
        assert_eq!(structure.regions.len(), 1);
        let keep = &structure.regions[0];
        assert_eq!(Blocks::parse(text).as_slice()[keep.close].range.start, text.len() - 4);
    }

    #[test]
    fn column_runs_part_at_full_width_and_drop_blank_ends() {
        let text = "::: columns\n\na\n\nb\n\n::: full-width\nc\n:::\n\nd\n:::\n";
        let blocks = Blocks::parse(text);
        let structure = read(text, &blocks);
        let runs: Vec<Vec<&str>> = structure
            .column_runs(&blocks)
            .into_iter()
            .map(|run| run.map(|at| &text[blocks.as_slice()[at].range.clone()]).collect())
            .collect();
        assert_eq!(runs, [vec!["a\n", "\n", "b\n"], vec!["d\n"]]);
    }

    #[test]
    fn a_nested_group_is_one_unit() {
        let text = "::: columns\na\n\n::: keep\nb\n\nc\n:::\n\nd\n:::\n";
        let blocks = Blocks::parse(text);
        let structure = read(text, &blocks);
        let run = structure.column_runs(&blocks).remove(0);
        let sizes: Vec<usize> = structure.units(&run).iter().map(Range::len).collect();
        assert_eq!(sizes, [1, 1, 5, 1, 1]);
        let keep = structure.fence(3).expect("the keep's opening line");
        assert_eq!(structure.regions[keep].kind, Kind::Keep);
    }
}
