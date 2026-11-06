//! Two balanced columns from a run of blocks. The engine breaks between lines where both columns are most even; Write
//! never splits a block, so it parts the run between two of its units where the taller column is shortest, with the
//! first column the fuller one when two choices tie. A heading stays with the text after it.

use gpui::{Pixels, px};

/// A unit of a run, which goes whole into one column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub height: Pixels,
    /// A blank separator, which takes no room at a column's foot.
    pub blank: bool,
    /// A heading, which does not end a column.
    pub keep_with_next: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Split {
    /// The first part of the second column, or the count of parts when the first holds them all.
    pub second: usize,
    pub first_height: Pixels,
    pub second_height: Pixels,
}

impl Split {
    /// The height of the two columns side by side.
    pub fn height(&self) -> Pixels {
        self.first_height.max(self.second_height)
    }
}

/// The height of `parts` in one column, without blank separators at its foot.
fn column_height(parts: &[Part]) -> Pixels {
    let end = parts.iter().rposition(|part| !part.blank).map_or(0, |last| last + 1);
    parts[..end]
        .iter()
        .map(|part| part.height)
        .fold(px(0.), |sum, height| sum + height)
}

pub fn balance(parts: &[Part]) -> Split {
    let whole = Split {
        second: parts.len(),
        first_height: column_height(parts),
        second_height: px(0.),
    };
    let candidates = |strict: bool| {
        (1..parts.len()).filter(move |&second| {
            !parts[second].blank
                && !(strict
                    && parts[..second]
                        .iter()
                        .rev()
                        .find(|part| !part.blank)
                        .is_some_and(|part| part.keep_with_next))
        })
    };
    let best = |strict: bool| {
        candidates(strict)
            .map(|second| Split {
                second,
                first_height: column_height(&parts[..second]),
                second_height: column_height(&parts[second..]),
            })
            .min_by(|a, b| {
                let fuller_first = |split: &Split| split.first_height < split.second_height;
                a.height()
                    .partial_cmp(&b.height())
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(fuller_first(a).cmp(&fuller_first(b)))
            })
    };
    best(true).or_else(|| best(false)).unwrap_or(whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(height: f32) -> Part {
        Part {
            height: px(height),
            blank: false,
            keep_with_next: false,
        }
    }

    fn gap() -> Part {
        Part {
            height: px(10.),
            blank: true,
            keep_with_next: false,
        }
    }

    #[test]
    fn parts_where_the_taller_column_is_shortest_and_a_gap_at_the_foot_takes_no_room() {
        let split = balance(&[text(60.), gap(), text(40.), gap(), text(50.), gap(), text(30.)]);
        assert_eq!(split.second, 4);
        assert_eq!(split.first_height, px(110.));
        assert_eq!(split.second_height, px(90.));
        let split = balance(&[text(100.), gap(), text(30.), gap(), text(30.)]);
        assert_eq!((split.second, split.height()), (2, px(100.)));
    }

    #[test]
    fn a_tie_leaves_the_first_column_fuller() {
        let split = balance(&[text(50.), gap(), text(50.)]);
        assert_eq!(
            (split.second, split.first_height, split.second_height),
            (2, px(50.), px(50.))
        );
        let split = balance(&[text(40.), text(20.), text(40.)]);
        assert_eq!(split.second, 2);
    }

    #[test]
    fn one_block_stays_whole_in_the_first_column() {
        let split = balance(&[text(80.)]);
        assert_eq!(
            (split.second, split.first_height, split.second_height),
            (1, px(80.), px(0.))
        );
    }

    #[test]
    fn a_heading_does_not_end_a_column_unless_nothing_else_can() {
        let heading = Part {
            keep_with_next: true,
            ..text(20.)
        };
        let split = balance(&[text(50.), gap(), heading, text(100.)]);
        assert_eq!(split.second, 2);
        let split = balance(&[heading, text(50.)]);
        assert_eq!(split.second, 1);
    }
}
