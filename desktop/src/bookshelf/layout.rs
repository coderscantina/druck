//! Where the spines stand: which chapters make a group, and how wide each spine is. Plain data, no gpui, so the rules
//! have tests of their own.
//!
//! A spine is as wide as its chapter is long. The formula is [`SPINE_MIN_WIDTH`] plus [`SPINE_WORD_WIDTH`] per word, up
//! to [`SPINE_MAX_WIDTH`]. When the row would not fit the window, every spine's share of words shrinks by the same
//! factor, down to the least width: 40 chapters then stand in 720 px. Only below that does the row scroll.

use std::collections::HashMap;

use crate::navigator::book::{Book, Kind};
use crate::session::{ChapterId, Row};
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
    /// Front matter, back matter, a part's own file, an unnumbered chapter, and files not in the book have narrow spines
    /// with upright titles.
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

/// The groups of the shelf, in order, from the book's matter and parts as the path bar reads them: the front matter and
/// the back matter without a label and with narrow spines, each part labelled by the file that opens it, which stands
/// first as a narrow spine, and chapters before the first part without a label. Files not in the book stand last.
pub fn groups(book: &Book, not_in_book: &[Row], words: &HashMap<ChapterId, usize>) -> Vec<Group> {
    let mut groups: Vec<Group> = (book.groups.iter())
        .map(|group| {
            let matter = matches!(group.kind, Kind::FrontMatter | Kind::BackMatter);
            let opener = group.opener.iter().map(|entry| spine(&entry.row, words, true));
            let chapters =
                (group.entries.iter()).map(|entry| spine(&entry.row, words, matter || entry.row.number.is_none()));
            Group {
                label: group.title().map(str::to_owned),
                spines: opener.chain(chapters).collect(),
                loose: false,
            }
        })
        .collect();
    if !not_in_book.is_empty() {
        groups.push(Group {
            label: Some(crate::t!(super::strings::Key::NotInBook).to_owned()),
            // Files outside the book have no length that counts, so their spines are all the least width.
            spines: (not_in_book.iter())
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
    use crate::session::{Contents, RowKind, Session};

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

    /// The shelf's groups for chapters with these titles, numbers, and texts, and files not in the book.
    fn shelf(rows: &[(&str, bool, &str)], loose: &[&str], words: usize) -> Vec<Group> {
        let titles: Vec<(&str, bool)> = rows.iter().map(|(title, numbered, _)| (*title, *numbered)).collect();
        let contents = contents(&titles, loose);
        let texts: HashMap<ChapterId, &str> = (contents.book.iter())
            .zip(rows)
            .map(|(row, (_, _, text))| (row.id, *text))
            .collect();
        let book = crate::navigator::book::structure(&contents.book, &|id| texts[&id]);
        let words = contents.book.iter().map(|row| (row.id, words)).collect();
        groups(&book, &contents.not_in_book, &words)
    }

    #[test]
    fn matter_and_parts_are_the_path_bars_and_files_outside_the_book_stand_last() {
        let groups = shelf(
            &[
                (
                    "Title",
                    false,
                    "::: front-matter\n\n# The Salt Road {-}\n\nBy Ines Hartmann.\n",
                ),
                ("Part I", false, "::: main-matter\n\n# Part I {-}\n"),
                ("One", true, "# One\n\nText.\n"),
                ("Two", true, "# Two\n\nText.\n"),
                ("Part II", false, "# Part II {-}\n\n> A motto.\n"),
                ("Three", true, "# Three\n\nText.\n"),
                ("Notes", false, "::: back-matter\n\n# Notes {-}\n\nText.\n\nMore.\n"),
            ],
            &["Old ending"],
            0,
        );
        assert_eq!(
            labels(&groups),
            [
                (None, vec!["Title"]),
                (Some("Part I"), vec!["Part I", "One", "Two"]),
                (Some("Part II"), vec!["Part II", "Three"]),
                (None, vec!["Notes"]),
                (Some("Not in book"), vec!["Old ending"]),
            ]
        );
        assert!(groups[4].loose);
        assert!(groups[0].spines.iter().all(|spine| spine.matter));
        assert_eq!(
            groups[1].spines.iter().map(|spine| spine.matter).collect::<Vec<_>>(),
            [true, false, false]
        );
    }

    #[test]
    fn a_book_without_parts_or_matter_is_one_group_of_chapters() {
        let groups = shelf(&[("A", true, "# A\n\nText.\n"), ("B", true, "# B\n\nText.\n")], &[], 0);
        assert_eq!(labels(&groups), [(None, vec!["A", "B"])]);
        assert!(groups[0].spines.iter().all(|spine| !spine.matter));
    }

    fn numbered_book(count: usize) -> Vec<Group> {
        let rows: Vec<_> = (0..count).map(|_| ("Chapter", true, "# Chapter\n\nText.\n")).collect();
        shelf(&rows, &[], 3000)
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
