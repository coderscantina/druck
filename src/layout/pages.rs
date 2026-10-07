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
//! A table's rows are lines like any other. Where a page or column starts at one of its rows, the
//! table's header row is set again above it, and its height counts toward that page or column.

use std::collections::BTreeMap;
use std::ops::Range;

use super::{Break, FlowLine, Line, translate};
use crate::config::resolved::{Config, PageGeometry};
use crate::config::source::Source;
use crate::config::values::Pt;
use crate::diagnostic::Diagnostic;
use crate::document::Location;
use crate::page::{Item, Page, Rect};

/// Space between blocks may grow by this share of its natural height.
const STRETCH: f64 = 0.5;
/// Cost of a page whose content ends one body line short of the bottom after stretching. Grows
/// with the square of the shortfall.
const SHORT: f64 = 1000.0;
/// Cost of a page whose spaces stretch fully. Grows with the cube of the share used.
const STRETCHED: f64 = 100.0;
/// Cost of a break between two lines of a block.
const INSIDE: f64 = 50.0;
/// Cost of a break that leaves a block's first line alone at the bottom of a page.
const ORPHAN: f64 = 5000.0;
/// Cost of a break that leaves a block's last line alone at the top of a page.
const WIDOW: f64 = 5000.0;
/// Cost of continuing a footnote on the next page.
const SPLIT_NOTE: f64 = 2000.0;
/// Cost of a page that holds only the continuation of footnotes.
const NOTE_PAGE: f64 = 10_000.0;
/// Cost of a column region one body line taller than its most even split. Grows with the square,
/// so a region gets one line uneven rather than strand a line, but not two.
const UNEVEN: f64 = 3000.0;

/// The width of each of two columns in the prose width.
pub(super) fn column_width(page: &PageGeometry) -> f64 {
    (page.prose_width.0 - page.column_gap.0) / 2.0
}

/// The rule after line `index` of a block of `count` lines.
pub(super) fn line_break(index: usize, count: usize) -> Break {
    if index + 1 == count {
        return Break::Allowed(0.0);
    }
    let mut cost = INSIDE;
    if index == 0 {
        cost += ORPHAN;
    }
    if index + 2 == count {
        cost += WIDOW;
    }
    Break::Allowed(cost)
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

/// Everything that goes on the pages.
pub(super) struct Content {
    pub body: Vec<FlowLine>,
    /// Line ranges of keep groups with their directive locations.
    pub keeps: Vec<(Range<usize>, Location)>,
    /// Line ranges set in two columns, in order.
    pub columns: Vec<Range<usize>>,
    /// Tables in order.
    pub tables: Vec<Table>,
    /// The lines of each footnote, in reference order.
    pub notes: Vec<Vec<FlowLine>>,
    /// The continuation marker of each footnote.
    pub continued: Vec<Vec<FlowLine>>,
}

/// Chooses page breaks and positions the content on pages. `first` is the physical index of the first
/// page, which decides parity: odd pages, counted from 1, have the inner margin on the left.
pub(super) fn compose(
    content: Content,
    first: usize,
    config: &Config,
    source: &Source,
) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let Content {
        mut body,
        keeps,
        columns,
        tables,
        notes,
        continued,
    } = content;
    let footnotes = &config.footnotes;
    let overhead = footnotes.gap.0 + footnotes.separator_thickness.0;
    let notes = Notes::new(notes, continued, overhead, footnotes.spacing.0);
    let body_line = config.styles.body.size.0 * config.styles.body.line_height;
    let mut headers = vec![None; body.len()];
    for table in &tables {
        headers[table.rows.clone()].fill(Some(&table.header));
    }
    let composer = Composer::new(
        &body,
        &columns,
        &headers,
        &notes,
        config.page.text_height().0,
        body_line,
    );
    composer.check(&keeps, &tables, source)?;
    let plans = composer.search().ok_or_else(|| {
        vec![Diagnostic::new(
            Some(source.clone()),
            "no page breaks satisfy the layout constraints",
        )]
    })?;
    Ok(render(&plans, &mut body, &headers, notes, first, config))
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
    cost: f64,
    previous: Option<usize>,
    stretch: f64,
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
        }
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
        tables: &[Table],
        source: &Source,
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
            if line.after == Break::Never && index + 1 < self.body.len() {
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
            start = index + 1;
            natural = 0.0;
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
    fn search(&self) -> Option<Vec<Plan>> {
        let count = self.body.len();
        let mut states = vec![State {
            at: 0,
            placed: 0,
            cost: 0.0,
            previous: None,
            stretch: 0.0,
        }];
        // For each candidate break, the best state per number of footnote lines placed.
        let mut reached: Vec<BTreeMap<usize, usize>> = (0..=count).map(|_| BTreeMap::new()).collect();
        reached[0].insert(0, 0);
        for at in 0..=count {
            let mut next = 0;
            while let Some((placed, index)) = reached[at].range(next..).next().map(|(&p, &i)| (p, i)) {
                next = placed + 1;
                for (to, placed_to, cost, stretch) in self.pages(&states[index]) {
                    let state = State {
                        at: to,
                        placed: placed_to,
                        cost,
                        previous: Some(index),
                        stretch,
                    };
                    match reached[to].get(&placed_to) {
                        Some(&existing) if states[existing].cost <= cost => {}
                        Some(&existing) => states[existing] = state,
                        None => {
                            reached[to].insert(placed_to, states.len());
                            states.push(state);
                        }
                    }
                }
            }
        }
        let end = *reached[count].get(&self.notes.lines.len())?;
        let mut plans = Vec::new();
        let mut index = end;
        while let Some(previous) = states[index].previous {
            let (from, to) = (&states[previous], &states[index]);
            plans.push(Plan {
                lines: from.at..to.at,
                regions: self.regions(from.at, to.at),
                stretch: to.stretch,
                notes: from.placed..to.placed,
            });
            index = previous;
        }
        plans.reverse();
        Some(plans)
    }

    /// The pages that can follow `state`, as (end line, footnote lines placed, total cost, stretch).
    fn pages(&self, state: &State) -> Vec<(usize, usize, f64, f64)> {
        let (start, from) = (state.at, state.placed);
        let mut pages = Vec::new();
        let pending = self.notes.start[self.through[start]];
        if from < pending
            && let Some(placed) = self.notes.fit(from, from, pending, self.height)
            && placed > from
        {
            let split = if placed < pending { SPLIT_NOTE } else { 0.0 };
            pages.push((start, placed, state.cost + NOTE_PAGE + split, 0.0));
        }

        // Natural height, stretch bound, and column break cost of the regions finished so far. A page
        // starting inside a full-width table begins with its header; in columns, balancing adds it.
        let header = match self.runs.get(start) {
            Some(None) => self.repeat(start),
            _ => 0.0,
        };
        let (mut natural, mut stretch, mut cost) = (header, 0.0, 0.0);
        for (index, line) in self.body.iter().enumerate().skip(start) {
            let end = index + 1;
            let last = end == self.body.len();
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
                    if line.after == Break::Never && !last && end < run.end {
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
                Break::Never => continue,
                Break::Forced => 0.0,
                Break::Allowed(cost) => cost,
            };
            let max = self.notes.start[referenced];
            let Some(placed) = self.notes.fit(from, min, max, self.height - page) else {
                break;
            };
            let short = (self.height - page - self.notes.area(from, placed)).max(0.0);
            let (fill, ratio) = if last || line.after == Break::Forced {
                (0.0, 0.0)
            } else {
                let used = short.min(page_stretch);
                let ratio = if page_stretch > 0.0 { used / page_stretch } else { 0.0 };
                let lines = (short - used + deficit) / self.line;
                (STRETCHED * ratio.powi(3) + SHORT * lines * lines, ratio)
            };
            let split = if placed < max { SPLIT_NOTE } else { 0.0 };
            pages.push((end, placed, state.cost + fill + penalty + split + page_cost, ratio));
            if line.after == Break::Forced {
                break;
            }
        }
        pages
    }
}

impl Composer<'_> {
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
                Break::Allowed(cost) => cost,
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
        let allowed = |&at: &usize| at == b || matches!(self.body[at - 1].after, Break::Allowed(_));
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

/// Positions the planned pages. Body lines are moved out of `body`. `headers` holds the header row to
/// repeat above each body line where a page or column starts.
fn render(
    plans: &[Plan],
    body: &mut [FlowLine],
    headers: &[Option<&Line>],
    mut notes: Notes,
    first: usize,
    config: &Config,
) -> Vec<Page> {
    let geometry = &config.page;
    let footnotes = &config.footnotes;
    let height = geometry.text_height().0;
    let second_column = column_width(geometry) + geometry.column_gap.0;
    let mut pages = Vec::with_capacity(plans.len().max(1));
    for plan in plans {
        let index = first + pages.len();
        let left = geometry.left_margin(index).0;
        let shift = geometry.prose_shift(index);
        let top = geometry.margin_top.0;
        let mut items = Vec::new();
        let place = |items: &mut Vec<Item>, line: Vec<Item>, y: f64| {
            items.extend(line.into_iter().map(|item| translate(item, left + shift, top + y)));
        };

        let mut y = top;
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
                    y = stack(&mut items, body, headers, lines.clone(), x, y, plan.stretch);
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
                        stack(&mut items, body, headers, lines, x, y, ratio);
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
        pages.push(Page {
            width: geometry.width,
            height: geometry.height,
            items,
        });
    }
    if pages.is_empty() {
        pages.push(Page {
            width: geometry.width,
            height: geometry.height,
            items: Vec::new(),
        });
    }
    pages
}

/// Places body lines from `y` down, after the repeated header row if the first one has one, dropping
/// the space above the first line and stretching the others by `ratio` of their bound. `x` is the left
/// edge and how far prose lines move right from it. Returns the bottom of the last line.
fn stack(
    items: &mut Vec<Item>,
    body: &mut [FlowLine],
    headers: &[Option<&Line>],
    lines: Range<usize>,
    (x, shift): (f64, f64),
    mut y: f64,
    ratio: f64,
) -> f64 {
    let left = |line: &FlowLine| x + line.wide.map_or(shift, |room| shift.min(room));
    if let Some(Some(header)) = headers.get(lines.start).filter(|_| !lines.is_empty()) {
        let x = left(&body[lines.start]);
        items.extend(header.items.iter().cloned().map(|item| translate(item, x, y)));
        y += header.height;
    }
    for index in lines.clone() {
        let line = &mut body[index];
        if index > lines.start {
            y += line.space_before * (1.0 + STRETCH * ratio);
        }
        let x = left(line);
        let placed = std::mem::take(&mut line.line.items);
        items.extend(placed.into_iter().map(|item| translate(item, x, y)));
        y += line.line.height;
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
            },
            space_before: 0.0,
            after,
            at: Location { line: 1, column: 1 },
            wide: None,
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
            .search()
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
