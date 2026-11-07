//! Page composition: chooses page breaks for the whole flow and places footnotes.
//!
//! Every place a page may end is a candidate. The search keeps, for each candidate and each amount
//! of footnote text already placed, the cheapest way to get there, and extends it by one page at a
//! time, at most a page of lines ahead. A page costs the penalty of its break plus how far its
//! content falls short of the bottom after the space between blocks has stretched within bounds.
//! The final page and pages ending at an explicit break may be short at no cost.
//!
//! Footnotes are placed while breaks are chosen, so their height moves the break. A note starts on
//! the page of its reference. Only the last note on a page may continue on the next one, where a
//! marker with its number and the `continued` label precedes the rest.
//!
//! A page holds stacked body regions and one footnote area across the text width. A region is
//! either full width or the part of a column run on that page. Text in a column region flows down
//! the first column and then the second, split where both columns are most even. The next region
//! starts below the taller column. Column regions take part in the search like any other lines:
//! their balanced height counts toward the page, and the cost of their column break toward its cost.
//!
//! A `::: bottom` group is a keep group that ends its page. It is set at the bottom of the text area,
//! above the footnotes.
//!
//! A table's rows are lines like any other. Where a page or column starts at one of its rows, the
//! table's header row is set again above it, and its height counts toward that page or column.
//!
//! A figure or table placed at the top or bottom is not in the flow. A page may set the placed blocks
//! whose anchor lies before its end, in document order, and leaves the rest of its height to the text.
//! Blocks still waiting at a forced break or the end get pages of their own before it.
//!
//! Margin notes and sidenotes sit beside the line that references them, in a column outside the
//! prose. A page's notes must fit the text height together, since they may move up as far as needed.

use std::collections::BTreeMap;
use std::ops::{Bound, Range};

use super::callout::CalloutBox;
use super::{Break, Flaw, FlowLine, Line, translate};
use crate::cancel::{Cancel, CheckPoint};
use crate::config::resolved::{Config, PageGeometry};
use crate::config::source::InputSource;
use crate::config::values::Pt;
use crate::diagnostic::Diagnostic;
use crate::document::Location;
use crate::output::{Aside, Item, NoteKind, Page, Position, Rect};
use crate::renderer::Failure;

/// Space between blocks may grow by this share of its natural height.
const STRETCH: f64 = 0.5;

/// Cost of a page whose content ends one body line short of the bottom after stretching. Grows
/// with the square of the shortfall.
const SHORT: f64 = 1000.0;
/// Cost of a page whose spaces stretch fully. Grows with the cube of the share used.
const STRETCHED: f64 = 100.0;
/// Cost of a break between two lines of a block.
pub(super) const INSIDE: f64 = 50.0;
/// Cost of a column break or a footnote break that leaves a block's first line alone at the bottom.
/// Pages never end there.
const ORPHAN: f64 = 5000.0;
/// Cost of a column break or a footnote break that leaves a block's last line alone at the top.
/// Pages never end there.
const WIDOW: f64 = 5000.0;
/// Cost of a page or column ending after a hyphenated line, as TeX's `\brokenpenalty`.
pub(super) const HYPHENATED: f64 = 1000.0;
/// Cost of a page or column ending after a paragraph that ends in a colon, before the list or code
/// block it introduces.
pub(super) const INTRODUCTION: f64 = 500.0;
/// Cost of continuing a footnote on the next page.
const SPLIT_NOTE: f64 = 2000.0;
/// Cost of a page that holds only the continuation of footnotes.
const NOTE_PAGE: f64 = 10_000.0;
/// Cost of a page break that leaves a one-line paragraph alone at the top or bottom of a page, as much
/// as a page four lines short.
pub(super) const ONE_LINER: f64 = 16.0 * SHORT;
/// Cost of each line a heading at the foot of a page has fewer below it than `typesetting.heading-lines`,
/// as much as a page one line short.
pub(super) const HEADING_LINE: f64 = SHORT;
/// Cost of each page a placed block waits after the page its anchor falls on.
const WAIT: f64 = 10_000.0;
/// Cost of a page that holds only placed blocks amid the text, so that is a last resort for blocks
/// that cannot share a page with text. Before a forced break or the end such pages cost nothing more
/// than the waiting.
const FLOAT_PAGE: f64 = 100_000.0;
/// Cost of a column region one body line taller than its most even split. Grows with the square,
/// so a region gets one line uneven rather than strand a line, but not two.
const UNEVEN: f64 = 3000.0;

/// A page rule that a page break after a line breaks, at the cost added to the line's break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rule {
    /// The next line is a one-line paragraph, which would start the next page alone.
    OneLinerAtTop,
    /// The line is a one-line paragraph, which would end the page alone.
    OneLinerAtBottom,
    /// The line is this many lines below a heading.
    HeadingLines(usize),
    /// The line ends the first item of a list, of one line, which would end the page alone.
    ListItemAtBottom,
    /// The next line is the last item of a list, of one line, which would start the next page alone.
    ListItemAtTop,
}

/// The rules the planned page at physical index `page` breaks: its own, the break after it, and those of
/// its lines.
fn page_issues(plan: &Plan, body: &[FlowLine], page: usize) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut issue = |page, at, kind| issues.push(Issue { page, at, kind });
    let flaws = plan.flaws;
    // A page one line short is the cost of keeping the other rules, and is not reported.
    if flaws.short > 1.0 {
        issue(page, None, IssueKind::Short(flaws.short));
    }
    if flaws.uneven > 0.5 {
        issue(page, None, IssueKind::Uneven(flaws.uneven));
    }
    if let Some(lines) = flaws.chapter {
        issue(page, None, IssueKind::ChapterEnd(lines));
    }
    let Range { start, end } = plan.lines;
    let broken = end > start && end < body.len() && matches!(body[end - 1].after, Break::Allowed(_));
    if let Some(rule) = body[start..end].last().and_then(|line| line.rule).filter(|_| broken) {
        match rule {
            Rule::OneLinerAtTop | Rule::ListItemAtTop => issue(page + 1, Some(body[end].at), IssueKind::Break(rule)),
            _ => issue(page, Some(body[end - 1].at), IssueKind::Break(rule)),
        }
    }
    for line in &body[start..end] {
        if let Some(flaw) = line.line.flaw {
            issue(page, Some(line.at), IssueKind::Line(flaw));
        }
    }
    issues
}

/// The width of each of two columns in the prose width.
pub(super) fn column_width(page: &PageGeometry) -> f64 {
    (page.prose_width.0 - page.column_gap.0) / 2.0
}

/// The rule after line `index` of a block of `count` lines. A page never leaves a block's first line
/// alone at its bottom or its last line alone at its top; a column or a footnote does at a cost.
pub(super) fn line_break(index: usize, count: usize) -> Break {
    if index + 1 == count {
        return Break::Allowed(0.0);
    }
    let orphan = if index == 0 { ORPHAN } else { 0.0 };
    let widow = if index + 2 == count { WIDOW } else { 0.0 };
    if orphan + widow > 0.0 {
        Break::Avoid(INSIDE + orphan + widow)
    } else {
        Break::Allowed(INSIDE)
    }
}

/// Keeps a callout's text lines from stranding one line: a page does not end after the first line or
/// before the last, a column does at a cost. A callout of fewer than four lines stays whole.
pub(super) fn guard_ends(lines: &mut [FlowLine]) {
    let Some(second_last) = lines.len().checked_sub(2) else {
        return;
    };
    for (index, cost) in [(0, ORPHAN), (second_last, WIDOW)] {
        if let Break::Allowed(base) = lines[index].after {
            lines[index].after = Break::Avoid(base + cost);
        }
    }
}

/// A table in the flow, whose header row repeats where a page or column starts inside it.
#[derive(Debug)]
pub(super) struct Table {
    /// The table's lines: caption, header row, and body rows.
    pub lines: Range<usize>,
    /// The rows a page or column may start at, which then get the header above them.
    pub rows: Range<usize>,
    /// A copy of the header row.
    pub header: Line,
    pub at: Location,
}

/// A figure or table set at the top or bottom of a page, apart from the flow.
#[derive(Debug)]
pub(super) struct Float {
    pub lines: Vec<FlowLine>,
    /// The height of the lines with the space between them.
    pub height: f64,
    /// The number of body lines before it.
    pub anchor: usize,
    /// Whether a forced break comes right before it, so it belongs after the break.
    pub after_break: bool,
    /// At the top of its page, else at the bottom.
    pub top: bool,
    pub at: Location,
    /// "figure" or "table", for messages.
    pub what: &'static str,
}

impl Float {
    /// Orders blocks by where they stand: a block after a forced break comes after one before it.
    fn key(&self) -> usize {
        2 * self.anchor + usize::from(self.after_break)
    }
}

/// A margin note or sidenote set in the note column.
#[derive(Debug)]
pub(super) struct Note {
    pub lines: Vec<FlowLine>,
    pub height: f64,
    /// The first baseline below the note's top.
    pub baseline: f64,
    pub at: Location,
    pub kind: NoteKind,
}

/// The notes in the note column in reading order, with the body line that references each and the
/// baseline it is referenced on, below the line's top.
#[derive(Debug, Default)]
pub(super) struct Margin {
    pub notes: Vec<Note>,
    pub references: Vec<(usize, f64)>,
}

/// Everything that goes on the pages.
pub(super) struct Content {
    pub body: Vec<FlowLine>,
    /// The body lines that start an odd page, after a blank page if needed, in order.
    pub rectos: Vec<usize>,
    /// Line ranges of keep groups with their directive locations.
    pub keeps: Vec<(Range<usize>, Location)>,
    /// The first lines of the keep groups set at the bottom of their page.
    pub bottoms: Vec<usize>,
    /// Line ranges set in two columns, in order.
    pub columns: Vec<Range<usize>>,
    /// Tables in order.
    pub tables: Vec<Table>,
    /// The lines of each footnote, in reference order.
    pub notes: Vec<Vec<FlowLine>>,
    /// The continuation marker of each footnote.
    pub continued: Vec<Vec<FlowLine>>,
    /// Figures and tables placed at the top or bottom of a page, in document order.
    pub floats: Vec<Float>,
    pub margin: Margin,
}

/// The body pages.
pub(super) struct Composed {
    pub pages: Vec<Page>,
    /// The physical indices of blank pages among them.
    pub blanks: Vec<usize>,
    /// The first body line of each page, `None` for a blank page or one of placed blocks only.
    pub starts: Vec<Option<usize>>,
    /// The margin notes and sidenotes on the pages.
    pub asides: Vec<Aside>,
    /// The rules the pages break, in page order.
    pub issues: Vec<Issue>,
}

/// A rule a page breaks, since no better layout was found.
#[derive(Debug)]
pub(super) struct Issue {
    /// The physical index of the page.
    pub page: usize,
    /// The source of the content concerned, if one block is.
    pub at: Option<Location>,
    pub kind: IssueKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum IssueKind {
    /// The page ends this many lines short of the bottom after its spaces stretched.
    Short(f64),
    /// The page ends this many lines higher or lower than the page facing it.
    Uneven(f64),
    /// The page is the last of a chapter and holds this many lines, fewer than wanted.
    ChapterEnd(usize),
    /// The page break after the page's last line breaks this rule.
    Break(Rule),
    /// A line on the page breaks this paragraph rule.
    Line(Flaw),
}

/// The page rules a planned page breaks.
#[derive(Debug, Clone, Copy, Default)]
struct PageFlaws {
    /// Lines short of the bottom after stretching, 0 for a page that may be short.
    short: f64,
    /// Lines of difference to the facing page.
    uneven: f64,
    /// The lines on the last page of a chapter, if fewer than wanted.
    chapter: Option<usize>,
}

/// How many lines the page break search handles between looks at the cancel token.
const CANCEL_LINES: usize = 64;

/// Chooses page breaks and positions the content on pages. `first` is the physical index of the first
/// page, which decides parity: odd pages, counted from 1, have the inner margin on the left. Blank pages
/// are inserted so that the `rectos` lines start odd pages.
pub(super) fn compose(
    content: Content,
    first: usize,
    config: &Config,
    source: &InputSource,
    cancel: &Cancel,
) -> Result<Composed, Failure> {
    let Content {
        mut body,
        rectos,
        keeps,
        bottoms,
        columns,
        tables,
        notes,
        continued,
        mut floats,
        margin,
    } = content;
    let footnotes = &config.footnotes;
    let overhead = footnotes.gap.0 + footnotes.separator_thickness.0;
    let notes = Notes::new(notes, continued, overhead, footnotes.spacing.0);
    let body_line = config.styles.body.size.0 * config.styles.body.line_height;
    let mut headers = vec![None; body.len()];
    for table in &tables {
        headers[table.rows.clone()].fill(Some(&table.header));
    }
    let mut composer = Composer::new(
        &body,
        &columns,
        &headers,
        &notes,
        config.page.text_height().0,
        body_line,
    );
    composer.paginate(
        config.document.duplex.then_some(first),
        &rectos,
        config.typesetting.chapter_last_lines.get(),
    );
    composer.place(&floats, config.styles.caption.space_after.0);
    composer.annotate(&margin, config.margin_notes.spacing.0);
    composer.check(&keeps, &bottoms, &tables, source)?;
    let plans = composer.search(cancel)?.ok_or_else(|| {
        vec![Diagnostic::new(
            Some(source.clone()),
            "no page breaks satisfy the layout constraints",
        )]
    })?;
    drop(composer);
    Ok(render(
        &plans,
        &mut body,
        &headers,
        notes,
        (&mut floats, margin),
        (first, &rectos),
        &bottoms,
        config,
    ))
}

/// A planned page: body regions stacked from the top and one footnote area at the bottom.
#[derive(Debug)]
struct Plan {
    /// The body lines on the page.
    lines: Range<usize>,
    regions: Vec<Region>,
    /// The share of their bound by which spaces and column regions stretch.
    stretch: f64,
    /// The footnote lines on the page, as a range of the footnote stream.
    notes: Range<usize>,
    /// The placed blocks on the page, tops before bottoms.
    floats: Range<usize>,
    flaws: PageFlaws,
}

#[derive(Debug)]
enum Region {
    Full(Range<usize>),
    Columns(Columns),
}

/// Lines set in two columns: `lines.start..split` in the first and the rest in the second.
#[derive(Debug, Clone)]
struct Columns {
    lines: Range<usize>,
    split: usize,
    /// The natural height of the taller column.
    height: f64,
    /// How far the region can grow with both columns stretching their spaces within bounds.
    grow: f64,
    /// How far the shorter column stays above the region's bottom even when fully stretched.
    deficit: f64,
    /// The cost of the column break and of uneven columns.
    cost: f64,
    /// The natural height and the stretch bound of each column.
    columns: [(f64, f64); 2],
}

/// All footnote lines as one stream in reference order.
struct Notes {
    lines: Vec<FlowLine>,
    /// Stream index of each note's first line, then the stream length.
    start: Vec<usize>,
    /// The note of each stream line.
    note: Vec<usize>,
    /// Height of the stream before each line and at its end. Each line counts with the space above
    /// it, which is the footnote spacing for a note's first line.
    top: Vec<f64>,
    continued: Vec<Vec<FlowLine>>,
    /// Height of each continuation marker with the spacing above it.
    continued_height: Vec<f64>,
    /// Gap and separator above the first note.
    overhead: f64,
    spacing: f64,
}

impl Notes {
    /// `overhead` is the space above the first note: the gap and the separator.
    fn new(notes: Vec<Vec<FlowLine>>, continued: Vec<Vec<FlowLine>>, overhead: f64, spacing: f64) -> Self {
        let mut stream = Self {
            lines: Vec::new(),
            start: Vec::with_capacity(notes.len() + 1),
            note: Vec::new(),
            top: vec![0.0],
            continued_height: continued
                .iter()
                .map(|lines| spacing + lines.iter().map(|l| l.line.height).sum::<f64>())
                .collect(),
            continued,
            overhead,
            spacing,
        };
        for (index, lines) in notes.into_iter().enumerate() {
            stream.start.push(stream.lines.len());
            for (number, line) in lines.into_iter().enumerate() {
                let above = if number == 0 { spacing } else { line.space_before };
                let top = stream.top[stream.top.len() - 1];
                stream.top.push(top + above + line.line.height);
                stream.note.push(index);
                stream.lines.push(line);
            }
        }
        stream.start.push(stream.lines.len());
        stream
    }

    fn is_first(&self, line: usize) -> bool {
        self.start[self.note[line]] == line
    }

    /// Height of the footnote area holding stream lines `from..to`, or zero without lines.
    fn area(&self, from: usize, to: usize) -> f64 {
        if to == from {
            return 0.0;
        }
        self.base(from) + self.top[to]
    }

    /// The area height from `from` minus the stream height before the end.
    fn base(&self, from: usize) -> f64 {
        let marker = if self.is_first(from) {
            0.0
        } else {
            self.continued_height[self.note[from]]
        };
        self.overhead + marker - self.top[from]
    }

    /// The cost of a page whose note area holds stream lines `from..placed` when the notes referenced
    /// so far end at `end`: none if all are placed, else the cost of continuing a note, plus the break
    /// cost of the line it stops after, so continued notes follow the rules of body text.
    fn split(&self, from: usize, placed: usize, end: usize) -> f64 {
        if placed >= end {
            return 0.0;
        }
        let line = match self.lines[..placed].last().filter(|_| placed > from) {
            None => 0.0,
            Some(line) => match line.after {
                Break::Allowed(cost) | Break::Avoid(cost) => cost,
                Break::Never | Break::Forced => WIDOW,
            },
        };
        SPLIT_NOTE + line
    }

    /// The furthest end in `min..=max` whose area from `from` fits in `room`, if `min` fits.
    fn fit(&self, from: usize, min: usize, max: usize, room: f64) -> Option<usize> {
        if self.area(from, min) > room {
            return None;
        }
        if from == self.lines.len() || min >= max {
            return Some(min);
        }
        let limit = room - self.base(from);
        Some(min + self.top[min + 1..=max].partition_point(|&top| top <= limit))
    }
}

/// A way to reach a candidate break: the page breaks before it and their total cost.
struct State {
    at: usize,
    placed: usize,
    /// The placed blocks set so far.
    floats: usize,
    cost: f64,
    previous: Option<usize>,
    stretch: f64,
    /// Whether the next page is a left-hand page, in duplex only.
    verso: bool,
    /// How many lines short a left-hand page ends that faces the next page, in duplex only.
    facing: Option<f64>,
    /// The rules the page before this state breaks.
    flaws: PageFlaws,
}

/// The states of one candidate break: footnote lines set, placed blocks set, whether the next page is a
/// left-hand one, and the facing page's shortfall in half lines, or -1 for none.
type Key = (usize, usize, bool, i64);

/// A page that can follow a state: where it ends, the footnote lines and placed blocks set by then,
/// the total cost, and the stretch of its spaces.
struct Step {
    at: usize,
    placed: usize,
    floats: usize,
    cost: f64,
    stretch: f64,
    verso: bool,
    facing: Option<f64>,
    flaws: PageFlaws,
}

impl Step {
    fn key(&self) -> Key {
        let facing = self.facing.map_or(-1, |lines| (2.0 * lines).round() as i64);
        (self.placed, self.floats, self.verso, facing)
    }
}

struct Composer<'a> {
    body: &'a [FlowLine],
    /// The number of footnotes referenced before each body line, and in the whole body.
    through: Vec<usize>,
    /// The height of the body before each line and at its end, each line with the space above it.
    top: Vec<f64>,
    /// The space above the lines before each line and at the end.
    space: Vec<f64>,
    /// The column run of each line, or `None` for a full-width line.
    runs: Vec<Option<Range<usize>>>,
    /// The header row set above each line when a page or column starts there.
    headers: &'a [Option<&'a Line>],
    notes: &'a Notes,
    height: f64,
    /// A body line's height, the unit for measuring short pages.
    line: f64,
    /// The placed blocks, empty without any.
    floats: &'a [Float],
    /// The height of the placed blocks before each one and at the end, each with the space below or above it.
    float_top: Vec<f64>,
    /// The number of bottom blocks followed by a top block before each placed block.
    switches: Vec<usize>,
    float_spacing: f64,
    /// The number of margin notes referenced before each body line and at the end, empty without any.
    note_first: Vec<usize>,
    /// The height of the margin notes before each one and at the end, each with the spacing below it.
    note_top: Vec<f64>,
    note_spacing: f64,
    /// The margin notes, for messages.
    margin: Option<&'a Margin>,
    /// The physical index of the first page in a duplex document, whose facing pages should end level.
    duplex: Option<usize>,
    /// The body lines that start an odd page.
    rectos: &'a [usize],
    /// The fewest lines the last page of a chapter wants.
    chapter_lines: usize,
}

impl<'a> Composer<'a> {
    fn new(
        body: &'a [FlowLine],
        columns: &[Range<usize>],
        headers: &'a [Option<&'a Line>],
        notes: &'a Notes,
        height: f64,
        line: f64,
    ) -> Self {
        let mut through = Vec::with_capacity(body.len() + 1);
        let mut top = Vec::with_capacity(body.len() + 1);
        let mut space = Vec::with_capacity(body.len() + 1);
        through.push(0);
        top.push(0.0);
        space.push(0.0);
        for flow_line in body {
            let referenced = flow_line.line.notes.iter().map(|note| note + 1).max().unwrap_or(0);
            through.push(referenced.max(through[through.len() - 1]));
            top.push(top[top.len() - 1] + flow_line.space_before + flow_line.line.height);
            space.push(space[space.len() - 1] + flow_line.space_before);
        }
        let mut runs = vec![None; body.len()];
        for run in columns {
            runs[run.clone()].fill(Some(run.clone()));
        }
        Self {
            body,
            through,
            top,
            space,
            runs,
            headers,
            notes,
            height,
            line,
            floats: &[],
            float_top: vec![0.0],
            switches: vec![0],
            float_spacing: 0.0,
            note_first: Vec::new(),
            note_top: vec![0.0],
            note_spacing: 0.0,
            margin: None,
            duplex: None,
            rectos: &[],
            chapter_lines: 1,
        }
    }

    /// Sets the page parity rules: with `duplex`, the physical index of the first page, facing pages should
    /// end level; `rectos` start odd pages; a chapter's last page wants `chapter_lines` lines.
    fn paginate(&mut self, duplex: Option<usize>, rectos: &'a [usize], chapter_lines: usize) {
        self.duplex = duplex;
        self.rectos = rectos;
        self.chapter_lines = chapter_lines;
    }

    /// Whether a page after `state` that starts at line `start` is a left-hand page. A blank page goes
    /// before one that would start a recto line on the left.
    fn is_verso(&self, state: &State, start: usize, only_floats: bool) -> bool {
        state.verso && (only_floats || self.rectos.binary_search(&start).is_err())
    }

    /// Adds the placed blocks, `spacing` apart from the text and each other.
    fn place(&mut self, floats: &'a [Float], spacing: f64) {
        self.floats = floats;
        self.float_spacing = spacing;
        for (index, float) in floats.iter().enumerate() {
            let top = self.float_top[index];
            self.float_top.push(top + float.height + spacing);
            let switch = !float.top && floats.get(index + 1).is_some_and(|next| next.top);
            let count = self.switches[index];
            self.switches.push(count + usize::from(switch));
        }
    }

    /// Adds the margin notes, `spacing` apart in their column.
    fn annotate(&mut self, margin: &'a Margin, spacing: f64) {
        if margin.notes.is_empty() {
            return;
        }
        self.margin = Some(margin);
        self.note_spacing = spacing;
        self.note_first = vec![0; self.body.len() + 1];
        for &(line, _) in &margin.references {
            self.note_first[line + 1] += 1;
        }
        for index in 0..self.body.len() {
            self.note_first[index + 1] += self.note_first[index];
        }
        for note in &margin.notes {
            let top = self.note_top[self.note_top.len() - 1];
            self.note_top.push(top + note.height + spacing);
        }
    }

    /// The height of the margin notes referenced on lines `start..end`, stacked `note_spacing` apart.
    fn note_height(&self, start: usize, end: usize) -> f64 {
        let Some((&first, &last)) = self.note_first.get(start).zip(self.note_first.get(end)) else {
            return 0.0;
        };
        if last == first {
            return 0.0;
        }
        self.note_top[last] - self.note_top[first] - self.note_spacing
    }

    /// The number of placed blocks a page ending at `end` may have set: those anchored before it.
    fn eligible(&self, end: usize) -> usize {
        self.floats.partition_point(|float| float.key() <= 2 * end)
    }

    /// The height placed blocks `from..to` take from a page, with their spacing.
    fn float_area(&self, from: usize, to: usize) -> f64 {
        self.float_top[to] - self.float_top[from]
    }

    /// Whether placed blocks `from..to` can share a page: tops first, then bottoms.
    fn in_order(&self, from: usize, to: usize) -> bool {
        to <= from + 1 || self.switches[to - 1] == self.switches[from]
    }

    /// The height of the header row set above line `index` when a page or column starts there.
    fn repeat(&self, index: usize) -> f64 {
        self.headers[index].map_or(0.0, |header| header.height)
    }

    /// Reports content that must stay on one page but cannot, with the first line of each of its
    /// footnotes, and footnote lines too tall for a page of their own.
    fn check(
        &self,
        keeps: &[(Range<usize>, Location)],
        bottoms: &[usize],
        tables: &[Table],
        source: &InputSource,
    ) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        let mut error = |at: Location, message: String| {
            errors.push(Diagnostic::new(Some(source.clone()), message).at(at.line, at.column));
        };
        let mut start = 0;
        let mut natural = 0.0;
        for (index, line) in self.body.iter().enumerate() {
            natural += if index > start {
                line.space_before
            } else {
                self.repeat(index)
            };
            natural += line.line.height;
            if !line.after.ends_page() && index + 1 < self.body.len() {
                continue;
            }
            let (first, last) = (self.through[start], self.through[index + 1]);
            let demand = if last > first {
                self.notes.area(self.notes.start[first], self.notes.start[last - 1] + 1)
            } else {
                0.0
            };
            let total = natural + demand;
            if total > self.height {
                let keep = keeps
                    .iter()
                    .find(|(lines, _)| lines.start <= index && start < lines.end);
                let with_notes = if demand > 0.0 {
                    " with the first lines of its footnotes"
                } else {
                    ""
                };
                let table = tables.iter().find(|table| table.lines.start == start);
                let (at, what) = match keep {
                    Some((lines, at)) if bottoms.contains(&lines.start) => (*at, "this bottom group"),
                    Some((_, at)) => (*at, "this keep group"),
                    None if self.headers[start].is_some() => (line.at, "this table row with the repeated header"),
                    None if let Some(table) = table => (table.at, "the start of this table up to its first row"),
                    None if index > start => (self.body[start].at, "this heading and the text kept with it"),
                    None => (line.at, "this line"),
                };
                let message = format!(
                    "{what} is {total:.1}pt high{with_notes}, more than the {:.1}pt text area, so it cannot fit on a page",
                    self.height
                );
                error(at, message);
            }
            let notes = self.note_height(start, index + 1);
            if notes > self.height
                && let Some(margin) = self.margin
            {
                let first = self.note_first[start];
                let others = self.note_first[index + 1] - first - 1;
                let note = &margin.notes[first];
                let what = match note.kind {
                    NoteKind::Margin(_) => "margin note",
                    NoteKind::Side(_) => "sidenote",
                };
                let message = if others == 0 {
                    format!(
                        "this {what} is {notes:.1}pt high, more than the {:.1}pt text area",
                        self.height
                    )
                } else {
                    format!(
                        "this {what} and the {others} referenced with it on lines that stay together need {notes:.1}pt, \
                         more than the {:.1}pt text area",
                        self.height
                    )
                };
                error(note.at, message);
            }
            start = index + 1;
            natural = 0.0;
        }
        for float in self.floats {
            if float.height > self.height {
                let message = format!(
                    "this {} placed at the top or bottom is {:.1}pt high, more than the {:.1}pt text area, so it \
                     cannot fit on a page",
                    float.what, float.height, self.height
                );
                error(float.at, message);
            }
        }
        for line in 0..self.notes.lines.len() {
            let total = self.notes.area(line, line + 1);
            if total > self.height {
                let at = self.notes.lines[line].at;
                let message = format!(
                    "a line of this footnote needs {total:.1}pt, more than the {:.1}pt text area",
                    self.height
                );
                error(at, message);
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    /// Finds the cheapest sequence of pages, or `None` if constraints leave no way through.
    fn search(&self, cancel: &Cancel) -> Result<Option<Vec<Plan>>, Failure> {
        let count = self.body.len();
        let verso = self.duplex.is_some_and(|first| first % 2 == 1);
        let mut states = vec![State {
            at: 0,
            placed: 0,
            floats: 0,
            cost: 0.0,
            previous: None,
            stretch: 0.0,
            verso,
            facing: None,
            flaws: PageFlaws::default(),
        }];
        // For each candidate break, the best state per key.
        let mut reached: Vec<BTreeMap<Key, usize>> = (0..=count).map(|_| BTreeMap::new()).collect();
        reached[0].insert((0, 0, verso, -1), 0);
        for at in 0..=count {
            if at % CANCEL_LINES == 0 {
                cancel.check(CheckPoint::Pagination)?;
            }
            // Pages that end where they start add states with more notes or blocks set, so later keys.
            let mut next = Bound::Unbounded;
            while let Some((key, index)) = reached[at]
                .range((next, Bound::Unbounded))
                .next()
                .map(|(&k, &i)| (k, i))
            {
                next = Bound::Excluded(key);
                for step in self.pages(&states[index]) {
                    let key = step.key();
                    let state = State {
                        at: step.at,
                        placed: step.placed,
                        floats: step.floats,
                        cost: step.cost,
                        previous: Some(index),
                        stretch: step.stretch,
                        verso: step.verso,
                        facing: step.facing,
                        flaws: step.flaws,
                    };
                    match reached[step.at].get(&key) {
                        Some(&existing) if states[existing].cost <= step.cost => {}
                        Some(&existing) => states[existing] = state,
                        None => {
                            reached[step.at].insert(key, states.len());
                            states.push(state);
                        }
                    }
                }
            }
        }
        let (notes, floats) = (self.notes.lines.len(), self.floats.len());
        let end = reached[count]
            .range((notes, floats, false, i64::MIN)..=(notes, floats, true, i64::MAX))
            .map(|(_, &index)| index)
            .min_by(|&a, &b| states[a].cost.total_cmp(&states[b].cost));
        let Some(end) = end else {
            return Ok(None);
        };
        let mut plans = Vec::new();
        let mut index = end;
        while let Some(previous) = states[index].previous {
            let (from, to) = (&states[previous], &states[index]);
            plans.push(Plan {
                lines: from.at..to.at,
                regions: self.regions(from.at, to.at),
                stretch: to.stretch,
                notes: from.placed..to.placed,
                floats: from.floats..to.floats,
                flaws: to.flaws,
            });
            index = previous;
        }
        plans.reverse();
        Ok(Some(plans))
    }

    /// The pages that can follow `state`.
    fn pages(&self, state: &State) -> Vec<Step> {
        let (start, from, done) = (state.at, state.placed, state.floats);
        let mut pages = Vec::new();
        let pending = self.notes.start[self.through[start]];
        if from < pending
            && let Some(placed) = self.notes.fit(from, from, pending, self.height)
            && placed > from
        {
            let split = self.notes.split(from, placed, pending);
            pages.push(Step {
                at: start,
                placed,
                floats: done,
                cost: state.cost + NOTE_PAGE + split,
                stretch: 0.0,
                verso: self.next_verso(self.is_verso(state, start, false)),
                facing: None,
                flaws: PageFlaws::default(),
            });
        }
        // Placed blocks anchored before here may fill a page of their own, stacked from the top.
        let waiting = self.eligible(start);
        if done < waiting {
            let forced = start == self.body.len() || start > 0 && self.body[start - 1].after == Break::Forced;
            let page = if forced { 0.0 } else { FLOAT_PAGE };
            for to in done + 1..=waiting {
                if self.float_area(done, to) - self.float_spacing > self.height {
                    break;
                }
                let cost = state.cost + page + WAIT * (waiting - to) as f64;
                pages.push(Step {
                    at: start,
                    placed: from,
                    floats: to,
                    cost,
                    stretch: 0.0,
                    verso: self.next_verso(self.is_verso(state, start, true)),
                    facing: None,
                    flaws: PageFlaws::default(),
                });
            }
            // A forced break and the end are not passed while blocks anchored before them wait.
            if forced {
                return pages;
            }
        }

        // Natural height, stretch bound, and column break cost of the regions finished so far. A page
        // starting inside a full-width table begins with its header; in columns, balancing adds it.
        let header = match self.runs.get(start) {
            Some(None) => self.repeat(start),
            _ => 0.0,
        };
        let (mut natural, mut stretch, mut cost) = (header, 0.0, 0.0);
        let verso = self.is_verso(state, start, false);
        // The first page of a chapter may be its last too.
        let continues = start > 0 && self.body[start - 1].after != Break::Forced;
        'lines: for (index, line) in self.body.iter().enumerate().skip(start) {
            let end = index + 1;
            let last = end == self.body.len();
            // Notes beside the text may move up as far as needed, so they fit if their heights do.
            if self.note_height(start, end) > self.height {
                break;
            }
            let referenced = self.through[end];
            let min = if referenced > self.through[start] {
                self.notes.start[referenced - 1] + 1
            } else {
                from
            };
            // The body can use what the first lines of the page's notes leave.
            let room = self.height - self.notes.area(from, min);
            // A page ending in columns falls short by its shorter column's deficit too.
            let (page, page_stretch, page_cost, deficit) = match &self.runs[index] {
                None => {
                    if index > start {
                        natural += line.space_before;
                        stretch += line.space_before * STRETCH;
                    }
                    natural += line.line.height;
                    if natural > room {
                        break;
                    }
                    (natural, stretch, cost, 0.0)
                }
                Some(run) => {
                    if !line.after.ends_page() && !last && end < run.end {
                        continue;
                    }
                    let first = run.start.max(start);
                    let gap = if first > start {
                        self.body[first].space_before
                    } else {
                        0.0
                    };
                    let (columns, least) = self.balance(first, end);
                    if natural + gap + least > room {
                        break;
                    }
                    let region = (
                        natural + gap + columns.height,
                        stretch + gap * STRETCH + columns.grow,
                        cost + columns.cost,
                    );
                    if end == run.end {
                        (natural, stretch, cost) = region;
                    }
                    if region.0 > room {
                        continue;
                    }
                    (region.0, region.1, region.2, columns.deficit)
                }
            };
            let penalty = match line.after {
                _ if last => 0.0,
                Break::Never | Break::Avoid(_) => continue,
                Break::Forced => 0.0,
                Break::Allowed(cost) => cost,
            };
            let max = self.notes.start[referenced];
            // The page may also set placed blocks anchored before its end, which take from its height.
            let eligible = self.eligible(end);
            for floats in done..=eligible.max(done) {
                if !self.in_order(done, floats) {
                    break;
                }
                let page = page + self.float_area(done, floats);
                let Some(fitted) = self.notes.fit(from, min, max, self.height - page) else {
                    if floats == done {
                        break 'lines;
                    }
                    break;
                };
                let wait = WAIT * eligible.saturating_sub(floats) as f64;
                // A continued note may also stop a line earlier, so it strands no line.
                let earlier = (fitted < max && fitted > min).then(|| fitted - 1);
                for placed in std::iter::once(fitted).chain(earlier) {
                    let short = (self.height - page - self.notes.area(from, placed)).max(0.0);
                    let free = last || line.after == Break::Forced;
                    let (fill, ratio, lines) = if free {
                        (0.0, 0.0, 0.0)
                    } else {
                        let used = short.min(page_stretch);
                        let ratio = if page_stretch > 0.0 { used / page_stretch } else { 0.0 };
                        let lines = (short - used + deficit) / self.line;
                        (STRETCHED * ratio.powi(3) + SHORT * lines * lines, ratio, lines)
                    };
                    let (rules, flaws) = self.page_rules(state, (start, end), (lines, free), continues, verso);
                    let split = self.notes.split(from, placed, max);
                    pages.push(Step {
                        at: end,
                        placed,
                        floats,
                        cost: state.cost + fill + rules + penalty + split + page_cost + wait,
                        stretch: ratio,
                        verso: self.next_verso(verso),
                        facing: (self.duplex.is_some() && verso && !free).then_some(lines),
                        flaws,
                    });
                }
            }
            if line.after == Break::Forced {
                break;
            }
        }
        pages
    }
}

impl Composer<'_> {
    /// Whether the page after a page on the left, or on the right, is a left-hand one. Always false outside
    /// duplex, where no page faces another.
    fn next_verso(&self, verso: bool) -> bool {
        self.duplex.is_some() && !verso
    }

    /// The cost and flaws of the rules for a page of body lines `lines`: a chapter's last page holding too
    /// few lines, and a right-hand page ending at another height than the left-hand page it faces. The page
    /// ends `short` lines short, or is `free` to be short; `continues` tells whether it continues a chapter.
    fn page_rules(
        &self,
        state: &State,
        lines: (usize, usize),
        (short, free): (f64, bool),
        continues: bool,
        verso: bool,
    ) -> (f64, PageFlaws) {
        let mut flaws = PageFlaws {
            short,
            ..PageFlaws::default()
        };
        let mut cost = 0.0;
        let count = lines.1 - lines.0;
        if free && continues && count < self.chapter_lines {
            let missing = (self.chapter_lines - count) as f64;
            cost += SHORT * missing * missing;
            flaws.chapter = Some(count);
        }
        if let Some(left) = state.facing.filter(|_| !verso && !free) {
            let uneven = (short - left).abs();
            cost += SHORT * uneven * uneven;
            flaws.uneven = uneven;
        }
        (cost, flaws)
    }

    /// The regions of a page holding body lines `start..end`.
    fn regions(&self, start: usize, end: usize) -> Vec<Region> {
        let mut regions = Vec::new();
        let mut index = start;
        while index < end {
            let stop = match &self.runs[index] {
                Some(run) => {
                    let stop = run.end.min(end);
                    regions.push(Region::Columns(self.balance(index, stop).0));
                    stop
                }
                None => {
                    let stop = (index..end).find(|&i| self.runs[i].is_some()).unwrap_or(end);
                    regions.push(Region::Full(index..stop));
                    stop
                }
            };
            index = stop;
        }
        regions
    }

    /// The best way to set lines `a..b` in two columns, and the least height any allowed split
    /// reaches. A split may follow any line a page may end after; the second column may be empty.
    ///
    /// The taller column is lowest where the columns cross, so the search starts there and moves
    /// outward in both directions only while being less even could still pay off.
    fn balance(&self, a: usize, b: usize) -> (Columns, f64) {
        let column = |from: usize, to: usize| {
            if to == from {
                return (0.0, 0.0);
            }
            let height = self.repeat(from) + self.top[to] - self.top[from] - self.body[from].space_before;
            (height, STRETCH * (self.space[to] - self.space[from + 1]))
        };
        let split = |at: usize| {
            let columns = [column(a, at), column(at, b)];
            let height = columns[0].0.max(columns[1].0);
            let reach = |(height, stretch): (f64, f64)| height + stretch;
            let shorter = reach(columns[0]).min(reach(columns[1]));
            let grow = if at == b {
                columns[0].1
            } else {
                (shorter - height).max(0.0)
            };
            let cost = match self.body[at - 1].after {
                _ if at == b => 0.0,
                Break::Allowed(cost) | Break::Avoid(cost) => cost,
                Break::Never | Break::Forced => unreachable!("splits follow lines a column may end after"),
            };
            Columns {
                lines: a..b,
                split: at,
                height,
                grow,
                deficit: (height - shorter).max(0.0),
                cost,
                columns,
            }
        };
        let (mut low, mut high) = (a + 1, b);
        while low < high {
            let middle = (low + high) / 2;
            if column(a, middle).0 < column(middle, b).0 {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let allowed = |&at: &usize| at == b || matches!(self.body[at - 1].after, Break::Allowed(_) | Break::Avoid(_));
        let mut down = (a + 1..low).rev().filter(allowed);
        let mut up = (low..=b).filter(allowed);
        let least = [down.clone().next(), up.clone().next()]
            .into_iter()
            .flatten()
            .map(|at| split(at).height)
            .fold(f64::INFINITY, f64::min);
        // Ties go to the taller first column, so the second column is the shorter one.
        let mut best: Option<(f64, Columns)> = None;
        let mut scan = |candidates: &mut dyn Iterator<Item = usize>| {
            for at in candidates {
                let columns = split(at);
                let lines = (columns.height - least) / self.line;
                let uneven = UNEVEN * lines * lines;
                if best.as_ref().is_some_and(|(score, _)| uneven >= *score) {
                    break;
                }
                let score = uneven + columns.cost;
                if best.as_ref().is_none_or(|(best, _)| score < *best) {
                    best = Some((score, columns));
                }
            }
        };
        scan(&mut up);
        scan(&mut down);
        let (score, mut columns) = best.expect("a column region can always end at its last line");
        columns.cost = score;
        (columns, least)
    }
}

/// Positions the planned pages. Body lines are moved out of `body`, and the lines of placed blocks
/// out of `floats`. `headers` holds the header row to repeat above each body line where a page or
/// column starts.
#[allow(
    clippy::too_many_arguments,
    reason = "the parts of the content and where the pages start"
)]
fn render(
    plans: &[Plan],
    body: &mut [FlowLine],
    headers: &[Option<&Line>],
    mut notes: Notes,
    (floats, mut margin): (&mut [Float], Margin),
    (first, rectos): (usize, &[usize]),
    bottoms: &[usize],
    config: &Config,
) -> Composed {
    let geometry = &config.page;
    let footnotes = &config.footnotes;
    let height = geometry.text_height().0;
    let spacing = config.styles.caption.space_after.0;
    let second_column = column_width(geometry) + geometry.column_gap.0;
    let mut pages = Vec::with_capacity(plans.len().max(1));
    let mut blanks = Vec::new();
    let mut starts = Vec::with_capacity(plans.len());
    let mut asides = Vec::with_capacity(margin.notes.len());
    // Where each body line's top lands on its page, for the notes beside it.
    let mut tops = vec![0.0; if margin.notes.is_empty() { 0 } else { body.len() }];
    let mut referenced = margin.references.iter().enumerate().peekable();
    let mut issues = Vec::new();
    for plan in plans {
        let only_floats = plan.lines.is_empty() && !plan.floats.is_empty();
        if !only_floats && rectos.binary_search(&plan.lines.start).is_ok() && !(first + pages.len()).is_multiple_of(2) {
            blanks.push(first + pages.len());
            pages.push(super::blank_page(config));
            starts.push(None);
        }
        starts.push((!only_floats).then_some(plan.lines.start));
        let index = first + pages.len();
        issues.extend(page_issues(plan, body, index));
        let left = geometry.left_margin(index).0;
        let shift = geometry.prose_shift(index);
        let top = geometry.margin_top.0;
        let mut items = Vec::new();
        let place = |items: &mut Vec<Item>, line: Vec<Item>, y: f64| {
            items.extend(line.into_iter().map(|item| translate(item, left + shift, top + y)));
        };

        let notes_height = if plan.notes.is_empty() {
            0.0
        } else {
            notes.area(plan.notes.start, plan.notes.end)
        };
        // Top blocks go above the text, bottom blocks below it, above the footnotes. A page of placed
        // blocks only stacks them all from the top.
        let mut y = top;
        let mut below = 0.0;
        for float in &mut floats[plan.floats.clone()] {
            if float.top || only_floats {
                let lines = 0..float.lines.len();
                y = stack(
                    &mut items,
                    &mut float.lines,
                    &[],
                    lines,
                    (left, shift),
                    y,
                    0.0,
                    0,
                    &mut [],
                );
                y += spacing;
            } else {
                below += spacing + float.height;
            }
        }
        let floor = top + height - notes_height - below;
        let mut bottom = floor;
        for float in floats[plan.floats.clone()]
            .iter_mut()
            .filter(|float| !float.top && !only_floats)
        {
            let lines = 0..float.lines.len();
            bottom = stack(
                &mut items,
                &mut float.lines,
                &[],
                lines,
                (left, shift),
                bottom + spacing,
                0.0,
                0,
                &mut [],
            );
        }
        for region in &plan.regions {
            let first = match region {
                Region::Full(lines) => lines.start,
                Region::Columns(columns) => columns.lines.start,
            };
            if first > plan.lines.start {
                y += body[first].space_before * (1.0 + STRETCH * plan.stretch);
            }
            match region {
                Region::Full(lines) => {
                    let x = (left, shift);
                    // A bottom group ends its page, so it ends the page's last region.
                    let split = bottoms.iter().copied().find(|start| lines.contains(start));
                    let above = lines.start..split.unwrap_or(lines.end);
                    y = stack(
                        &mut items,
                        body,
                        headers,
                        above,
                        x,
                        y,
                        plan.stretch,
                        plan.lines.start,
                        &mut tops,
                    );
                    if let Some(split) = split {
                        let group = split..lines.end;
                        let natural: f64 = group
                            .clone()
                            .map(|index| {
                                let space = if index > split { body[index].space_before } else { 0.0 };
                                space + body[index].line.height
                            })
                            .sum();
                        y = stack(
                            &mut items,
                            body,
                            headers,
                            group,
                            x,
                            floor - natural,
                            0.0,
                            plan.lines.start,
                            &mut tops,
                        );
                    }
                }
                Region::Columns(columns) => {
                    // Each column stretches to the region's height if its spaces allow, else stays natural.
                    let height = columns.height + plan.stretch * columns.grow;
                    let parts = [columns.lines.start..columns.split, columns.split..columns.lines.end];
                    for (index, (lines, (natural, bound))) in parts.into_iter().zip(columns.columns).enumerate() {
                        let need = (height - natural).max(0.0);
                        let ratio = if bound > 0.0 && need <= bound + 1e-9 {
                            need / bound
                        } else {
                            0.0
                        };
                        let x = (left + index as f64 * second_column, shift);
                        stack(
                            &mut items,
                            body,
                            headers,
                            lines,
                            x,
                            y,
                            ratio,
                            plan.lines.start,
                            &mut tops,
                        );
                    }
                    y += height;
                }
            }
        }

        let Range { start, end } = plan.notes.clone();
        if end > start {
            let mut y = height - notes.area(start, end) + footnotes.gap.0;
            let rule = Rect {
                x: Pt(left + shift),
                y: Pt(top + y),
                width: footnotes.separator_width,
                height: footnotes.separator_thickness,
            };
            y += footnotes.separator_thickness.0;
            if !notes.is_first(start) {
                y += notes.spacing;
                let note = notes.note[start];
                for line in &notes.continued[note] {
                    place(&mut items, line.line.items.clone(), y);
                    y += line.line.height;
                }
            }
            for index in start..end {
                y += if notes.is_first(index) {
                    notes.spacing
                } else {
                    notes.lines[index].space_before
                };
                let line = &mut notes.lines[index];
                place(&mut items, std::mem::take(&mut line.line.items), y);
                y += line.line.height;
            }
            items.push(Item::Rect {
                rect: rule,
                color: footnotes.separator_color,
            });
        }

        // The notes beside this page's lines, each wanting its first baseline on its reference's.
        let mut wanted = Vec::new();
        while let Some((note, &(line, offset))) = referenced.next_if(|(_, (line, _))| plan.lines.contains(line)) {
            wanted.push((note, tops[line] + offset));
        }
        let column = geometry.note_column(index, &config.margin_notes);
        for (note, top, baseline) in stack_notes(
            &wanted,
            &margin.notes,
            (top, top + height),
            config.margin_notes.spacing.0,
        ) {
            let placed = &mut margin.notes[note];
            let lines = 0..placed.lines.len();
            stack(
                &mut items,
                &mut placed.lines,
                &[],
                lines,
                (column, 0.0),
                top,
                0.0,
                0,
                &mut [],
            );
            asides.push(Aside {
                page: index,
                rect: Rect {
                    x: Pt(column),
                    y: Pt(top),
                    width: config.margin_notes.width,
                    height: Pt(placed.height),
                },
                note: placed.kind,
                reference: Position {
                    page: index,
                    x: Pt(left + shift),
                    y: Pt(baseline),
                },
            });
        }
        pages.push(Page {
            width: geometry.width,
            height: geometry.height,
            items,
        });
    }
    if pages.is_empty() {
        pages.push(super::blank_page(config));
        starts.push(None);
    }
    Composed {
        pages,
        blanks,
        starts,
        asides,
        issues,
    }
}

/// The tops of the notes of a page from where each `wanted` its first baseline, in reading order, as
/// (note, top, wanted baseline), top to bottom. Notes that would overlap stack downward `spacing`
/// apart; if the last runs past the bottom of `area`, they move up from there as far as needed.
fn stack_notes(wanted: &[(usize, f64)], notes: &[Note], area: (f64, f64), spacing: f64) -> Vec<(usize, f64, f64)> {
    let mut order: Vec<(usize, f64, f64)> = wanted
        .iter()
        .map(|&(note, baseline)| (note, baseline - notes[note].baseline, baseline))
        .collect();
    // Stable, so notes wanted at the same height keep their reading order.
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut floor = area.0;
    for (note, top, _) in &mut order {
        *top = top.max(floor);
        floor = *top + notes[*note].height + spacing;
    }
    let mut ceiling = area.1;
    for (note, top, _) in order.iter_mut().rev() {
        *top = top.min(ceiling - notes[*note].height);
        ceiling = *top - spacing;
    }
    order
}

/// Places body lines from `y` down, after the repeated header row if the first one has one, dropping
/// the space above the first line and stretching the others by `ratio` of their bound. `x` is the left
/// edge and how far prose lines move right from it. A line shown only at a page top is empty unless it is
/// the page's first line, `top`. Records where each line's top lands in `tops`, if it has room for it.
/// Returns the bottom of the last line.
#[allow(clippy::too_many_arguments, reason = "the region and where its page starts")]
fn stack(
    items: &mut Vec<Item>,
    body: &mut [FlowLine],
    headers: &[Option<&Line>],
    lines: Range<usize>,
    (x, shift): (f64, f64),
    mut y: f64,
    ratio: f64,
    top: usize,
    tops: &mut [f64],
) -> f64 {
    let left = |line: &FlowLine| x + line.wide.map_or(shift, |room| shift.min(room));
    if let Some(Some(header)) = headers.get(lines.start).filter(|_| !lines.is_empty()) {
        let x = left(&body[lines.start]);
        items.extend(header.items.iter().cloned().map(|item| translate(item, x, y)));
        y += header.height;
    }
    // The callout box around the lines being placed: where its items go, its top, its first line's box, and
    // the left edge of its lines.
    let mut open = None;
    for index in lines.clone() {
        let line = &mut body[index];
        if index > lines.start {
            y += line.space_before * (1.0 + STRETCH * ratio);
        }
        let x = left(line);
        if let (Some(callout), None) = (line.callout, &open) {
            open = Some((items.len(), y, callout, x));
        }
        if let Some(slot) = tops.get_mut(index) {
            *slot = y;
        }
        let placed = std::mem::take(&mut line.line.items);
        if !line.top_only || index == top {
            items.extend(placed.into_iter().map(|item| translate(item, x, y)));
        }
        y += line.line.height;
        if let (Some(callout), Some((position, start, first, x))) = (line.callout, open)
            && (callout.last || index + 1 == lines.end)
        {
            let behind = CalloutBox::items(&first, &callout, x, start, y);
            items.splice(position..position, behind);
            open = None;
        }
    }
    y
}

#[cfg(test)]
mod tests {
    use super::super::Line;
    use super::*;

    /// Lines are 10pt high and a page holds ten of them.
    const PAGE: f64 = 100.0;

    fn line(after: Break, notes: Vec<usize>) -> FlowLine {
        FlowLine {
            line: Line {
                height: 10.0,
                baseline: 8.0,
                items: Vec::new(),
                notes,
                marks: Vec::new(),
                hyphenated: false,
                flaw: None,
                sources: Vec::new(),
            },
            space_before: 0.0,
            after,
            at: Location { line: 1, column: 1 },
            wide: None,
            top_only: false,
            callout: None,
            rule: None,
        }
    }

    /// A paragraph of `count` lines with the usual break costs.
    fn paragraph(count: usize) -> Vec<FlowLine> {
        (0..count)
            .map(|index| line(line_break(index, count), Vec::new()))
            .collect()
    }

    /// Body lines that may break anywhere at no cost, with notes referenced on the given lines.
    fn loose(count: usize, references: &[(usize, usize)]) -> Vec<FlowLine> {
        (0..count)
            .map(|index| {
                let notes = references.iter().filter(|r| r.0 == index).map(|r| r.1).collect();
                line(Break::Allowed(0.0), notes)
            })
            .collect()
    }

    /// The (body lines, note lines) of each page.
    fn pages(body: &[FlowLine], notes: &[usize]) -> Vec<(Range<usize>, Range<usize>)> {
        let notes = notes.iter().map(|&count| loose(count, &[])).collect();
        let continued = (0..body.len()).map(|_| loose(1, &[])).collect();
        let notes = Notes::new(notes, continued, 5.0, 0.0);
        let plans = Composer::new(body, &[], &vec![None; body.len()], &notes, PAGE, 10.0)
            .search(&Cancel::default())
            .unwrap_or_else(|f| panic!("{f:?}"))
            .expect("breaks exist");
        plans.into_iter().map(|plan| (plan.lines, plan.notes)).collect()
    }

    fn body_pages(body: &[FlowLine]) -> Vec<Range<usize>> {
        pages(body, &[]).into_iter().map(|(lines, _)| lines).collect()
    }

    #[test]
    fn a_heading_near_the_bottom_moves_with_its_text() {
        let mut body = paragraph(8);
        let mut heading = line(Break::Never, Vec::new());
        heading.space_before = 10.0;
        body.push(heading);
        body.extend(paragraph(5));

        assert_eq!(body_pages(&body), vec![0..8, 8..14]);
    }

    /// The body lines of each page with the page rules: `duplex` is the physical index of the first page,
    /// and a chapter's last page wants `chapter` lines.
    fn ruled_pages(body: &[FlowLine], duplex: Option<usize>, chapter: usize) -> Vec<Range<usize>> {
        let notes = Notes::new(Vec::new(), Vec::new(), 0.0, 0.0);
        let headers = vec![None; body.len()];
        let mut composer = Composer::new(body, &[], &headers, &notes, PAGE, 10.0);
        composer.paginate(duplex, &[], chapter);
        let plans = composer
            .search(&Cancel::default())
            .unwrap_or_else(|f| panic!("{f:?}"))
            .expect("breaks exist");
        plans.into_iter().map(|plan| plan.lines).collect()
    }

    #[test]
    fn a_chapter_does_not_end_on_a_page_of_one_line() {
        let body = loose(11, &[]);
        assert_eq!(body_pages(&body), vec![0..10, 10..11]);
        // Each line the last page lacks costs like a line short on the page before, squared.
        assert_eq!(ruled_pages(&body, None, 5), vec![0..8, 8..11]);
    }

    #[test]
    fn a_right_hand_page_ends_near_the_height_of_the_left_hand_page_it_faces() {
        // The left-hand page ends two lines short before a group of four that stays together.
        let mut body = loose(32, &[]);
        for flow_line in &mut body[8..11] {
            flow_line.after = Break::Never;
        }
        assert_eq!(body_pages(&body)[..2], [0..8, 8..18]);
        // The facing page then ends a line short too, which costs less than two lines of difference.
        assert_eq!(ruled_pages(&body, Some(1), 1)[..2], [0..8, 8..17]);
    }

    #[test]
    fn avoids_widows_and_orphans() {
        // Ten lines fit, which would leave the last line of eleven alone on the next page.
        assert_eq!(body_pages(&paragraph(11)), vec![0..9, 9..11]);

        // Nine lines and the first line of the next paragraph fit, which would strand that line.
        let mut body = paragraph(9);
        body.extend(paragraph(5));
        assert_eq!(body_pages(&body), vec![0..9, 9..14]);
    }

    #[test]
    fn a_page_strands_no_line_even_when_it_runs_short() {
        // A two-line heading after seven lines leaves room for one line of the next paragraph only.
        let mut body = paragraph(7);
        body.extend([line(Break::Never, Vec::new()), line(Break::Never, Vec::new())]);
        body.extend(paragraph(5));
        assert_eq!(body_pages(&body), vec![0..7, 7..14]);
    }

    #[test]
    fn a_continued_note_strands_no_line_where_the_body_can_make_room() {
        // Nine lines with the note's first line would fill the page, but strand that line.
        let body = loose(14, &[(8, 0)]);
        let notes = Notes::new(vec![paragraph(4)], vec![loose(1, &[])], 0.0, 0.0);
        let plans = Composer::new(&body, &[], &vec![None; body.len()], &notes, PAGE, 10.0)
            .search(&Cancel::default())
            .unwrap_or_else(|f| panic!("{f:?}"))
            .expect("breaks exist");
        let pages: Vec<_> = plans.into_iter().map(|plan| (plan.lines, plan.notes)).collect();
        assert_eq!(pages, vec![(0..8, 0..0), (8..14, 0..4)]);
    }

    #[test]
    fn keeps_a_group_on_one_page_and_honours_explicit_breaks() {
        // Lines 7 to 12 are kept together, so the first page cannot end inside them.
        let mut body = loose(16, &[]);
        for flow_line in &mut body[7..12] {
            flow_line.after = Break::Never;
        }
        assert_eq!(body_pages(&body), vec![0..7, 7..16]);

        let mut body = loose(12, &[]);
        body[2].after = Break::Forced;
        assert_eq!(body_pages(&body), vec![0..3, 3..12]);
    }

    #[test]
    fn places_notes_in_order_on_the_pages_of_their_references() {
        let body = loose(14, &[(1, 0), (2, 1), (12, 2)]);
        // Two notes of one line and their 5pt overhead leave room for seven body lines.
        assert_eq!(pages(&body, &[1, 1, 2]), vec![(0..7, 0..2), (7..14, 2..4)]);
    }

    #[test]
    fn a_note_moves_the_break_before_its_reference() {
        let body = loose(14, &[(8, 0)]);
        assert_eq!(body_pages(&loose(14, &[])), vec![0..10, 10..14]);
        // The note's three lines do not fit below line 8, so that line moves to the next page.
        assert_eq!(pages(&body, &[3]), vec![(0..8, 0..0), (8..14, 0..3)]);
    }

    #[test]
    fn a_long_note_continues_on_the_next_page() {
        let body = loose(12, &[(5, 0)]);
        let pages = pages(&body, &[15]);

        // The note starts on the page of its reference and each page continues where the last stopped.
        assert!(pages[0].0.contains(&5) && pages[0].1.start == 0, "{pages:?}");
        assert!(pages[0].1.end > 0 && pages[0].1.end < 15, "{pages:?}");
        for pair in pages.windows(2) {
            assert_eq!(pair[0].1.end, pair[1].1.start);
        }
        assert_eq!(pages[pages.len() - 1].1.end, 15);
    }
}
