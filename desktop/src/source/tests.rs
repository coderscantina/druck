//! Round trips and incremental segmentation over the real documents. The external book is optional, so these pass on
//! other machines: a missing corpus prints a note and is skipped.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::blocks::BlockChange;
use super::buffer::Splice;
use super::*;

const CORPUS: &str = "Development/books/ttp404-gott-nicht-gefunden/buch-1-ttp404/manuscript";

fn markdown_in(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    files.sort();
    files
}

fn samples() -> Vec<PathBuf> {
    let files = markdown_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples"));
    assert!(!files.is_empty(), "the samples ship with the repository");
    files
}

fn corpus() -> Vec<PathBuf> {
    let files = std::env::var_os("HOME")
        .map(|home| markdown_in(&Path::new(&home).join(CORPUS)))
        .unwrap_or_default();
    if files.is_empty() {
        println!("note: the corpus at ~/{CORPUS} is missing, testing the samples only");
    }
    files
}

fn documents() -> Vec<(PathBuf, Vec<u8>)> {
    samples()
        .into_iter()
        .chain(corpus())
        .map(|path| {
            let bytes = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            (path, bytes)
        })
        .collect()
}

fn replace(buffer: &mut Buffer, blocks: &mut Blocks, range: Range<usize>, text: &str) -> BlockChange {
    let caret = Selection::caret(range.start);
    let splice = buffer
        .transact(
            EditKind::Discrete,
            vec![Edit::new(range, text)],
            caret,
            caret,
            Instant::now(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    blocks.apply(buffer.text(), splice)
}

#[test]
fn loading_and_saving_keeps_every_byte() {
    for (path, bytes) in documents() {
        let buffer = Buffer::from_bytes(bytes.clone()).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(buffer.text().as_bytes() == bytes, "{}", path.display());
        let blocks = Blocks::parse(buffer.text());
        let joined: String = blocks
            .as_slice()
            .iter()
            .map(|block| &buffer.text()[block.range.clone()])
            .collect();
        assert!(joined.as_bytes() == bytes, "{}: blocks lose text", path.display());
    }
}

#[test]
fn an_edit_in_one_block_leaves_all_other_block_bytes_identical_and_undo_restores_them() {
    for (path, bytes) in documents() {
        let original = String::from_utf8(bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let mut buffer = Buffer::new(original.clone());
        let mut blocks = Blocks::parse(buffer.text());
        let before = blocks.clone();
        let paragraphs: Vec<usize> = before
            .as_slice()
            .iter()
            .enumerate()
            .filter(|(_, block)| block.kind == BlockKind::Paragraph)
            .map(|(index, _)| index)
            .collect();
        let Some(&target) = paragraphs.get(paragraphs.len() / 2) else {
            continue;
        };
        let at = before.as_slice()[target].range.start;
        replace(&mut buffer, &mut blocks, at..at, "Eingefügt ");

        let after = blocks.as_slice();
        assert_eq!(after.len(), before.as_slice().len(), "{}", path.display());
        for (index, (old, new)) in before.as_slice().iter().zip(after).enumerate() {
            let old_text = &original[old.range.clone()];
            let new_text = &buffer.text()[new.range.clone()];
            if index == target {
                assert_eq!(
                    new_text.strip_prefix("Eingefügt "),
                    Some(old_text),
                    "{}",
                    path.display()
                );
            } else {
                assert_eq!(new_text, old_text, "{}: block {index}", path.display());
            }
        }

        let undone = buffer.undo().unwrap_or_else(|| panic!("the edit is on the undo stack"));
        blocks.apply(buffer.text(), undone.splice);
        assert!(buffer.text() == original, "{}", path.display());
        assert_eq!(blocks, before, "{}", path.display());
    }
}

/// A small deterministic generator, so a failure reproduces.
struct Random(u64);

impl Random {
    fn next(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % below as u64) as usize
    }
}

fn boundary_near(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

const INSERTS: &[&str] = &[
    "x",
    "# ",
    "> ",
    "- ",
    "1. ",
    "\n",
    "\n\n",
    "```\n",
    "~~~\n",
    "<!-- ",
    " -->",
    "---\n",
    "***\n",
    "::: keep\n",
    ":::\n",
    "| a | b |\n|---|---|\n",
    "    ",
    "[^n]: ",
    "===\n",
    "[a]: b\n",
    "<div>\n",
    "ä",
    "\r\n",
    "\r",
];

#[test]
fn incremental_updates_always_match_a_full_parse() {
    let mut random = Random(0x2545_F491_4F6C_DD1D);
    let documents = documents();
    for (path, bytes) in &documents {
        let mut buffer = Buffer::new(String::from_utf8_lossy(bytes).into_owned());
        // Long books get a fragment: the point is many edits, and a full parse after each one is the slow part.
        if buffer.len() > 8000 {
            let end = boundary_near(buffer.text(), 8000);
            buffer = Buffer::new(&buffer.text()[..end]);
        }
        let mut blocks = Blocks::parse(buffer.text());
        for _ in 0..200 {
            let len = buffer.len();
            let start = boundary_near(buffer.text(), random.next(len + 1));
            let (range, insert) = if random.next(3) == 0 {
                let end = boundary_near(buffer.text(), (start + random.next(40)).min(len));
                (start..end, "")
            } else {
                (start..start, INSERTS[random.next(INSERTS.len())])
            };
            let old = buffer.text().to_owned();
            replace(&mut buffer, &mut blocks, range.clone(), insert);
            let full = Blocks::parse(buffer.text());
            assert!(
                blocks == full,
                "{}: replacing {range:?} with {insert:?} diverged\nold: {old:?}\nincremental: {blocks:?}\nfull: {full:?}",
                path.display()
            );
        }
    }
}

/// The engine's reading: every top-level block the parser sees starts a block of the same family here.
#[test]
fn block_starts_agree_with_the_engine_parser() {
    for (path, bytes) in documents() {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let blocks = Blocks::parse(&text);
        let mut body = text.clone().into_bytes();
        for block in blocks.as_slice() {
            if matches!(block.kind, BlockKind::Directive | BlockKind::FrontMatter) {
                for byte in &mut body[block.range.clone()] {
                    if *byte != b'\n' {
                        *byte = b' ';
                    }
                }
            }
        }
        let body = String::from_utf8_lossy(&body).into_owned();
        assert_eq!(
            body.len(),
            text.len(),
            "{}: the test needs ASCII directives",
            path.display()
        );
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_SMART_PUNCTUATION
            | Options::ENABLE_GFM;
        let mut depth = 0;
        for (event, range) in Parser::new_ext(&body, options).into_offset_iter() {
            match event {
                Event::Start(tag) => {
                    if depth == 0 {
                        let line_start = body[..range.start].rfind('\n').map_or(0, |at| at + 1);
                        let line = body[..line_start].matches('\n').count() + 1;
                        let index = blocks.index_at(line_start);
                        let block = &blocks.as_slice()[index];
                        assert_eq!(
                            block.range.start,
                            line_start,
                            "{}:{line}: {tag:?} starts inside {:?}",
                            path.display(),
                            block.kind,
                        );
                        assert!(
                            family_matches(&tag, block.kind),
                            "{}:{line}: {tag:?} is {:?}",
                            path.display(),
                            block.kind
                        );
                    }
                    depth += 1;
                }
                Event::End(end) => {
                    depth -= 1;
                    let _: TagEnd = end;
                }
                Event::Rule if depth == 0 => {
                    let line_start = body[..range.start].rfind('\n').map_or(0, |at| at + 1);
                    let index = blocks.index_at(line_start);
                    assert_eq!(
                        blocks.as_slice()[index].kind,
                        BlockKind::SceneBreak,
                        "{}",
                        path.display()
                    );
                }
                _ => {}
            }
        }
    }
}

fn family_matches(tag: &Tag, kind: BlockKind) -> bool {
    match tag {
        Tag::Paragraph => kind == BlockKind::Paragraph,
        Tag::Heading { .. } => matches!(kind, BlockKind::Heading { .. }),
        Tag::List(_) => kind == BlockKind::List,
        Tag::BlockQuote(_) => kind == BlockKind::Quote,
        Tag::CodeBlock(_) => kind == BlockKind::Code,
        Tag::Table(_) => kind == BlockKind::Table,
        Tag::HtmlBlock => matches!(kind, BlockKind::Comment | BlockKind::Raw),
        Tag::FootnoteDefinition(_) => kind == BlockKind::FootnoteDefinition,
        _ => true,
    }
}

/// A 10,000-word chapter from the real text, or from the samples on other machines. The budget is a frame at 120 Hz,
/// so this holds in an unoptimized build. The printed numbers are the measurement.
#[test]
fn an_edit_resegments_a_ten_thousand_word_chapter_far_below_one_frame() {
    let mut chapter = String::new();
    while chapter.split_whitespace().count() < 10_000 {
        for (_, bytes) in documents() {
            chapter.push_str(&String::from_utf8_lossy(&bytes));
            chapter.push_str("\n\n");
            if chapter.split_whitespace().count() >= 10_000 {
                break;
            }
        }
    }
    let mut buffer = Buffer::new(chapter);
    let started = Instant::now();
    let mut blocks = Blocks::parse(buffer.text());
    let full_parse = started.elapsed();

    let mut worst = Duration::ZERO;
    let mut total = Duration::ZERO;
    let edits = 1000;
    let mut unclosed_worst = Duration::ZERO;
    for _ in 0..20 {
        let mut copy = blocks.clone();
        let splice = Splice {
            start: 0,
            old_end: 0,
            new_end: 0,
        };
        let started = Instant::now();
        copy.apply(buffer.text(), splice);
        unclosed_worst = unclosed_worst.max(started.elapsed());
    }
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for _ in 0..edits {
        // Paragraph starts, because an edit to a closing fence or comment would swallow the text after it, which is
        // correct but measures one full scan instead of the typing that happens in prose.
        let paragraphs: Vec<usize> = blocks
            .as_slice()
            .iter()
            .filter(|block| block.kind == BlockKind::Paragraph)
            .map(|block| block.range.start)
            .collect();
        let at = paragraphs[random.next(paragraphs.len())];
        let started = Instant::now();
        let caret = Selection::caret(at);
        let splice = buffer
            .transact(
                EditKind::Typing,
                vec![Edit::new(at..at, "x")],
                caret,
                Selection::caret(at + 1),
                Instant::now(),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        blocks.apply(buffer.text(), splice);
        let elapsed = started.elapsed();
        total += elapsed;
        worst = worst.max(elapsed);
    }
    println!(
        "resegmentation on {} KB and {} blocks: full parse {:?}, edit mean {:?}, worst {:?}, no-op at start {:?}",
        buffer.len() / 1024,
        blocks.as_slice().len(),
        full_parse,
        total / edits,
        worst,
        unclosed_worst,
    );
    assert!(worst < Duration::from_millis(8), "worst edit took {worst:?}");
}
