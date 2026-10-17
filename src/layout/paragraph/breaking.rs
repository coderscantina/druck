//! Paragraph-wide line breaking after Knuth and Plass.
//!
//! A paragraph is a list of boxes, glue, and penalties. Every feasible break is scored, and the
//! breaks with the least total demerits over the whole paragraph win. A first pass only accepts
//! lines up to [`TOLERANCE`]; if that finds no solution, a final pass accepts any line that is not
//! overfull. Ties go to the earlier candidate, so the result is deterministic.

/// Badness limit of the first pass. 200 lets interword spaces stretch to about 1.26 times their
/// permitted stretch.
const TOLERANCE: f64 = 200.0;
/// Badness of a line that falls short and cannot stretch at all, such as a single word in a
/// justified line. Other badness is not capped, so very loose lines still compare: a nearly empty
/// line costs more than a fuller one.
const NO_STRETCH: f64 = 10_000.0;
/// Added to every line's badness, so fewer lines are preferred.
const LINE_PENALTY: f64 = 10.0;
/// Demerits for two hyphenated lines in a row.
const DOUBLE_HYPHEN: f64 = 10_000.0;
/// Demerits for a hyphen on the line before a forced break, which leaves a short word fragment
/// on the last line.
const FINAL_HYPHEN: f64 = 5_000.0;
/// Demerits for a paragraph's last line that is shorter than [`Measure::last`], which leaves a
/// stub that reads as a widow.
const SHORT_LAST: f64 = 5_000.0;
/// Demerits for a hyphenated line after [`Measure::hyphens`] hyphenated lines. Large enough to beat
/// any ordinary alternative, but finite so a paragraph that cannot avoid it still breaks.
const HYPHEN_LIMIT: f64 = 10_000_000.0;
/// Demerits for adjacent lines whose spacing classes are more than one apart, such as a tight
/// line next to a loose one.
const FITNESS_CHANGE: f64 = 10_000.0;

/// How far the content next to a break hangs into the margins: `end` at the right of the line
/// that ends here, `start` at the left of the line that begins after it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Hang {
    pub end: f64,
    pub start: f64,
}

/// One element of a paragraph. Breaks are possible at glue that follows a box, at penalties,
/// and at forced breaks. `at` identifies the break for the caller.
#[derive(Debug, Clone, Copy)]
pub enum Item<P> {
    /// Unbreakable content of a natural width.
    Box(f64),
    /// Interword space. Breaking here drops the space.
    Glue {
        width: f64,
        stretch: f64,
        shrink: f64,
        at: P,
        hang: Hang,
    },
    /// An optional break. `width` is only set when breaking here, as for a hyphen. `flagged`
    /// marks hyphen breaks.
    Penalty {
        width: f64,
        cost: f64,
        flagged: bool,
        at: P,
        hang: Hang,
    },
    /// Infinitely stretchable space before a forced break, so that line keeps natural spacing.
    Fill,
    /// A required break: a hard line break or the paragraph end.
    Break { at: P, hang: Hang },
}

/// The lines to fill: the first `narrow` lines `first` wide, the rest `rest` wide. `stretch` is extra
/// stretch for every line, which sets ragged text against a soft right edge. `hang` is the left
/// protrusion of the first line. `hyphens` is the most hyphenated lines in a row, and `last` the
/// least natural width the last line of a paragraph of several lines wants.
#[derive(Debug, Clone, Copy)]
pub struct Measure {
    pub first: f64,
    pub narrow: usize,
    pub rest: f64,
    pub stretch: f64,
    pub hang: f64,
    pub hyphens: usize,
    pub last: f64,
}

/// A rule a chosen line breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defect {
    /// Its spaces stretch beyond [`TOLERANCE`]. Only justified lines count, ragged ones may be short.
    Loose,
    /// It is hyphenated after [`Measure::hyphens`] hyphenated lines.
    Hyphens,
    /// It is the last line of a paragraph of several lines and shorter than [`Measure::last`].
    ShortLast,
}

/// A chosen break: the `at` of its item, and the rule the line it ends breaks, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chosen<P> {
    pub at: P,
    pub defect: Option<Defect>,
}

/// The chosen breaks in order. The last item must be a [`Item::Break`]. `None` if some content is
/// wider than its line.
pub fn breaks<P: Copy>(items: &[Item<P>], measure: &Measure) -> Option<Vec<Chosen<P>>> {
    debug_assert!(matches!(items.last(), Some(Item::Break { .. })));
    let sums = sums(items);
    let positions = pass(items, &sums, measure, TOLERANCE).or_else(|| pass(items, &sums, measure, f64::INFINITY))?;
    Some(
        positions
            .into_iter()
            .map(|(position, defect)| match items[position] {
                Item::Glue { at, .. } | Item::Penalty { at, .. } | Item::Break { at, .. } => Chosen { at, defect },
                Item::Box(_) | Item::Fill => unreachable!("breaks are only taken at glue, penalties, and breaks"),
            })
            .collect(),
    )
}

/// Totals of all items before an index.
#[derive(Debug, Clone, Copy, Default)]
struct Sums {
    width: f64,
    stretch: f64,
    shrink: f64,
    fills: u32,
}

fn sums<P>(items: &[Item<P>]) -> Vec<Sums> {
    let mut sums = Vec::with_capacity(items.len() + 1);
    let mut total = Sums::default();
    sums.push(total);
    for item in items {
        match *item {
            Item::Box(width) => total.width += width,
            Item::Glue {
                width, stretch, shrink, ..
            } => {
                total.width += width;
                total.stretch += stretch;
                total.shrink += shrink;
            }
            Item::Fill => total.fills += 1,
            Item::Penalty { .. } | Item::Break { .. } => {}
        }
        sums.push(total);
    }
    sums
}

/// A feasible break with the best way to reach it in one fitness class and hyphen run.
struct Node {
    /// Index of the break item.
    position: usize,
    /// Index of the first item of the next line, after discarded glue and penalties.
    after: usize,
    /// Left protrusion of the next line.
    hang: f64,
    line: usize,
    fitness: Fitness,
    /// Hyphenated lines in a row ending here, at most [`Measure::hyphens`].
    run: usize,
    /// The rule the line ending here breaks.
    defect: Option<Defect>,
    demerits: f64,
    previous: Option<usize>,
}

/// The best way to reach a break: total demerits, the previous node, the line's ratio, and its defect.
type Candidate = (f64, usize, f64, Option<Defect>);

/// Spacing class of a line, from tight to very loose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fitness {
    Tight,
    Decent,
    Loose,
    VeryLoose,
}

impl Fitness {
    fn of(ratio: f64) -> Self {
        if ratio < -0.5 {
            Self::Tight
        } else if ratio <= 0.5 {
            Self::Decent
        } else if ratio <= 1.0 {
            Self::Loose
        } else {
            Self::VeryLoose
        }
    }
}

/// One breaking pass. Returns the item indices of the chosen breaks with their defects.
fn pass<P>(
    items: &[Item<P>],
    sums: &[Sums],
    measure: &Measure,
    tolerance: f64,
) -> Option<Vec<(usize, Option<Defect>)>> {
    // The best node per break is kept for each fitness class and hyphen run, so the limit on
    // hyphens in a row stays exact.
    let runs = measure.hyphens + 1;
    let mut nodes = vec![Node {
        position: 0,
        after: 0,
        hang: measure.hang,
        line: 0,
        fitness: Fitness::Decent,
        run: 0,
        defect: None,
        demerits: 0.0,
        previous: None,
    }];
    // Nodes from which a next line can still start. Nodes whose line would be overfull drop out,
    // so this holds at most about one line's worth of breaks per fitness class.
    let mut active = vec![0];
    for (position, item) in items.iter().enumerate() {
        let (width, cost, flagged, hang, forced) = match *item {
            Item::Glue { hang, .. } if position > 0 && matches!(items[position - 1], Item::Box(_)) => {
                (0.0, 0.0, false, hang, false)
            }
            Item::Penalty {
                width,
                cost,
                flagged,
                hang,
                ..
            } => (width, cost, flagged, hang, false),
            Item::Break { hang, .. } => (0.0, f64::NEG_INFINITY, false, hang, true),
            Item::Box(_) | Item::Glue { .. } | Item::Fill => continue,
        };
        let last = position == items.len() - 1;
        let mut best: Vec<Option<Candidate>> = vec![None; 4 * runs];
        active.retain(|&index| {
            let node = &nodes[index];
            if node.after > position {
                return true;
            }
            let (start, end) = (sums[node.after], sums[position]);
            let natural = end.width - start.width + width;
            let line_width = if node.line < measure.narrow {
                measure.first
            } else {
                measure.rest
            };
            let target = line_width + node.hang + hang.end;
            let ratio = ratio(
                natural,
                target,
                end.stretch - start.stretch + measure.stretch,
                end.shrink - start.shrink,
                end.fills > start.fills,
            );
            if ratio >= -1.0 {
                let badness = if ratio.is_finite() {
                    100.0 * ratio.abs().powi(3)
                } else {
                    NO_STRETCH
                };
                if badness <= tolerance {
                    let fitness = Fitness::of(ratio);
                    let mut demerits = (LINE_PENALTY + badness).powi(2);
                    let mut defect = (badness > TOLERANCE && measure.stretch == 0.0).then_some(Defect::Loose);
                    if cost.is_finite() {
                        demerits += cost * cost.abs();
                    }
                    if node.run > 0 && flagged {
                        demerits += DOUBLE_HYPHEN;
                    }
                    if node.run >= measure.hyphens && flagged {
                        demerits += HYPHEN_LIMIT;
                        defect = Some(Defect::Hyphens);
                    }
                    if node.run > 0 && forced {
                        demerits += FINAL_HYPHEN;
                    }
                    if last && node.line > 0 && natural < measure.last {
                        demerits += SHORT_LAST;
                        defect = defect.or(Some(Defect::ShortLast));
                    }
                    if (fitness as usize).abs_diff(node.fitness as usize) > 1 {
                        demerits += FITNESS_CHANGE;
                    }
                    let total = node.demerits + demerits;
                    let run = if flagged {
                        (node.run + 1).min(measure.hyphens)
                    } else {
                        0
                    };
                    let slot = &mut best[fitness as usize * runs + run];
                    if slot.is_none_or(|(least, ..)| total < least) {
                        *slot = Some((total, index, ratio, defect));
                    }
                }
            }
            ratio >= -1.0 && !forced
        });
        let after = after(items, position);
        for (total, previous, ratio, defect) in best.into_iter().flatten() {
            active.push(nodes.len());
            nodes.push(Node {
                position,
                after,
                hang: hang.start,
                line: nodes[previous].line + 1,
                fitness: Fitness::of(ratio),
                run: if flagged {
                    (nodes[previous].run + 1).min(measure.hyphens)
                } else {
                    0
                },
                defect,
                demerits: total,
                previous: Some(previous),
            });
        }
        if active.is_empty() {
            return None;
        }
    }
    let last = items.len() - 1;
    let mut index = active
        .into_iter()
        .filter(|&index| nodes[index].position == last && nodes[index].previous.is_some())
        .reduce(|best, index| {
            if nodes[index].demerits < nodes[best].demerits {
                index
            } else {
                best
            }
        })?;
    let mut positions = Vec::with_capacity(nodes[index].line);
    while let Some(previous) = nodes[index].previous {
        positions.push((nodes[index].position, nodes[index].defect));
        index = previous;
    }
    positions.reverse();
    Some(positions)
}

/// How much of its permitted stretch (positive) or shrink (negative) a line uses.
fn ratio(natural: f64, target: f64, stretch: f64, shrink: f64, fill: bool) -> f64 {
    if natural < target {
        if fill {
            0.0
        } else if stretch > 0.0 {
            (target - natural) / stretch
        } else {
            f64::INFINITY
        }
    } else if natural > target {
        if shrink > 0.0 {
            (target - natural) / shrink
        } else {
            f64::NEG_INFINITY
        }
    } else {
        0.0
    }
}

/// The first item of the line after a break at `position`: glue and penalties are discarded.
fn after<P>(items: &[Item<P>], position: usize) -> usize {
    let mut index = position + 1;
    while matches!(items.get(index), Some(Item::Glue { .. } | Item::Penalty { .. })) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(width: f64) -> Item<usize> {
        Item::Box(width)
    }

    fn space(at: usize) -> Item<usize> {
        Item::Glue {
            width: 1.0,
            stretch: 6.0,
            shrink: 0.5,
            at,
            hang: Hang::default(),
        }
    }

    fn paragraph(widths: &[f64]) -> Vec<Item<usize>> {
        let mut items = Vec::new();
        for (index, &width) in widths.iter().enumerate() {
            if index > 0 {
                items.push(space(index));
            }
            items.push(word(width));
        }
        items.push(Item::Fill);
        items.push(Item::Break {
            at: widths.len(),
            hang: Hang::default(),
        });
        items
    }

    fn measure(width: f64) -> Measure {
        Measure {
            first: width,
            narrow: 1,
            rest: width,
            stretch: 0.0,
            hang: 0.0,
            hyphens: 2,
            last: 0.0,
        }
    }

    fn hyphen(at: usize) -> Item<usize> {
        Item::Penalty {
            width: 1.0,
            cost: 0.0,
            flagged: true,
            at,
            hang: Hang::default(),
        }
    }

    fn at(chosen: Vec<Chosen<usize>>) -> Vec<usize> {
        chosen.into_iter().map(|chosen| chosen.at).collect()
    }

    /// The longest stretch of consecutive lines that end at a hyphen, given the hyphen positions.
    fn longest_run(chosen: &[Chosen<usize>], hyphens: &[usize]) -> usize {
        let (mut run, mut longest) = (0, 0);
        for chosen in chosen {
            run = if hyphens.contains(&chosen.at) { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        longest
    }

    #[test]
    fn looks_ahead_where_greedy_filling_strands_a_word() {
        // Greedy filling puts three words on the first line, then "d" alone on a line it cannot
        // fill, since "d e" is too wide even when shrunk. Moving "c" down fills both lines.
        let items = paragraph(&[6.0, 6.0, 6.0, 10.0, 10.0]);

        assert_eq!(breaks(&items, &measure(20.0)).map(at), Some(vec![2, 4, 5]));
    }

    #[test]
    fn prefers_the_fuller_line_when_no_line_fits_within_tolerance() {
        // Neither "a" nor "a b" fits within tolerance before the wide word. Both used to cap at the
        // same badness, so the earlier break won and left "a" alone.
        let items: Vec<_> = paragraph(&[2.0, 2.0, 16.0])
            .into_iter()
            .map(|item| match item {
                Item::Glue { width, at, hang, .. } => Item::Glue {
                    width,
                    stretch: 0.0,
                    shrink: 0.0,
                    at,
                    hang,
                },
                item => item,
            })
            .collect();
        let ragged = Measure {
            stretch: 3.0,
            ..measure(20.0)
        };

        assert_eq!(breaks(&items, &ragged).map(at), Some(vec![2, 3]));
    }

    #[test]
    fn reports_content_wider_than_the_line() {
        assert_eq!(breaks(&paragraph(&[6.0, 25.0]), &measure(20.0)), None);
    }

    /// Boxes of `width` joined by the given separators: `h` for a hyphen, anything else for a space.
    fn chain(separators: &str, width: f64) -> Vec<Item<usize>> {
        let mut items = vec![word(width)];
        for (index, separator) in separators.chars().enumerate() {
            items.push(if separator == 'h' {
                hyphen(index + 1)
            } else {
                space(index + 1)
            });
            items.push(word(width));
        }
        items.push(Item::Fill);
        items.push(Item::Break {
            at: separators.len() + 1,
            hang: Hang::default(),
        });
        items
    }

    fn defects(chosen: &[Chosen<usize>]) -> Vec<Option<Defect>> {
        chosen.iter().map(|chosen| chosen.defect).collect()
    }

    #[test]
    fn limits_hyphens_in_a_row_where_the_old_cost_allows_three() {
        // Two boxes and a hyphen fill a line exactly, so without a limit the breaks after boxes
        // 2, 4, and 6 are cheapest. A space after the first box offers a way around.
        let items = chain("ghhhhhg", 4.5);
        let hyphens = [2, 3, 4, 5, 6];

        let free = breaks(
            &items,
            &Measure {
                hyphens: 10,
                ..measure(10.0)
            },
        )
        .unwrap();
        assert_eq!(longest_run(&free, &hyphens), 3);

        let limited = breaks(&items, &measure(10.0)).unwrap();
        assert_eq!(longest_run(&limited, &hyphens), 2);
        assert!(!defects(&limited).contains(&Some(Defect::Hyphens)));
    }

    #[test]
    fn avoids_a_short_last_line_when_another_break_exists() {
        let items = paragraph(&[4.0, 4.0, 4.0, 4.0, 3.0]);

        let free = breaks(&items, &measure(20.0)).unwrap();
        assert_eq!(at(free), vec![4, 5]);

        let wanted = Measure {
            last: 6.0,
            ..measure(20.0)
        };
        let moved = breaks(&items, &wanted).unwrap();
        assert_eq!(at(moved.clone()), vec![3, 5]);
        assert_eq!(defects(&moved), vec![None, None]);
    }

    #[test]
    fn reports_the_rule_each_chosen_line_breaks() {
        // Nothing avoids a third hyphen when every break is one.
        let hyphenated = breaks(&chain("hhhhhhh", 4.5), &measure(10.0)).unwrap();
        assert_eq!(defects(&hyphenated), vec![None, None, Some(Defect::Hyphens), None]);

        // Nothing avoids a last line of one word, which is shorter than wanted.
        let items = paragraph(&[9.0, 9.0, 9.0, 9.0, 2.0]);
        let short = breaks(
            &items,
            &Measure {
                last: 6.0,
                ..measure(20.0)
            },
        )
        .unwrap();
        assert_eq!(short.last().unwrap().defect, Some(Defect::ShortLast));

        // The second pass accepts a loose line in justified text, but not in ragged text.
        let items = chain("hgh", 4.5);
        let justified = breaks(&items, &measure(10.0)).unwrap();
        assert_eq!(defects(&justified), vec![Some(Defect::Loose), None]);
        let ragged = breaks(
            &items,
            &Measure {
                stretch: 30.0,
                ..measure(10.0)
            },
        )
        .unwrap();
        assert_eq!(defects(&ragged), vec![None, None]);
    }
}
