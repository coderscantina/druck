//! The book as the path bar, its columns, and the overview show it: the front matter, the parts, and the back matter in
//! reading order, then what is set aside. Read from the session's text each time, never stored. Holds no gpui types.
//!
//! The manifest lists chapters flat, so the structure comes from the text:
//!
//! - A file's matter is the one in force at its first content, after `::: front-matter`, `::: main-matter`, or
//!   `::: back-matter`. Content before the first of these is front matter, and a book without them is all body.
//! - A part opens with a file of the body whose only heading is an unnumbered level 1 heading, with at most a motto
//!   under it, as the part template writes it. Body files before the first part have a group without a title.
//! - Files under `notes/probes/` are style probes, not notes.

use std::path::Path;

use pulldown_cmark::{Event, Parser};

use crate::session::{ChapterId, Contents, Row, Session, heading_text};
use crate::source::{BlockKind, Blocks};

/// How many paragraphs a chapter's opening keeps.
const OPENING_PARAGRAPHS: usize = 2;
/// The folder of style probes inside `notes/`.
const PROBES: &str = "probes";

/// A group of the book in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    FrontMatter,
    /// Body files outside any part: a book without parts, or what comes before the first.
    Chapters,
    Part,
    BackMatter,
}

/// Files outside the book, in the order they are set aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    NotInBook,
    Research,
    Notes,
    Probes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    /// Where the heading starts in its chapter's text.
    pub offset: usize,
}

/// One file, or one section of a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub row: Row,
    /// Its place in the book's order, for a file in the book.
    pub place: Option<usize>,
    /// The headings after its title, which is its first.
    pub headings: Vec<Heading>,
    /// Its first paragraphs, without Markdown.
    pub opening: Vec<String>,
}

impl Entry {
    /// The title with the chapter's number before it, as the path names it.
    pub fn numbered_title(&self) -> String {
        match self.row.number {
            Some(number) => format!("{number} {}", self.row.title),
            None => self.row.title.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub kind: Kind,
    /// The file that opens a part, which names it.
    pub opener: Option<Entry>,
    pub entries: Vec<Entry>,
}

impl Group {
    /// The part's title, for a part.
    pub fn title(&self) -> Option<&str> {
        self.opener.as_ref().map(|opener| opener.row.title.as_str())
    }

    /// Whether `id` is this group's opener or one of its entries.
    pub fn holds(&self, id: ChapterId) -> bool {
        self.opener.iter().chain(&self.entries).any(|entry| entry.row.id == id)
    }

    /// The place in the book just after this group's last file.
    pub fn end(&self) -> Option<usize> {
        self.opener
            .iter()
            .chain(&self.entries)
            .filter_map(|entry| entry.place)
            .max()
            .map(|place| place + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Book {
    pub groups: Vec<Group>,
    /// Only the shelves that hold something.
    pub shelves: Vec<(Shelf, Vec<Entry>)>,
}

/// Where the caret is, from the outside in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The group's index in [`Book::groups`], or the shelf's in [`Book::shelves`] past them.
    pub group: usize,
    pub chapter: ChapterId,
    /// The heading whose section holds the caret, as an index into the chapter's headings.
    pub section: Option<usize>,
}

impl Book {
    pub fn read(session: &Session) -> Self {
        let contents = session.contents();
        let base = session.base_dir();
        let text_of = |id: ChapterId| session.chapter(id).map_or("", |chapter| chapter.text());
        let mut book = structure(&contents.book, &text_of);
        let probe = |row: &Row| {
            session
                .chapter(row.id)
                .is_some_and(|chapter| is_probe(&base, chapter.path()))
        };
        let Contents {
            not_in_book,
            notes,
            research,
            ..
        } = contents;
        let (probes, notes): (Vec<Row>, Vec<Row>) = notes.into_iter().partition(probe);
        for (shelf, rows) in [
            (Shelf::NotInBook, not_in_book),
            (Shelf::Research, research),
            (Shelf::Notes, notes),
            (Shelf::Probes, probes),
        ] {
            if !rows.is_empty() {
                let entries = rows
                    .into_iter()
                    .map(|row| {
                        let text = text_of(row.id);
                        entry(row, None, text)
                    })
                    .collect();
                book.shelves.push((shelf, entries));
            }
        }
        book
    }

    /// How many lists the parts column shows: the groups, then the shelves.
    pub fn lists(&self) -> usize {
        self.groups.len() + self.shelves.len()
    }

    /// The files of the group or shelf at `list`, counting shelves after the groups. A part's opener is not among them.
    pub fn entries(&self, list: usize) -> &[Entry] {
        match self.groups.get(list) {
            Some(group) => &group.entries,
            None => self
                .shelves
                .get(list - self.groups.len())
                .map_or(&[], |(_, entries)| entries.as_slice()),
        }
    }

    /// The entry of a file, wherever it stands.
    pub fn entry(&self, id: ChapterId) -> Option<&Entry> {
        self.groups
            .iter()
            .flat_map(|group| group.opener.iter().chain(&group.entries))
            .chain(self.shelves.iter().flat_map(|(_, entries)| entries))
            .find(|entry| entry.row.id == id)
    }

    /// Where a caret at `offset` in chapter `id` is.
    pub fn place(&self, id: ChapterId, offset: usize) -> Option<Place> {
        let in_book = self.groups.iter().position(|group| group.holds(id));
        let on_shelf = || {
            (self.shelves.iter())
                .position(|(_, entries)| entries.iter().any(|entry| entry.row.id == id))
                .map(|at| self.groups.len() + at)
        };
        let group = in_book.or_else(on_shelf)?;
        let entry = self.entry(id)?;
        let section = entry.headings.iter().rposition(|heading| heading.offset <= offset);
        Some(Place {
            group,
            chapter: id,
            section,
        })
    }
}

/// Which part of a book a file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Matter {
    Front,
    Body,
    Back,
}

/// The matter a directive line starts, if it is one of the three.
fn matter_of(line: &str) -> Option<Matter> {
    match line.trim_end() {
        "::: front-matter" => Some(Matter::Front),
        "::: main-matter" => Some(Matter::Body),
        "::: back-matter" => Some(Matter::Back),
        _ => None,
    }
}

/// Groups the book's rows by matter and part, in reading order. The one reading of the book's structure, which the
/// bookshelf and the heading ticks' part line share with the path bar and the overview.
pub fn structure<'a>(rows: &[Row], text_of: &dyn Fn(ChapterId) -> &'a str) -> Book {
    let has_matter = rows.iter().any(|row| {
        let text = text_of(row.id);
        Blocks::parse(text).as_slice().iter().any(|block| {
            block.kind == BlockKind::Directive && matter_of(first_line(&text[block.range.clone()])).is_some()
        })
    });
    let mut matter = if has_matter { Matter::Front } else { Matter::Body };
    let mut groups: Vec<Group> = Vec::new();
    for (place, row) in rows.iter().enumerate() {
        let text = text_of(row.id);
        let blocks = Blocks::parse(text);
        let mut file_matter = None;
        for block in blocks.as_slice() {
            let line = first_line(&text[block.range.clone()]);
            let started = (block.kind == BlockKind::Directive).then(|| matter_of(line)).flatten();
            match (started, block.kind) {
                (Some(started), _) => matter = started,
                (None, BlockKind::Blank | BlockKind::Comment | BlockKind::FrontMatter) => {}
                (None, _) => {
                    file_matter.get_or_insert(matter);
                }
            }
        }
        let entry = entry(row.clone(), Some(place), text);
        let kind = match file_matter.unwrap_or(matter) {
            Matter::Front => Kind::FrontMatter,
            Matter::Back => Kind::BackMatter,
            Matter::Body if opens_part(text, &blocks) => {
                groups.push(Group {
                    kind: Kind::Part,
                    opener: Some(entry),
                    entries: Vec::new(),
                });
                continue;
            }
            Matter::Body => match groups.last() {
                Some(last) if last.kind == Kind::Part => Kind::Part,
                _ => Kind::Chapters,
            },
        };
        match groups.last_mut() {
            Some(last) if last.kind == kind => last.entries.push(entry),
            _ => groups.push(Group {
                kind,
                opener: None,
                entries: vec![entry],
            }),
        }
    }
    Book {
        groups,
        shelves: Vec::new(),
    }
}

/// Whether a file of the body opens a part: its only heading is an unnumbered level 1 heading, with at most one block of
/// text under it, such as a motto.
fn opens_part(text: &str, blocks: &Blocks) -> bool {
    let mut heading = None;
    let mut text_blocks = 0;
    for block in blocks.as_slice() {
        match block.kind {
            BlockKind::Heading { level } if heading.is_none() => heading = Some((level, block.range.clone())),
            BlockKind::Heading { .. } => return false,
            BlockKind::Blank
            | BlockKind::Comment
            | BlockKind::FrontMatter
            | BlockKind::Directive
            | BlockKind::SceneBreak => {}
            _ => text_blocks += 1,
        }
    }
    heading.is_some_and(|(level, range)| level == 1 && heading_text(first_line(&text[range])).1) && text_blocks <= 1
}

fn entry(row: Row, place: Option<usize>, text: &str) -> Entry {
    let blocks = Blocks::parse(text);
    let mut headings = Vec::new();
    let mut opening = Vec::new();
    let mut titled = false;
    for block in blocks.as_slice() {
        match block.kind {
            BlockKind::Heading { level } => {
                if titled {
                    let (title, _) = heading_text(first_line(&text[block.range.clone()]));
                    headings.push(Heading {
                        level,
                        title,
                        offset: block.range.start,
                    });
                }
                titled = true;
            }
            BlockKind::Paragraph if opening.len() < OPENING_PARAGRAPHS => {
                let plain = plain_text(&text[block.range.clone()]);
                if !plain.is_empty() {
                    opening.push(plain);
                }
            }
            _ => {}
        }
    }
    Entry {
        row,
        place,
        headings,
        opening,
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

/// A paragraph's words without Markdown, on one line, without a closing `{...}` of attributes.
fn plain_text(markdown: &str) -> String {
    let mut plain = String::new();
    for event in Parser::new(markdown) {
        match event {
            Event::Text(text) | Event::Code(text) => plain.push_str(&text),
            Event::SoftBreak | Event::HardBreak => plain.push(' '),
            _ => {}
        }
    }
    let plain = plain.trim_end();
    let plain = match plain.rfind('{') {
        Some(open) if plain.ends_with('}') => plain[..open].trim_end(),
        _ => plain,
    };
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a file stands under `notes/probes/` of a project at `base`.
fn is_probe(base: &Path, file: &Path) -> bool {
    file.starts_with(base.join("notes").join(PROBES))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::RowKind;

    fn rows(count: u32) -> Vec<Row> {
        (0..count)
            .map(|at| Row {
                id: crate::session::ChapterId::for_tests(at),
                title: format!("file {at}"),
                kind: RowKind::Plain,
                number: None,
                broken: false,
                status: None,
            })
            .collect()
    }

    fn read(texts: &[&'static str]) -> Book {
        let rows = rows(texts.len() as u32);
        let text_of = |id: ChapterId| texts[id.number()];
        structure(&rows, &text_of)
    }

    fn shape(book: &Book) -> Vec<(Kind, Option<usize>, Vec<usize>)> {
        book.groups
            .iter()
            .map(|group| {
                (
                    group.kind,
                    group.opener.as_ref().and_then(|opener| opener.place),
                    group.entries.iter().filter_map(|entry| entry.place).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn matter_directives_group_the_files_and_what_comes_first_is_front_matter() {
        let book = read(&[
            "Title page\n\n::: front-matter\n\n::: toc\n\n::: main-matter\n",
            "# One\n\nText.\n",
            "# Two\n\nText.\n",
            "::: back-matter\n\n# Notes {-}\n\nText.\n",
        ]);
        assert_eq!(
            shape(&book),
            [
                (Kind::FrontMatter, None, vec![0]),
                (Kind::Chapters, None, vec![1, 2]),
                (Kind::BackMatter, None, vec![3]),
            ]
        );
    }

    #[test]
    fn a_short_unnumbered_level_one_heading_opens_a_part_and_a_long_one_does_not() {
        let book = read(&[
            "# Prologue {-}\n\nOne.\n\nTwo.\n",
            "<!-- note -->\n\n# Part I: The road {-}\n\n»A motto.« {.dedication}\n",
            "# Chapter\n\nText.\n",
            "# Part II {-}\n\n::: page-break\n",
            "# Interlude {-}\n\nOne.\n\nTwo.\n",
        ]);
        assert_eq!(
            shape(&book),
            [
                (Kind::Chapters, None, vec![0]),
                (Kind::Part, Some(1), vec![2]),
                (Kind::Part, Some(3), vec![4]),
            ]
        );
        // A part is named by its opener's row, whose title the session reads from the heading.
        assert_eq!(book.groups[1].title(), Some("file 1"));
        assert_eq!(book.groups[1].end(), Some(3));
    }

    #[test]
    fn headings_after_the_title_and_the_opening_lines_are_read_plain() {
        let text = "<!-- a note -->\n\n# Tolls\n\nSalt *paid* for\nthe road. {.lead}\n\n## The bridge\n\nAt Golling.\n\n### Smuggling\n";
        let found = entry(rows(1).remove(0), None, text);
        let titles: Vec<_> = found.headings.iter().map(|h| (h.level, h.title.as_str())).collect();
        assert_eq!(titles, [(2, "The bridge"), (3, "Smuggling")]);
        assert_eq!(&text[found.headings[0].offset..][..15], "## The bridge\n\n");
        assert_eq!(found.opening, ["Salt paid for the road.", "At Golling."]);
    }

    #[test]
    fn the_caret_is_in_the_section_of_the_last_heading_before_it() {
        let book = read(&["# One\n\nIntro.\n\n## A\n\nText.\n\n## B\n\nText.\n"]);
        let id = book.groups[0].entries[0].row.id;
        assert_eq!(book.place(id, 3).map(|place| place.section), Some(None));
        let b = book.groups[0].entries[0].headings[1].offset;
        assert_eq!(book.place(id, b).map(|place| place.section), Some(Some(1)));
        assert_eq!(book.place(id, b - 1).map(|place| place.section), Some(Some(0)));
    }

    #[test]
    fn probes_are_the_notes_under_their_folder() {
        let base = Path::new("/book");
        assert!(is_probe(base, Path::new("/book/notes/probes/sea.md")));
        assert!(!is_probe(base, Path::new("/book/notes/plan.md")));
    }
}
