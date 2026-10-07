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
//! A page holds stacked body regions and one footnote area across the text width. Each page has a
//! single full-width region for now; milestone 05 adds column regions.

use std::collections::BTreeMap;
use std::ops::Range;

use super::{Break, FlowLine, translate};
use crate::config::resolved::Config;
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

/// Everything that goes on the pages.
pub(super) struct Content {
    pub body: Vec<FlowLine>,
    /// Line ranges of keep groups with their directive locations.
    pub keeps: Vec<(Range<usize>, Location)>,
    /// The lines of each footnote, in reference order.
    pub notes: Vec<Vec<FlowLine>>,
    /// The continuation marker of each footnote.
    pub continued: Vec<Vec<FlowLine>>,
}

/// Chooses page breaks and positions the content on pages. Odd pages have the inner margin on the left.
pub(super) fn compose(content: Content, config: &Config, source: &Source) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let Content {
        mut body,
        keeps,
        notes,
        continued,
    } = content;
    let footnotes = &config.footnotes;
    let overhead = footnotes.gap.0 + footnotes.separator_thickness.0;
    let notes = Notes::new(notes, continued, overhead, footnotes.spacing.0);
    let body_line = config.styles.body.size.0 * config.styles.body.line_height;
    let composer = Composer::new(&body, &notes, config.page.text_height().0, body_line);
    composer.check(&keeps, source)?;
    let plans = composer.search().ok_or_else(|| {
        vec![Diagnostic::new(
            Some(source.clone()),
            "no page breaks satisfy the layout constraints",
        )]
    })?;
    Ok(render(&plans, &mut body, notes, config))
}

/// A planned page: body regions stacked from the top and one footnote area at the bottom.
#[derive(Debug)]
struct Plan {
    regions: Vec<Region>,
    /// The footnote lines on the page, as a range of the footnote stream.
    notes: Range<usize>,
}

/// A full-width run of body lines whose spaces stretch by `stretch` of their bound.
#[derive(Debug)]
struct Region {
    lines: Range<usize>,
    stretch: f64,
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
    notes: &'a Notes,
    height: f64,
    /// A body line's height, the unit for measuring short pages.
    line: f64,
}

impl<'a> Composer<'a> {
    fn new(body: &'a [FlowLine], notes: &'a Notes, height: f64, line: f64) -> Self {
        let mut through = Vec::with_capacity(body.len() + 1);
        through.push(0);
        for flow_line in body {
            let referenced = flow_line.line.notes.iter().map(|note| note + 1).max().unwrap_or(0);
            through.push(referenced.max(through[through.len() - 1]));
        }
        Self {
            body,
            through,
            notes,
            height,
            line,
        }
    }

    /// Reports content that must stay on one page but cannot, with the first line of each of its
    /// footnotes, and footnote lines too tall for a page of their own.
    fn check(&self, keeps: &[(Range<usize>, Location)], source: &Source) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        let mut error = |at: Location, message: String| {
            errors.push(Diagnostic::new(Some(source.clone()), message).at(at.line, at.column));
        };
        let mut start = 0;
        let mut natural = 0.0;
        for (index, line) in self.body.iter().enumerate() {
            if index > start {
                natural += line.space_before;
            }
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
                let (at, what) = match keep {
                    Some((_, at)) => (*at, "this keep group"),
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
                regions: vec![Region {
                    lines: from.at..to.at,
                    stretch: to.stretch,
                }],
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

        let (mut natural, mut stretch) = (0.0, 0.0);
        for (index, line) in self.body.iter().enumerate().skip(start) {
            if index > start {
                natural += line.space_before;
                stretch += line.space_before * STRETCH;
            }
            natural += line.line.height;
            if natural > self.height {
                break;
            }
            let end = index + 1;
            let last = end == self.body.len();
            let penalty = match line.after {
                _ if last => 0.0,
                Break::Never => continue,
                Break::Forced => 0.0,
                Break::Allowed(cost) => cost,
            };
            let referenced = self.through[end];
            let min = if referenced > self.through[start] {
                self.notes.start[referenced - 1] + 1
            } else {
                from
            };
            let max = self.notes.start[referenced];
            let Some(placed) = self.notes.fit(from, min, max, self.height - natural) else {
                break;
            };
            let short = (self.height - natural - self.notes.area(from, placed)).max(0.0);
            let (fill, ratio) = if last || line.after == Break::Forced {
                (0.0, 0.0)
            } else {
                let used = short.min(stretch);
                let ratio = if stretch > 0.0 { used / stretch } else { 0.0 };
                let lines = (short - used) / self.line;
                (STRETCHED * ratio.powi(3) + SHORT * lines * lines, ratio)
            };
            let split = if placed < max { SPLIT_NOTE } else { 0.0 };
            pages.push((end, placed, state.cost + fill + penalty + split, ratio));
            if line.after == Break::Forced {
                break;
            }
        }
        pages
    }
}

/// Positions the planned pages. Body lines are moved out of `body`.
fn render(plans: &[Plan], body: &mut [FlowLine], mut notes: Notes, config: &Config) -> Vec<Page> {
    let geometry = &config.page;
    let footnotes = &config.footnotes;
    let height = geometry.text_height().0;
    let mut pages = Vec::with_capacity(plans.len().max(1));
    for plan in plans {
        let left = if pages.len() % 2 == 0 {
            geometry.margin_inner.0
        } else {
            geometry.margin_outer.0
        };
        let top = geometry.margin_top.0;
        let mut items = Vec::new();
        let place = |items: &mut Vec<Item>, line: Vec<Item>, y: f64| {
            items.extend(line.into_iter().map(|item| translate(item, left, top + y)));
        };

        let mut y = 0.0;
        for region in &plan.regions {
            for index in region.lines.clone() {
                let line = &mut body[index];
                if index > region.lines.start {
                    y += line.space_before * (1.0 + STRETCH * region.stretch);
                }
                place(&mut items, std::mem::take(&mut line.line.items), y);
                y += line.line.height;
            }
        }

        let Range { start, end } = plan.notes.clone();
        if end > start {
            let mut y = height - notes.area(start, end) + footnotes.gap.0;
            let rule = Rect {
                x: Pt(left),
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
        let plans = Composer::new(body, &notes, PAGE, 10.0).search().expect("breaks exist");
        plans
            .into_iter()
            .map(|plan| (plan.regions[0].lines.clone(), plan.notes))
            .collect()
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
