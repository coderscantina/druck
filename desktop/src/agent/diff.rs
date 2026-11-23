//! A small line diff, for showing what a tool is about to write.

use std::ops::Range;

/// Beyond this many lines on both sides of what changed, the diff shows the middle as one replacement instead of
/// finding the longest common run, which grows with the product of the two sizes.
const MAX_MIDDLE: usize = 2_000;
/// Unchanged lines kept around a change.
const CONTEXT: usize = 2;
/// A diff longer than this many lines is cut, and says so.
const MAX_LINES: usize = 120;
/// Characters of a line kept on each side of what changed in it, and the most kept of a line with no pair.
const MARGIN: usize = 36;
const LINE_LIMIT: usize = 140;
/// The most kept of an unchanged line.
const CONTEXT_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// A line with its number in the file: the new one, or the old one for a removed line. A long line is cut to the
    /// words around its change, and `emphasis` is where in `text` the change is.
    Line {
        mark: Mark,
        number: usize,
        text: String,
        emphasis: Option<Range<usize>>,
    },
    /// Lines left out.
    Gap(usize),
}

/// The lines of `text`, without their line breaks. A trailing line break ends the last line.
pub fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line))
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// The lines of a unified diff, numbered from its hunk headers, and cut as [`diff`] cuts them. Anything else in it, such
/// as a file header, is left out.
pub fn unified(patch: &str) -> Vec<DiffLine> {
    let mut shown = unified_whole(patch);
    for hunk in shown.split_mut(|line| matches!(line, DiffLine::Gap(_))) {
        let marks: Vec<(Mark, String)> = (hunk.iter())
            .filter_map(|line| match line {
                DiffLine::Line { mark, text, .. } => Some((*mark, text.clone())),
                DiffLine::Gap(_) => None,
            })
            .collect();
        let marks: Vec<(Mark, &str)> = marks.iter().map(|(mark, text)| (*mark, text.as_str())).collect();
        for (line, (_, cut, span)) in hunk.iter_mut().zip(shorten(&marks)) {
            if let DiffLine::Line { text, emphasis, .. } = line {
                (*text, *emphasis) = (cut, span);
            }
        }
    }
    shown
}

/// The lines of a unified diff, whole.
fn unified_whole(patch: &str) -> Vec<DiffLine> {
    let (mut old, mut new) = (0, 0);
    let mut shown = Vec::new();
    for line in lines(patch) {
        if let Some(header) = line.strip_prefix("@@ -") {
            let start = |side: &str| side.split(',').next().and_then(|start| start.parse::<usize>().ok());
            let mut sides = header.split(" +");
            old = sides.next().and_then(start).unwrap_or(1);
            let next = (sides.next().and_then(|side| side.split(' ').next()).and_then(start)).unwrap_or(1);
            if !shown.is_empty() {
                shown.push(DiffLine::Gap(next.saturating_sub(new)));
            }
            new = next;
            continue;
        }
        let (mark, number, text) = match line.split_at_checked(1) {
            Some(("+", text)) if new > 0 => (Mark::Added, new, text),
            Some(("-", text)) if old > 0 => (Mark::Removed, old, text),
            Some((" ", text)) if new > 0 => (Mark::Context, new, text),
            _ => continue,
        };
        old += usize::from(mark != Mark::Added);
        new += usize::from(mark != Mark::Removed);
        shown.push(DiffLine::Line {
            mark,
            number,
            text: text.to_owned(),
            emphasis: None,
        });
    }
    shown
}

/// The difference between two texts line by line, with two lines of context around each change.
pub fn diff(old: &str, new: &str) -> Vec<DiffLine> {
    let (old, new) = (lines(old), lines(new));
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_middle = &old[prefix..old.len() - suffix];
    let new_middle = &new[prefix..new.len() - suffix];
    let mut marks: Vec<(Mark, &str)> = old[..prefix].iter().map(|line| (Mark::Context, *line)).collect();
    marks.extend(changes(old_middle, new_middle));
    marks.extend(old[old.len() - suffix..].iter().map(|line| (Mark::Context, *line)));
    number(&marks)
}

/// Removed and added lines between two runs that share nothing at their ends.
fn changes<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Mark, &'a str)> {
    if old.is_empty() || new.is_empty() || old.len() > MAX_MIDDLE || new.len() > MAX_MIDDLE {
        return old
            .iter()
            .map(|line| (Mark::Removed, *line))
            .chain(new.iter().map(|line| (Mark::Added, *line)))
            .collect();
    }
    // lengths[i][j]: the longest common run of old[i..] and new[j..].
    let width = new.len() + 1;
    let mut lengths = vec![0u16; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lengths[i * width + j] = if old[i] == new[j] {
                lengths[(i + 1) * width + j + 1] + 1
            } else {
                lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            out.push((Mark::Context, old[i]));
            i += 1;
            j += 1;
        } else if lengths[(i + 1) * width + j] >= lengths[i * width + j + 1] {
            out.push((Mark::Removed, old[i]));
            i += 1;
        } else {
            out.push((Mark::Added, new[j]));
            j += 1;
        }
    }
    out.extend(old[i..].iter().map(|line| (Mark::Removed, *line)));
    out.extend(new[j..].iter().map(|line| (Mark::Added, *line)));
    out
}

/// The text of each line as a diff shows it, cut as [`diff`] cuts them, for a list of a hunk's lines.
pub fn cut(marks: &[(Mark, &str)]) -> Vec<String> {
    shorten(marks).into_iter().map(|(_, text, _)| text).collect()
}

/// Cuts each line down to what a reader needs: a removed line and the added line that replaces it to the words
/// around the difference, with that difference marked, and any other line to its start.
fn shorten(marks: &[(Mark, &str)]) -> Vec<(Mark, String, Option<Range<usize>>)> {
    let mut out: Vec<(Mark, String, Option<Range<usize>>)> = marks
        .iter()
        .map(|(mark, text)| {
            let limit = if *mark == Mark::Context {
                CONTEXT_LIMIT
            } else {
                LINE_LIMIT
            };
            (*mark, head(text, limit), None)
        })
        .collect();
    let mut at = 0;
    while at < marks.len() {
        if marks[at].0 != Mark::Removed {
            at += 1;
            continue;
        }
        let removed = marks[at..]
            .iter()
            .take_while(|(mark, _)| *mark == Mark::Removed)
            .count();
        let added = marks[at + removed..]
            .iter()
            .take_while(|(mark, _)| *mark == Mark::Added)
            .count();
        if removed == added {
            for pair in 0..removed {
                let (old, new) = (marks[at + pair].1, marks[at + removed + pair].1);
                let (old_span, new_span) = changed_span(old, new);
                out[at + pair] = excerpt(Mark::Removed, old, old_span);
                out[at + removed + pair] = excerpt(Mark::Added, new, new_span);
            }
        }
        at += removed + added;
    }
    out
}

fn head(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}\u{2026}", &text[..end]),
        None => text.to_owned(),
    }
}

/// Where two lines differ, widened to whole words: what is left after their shared start and shared end, in bytes of
/// each.
fn changed_span(old: &str, new: &str) -> (Range<usize>, Range<usize>) {
    let word = |c: char| c.is_alphanumeric();
    let mut start = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let mut end = old[start..]
        .chars()
        .rev()
        .zip(new[start..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    if [&old[start..], &new[start..]]
        .iter()
        .any(|rest| rest.chars().next().is_some_and(word))
    {
        while let Some(previous) = old[..start].chars().next_back().filter(|c| word(*c)) {
            start -= previous.len_utf8();
        }
    }
    while let Some(first) = old[old.len() - end..].chars().next().filter(|c| word(*c)) {
        let before = |text: &str| text[start..text.len() - end].chars().next_back();
        if !(before(old).is_some_and(word) || before(new).is_some_and(word)) {
            break;
        }
        end -= first.len_utf8();
    }
    (start..old.len() - end, start..new.len() - end)
}

/// `text` cut to the margin around `span`, with the span's place in the result.
fn excerpt(mark: Mark, text: &str, span: Range<usize>) -> (Mark, String, Option<Range<usize>>) {
    let mut from = span.start;
    for _ in 0..MARGIN {
        match text[..from].char_indices().next_back() {
            Some((at, _)) => from = at,
            None => break,
        }
    }
    let to = text[span.end..]
        .char_indices()
        .nth(MARGIN)
        .map_or(text.len(), |(at, _)| span.end + at);
    let lead = if from > 0 { "\u{2026}" } else { "" };
    let tail = if to < text.len() { "\u{2026}" } else { "" };
    let shown = format!("{lead}{}{tail}", &text[from..to]);
    let shift = lead.len();
    let emphasis = (!span.is_empty()).then(|| span.start - from + shift..span.end - from + shift);
    (mark, shown, emphasis)
}

/// Numbers the lines, keeps context near changes only, and cuts a long result.
fn number(marks: &[(Mark, &str)]) -> Vec<DiffLine> {
    let shown = shorten(marks);
    let mut near = vec![false; marks.len()];
    for (at, (mark, _)) in marks.iter().enumerate() {
        if *mark != Mark::Context {
            let from = at.saturating_sub(CONTEXT);
            near[from..(at + CONTEXT + 1).min(marks.len())].fill(true);
        }
    }
    let (mut old_number, mut new_number) = (0, 0);
    let mut out = Vec::new();
    let mut skipped = 0;
    for (at, (mark, text, emphasis)) in shown.into_iter().enumerate() {
        let number = match mark {
            Mark::Removed => {
                old_number += 1;
                old_number
            }
            Mark::Added => {
                new_number += 1;
                new_number
            }
            Mark::Context => {
                old_number += 1;
                new_number += 1;
                new_number
            }
        };
        if !near[at] {
            skipped += 1;
            continue;
        }
        if skipped > 0 {
            out.push(DiffLine::Gap(skipped));
            skipped = 0;
        }
        out.push(DiffLine::Line {
            mark,
            number,
            text,
            emphasis,
        });
    }
    if skipped > 0
        && out
            .iter()
            .any(|line| matches!(line, DiffLine::Line { mark, .. } if *mark != Mark::Context))
    {
        out.push(DiffLine::Gap(skipped));
    }
    if out.len() > MAX_LINES {
        let left = out.len() - MAX_LINES;
        out.truncate(MAX_LINES);
        out.push(DiffLine::Gap(left));
    }
    out
}

/// What an `Edit` would change in `file`: the whole lines `old` sits in, replaced, with the lines around them. When
/// `old` is not in the file the diff shows the two strings alone.
pub fn edit(file: Option<&str>, old: &str, new: &str) -> Vec<DiffLine> {
    let Some((file, at)) = file
        .and_then(|file| file.find(old).map(|at| (file, at)))
        .filter(|_| !old.is_empty())
    else {
        return diff(old, new);
    };
    let mut start = file[..at].rfind('\n').map_or(0, |line_break| line_break + 1);
    let mut end = file[at + old.len()..]
        .find('\n')
        .map_or(file.len(), |line_break| at + old.len() + line_break + 1);
    // Context comes from the file, so extend the region by it on both sides.
    for _ in 0..CONTEXT {
        start = file[..start.saturating_sub(1)]
            .rfind('\n')
            .map_or(0, |line_break| line_break + 1);
        end = file[end..]
            .find('\n')
            .map_or(file.len(), |line_break| end + line_break + 1);
    }
    let mut replaced = file[start..at].to_owned();
    replaced.push_str(new);
    replaced.push_str(&file[at + old.len()..end]);
    let first_line = file[..start].matches('\n').count();
    let mut out = diff(&file[start..end], &replaced);
    // The diff numbers lines from the region. Move them to the file's.
    for line in &mut out {
        if let DiffLine::Line { number, .. } = line {
            *number += first_line;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(diff: &[DiffLine]) -> Vec<String> {
        diff.iter()
            .map(|line| match line {
                DiffLine::Line { mark, number, text, .. } => {
                    let sign = match mark {
                        Mark::Context => ' ',
                        Mark::Added => '+',
                        Mark::Removed => '-',
                    };
                    format!("{sign}{number} {text}")
                }
                DiffLine::Gap(count) => format!("..{count}"),
            })
            .collect()
    }

    #[test]
    fn a_changed_line_is_removed_and_added_between_its_neighbours() {
        let out = diff("a\nb\nc\nd\ne\nf\ng\n", "a\nb\nc\nD\ne\nf\ng\n");
        assert_eq!(
            rows(&out),
            ["..1", " 2 b", " 3 c", "-4 d", "+4 D", " 5 e", " 6 f", "..1"]
        );
    }

    #[test]
    fn a_new_file_is_all_added() {
        assert_eq!(rows(&diff("", "x\ny\n")), ["+1 x", "+2 y"]);
    }

    #[test]
    fn an_edit_shows_the_whole_lines_and_the_file_numbers() {
        let file = "# Notes\n\nHello world.\n\nA second line.\n";
        let out = edit(Some(file), "Hello", "Howdy");
        assert_eq!(
            rows(&out),
            [
                " 1 # Notes",
                " 2 ",
                "-3 Hello world.",
                "+3 Howdy world.",
                " 4 ",
                " 5 A second line."
            ]
        );
    }

    #[test]
    fn an_edit_of_text_the_file_lacks_shows_the_strings() {
        assert_eq!(rows(&edit(Some("x\n"), "gone", "here")), ["-1 gone", "+1 here"]);
        assert_eq!(rows(&edit(None, "a", "b")), ["-1 a", "+1 b"]);
    }

    #[test]
    fn a_hunks_long_lines_are_cut_around_their_change() {
        let old = format!("{} Amiga 500 {}", "x".repeat(90), "y".repeat(90));
        let new = old.replace("Amiga 500", "Atari ST");
        let marks = [
            (Mark::Context, "Before."),
            (Mark::Removed, old.as_str()),
            (Mark::Added, new.as_str()),
        ];
        let cut = cut(&marks);
        assert_eq!(cut[0], "Before.");
        assert!(
            cut[1].contains("Amiga 500") && cut[1].starts_with('\u{2026}'),
            "{}",
            cut[1]
        );
        assert!(cut[2].contains("Atari ST") && cut[2].len() < new.len(), "{}", cut[2]);
    }

    #[test]
    fn a_long_line_is_cut_to_the_words_around_its_change() {
        let old = format!("{} Amiga 500 {}\n", "x".repeat(90), "y".repeat(90));
        let new = old.replace("Amiga 500", "Atari ST");
        let out = diff(&old, &new);
        let [
            DiffLine::Line {
                text: removed,
                emphasis: Some(old_span),
                ..
            },
            DiffLine::Line {
                text: added,
                emphasis: Some(new_span),
                ..
            },
        ] = &out[..]
        else {
            panic!("a pair of lines: {out:?}")
        };
        assert_eq!(&removed[old_span.clone()], "Amiga 500");
        assert_eq!(&added[new_span.clone()], "Atari ST");
        assert!(removed.starts_with('\u{2026}') && removed.ends_with('\u{2026}') && removed.chars().count() < 100);
    }

    #[test]
    fn a_long_diff_is_cut_and_says_so() {
        let old = (0..400).map(|n| format!("old {n}\n")).collect::<String>();
        let out = diff(&old, "");
        assert_eq!(out.len(), MAX_LINES + 1);
        assert_eq!(out.last(), Some(&DiffLine::Gap(400 - MAX_LINES)));
    }

    #[test]
    fn a_long_line_of_a_unified_diff_is_cut_around_its_change() {
        let old = format!("{} Amiga 500 {}", "x".repeat(90), "y".repeat(90));
        let new = old.replace("Amiga 500", "Atari ST");
        let shown = unified(&format!("@@ -7 +7 @@\n-{old}\n+{new}\n"));
        let [
            DiffLine::Line {
                number: 7,
                text: removed,
                emphasis: Some(span),
                ..
            },
            DiffLine::Line { text: added, .. },
        ] = &shown[..]
        else {
            panic!("a pair of lines: {shown:?}")
        };
        assert_eq!(&removed[span.clone()], "Amiga 500");
        assert!(removed.starts_with('\u{2026}') && removed.chars().count() < 100);
        assert!(added.contains("Atari ST") && added.chars().count() < 100);
    }

    #[test]
    fn a_unified_diff_keeps_its_line_numbers() {
        let shown = unified("--- a/a.md\n+++ b/a.md\n@@ -3,2 +3,2 @@\n same\n-Hello world.\n+Howdy world.\n");
        let brief: Vec<_> = (shown.iter())
            .map(|line| match line {
                DiffLine::Line { mark, number, text, .. } => (*mark, *number, text.as_str()),
                DiffLine::Gap(_) => panic!("one hunk has no gap"),
            })
            .collect();
        assert_eq!(
            brief,
            [
                (Mark::Context, 3, "same"),
                (Mark::Removed, 4, "Hello world."),
                (Mark::Added, 4, "Howdy world."),
            ]
        );
    }
}
