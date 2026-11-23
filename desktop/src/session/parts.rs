//! The book's parts as the Binder shows them, where an added page goes, and keeping `::: front-matter`,
//! `::: main-matter`, and `::: back-matter` with the pages they open when the Binder changes the book. Pure functions
//! over the texts of the book's sections in reading order. Holds no gpui types.

use std::collections::BTreeMap;
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
struct Page {
    kind: Kind,
    /// It holds a page Druck writes, alone or with others.
    made: bool,
    /// Top-level blocks: the part a part line starts, or `None` for a block a page shows.
    marks: Vec<(Option<usize>, Range<usize>)>,
}

fn read(text: &str) -> Page {
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

/// What a part line opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens<Id> {
    /// A section's first page: the line stands before the section's first block a page shows, in that section or at
    /// the end of one before it.
    Page(Id),
    /// A later page of the section the line stands in, so it goes wherever the section goes.
    Within(Id),
    /// Nothing, as no page follows the line.
    Nothing,
}

/// A part line: the part it starts, as an index into [`PARTS`], and what it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line<Id> {
    pub part: usize,
    pub opens: Opens<Id>,
}

/// A part line where it stands, by the index of its section.
struct Found<Id> {
    section: usize,
    range: Range<usize>,
    line: Line<Id>,
}

/// A section as the part lines see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Section<Id> {
    id: Id,
    /// Where its first block a page shows starts, if it has one.
    shown: Option<usize>,
    /// The part in force at that block, or at the section's end: the last part line before it.
    part: Option<usize>,
}

/// The part lines of a book and its sections.
struct Layout<Id> {
    lines: Vec<Found<Id>>,
    sections: Vec<Section<Id>>,
}

fn layout<Id: Copy>(book: &[(Id, &str)]) -> Layout<Id> {
    let mut lines: Vec<Found<Id>> = Vec::new();
    let mut sections = Vec::with_capacity(book.len());
    let mut waiting = 0;
    for (section, (id, text)) in book.iter().enumerate() {
        let mut shown = None;
        let mut part_there = None;
        for (part, range) in read(text).marks {
            let Some(part) = part else {
                let opens = match shown {
                    None => Opens::Page(*id),
                    Some(_) => Opens::Within(*id),
                };
                if shown.is_none() {
                    shown = Some(range.start);
                    part_there = Some(lines.last().map(|found| found.line.part));
                }
                for found in &mut lines[waiting..] {
                    found.line.opens = opens;
                }
                waiting = lines.len();
                continue;
            };
            lines.push(Found {
                section,
                range,
                line: Line {
                    part,
                    opens: Opens::Nothing,
                },
            });
        }
        sections.push(Section {
            id: *id,
            shown,
            part: part_there.unwrap_or_else(|| lines.last().map(|found| found.line.part)),
        });
    }
    Layout { lines, sections }
}

/// Whether a page added at `place` goes after a part line of `part` that stands where it goes, so the line opens it:
/// opening pages stand before all parts, the contents in front matter, and pages at the end in the last part.
fn after_line(place: Place, part: usize) -> bool {
    match place {
        Place::Opening(_) => false,
        Place::Contents => part == 0,
        Place::Back | Place::End => true,
    }
}

/// A book's part lines and its sections, read before a structural action changes them. Each part line stays with the
/// page it opens: only an action that moves or removes that page, or puts one before it, carries the line, and no
/// action writes a line the book did not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parts<Id> {
    pub lines: Vec<Line<Id>>,
    sections: Vec<Section<Id>>,
}

impl<Id: Copy + Eq> Parts<Id> {
    pub fn read(book: &[(Id, &str)]) -> Self {
        let layout = layout(book);
        Self {
            lines: layout.lines.into_iter().map(|found| found.line).collect(),
            sections: layout.sections,
        }
    }

    fn index(&self, id: Id) -> Option<usize> {
        self.sections.iter().position(|section| section.id == id)
    }

    /// The first section that shows a page from place `at` on, leaving out `but`.
    fn page_from(&self, at: usize, but: Option<Id>) -> Opens<Id> {
        (self.sections.iter().skip(at))
            .find(|section| section.shown.is_some() && Some(section.id) != but)
            .map_or(Opens::Nothing, |section| Opens::Page(section.id))
    }

    /// The lines with those that open section `id`, and with `within` those inside it, passed to the page after it. A
    /// line with no page after it is dropped, as its part would be empty.
    fn carried(&self, id: Id, within: bool) -> Vec<Line<Id>> {
        let next = self.index(id).map_or(Opens::Nothing, |at| self.page_from(at + 1, None));
        (self.lines.iter())
            .filter_map(|line| match line.opens {
                Opens::Page(page) | Opens::Within(page)
                    if page == id && (within || line.opens == Opens::Page(page)) =>
                {
                    (next != Opens::Nothing).then_some(Line {
                        part: line.part,
                        opens: next,
                    })
                }
                _ => Some(*line),
            })
            .collect()
    }

    /// The lines wanted after section `id` moved before section `before`, or last. A line that opened it opens the
    /// page that followed it. Where it lands, the lines there of its own part and those before open it, so a page
    /// dropped at the edge of a part stays in its part.
    pub fn moved(&self, id: Id, before: Option<Id>) -> Vec<Line<Id>> {
        let Some(section) = self.index(id).map(|at| self.sections[at]) else {
            return self.lines.clone();
        };
        let at = before
            .and_then(|before| self.index(before))
            .unwrap_or(self.sections.len());
        let there = self.page_from(at, Some(id));
        let joins = |line: &Line<Id>| section.shown.is_some() && section.part.is_some_and(|part| line.part <= part);
        (self.carried(id, false).into_iter())
            .map(|line| match line.opens == there && joins(&line) {
                true => Line {
                    part: line.part,
                    opens: Opens::Page(id),
                },
                false => line,
            })
            .collect()
    }

    /// The lines wanted after section `id` left the book: those that opened it or stood inside it open the page that
    /// followed it.
    pub fn removed(&self, id: Id) -> Vec<Line<Id>> {
        self.carried(id, true)
    }

    /// The lines wanted after page `new` was added at `place`, before section `before` or last: a line that opened
    /// the page there opens the new one when the new page goes after it.
    pub fn added(&self, before: Option<Id>, place: Place, new: Id) -> Vec<Line<Id>> {
        let at = before
            .and_then(|before| self.index(before))
            .unwrap_or(self.sections.len());
        let there = self.page_from(at, None);
        (self.lines.iter())
            .map(|line| match line.opens == there && after_line(place, line.part) {
                true => Line {
                    part: line.part,
                    opens: Opens::Page(new),
                },
                false => *line,
            })
            .collect()
    }

    /// Whether `lines` start the same parts as these, so lines kept from an earlier state of the book still apply.
    pub fn same_parts(&self, lines: &[Line<Id>]) -> bool {
        let parts = |lines: &[Line<Id>]| {
            let mut parts: Vec<usize> = lines.iter().map(|line| line.part).collect();
            parts.sort_unstable();
            parts
        };
        parts(&self.lines) == parts(lines)
    }
}

/// The edits, by section, that make the book's part lines open the pages `wanted` names. A line already where it is
/// wanted stays. Any other is taken out, also from `left`, the sections that left the book, and a wanted line is
/// written at the start of the page it opens, in part order with the lines there. A line whose page no longer shows
/// anything opens the next page that does, so no line goes into an empty or missing file.
pub fn part_edits<Id: Copy + Eq>(book: &[(Id, &str)], left: &[(Id, &str)], wanted: &[Line<Id>]) -> Vec<(Id, Edit)> {
    let layout = layout(book);
    let index = |id: Id| book.iter().position(|(other, _)| *other == id);
    let resolve = |opens: Opens<Id>| match opens {
        Opens::Page(page) => (index(page)?..book.len())
            .find(|at| layout.sections[*at].shown.is_some())
            .map(|at| Opens::Page(book[at].0)),
        Opens::Within(page) => index(page).map(|_| opens),
        Opens::Nothing => Some(opens),
    };
    let mut stays = vec![false; layout.lines.len()];
    let mut missing = Vec::new();
    for line in wanted {
        let Some(opens) = resolve(line.opens) else {
            continue;
        };
        let line = Line { part: line.part, opens };
        match (layout.lines.iter().enumerate()).find(|(at, found)| !stays[*at] && found.line == line) {
            Some((at, _)) => stays[at] = true,
            None => missing.push(line),
        }
    }
    let mut edits: Vec<(usize, Edit)> = Vec::new();
    for (found, _) in (layout.lines.iter().zip(&stays)).filter(|(_, stays)| !**stays) {
        let text = book[found.section].1;
        edits.push((found.section, Edit::new(with_blank(text, &found.range), "")));
    }
    for (at, (_, text)) in left.iter().enumerate() {
        for (_, range) in read(text).marks.into_iter().filter(|(part, _)| part.is_some()) {
            edits.push((book.len() + at, Edit::new(with_blank(text, &range), "")));
        }
    }
    let mut inserts: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for line in missing {
        let later = (layout.lines.iter().zip(&stays))
            .find(|(found, stays)| **stays && found.line.opens == line.opens && found.line.part > line.part);
        let at = match (later, line.opens) {
            (Some((found, _)), _) => (found.section, found.range.start),
            (None, Opens::Page(page)) => {
                let section = index(page).expect("a resolved page is in the book");
                (
                    section,
                    layout.sections[section].shown.expect("a resolved page shows something"),
                )
            }
            (None, Opens::Nothing) => match (0..book.len()).rev().find(|at| layout.sections[*at].shown.is_some()) {
                Some(section) => (section, book[section].1.len()),
                None => continue,
            },
            // A line inside a section goes with it, so only the author's own change of the text misses one.
            (None, Opens::Within(_)) => continue,
        };
        inserts.entry(at).or_default().push(line.part);
    }
    for ((section, offset), mut parts) in inserts {
        parts.sort_unstable();
        let lines: Vec<String> = parts.iter().map(|part| format!("::: {}\n", PARTS[*part])).collect();
        let lines = lines.join("\n");
        let text = book[section].1;
        let written = if offset < text.len() {
            lines + "\n"
        } else if text.is_empty() || text.ends_with("\n\n") {
            lines
        } else if text.ends_with('\n') {
            format!("\n{lines}")
        } else {
            format!("\n\n{lines}")
        };
        edits.push((section, Edit::new(offset..offset, written)));
    }
    edits.sort_by_key(|(section, edit)| (*section, edit.range.start, edit.range.end));
    (edits.into_iter())
        .map(|(section, edit)| {
            let id = match book.get(section) {
                Some((id, _)) => *id,
                None => left[section - book.len()].0,
            };
            (id, edit)
        })
        .collect()
}

/// Where a page added at `place` before section `at` of a single file goes, as a section and an offset, when part
/// lines stand between the pages on either side, so each line keeps opening the page it opened: before the lines for
/// an opening page, after front matter's for the contents, and after all of them for a page at the end.
pub fn in_gap(sections: &[&str], at: usize, place: Place) -> Option<(usize, usize)> {
    let book: Vec<(usize, &str)> = sections.iter().copied().enumerate().collect();
    let layout = layout(&book);
    let there = (at..sections.len())
        .find(|section| layout.sections[*section].shown.is_some())
        .map_or(Opens::Nothing, Opens::Page);
    let gap: Vec<&Found<usize>> = (layout.lines.iter())
        .filter(|found| found.line.opens == there)
        .collect();
    let before = gap.iter().filter(|found| after_line(place, found.line.part)).count();
    match before.checked_sub(1) {
        None => gap.first().map(|found| (found.section, found.range.start)),
        Some(last) => {
            let found = gap[last];
            Some((found.section, with_blank(sections[found.section], &found.range).end))
        }
    }
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

    /// A book's part lines, its sections named by their index.
    fn parts(book: &[&str]) -> Parts<usize> {
        Parts::read(&book.iter().copied().enumerate().collect::<Vec<_>>())
    }

    /// The edits that keep the part lines as `wanted` says once the sections of `book` stand in `order`, by index,
    /// and those of `left` left the book.
    fn edits(book: &[&str], order: &[usize], left: &[usize], wanted: &[Line<usize>]) -> Vec<(usize, Edit)> {
        let entries = |ids: &[usize]| ids.iter().map(|id| (*id, book[*id])).collect::<Vec<_>>();
        part_edits(&entries(order), &entries(left), wanted)
    }

    /// The texts of `book`'s sections after the edits.
    fn kept(book: &[&str], order: &[usize], left: &[usize], wanted: &[Line<usize>]) -> Vec<String> {
        let edits = edits(book, order, left, wanted);
        (book.iter().enumerate())
            .map(|(id, text)| {
                let mut text = (*text).to_owned();
                for (_, edit) in edits.iter().filter(|(at, _)| *at == id).rev() {
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
        assert!(edits(&BOOK, &[0, 1, 2, 3, 4, 5], &[], &parts(&BOOK).lines).is_empty());
        use Matter::*;
        assert_eq!(matters(&BOOK), [Opening, Opening, Front, Front, Main, Back]);
    }

    #[test]
    fn a_book_without_parts_is_left_alone_and_groups_its_opening_pages() {
        let book = ["::: bottom\nA\n:::\n", "# One\n", "# Two\n"];
        assert!(edits(&book, &[0, 2, 1], &[], &parts(&book).moved(2, Some(1))).is_empty());
        assert_eq!(matters(&book), [Matter::Opening, Matter::Main, Matter::Main]);
    }

    #[test]
    fn a_move_leaves_the_part_lines_it_does_not_disturb() {
        // Main matter opens at an unnumbered prologue, and no chapter after the last numbered one opens back matter.
        let book = [
            "::: front-matter\n\n::: toc\n\n",
            "::: main-matter\n\n# Prologue {-}\n",
            "# One\n",
            "# Two\n",
            "# Epilogue {-}\n",
        ];
        assert!(edits(&book, &[0, 1, 3, 2, 4], &[], &parts(&book).moved(3, Some(2))).is_empty());
        assert!(edits(&book, &[0, 1, 4, 2, 3], &[], &parts(&book).moved(4, Some(2))).is_empty());
        let unnumbered = [
            "::: front-matter\n\n# Preface {-}\n",
            "::: main-matter\n\n# A {-}\n",
            "# B {-}\n",
            "# C {-}\n",
        ];
        assert!(edits(&unnumbered, &[0, 1, 3, 2], &[], &parts(&unnumbered).moved(3, Some(2))).is_empty());
    }

    #[test]
    fn a_moved_page_passes_the_part_line_that_opened_it_to_the_page_after_it() {
        let book = [
            "::: front-matter\n\n# Preface {-}\n",
            "::: main-matter\n\n# One\n",
            "# Two\n",
        ];
        let texts = kept(&book, &[0, 2, 1], &[], &parts(&book).moved(1, None));
        assert_eq!(texts, [book[0], "# One\n", "::: main-matter\n\n# Two\n"]);
    }

    #[test]
    fn a_page_dropped_at_the_edge_of_its_part_stays_in_it() {
        let book = [
            "::: front-matter\n\n::: toc\n\n::: main-matter\n\n",
            "# One\n",
            "# Two\n",
        ];
        assert!(edits(&book, &[0, 2, 1], &[], &parts(&book).moved(2, Some(1))).is_empty());
        // The preface dropped before the first chapter stays in front matter, which now opens at the contents.
        let book = [
            "::: front-matter\n\n# Preface {-}\n",
            "::: toc\n\n::: main-matter\n\n",
            "# One\n",
        ];
        let texts = kept(&book, &[1, 0, 2], &[], &parts(&book).moved(0, Some(2)));
        assert_eq!(
            texts,
            [
                "# Preface {-}\n",
                "::: front-matter\n\n::: toc\n\n",
                "::: main-matter\n\n# One\n"
            ]
        );
    }

    #[test]
    fn a_removed_page_passes_its_part_line_to_the_page_after_it() {
        let book = [
            "::: front-matter\n\n# Preface {-}\n",
            "::: main-matter\n\n# One\n",
            "# Two\n",
        ];
        let texts = kept(&book, &[0, 2], &[1], &parts(&book).removed(1));
        assert_eq!(texts, [book[0], "# One\n", "::: main-matter\n\n# Two\n"]);
    }

    #[test]
    fn a_part_line_is_never_written_into_a_missing_chapter() {
        let book = ["::: main-matter\n\n# One\n", "", "# Two\n"];
        let texts = kept(&book, &[1, 2], &[0], &parts(&book).removed(0));
        assert_eq!(texts, ["# One\n", "", "::: main-matter\n\n# Two\n"]);
        let wanted = [Line {
            part: 1,
            opens: Opens::Page(1),
        }];
        assert_eq!(kept(&book, &[1, 0, 2], &[], &wanted)[1], "");
    }

    #[test]
    fn an_added_page_goes_on_the_side_of_the_part_lines_its_place_gives() {
        let book = [
            "For you. {.dedication}\n\n::: front-matter\n\n",
            "# Preface {-}\n\n::: main-matter\n\n# One\n",
            "# Two\n",
            "Motto\n",
        ];
        // An opening page added after the dedication stays before front matter.
        let texts = kept(
            &book,
            &[0, 3, 1, 2],
            &[],
            &parts(&book[..3]).added(Some(1), Place::Opening(3), 3),
        );
        assert_eq!(texts[0], "For you. {.dedication}\n\n");
        assert_eq!(texts[1], format!("::: front-matter\n\n{}", book[1]));
        // The contents goes in front matter.
        let contents = parts(&book[..3]).added(Some(1), Place::Contents, 3);
        assert!(edits(&book, &[0, 3, 1, 2], &[], &contents).is_empty());
        // A page added before a file that holds a preface and main matter's line leaves that line alone.
        let book = [
            "# Preface {-}\n\n::: main-matter\n\n# One\n",
            "# Two\n",
            "For you. {.dedication}\n",
        ];
        let wanted = parts(&book[..2]).added(Some(0), Place::Opening(2), 2);
        assert!(edits(&book, &[2, 0, 1], &[], &wanted).is_empty());
    }

    #[test]
    fn a_page_added_to_a_single_file_goes_on_the_side_of_its_part_lines() {
        let sections = ["For you.\n\n::: front-matter\n\n", "# Preface {-}\n"];
        assert_eq!(in_gap(&sections, 1, Place::Opening(3)), Some((0, 10)));
        assert_eq!(in_gap(&sections, 1, Place::Contents), Some((0, sections[0].len())));
        assert_eq!(in_gap(&["For you.\n\n", "# Preface {-}\n"], 1, Place::Opening(3)), None);
    }

    #[test]
    fn a_part_line_in_a_section_of_several_pages_stays_where_it_works() {
        let front = "::: bottom\nA\n:::\n\nFor you.\n\n::: front-matter\n\n::: toc\n\n::: main-matter\n";
        let book = [front, "# One\n", "# Two\n"];
        assert!(edits(&book, &[0, 2, 1], &[], &parts(&book).moved(2, Some(1))).is_empty());
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
