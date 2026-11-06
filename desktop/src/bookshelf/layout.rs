//! Where the spines stand: which chapters make a group, and how wide each spine is. Plain data, no gpui, so the rules
//! have tests of their own.
//!
//! A spine is as wide as its chapter is long. The formula is [`SPINE_MIN_WIDTH`] plus [`SPINE_WORD_WIDTH`] per word, up
//! to [`SPINE_MAX_WIDTH`]. When the row would not fit the window, every spine's share of words shrinks by the same
//! factor, down to the least width: 40 chapters then stand in 720 px. Only below that does the row scroll.

use std::collections::HashMap;

use crate::session::{ChapterId, Contents, Row};
use crate::tokens::{
    SHELF_END_SLOT, SHELF_GROUP_GAP, SHELF_LOOSE_MIN_WIDTH, SPINE_GAP, SPINE_MAX_WIDTH, SPINE_MIN_WIDTH,
    SPINE_WORD_WIDTH,
};

/// A chapter on the shelf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spine {
    pub id: ChapterId,
    pub number: Option<u32>,
    pub title: String,
    pub words: Option<usize>,
    /// Front matter, back matter, a part's own file, and files not in the book have narrow spines with upright titles.
    pub matter: bool,
}

/// Spines that stand together, with the label that hangs below them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub label: Option<String>,
    pub spines: Vec<Spine>,
    /// Files not in the book, which stand at the right end.
    pub loose: bool,
}

fn spine(row: &Row, words: &HashMap<ChapterId, usize>, matter: bool) -> Spine {
    Spine {
        id: row.id,
        number: row.number,
        title: row.title.clone(),
        words: words.get(&row.id).copied(),
        matter,
    }
}

/// Where a chapter of the book stands in it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Front,
    Body,
    Back,
}

/// The groups of the shelf, in order. Nothing in a project says what a part or the matter is, so this reads it off the
/// numbers. A chapter without one before the first numbered chapter is front matter and after the last is back matter,
/// each in a group without a label. In between, a chapter without a number opens a part named by its title, unless the
/// one before it is also without a number, as a part's own file and an interlude after it are. A book with no numbered
/// chapter has no matter.
pub fn groups(contents: &Contents, words: &HashMap<ChapterId, usize>) -> Vec<Group> {
    let numbered = |row: &Row| row.number.is_some();
    let first = contents.book.iter().position(numbered);
    let last = contents.book.iter().rposition(numbered);
    let mut groups: Vec<Group> = Vec::new();
    let mut zone = None;
    let mut previous_numbered = false;
    for (at, row) in contents.book.iter().enumerate() {
        let here = match (first, last) {
            (Some(first), _) if at < first => Zone::Front,
            (_, Some(last)) if at > last => Zone::Back,
            _ => Zone::Body,
        };
        let matter = first.is_some() && !numbered(row);
        let opens_part = here == Zone::Body && matter && previous_numbered;
        if zone != Some(here) || opens_part {
            groups.push(Group {
                label: opens_part.then(|| row.title.clone()),
                spines: Vec::new(),
                loose: false,
            });
            zone = Some(here);
        }
        if let Some(group) = groups.last_mut() {
            group.spines.push(spine(row, words, matter));
        }
        previous_numbered = numbered(row);
    }
    if !contents.not_in_book.is_empty() {
        groups.push(Group {
            label: Some(crate::t!(super::strings::Key::NotInBook).to_owned()),
            // Files outside the book have no length that counts, so their spines are all the least width.
            spines: (contents.not_in_book.iter())
                .map(|row| spine(row, &HashMap::new(), true))
                .collect(),
            loose: true,
        });
    }
    groups
}

/// The widths of the spines in group order, and the row's total width with the room kept for a dropped spine.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub widths: Vec<f32>,
    pub total: f32,
}

/// The room a group of files not in the book keeps beyond its spines, for its label.
pub fn loose_pad(group: &Group) -> f32 {
    let spines = group.spines.len() as f32;
    let natural = spines * SPINE_MIN_WIDTH + (spines - 1.).max(0.) * SPINE_GAP;
    if group.loose {
        (SHELF_LOOSE_MIN_WIDTH - natural).max(0.)
    } else {
        0.
    }
}

/// The room the row needs whatever the spines' lengths are: gaps within groups and between them, the end slot, and the
/// loose group's label.
fn gaps(groups: &[Group]) -> f32 {
    let within: usize = groups.iter().map(|group| group.spines.len().saturating_sub(1)).sum();
    let pads: f32 = groups.iter().map(loose_pad).sum();
    within as f32 * SPINE_GAP + groups.len().saturating_sub(1) as f32 * SHELF_GROUP_GAP + SHELF_END_SLOT + pads
}

/// How wide each spine is in a row of at most `available`.
pub fn fit(groups: &[Group], available: f32) -> Fit {
    let spines: Vec<&Spine> = groups.iter().flat_map(|group| &group.spines).collect();
    let share = |spine: &Spine| {
        let words = spine.words.unwrap_or(0) as f32;
        (words * SPINE_WORD_WIDTH).min(SPINE_MAX_WIDTH - SPINE_MIN_WIDTH)
    };
    let fixed = spines.len() as f32 * SPINE_MIN_WIDTH + gaps(groups);
    let flexible: f32 = spines.iter().map(|spine| share(spine)).sum();
    let scale = if fixed + flexible <= available || flexible <= 0. {
        1.
    } else {
        ((available - fixed) / flexible).max(0.)
    };
    Fit {
        widths: spines
            .iter()
            .map(|spine| SPINE_MIN_WIDTH + share(spine) * scale)
            .collect(),
        total: fixed + flexible * scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{RowKind, Session};

    /// Rows with these titles: a number for each `Some`.
    fn contents(rows: &[(&str, bool)], loose: &[&str]) -> Contents {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let file = std::env::temp_dir().join(format!("druck-shelf-{}-{unique}.md", std::process::id()));
        let text: String = (0..rows.len() + loose.len()).map(|at| format!("# {at}\n\n")).collect();
        std::fs::write(&file, text).expect("file");
        let ids = Session::open(&file).expect("opens").book().to_vec();
        std::fs::remove_file(&file).ok();
        let mut number = 0;
        let mut row = |at: usize, title: &str, numbered: bool| Row {
            id: ids[at],
            title: title.to_owned(),
            kind: if numbered {
                RowKind::Numbered
            } else {
                RowKind::Unnumbered
            },
            number: numbered.then(|| {
                number += 1;
                number
            }),
            broken: false,
        };
        Contents {
            book: rows
                .iter()
                .enumerate()
                .map(|(at, (title, numbered))| row(at, title, *numbered))
                .collect(),
            not_in_book: loose
                .iter()
                .enumerate()
                .map(|(at, title)| row(rows.len() + at, title, false))
                .collect(),
            ..Contents::default()
        }
    }

    fn labels(groups: &[Group]) -> Vec<(Option<&str>, Vec<&str>)> {
        (groups.iter())
            .map(|group| {
                (
                    group.label.as_deref(),
                    group.spines.iter().map(|s| s.title.as_str()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn matter_stands_before_and_after_the_body_and_a_part_opens_a_labelled_group() {
        let contents = contents(
            &[
                ("Title", false),
                ("Part I", false),
                ("Interlude", false),
                ("One", true),
                ("Two", true),
                ("Part II", false),
                ("Three", true),
                ("Notes", false),
            ],
            &["Old ending"],
        );
        let groups = groups(&contents, &HashMap::new());
        assert_eq!(
            labels(&groups),
            [
                (None, vec!["Title", "Part I", "Interlude"]),
                (None, vec!["One", "Two"]),
                (Some("Part II"), vec!["Part II", "Three"]),
                (None, vec!["Notes"]),
                (Some("Not in book"), vec!["Old ending"]),
            ]
        );
        assert!(groups[4].loose);
        assert!(groups[0].spines.iter().all(|spine| spine.matter));
        assert!(groups[1].spines.iter().all(|spine| !spine.matter));
    }

    #[test]
    fn a_book_with_no_numbered_chapter_has_no_matter() {
        let contents = contents(&[("A", false), ("B", false)], &[]);
        let groups = groups(&contents, &HashMap::new());
        assert_eq!(groups.len(), 1);
        assert!(groups[0].spines.iter().all(|spine| !spine.matter));
    }

    fn numbered_book(count: usize) -> Vec<Group> {
        let rows: Vec<_> = (0..count).map(|_| ("Chapter", true)).collect();
        let contents = contents(&rows, &[]);
        let words = contents.book.iter().map(|row| (row.id, 3000)).collect();
        groups(&contents, &words)
    }

    #[test]
    fn width_follows_the_words_and_never_goes_under_the_least() {
        let groups = numbered_book(2);
        let fitted = fit(&groups, 2000.);
        assert_eq!(fitted.widths[0], SPINE_MIN_WIDTH + 3000. * SPINE_WORD_WIDTH);
        let mut empty = groups.clone();
        empty[0].spines[0].words = None;
        assert_eq!(fit(&empty, 2000.).widths[0], SPINE_MIN_WIDTH);
    }

    #[test]
    fn forty_chapters_fit_the_least_window_by_sharing_the_shortage() {
        let groups = numbered_book(40);
        let fitted = fit(&groups, 696.);
        assert!(fitted.total <= 696.01, "{}", fitted.total);
        assert!(fitted.widths.iter().all(|width| *width >= SPINE_MIN_WIDTH));
        assert!(fitted.widths.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn a_row_that_cannot_fit_keeps_the_least_width_and_overflows() {
        let groups = numbered_book(80);
        let fitted = fit(&groups, 400.);
        assert!(fitted.widths.iter().all(|width| *width == SPINE_MIN_WIDTH));
        assert!(fitted.total > 400.);
    }
}
