//! What a place on the pages is in the written text, and where written text is on the pages: the proof slip's paragraph
//! and the marks of problems. Reads the engine's source map and holds no gpui types.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use druck::output::{
    BlockKind, BlockSource, ChapterId, EM_ABOVE_BASELINE, Item, Layout, Pt, Rect, SourceMap, SourcePos,
};

/// How many pages in a row may hold no text of a block before the search for it stops, as a figure's page can.
const PAGES_WITHOUT_TEXT: usize = 1;

/// A block around another, with where it is written, which says a directive's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parent {
    pub kind: BlockKind,
    pub range: Range<usize>,
}

/// The block under a click, as a place in its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockAt {
    pub file: PathBuf,
    /// Where the click landed.
    pub offset: usize,
    pub range: Range<usize>,
    pub kind: BlockKind,
    /// The blocks around it, outermost first.
    pub parents: Vec<Parent>,
}

/// A problem to mark on the pages: the text it is about, the page, or both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub place: Option<(PathBuf, usize)>,
    pub page: Option<usize>,
    /// A typesetting warning, which only a proof pass looks at. Errors and open placeholders are always marked.
    pub warning: bool,
}

/// A rectangle on a page, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub page: usize,
    pub rect: Rect,
}

/// The problems as they stand on the pages of one layout.
#[derive(Default)]
pub struct Marks {
    /// What each problem marks, by its place in the list.
    pub of_problem: Vec<Vec<Mark>>,
    /// The pages with an error or an open placeholder, for the dots on a contact sheet.
    pub pages: HashSet<usize>,
}

fn range_of(block: &BlockSource) -> Range<u32> {
    block.range.0.offset..block.range.0.offset + block.range.1
}

/// The innermost block written at `at`.
fn block_at(map: &SourceMap, at: SourcePos) -> Option<&BlockSource> {
    let blocks = map.blocks();
    let after = blocks.partition_point(|block| block.range.0 <= at);
    let mut candidate = blocks.get(after.checked_sub(1)?);
    while let Some(block) = candidate {
        if block.range.0.chapter == at.chapter && range_of(block).contains(&at.offset) {
            return Some(block);
        }
        candidate = block.parent.map(|id| &blocks[id.0 as usize]);
    }
    None
}

/// The block under a point of a page, if the nearest text there has one.
pub fn block_under(laid: &Layout, page: usize, x: f32, y: f32) -> Option<BlockAt> {
    let map = laid.source_map()?;
    let position = laid.position_at(page, Pt(f64::from(x)), Pt(f64::from(y)))?;
    let block = block_at(map, position)?;
    let file = map.chapters().get(position.chapter.0 as usize)?.path.clone();
    let blocks = map.blocks();
    let mut parents = Vec::new();
    let mut at = block.parent;
    while let Some(id) = at {
        let parent = &blocks[id.0 as usize];
        let range = range_of(parent);
        parents.push(Parent {
            kind: parent.kind,
            range: range.start as usize..range.end as usize,
        });
        at = parent.parent;
    }
    parents.reverse();
    let range = range_of(block);
    Some(BlockAt {
        file,
        offset: position.offset as usize,
        range: range.start as usize..range.end as usize,
        kind: block.kind,
        parents,
    })
}

fn chapter_of(map: &SourceMap, file: &Path) -> Option<ChapterId> {
    let at = map.chapters().iter().position(|chapter| chapter.path == file)?;
    Some(ChapterId(at as u32))
}

/// The rectangle a run of text takes, as tall as its size.
fn text_rect(x: Pt, y: Pt, width: Pt, size: Pt) -> Rect {
    Rect {
        x,
        y: Pt(y.0 - EM_ABOVE_BASELINE * size.0),
        width,
        height: size,
    }
}

fn union(first: Rect, other: Rect) -> Rect {
    let (left, top) = (first.x.0.min(other.x.0), first.y.0.min(other.y.0));
    let right = (first.x.0 + first.width.0).max(other.x.0 + other.width.0);
    let bottom = (first.y.0 + first.height.0).max(other.y.0 + other.height.0);
    Rect {
        x: Pt(left),
        y: Pt(top),
        width: Pt(right - left),
        height: Pt(bottom - top),
    }
}

/// Where the blocks that start in `range` of a file are on the pages: one rectangle per page, in reading order.
pub fn marks_of(laid: &Layout, file: &Path, range: Range<usize>) -> Vec<Mark> {
    let Some(map) = laid.source_map() else {
        return Vec::new();
    };
    let Some(chapter) = chapter_of(map, file) else {
        return Vec::new();
    };
    let start = SourcePos {
        chapter,
        offset: range.start as u32,
    };
    let Some((first, _)) = map.locate(start) else {
        return Vec::new();
    };
    let wanted = range.start as u32..(range.end as u32).max(range.start as u32 + 1);
    let blocks = map.blocks();
    let mut marks = Vec::new();
    let mut empty = 0;
    for (page, sheet) in laid.pages().iter().enumerate().skip(first) {
        let mut found: Option<Rect> = None;
        for item in &sheet.items {
            let Item::Text {
                x,
                y,
                run,
                source: Some(id),
                ..
            } = item
            else {
                continue;
            };
            let block = &blocks[map.text(*id, run).block.0 as usize];
            if block.range.0.chapter == chapter && wanted.contains(&block.range.0.offset) {
                let rect = text_rect(*x, *y, run.width, run.size);
                found = Some(found.map_or(rect, |so_far| union(so_far, rect)));
            }
        }
        match found {
            Some(rect) => {
                marks.push(Mark { page, rect });
                empty = 0;
            }
            None if empty >= PAGES_WITHOUT_TEXT => break,
            None => empty += 1,
        }
    }
    marks
}

/// The text a page's body has: everything with a place in the written text, so not its running heads.
fn body_of_page(laid: &Layout, page: usize) -> Option<Rect> {
    laid.pages()
        .get(page)?
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text {
                x,
                y,
                run,
                source: Some(_),
                ..
            } => Some(text_rect(*x, *y, run.width, run.size)),
            _ => None,
        })
        .reduce(union)
}

/// The block of the first text on the pages that holds `words`. For scripted runs, which have no pointer.
pub fn block_with_text(laid: &Layout, words: &str) -> Option<BlockAt> {
    laid.pages().iter().enumerate().find_map(|(page, sheet)| {
        sheet.items.iter().find_map(|item| match item {
            Item::Text { x, y, run, .. } if run.text.contains(words) => {
                block_under(laid, page, (x.0 + run.width.0 / 2.0) as f32, (y.0 - 2.0) as f32)
            }
            _ => None,
        })
    })
}

/// A paragraph at about the middle of a page, or of the next page that has one. For scripted runs.
pub fn paragraph_on_page(laid: &Layout, page: usize) -> Option<(usize, BlockAt)> {
    let pages = laid.pages();
    let first = page.checked_rem(pages.len())?;
    pages.iter().enumerate().skip(first).find_map(|(page, sheet)| {
        let texts: Vec<_> = sheet
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text {
                    x,
                    y,
                    run,
                    source: Some(_),
                    ..
                } => Some((x.0 + run.width.0 / 2.0, y.0 - 2.0)),
                _ => None,
            })
            .collect();
        let middle = texts.len() / 2;
        (texts[middle.min(texts.len())..]
            .iter()
            .chain(&texts[..middle.min(texts.len())]))
        .find_map(|(x, y)| {
            let block = block_under(laid, page, *x as f32, *y as f32)?;
            (block.kind == BlockKind::Paragraph && block.parents.is_empty()).then_some((page, block))
        })
    })
}

/// The written place of the first text on a page, which says what chapter the page is in.
pub fn first_place(laid: &Layout, page: usize) -> Option<(PathBuf, usize)> {
    let map = laid.source_map()?;
    laid.pages().get(page)?.items.iter().find_map(|item| match item {
        Item::Text { source: Some(id), .. } => {
            let position = map.nearest(*id, 0);
            let chapter = map.chapters().get(position.chapter.0 as usize)?;
            Some((chapter.path.clone(), position.offset as usize))
        }
        _ => None,
    })
}

/// Where each problem stands on these pages. Text is marked on the problem's page if it names one and the text is
/// there. A problem with a page and no text marks that page's body.
pub fn resolve(laid: &Layout, problems: &[Problem]) -> Marks {
    let map = laid.source_map();
    let mut marks = Marks::default();
    for problem in problems {
        let of_text = problem.place.as_ref().and_then(|(file, offset)| {
            let map = map?;
            let chapter = chapter_of(map, file)?;
            let at = SourcePos {
                chapter,
                offset: *offset as u32,
            };
            let block = block_at(map, at)?;
            let range = range_of(block);
            let all = marks_of(laid, file, range.start as usize..range.end as usize);
            let on_page: Vec<Mark> = (all.iter().copied())
                .filter(|mark| Some(mark.page) == problem.page)
                .collect();
            Some(if on_page.is_empty() { all } else { on_page })
        });
        let found = match (of_text, problem.page) {
            (Some(found), _) if !found.is_empty() => found,
            (_, Some(page)) => body_of_page(laid, page)
                .map(|rect| Mark { page, rect })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        };
        if !problem.warning {
            marks.pages.extend(found.iter().map(|mark| mark.page));
        }
        marks.of_problem.push(found);
    }
    marks
}

#[cfg(test)]
mod tests {
    use druck::cancel::Cancel;

    use super::*;
    use crate::preview::job::{Layout as Laid, Typesetter};
    use crate::session::{Joined, Kind};

    const TEXT: &str = "# One\n\nFirst paragraph here.\n\n> A quoted line.\n\n::: page-break\n\nSecond page text.\n";

    fn laid(text: &str) -> (PathBuf, Laid) {
        let path = std::env::temp_dir().join("druck-proof-probe.md");
        let result = Typesetter::new(Kind::Single { path: path.clone() }, None)
            .run(Joined::single(text.to_owned(), 0), &Cancel::default());
        let Ok(layout) = result else {
            panic!("the text typesets");
        };
        (path, layout)
    }

    /// The point in the middle of the first run of `words`.
    fn point_on(laid: &Layout, words: &str) -> (usize, f32, f32) {
        for (page, sheet) in laid.pages().iter().enumerate() {
            for item in &sheet.items {
                if let Item::Text { x, y, run, .. } = item
                    && run.text.contains(words)
                {
                    return (page, (x.0 + run.width.0 / 2.0) as f32, (y.0 - 2.0) as f32);
                }
            }
        }
        panic!("{words} is on no page");
    }

    #[test]
    fn a_click_finds_the_paragraph_and_what_is_around_it() {
        let (file, layout) = laid(TEXT);
        let (page, x, y) = point_on(&layout.laid, "First");
        let found = block_under(&layout.laid, page, x, y).expect("text is there");
        assert_eq!(found.file, file);
        assert_eq!(found.kind, BlockKind::Paragraph);
        assert_eq!(&TEXT[found.range.clone()].trim_end(), &"First paragraph here.");
        assert!(found.parents.is_empty());

        let (page, x, y) = point_on(&layout.laid, "quoted");
        let quoted = block_under(&layout.laid, page, x, y).expect("text is there");
        assert_eq!(quoted.parents[0].kind, BlockKind::Quote);
    }

    #[test]
    fn a_paragraph_is_marked_on_the_page_it_stands_on() {
        let (file, layout) = laid(TEXT);
        let at = TEXT.find("Second page").expect("in the text");
        let marks = marks_of(&layout.laid, &file, at..at + "Second page text.".len());
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].page, 1);
        assert!(marks[0].rect.width.0 > 10.0);
    }

    #[test]
    fn a_problem_marks_its_text_and_a_problem_of_a_page_marks_the_page() {
        let (file, layout) = laid(TEXT);
        let at = TEXT.find("First").expect("in the text");
        let problems = [
            Problem {
                place: Some((file.clone(), at)),
                page: Some(0),
                warning: false,
            },
            Problem {
                place: None,
                page: Some(1),
                warning: true,
            },
            Problem {
                place: None,
                page: None,
                warning: false,
            },
        ];
        let marks = resolve(&layout.laid, &problems);
        assert_eq!(marks.of_problem[0][0].page, 0);
        assert_eq!(marks.of_problem[1][0].page, 1);
        assert!(marks.of_problem[2].is_empty());
        assert_eq!(
            marks.pages,
            HashSet::from([0]),
            "a warning puts no dot on a contact sheet"
        );
    }

    #[test]
    fn the_first_text_of_a_page_says_where_the_page_is_written() {
        let (file, layout) = laid(TEXT);
        let (found, offset) = first_place(&layout.laid, 1).expect("the page has text");
        assert_eq!(found, file);
        assert_eq!(offset, TEXT.find("Second page").expect("in the text"));
    }
}
