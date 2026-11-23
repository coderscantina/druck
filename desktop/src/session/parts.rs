//! The book's parts as the Binder shows them, where an added page goes, and keeping `::: front-matter`,
//! `::: main-matter`, and `::: back-matter` in step with the order of the pages. Pure functions over the texts of the
//! book's sections in reading order. Holds no gpui types.

use std::ops::Range;

use super::outline::{self, Kind, PARTS, depths, directive_name};
use crate::source::{BlockKind, Blocks, Edit};

/// The four groups the Binder's rows stand in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Matter {
    /// Before the first part: title page, copyright, dedication.
    Opening,
    Front,
    Main,
    Back,
}

/// Where a page goes in a book, as books have it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Among the opening pages, after those of a lower rank: title page 0, copyright 1, dedication 2, any other 3.
    Opening(u8),
    /// After the opening pages, before the first page with a heading.
    Contents,
    /// At the end, before an index or bibliography there.
    Back,
    /// Last.
    End,
}

/// What the rules read of a section.
struct Page<'a> {
    text: &'a str,
    kind: Kind,
    /// It holds a page Druck writes, alone or with others.
    made: bool,
    /// Top-level blocks: the part a part line starts, or `None` for a block a page shows.
    marks: Vec<(Option<usize>, Range<usize>)>,
}

fn read(text: &str) -> Page<'_> {
    let blocks = Blocks::parse(text);
    let list = blocks.as_slice();
    let depth = depths(text, list);
    let mut made = false;
    let mut marks = Vec::new();
    for (block, depth) in list.iter().zip(depth) {
        if depth > 0
            || matches!(
                block.kind,
                BlockKind::Blank | BlockKind::Comment | BlockKind::FrontMatter
            )
        {
            continue;
        }
        let name = match block.kind {
            BlockKind::Directive => directive_name(&text[block.range.clone()]),
            _ => "",
        };
        made |= matches!(name, "toc" | "index" | "bibliography");
        match PARTS.iter().position(|part| *part == name) {
            Some(part) => marks.push((Some(part), block.range.clone())),
            None if name == "page-break" => {}
            None => marks.push((None, block.range.clone())),
        }
    }
    Page {
        text,
        kind: outline::outline(text).kind,
        made,
        marks,
    }
}

/// Where the opening pages end: at the first page with a heading or one Druck writes.
fn opening_end(pages: &[Page]) -> usize {
    (pages.iter())
        .position(|page| page.kind.has_heading() || page.made)
        .unwrap_or(pages.len())
}

/// The group of each section: the part in force where its first block a page shows stands. A book without part lines
/// has its opening pages and the rest.
pub fn matters(sections: &[&str]) -> Vec<Matter> {
    let pages: Vec<Page> = sections.iter().map(|text| read(text)).collect();
    if !pages
        .iter()
        .flat_map(|page| &page.marks)
        .any(|(part, _)| part.is_some())
    {
        let end = opening_end(&pages);
        return (0..pages.len())
            .map(|at| if at < end { Matter::Opening } else { Matter::Main })
            .collect();
    }
    let parts = [Matter::Front, Matter::Main, Matter::Back];
    let mut current = Matter::Opening;
    pages
        .iter()
        .map(|page| {
            let mut own = None;
            for (part, _) in &page.marks {
                match part {
                    Some(part) => current = parts[*part],
                    None => own = own.or(Some(current)),
                }
            }
            own.unwrap_or(current)
        })
        .collect()
}

/// Where a page of `place` goes among the sections, as an index to insert it at.
pub fn place(place: Place, sections: &[&str]) -> usize {
    let pages: Vec<Page> = sections.iter().map(|text| read(text)).collect();
    let end = opening_end(&pages);
    let made_at_end = |kind: Kind| matches!(kind, Kind::Index | Kind::Bibliography);
    match place {
        Place::Opening(rank) => {
            let rank_of = |kind: Kind| match kind {
                Kind::TitlePage => 0,
                Kind::Copyright => 1,
                Kind::Dedication => 2,
                _ => 3,
            };
            (0..end).find(|at| rank_of(pages[*at].kind) > rank).unwrap_or(end)
        }
        Place::Contents => (pages.iter())
            .position(|page| page.kind.has_heading() || made_at_end(page.kind))
            .unwrap_or(pages.len()),
        Place::Back => {
            let mut at = pages.len();
            while at > 0 && made_at_end(pages[at - 1].kind) {
                at -= 1;
            }
            at
        }
        Place::End => pages.len(),
    }
}

/// The edits, by section, that make the part lines agree with the order of the pages, for a book that uses parts:
/// front matter starts at the first page with a heading or one Druck writes after the opening pages, main matter at
/// the first numbered chapter, and back matter at the first page after the last numbered chapter. A part line already
/// in place stays, even inside a section of several pages. A book without part lines gets none.
pub fn part_edits(sections: &[&str]) -> Vec<(usize, Edit)> {
    let pages: Vec<Page> = sections.iter().map(|text| read(text)).collect();
    if !pages
        .iter()
        .flat_map(|page| &page.marks)
        .any(|(part, _)| part.is_some())
    {
        return Vec::new();
    }
    let main = pages.iter().position(|page| page.kind == Kind::Numbered);
    let front = Some(opening_end(&pages)).filter(|front| *front < pages.len() && main.is_none_or(|main| *front < main));
    let back = (pages.iter().rposition(|page| page.kind == Kind::Numbered))
        .map(|last| last + 1)
        .filter(|back| *back < pages.len());
    // The section of the first block a page shows after each part line.
    let next_shown = |section: usize, line: usize| {
        let later = pages[section].marks[line + 1..].iter().any(|(part, _)| part.is_none());
        if later {
            return Some(section);
        }
        (section + 1..pages.len()).find(|at| pages[*at].marks.iter().any(|(part, _)| part.is_none()))
    };
    let mut edits = Vec::new();
    for (part, target) in [front, main, back].into_iter().enumerate() {
        let lines: Vec<(usize, usize)> = (pages.iter().enumerate())
            .flat_map(|(section, page)| {
                (page.marks.iter().enumerate())
                    .filter(move |(_, (found, _))| *found == Some(part))
                    .map(move |(line, _)| (section, line))
            })
            .collect();
        let kept = target.and_then(|target| {
            (lines.iter().copied()).find(|(section, line)| next_shown(*section, *line) == Some(target))
        });
        for (section, line) in lines.iter().copied().filter(|line| Some(*line) != kept) {
            edits.push((
                section,
                Edit::new(with_blank(pages[section].text, &pages[section].marks[line].1), ""),
            ));
        }
        if let (Some(target), None) = (target, kept) {
            let at = after_settings(pages[target].text);
            edits.push((target, Edit::new(at..at, format!("::: {}\n\n", PARTS[part]))));
        }
    }
    edits.sort_by_key(|(section, edit)| (*section, edit.range.start, edit.range.end));
    edits
}

/// A line's range with the blank lines after it.
fn with_blank(text: &str, line: &Range<usize>) -> Range<usize> {
    let rest = &text[line.end..];
    line.start..line.end + (rest.len() - rest.trim_start_matches(['\n', '\r']).len())
}

/// Where a section's text starts after its settings and the blank lines after them.
pub fn after_settings(text: &str) -> usize {
    let blocks = Blocks::parse(text);
    let list = blocks.as_slice();
    match list.first() {
        Some(first) if first.kind == BlockKind::FrontMatter => (list.iter().skip(1))
            .find(|block| block.kind != BlockKind::Blank)
            .map_or(text.len(), |block| block.range.start),
        _ => 0,
    }
}

/// A page's Markdown with the page breaks it needs to stand on a page of its own between `before` and `after`, the
/// book's text on either side. A page that opens with a level 1 heading starts its page itself. Part lines count for
/// neither side, since keeping the parts in step may move them, and a page break beside one sets no blank page.
pub fn on_its_own_page(markdown: &str, before: &str, after: &str) -> String {
    if starts_with_heading(markdown) {
        return markdown.to_owned();
    }
    let passed = |text: &str, block: &crate::source::blocks::Block| match block.kind {
        BlockKind::Blank | BlockKind::Comment => true,
        BlockKind::Directive => PARTS.contains(&directive_name(&text[block.range.clone()])),
        _ => false,
    };
    let ended = {
        let blocks = Blocks::parse(before);
        let list = blocks.as_slice();
        (list.iter().enumerate().rev())
            .find(|(_, block)| !passed(before, block))
            .is_none_or(|(index, block)| match block.kind {
                BlockKind::FrontMatter => true,
                BlockKind::Directive => {
                    directive_name(&before[block.range.clone()]) == "page-break"
                        || outline::closes_bottom(before, list, index)
                }
                _ => false,
            })
    };
    let starts = {
        let blocks = Blocks::parse(after);
        (blocks.as_slice().iter())
            .find(|block| !passed(after, block))
            .is_none_or(|block| match block.kind {
                BlockKind::Heading { level: 1 } => true,
                BlockKind::Directive => directive_name(&after[block.range.clone()]) == "page-break",
                _ => false,
            })
    };
    let mut page = String::new();
    if !ended {
        page.push_str("::: page-break\n\n");
    }
    page.push_str(markdown.trim_end());
    page.push('\n');
    if !starts {
        page.push_str("\n::: page-break\n");
    }
    page
}

/// Whether a text's first block, past blank lines and comments, is a level 1 heading.
fn starts_with_heading(text: &str) -> bool {
    let blocks = Blocks::parse(text);
    (blocks.as_slice().iter())
        .find(|block| !matches!(block.kind, BlockKind::Blank | BlockKind::Comment))
        .is_some_and(|block| block.kind == BlockKind::Heading { level: 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The texts after applying edits by section.
    fn applied(sections: &[&str], edits: &[(usize, Edit)]) -> Vec<String> {
        (sections.iter().enumerate())
            .map(|(at, text)| {
                let mut text = (*text).to_owned();
                for (_, edit) in edits.iter().filter(|(section, _)| *section == at).rev() {
                    text.replace_range(edit.range.clone(), &edit.text);
                }
                text
            })
            .collect()
    }

    const BOOK: [&str; 6] = [
        "::: bottom\nImprint\n:::\n\n",
        "For you. {.dedication}\n\n::: front-matter\n\n",
        "# Preface {-}\n\nText\n\n",
        "::: page-break\n\n::: toc\n\n::: main-matter\n\n",
        "# One\n\nText\n\n::: back-matter\n\n",
        "# About {-}\n\nText\n",
    ];

    #[test]
    fn pages_in_place_need_no_edits_and_group_by_their_parts() {
        assert!(part_edits(&BOOK).is_empty());
        use Matter::*;
        assert_eq!(matters(&BOOK), [Opening, Opening, Front, Front, Main, Back]);
    }

    #[test]
    fn a_book_without_parts_is_left_alone_and_groups_its_opening_pages() {
        let book = ["::: bottom\nA\n:::\n", "# One\n", "# Two\n"];
        assert!(part_edits(&book).is_empty());
        assert_eq!(matters(&book), [Matter::Opening, Matter::Main, Matter::Main]);
    }

    #[test]
    fn a_moved_chapter_takes_the_part_lines_with_the_order() {
        // About the author moved before the only chapter: no back matter is left, and main matter starts at the chapter.
        let book = [BOOK[0], BOOK[1], BOOK[2], BOOK[3], BOOK[5], BOOK[4]];
        let fixed = applied(&book, &part_edits(&book));
        assert_eq!(fixed[3], "::: page-break\n\n::: toc\n\n");
        assert_eq!(fixed[5], "::: main-matter\n\n# One\n\nText\n\n");
        assert!(!fixed.concat().contains("back-matter"));
        assert!(part_edits(&fixed.iter().map(String::as_str).collect::<Vec<_>>()).is_empty());
    }

    #[test]
    fn a_page_after_the_last_chapter_opens_the_back_matter() {
        let book = ["::: main-matter\n\n# One\n\n", "# Thanks {-}\n"];
        let fixed = applied(&book, &part_edits(&book));
        assert_eq!(fixed[1], "::: back-matter\n\n# Thanks {-}\n");
    }

    #[test]
    fn a_part_line_in_a_section_of_several_pages_stays_where_it_works() {
        let front = "::: bottom\nA\n:::\n\nFor you.\n\n::: front-matter\n\n::: toc\n\n::: main-matter\n";
        assert!(part_edits(&[front, "# One\n", "# Two\n"]).is_empty());
    }

    #[test]
    fn added_pages_go_where_books_have_them() {
        assert_eq!(place(Place::Opening(0), &BOOK), 0);
        assert_eq!(place(Place::Opening(1), &BOOK), 1);
        assert_eq!(place(Place::Opening(3), &BOOK), 2);
        let without_contents = [BOOK[0], BOOK[1], BOOK[2], BOOK[4], BOOK[5], "::: index\n"];
        assert_eq!(place(Place::Contents, &without_contents), 2);
        assert_eq!(place(Place::Back, &without_contents), 5);
        assert_eq!(place(Place::End, &without_contents), 6);
    }

    #[test]
    fn an_added_page_gets_the_page_breaks_it_needs() {
        assert_eq!(
            on_its_own_page("For you.\n", "Text\n\n", "# One\n"),
            "::: page-break\n\nFor you.\n"
        );
        let after_a_part = on_its_own_page(
            "Motto\n",
            "For you.\n\n::: front-matter\n\n",
            "::: main-matter\n\n# One\n",
        );
        assert_eq!(after_a_part, "::: page-break\n\nMotto\n");
        assert_eq!(
            on_its_own_page("For you.\n", "::: bottom\nA\n:::\n\n", "Motto\n"),
            "For you.\n\n::: page-break\n"
        );
        assert_eq!(on_its_own_page("# Thanks {-}\n", "Text\n", "More\n"), "# Thanks {-}\n");
    }
}
