//! Every command against every kind of block, on the samples and the book, and every insert through the engine.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use druck::cancel::Cancel;

use super::convert::{self, BlockType, TARGETS};
use super::marks::{self, Mark};
use super::slash::{self, Action, Context, Standalone};
use super::{Command, applied, moves, notes};
use crate::preview::job::Typesetter;
use crate::session::{Session, TEMPLATES};
use crate::shell::strings::Language;
use crate::source::{BlockKind, Blocks, Buffer, EditKind, Selection};

const CORPUS: &str = "Development/books/ttp404-gott-nicht-gefunden/buch-1-ttp404/manuscript";
/// How many blocks of each kind to try per document, so a full book stays quick.
const PER_KIND: usize = 6;

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

/// Every kind of block the source layer knows, so each is tried on any machine.
const EVERY_KIND: &str = "---\ntitle: A\n---\n# One\n\nA *para*\nstill it\n\n- a\n- b\n\n  more\n\n> q\nlazy\n\n```rust\nx\n\ny\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n::: keep\ninside\n:::\n\n<!-- note\n\nstill note -->\n\n***\n\n[^n]: Note\n\n    second\n\n<div>\nraw\n</div>\n\n[a]: http://x\n\nTitle\n===\n";

fn documents() -> Vec<(PathBuf, String)> {
    let samples = markdown_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples"));
    assert!(!samples.is_empty(), "the samples ship with the repository");
    let corpus = std::env::var_os("HOME")
        .map(|home| markdown_in(&Path::new(&home).join(CORPUS)))
        .unwrap_or_default();
    if corpus.is_empty() {
        println!("note: the corpus at ~/{CORPUS} is missing, testing the samples only");
    }
    samples
        .into_iter()
        .chain(corpus)
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            (path, text)
        })
        .chain(std::iter::once((PathBuf::from("every kind"), EVERY_KIND.to_owned())))
        .collect()
}

/// A few blocks of each kind, over a selection of all of the block and a selection inside it.
fn samples_of(text: &str, blocks: &Blocks) -> Vec<(usize, Selection)> {
    let mut seen: Vec<(BlockKind, usize)> = Vec::new();
    let mut chosen = Vec::new();
    for (index, block) in blocks.as_slice().iter().enumerate() {
        if block.range.is_empty() {
            continue;
        }
        let count = match seen.iter_mut().find(|(kind, _)| *kind == block.kind) {
            Some((_, count)) => {
                *count += 1;
                *count
            }
            None => {
                seen.push((block.kind, 0));
                0
            }
        };
        if count >= PER_KIND {
            continue;
        }
        chosen.push((
            index,
            Selection {
                anchor: block.range.start,
                head: block.range.end,
            },
        ));
        let middle = text.floor_char_boundary(block.range.start + block.range.len() / 2);
        let end = text.floor_char_boundary((middle + 12).min(block.range.end));
        chosen.push((
            index,
            Selection {
                anchor: middle,
                head: end,
            },
        ));
        chosen.push((index, Selection::caret(middle)));
    }
    chosen
}

fn named(path: &Path, what: &str, selection: Selection) -> String {
    format!("{}: {what} at {selection:?}", path.display())
}

/// Applies a command through a buffer as one undo step. Blocks it does not reach must stay byte-identical and undo must
/// give back every byte.
fn check_round_trip(
    path: &Path,
    what: &str,
    text: &str,
    reach: std::ops::Range<usize>,
    selection: Selection,
    command: &Command,
) {
    let context = named(path, what, selection);
    let mut buffer = Buffer::new(text);
    let groups_before = buffer.revision();
    buffer
        .transact(
            EditKind::Discrete,
            command.edits.clone(),
            selection,
            command.selection,
            Instant::now(),
        )
        .unwrap_or_else(|error| panic!("{context}: {error}"));
    assert_eq!(buffer.revision(), groups_before + 1, "{context}");
    let first = command.edits.first().expect("a command edits").range.start;
    let last = command.edits.last().expect("a command edits").range.end;
    assert!(
        first >= reach.start && last <= reach.end,
        "{context}: {first}..{last} is outside {reach:?}"
    );
    let after = buffer.text();
    assert!(after[..first] == text[..first], "{context}: the text before changed");
    let tail = text.len() - last;
    assert!(
        after[after.len() - tail..] == text[last..],
        "{context}: the text after changed"
    );
    assert!(
        command.selection.anchor <= after.len() && after.is_char_boundary(command.selection.anchor),
        "{context}: selection anchor"
    );
    assert!(
        command.selection.head <= after.len() && after.is_char_boundary(command.selection.head),
        "{context}: selection head"
    );
    let restored = buffer.undo().unwrap_or_else(|| panic!("{context}: nothing to undo"));
    assert!(buffer.text() == text, "{context}: undo did not restore the text");
    assert_eq!(restored.selection, selection, "{context}");
    assert!(buffer.undo().is_none(), "{context}: more than one undo step");
    buffer.redo();
    assert!(
        buffer.text() == applied(text, &command.edits),
        "{context}: redo differs"
    );
}

/// A range covering a block and the blank-line gaps a command may add next to it.
fn reach_of(blocks: &Blocks, first: usize, last: usize) -> std::ops::Range<usize> {
    blocks.as_slice()[first].range.start..blocks.as_slice()[last].range.end
}

#[test]
fn every_mark_round_trips_on_every_kind_of_block() {
    for (path, text) in documents() {
        let blocks = Blocks::parse(&text);
        for (index, selection) in samples_of(&text, &blocks) {
            let last = blocks.index_at(selection.range().end.min(text.len()));
            let reach = reach_of(&blocks, index, last.max(index));
            let reach = reach.start..reach.end.max(blocks.as_slice()[index].range.end);
            for mark in Mark::ALL {
                let Some(command) = marks::toggle(&text, &blocks, selection, mark) else {
                    continue;
                };
                check_round_trip(&path, &format!("{mark:?}"), &text, reach.clone(), selection, &command);
                // Toggling back on the result leaves no marker of this kind behind where there was none.
                let result = applied(&text, &command.edits);
                let parsed = Blocks::parse(&result);
                assert_eq!(
                    parsed.as_slice().len(),
                    blocks.as_slice().len(),
                    "{}",
                    named(&path, &format!("{mark:?}"), selection)
                );
            }
            if let Some(command) = marks::set_link(&text, &blocks, selection, "https://example.org/x") {
                check_round_trip(&path, "link", &text, reach.clone(), selection, &command);
            }
            if let Some(command) = marks::remove_link(&text, &blocks, selection) {
                check_round_trip(&path, "remove link", &text, reach.clone(), selection, &command);
            }
        }
    }
}

#[test]
fn a_block_with_no_running_text_gets_no_mark_and_no_conversion() {
    let blocks = Blocks::parse(EVERY_KIND);
    let mut tried = 0;
    for (index, block) in blocks.as_slice().iter().enumerate() {
        if !matches!(
            block.kind,
            BlockKind::FrontMatter
                | BlockKind::Table
                | BlockKind::Directive
                | BlockKind::Comment
                | BlockKind::SceneBreak
                | BlockKind::Raw
                | BlockKind::Blank
        ) {
            continue;
        }
        tried += 1;
        let selection = Selection {
            anchor: block.range.start,
            head: block.range.end,
        };
        for mark in Mark::ALL {
            assert!(
                marks::toggle(EVERY_KIND, &blocks, selection, mark).is_none(),
                "{:?}",
                block.kind
            );
        }
        for target in TARGETS {
            assert!(
                convert::convert(EVERY_KIND, &blocks, index, target, selection).is_none(),
                "{:?}",
                block.kind
            );
        }
    }
    assert!(tried >= 7);
}

#[test]
fn every_conversion_round_trips_on_every_kind_of_block() {
    for (path, text) in documents() {
        let blocks = Blocks::parse(&text);
        for (index, selection) in samples_of(&text, &blocks) {
            let reach = blocks.as_slice()[index].range.clone();
            for target in TARGETS {
                if let Some(command) = convert::convert(&text, &blocks, index, target, selection) {
                    check_round_trip(&path, &format!("{target:?}"), &text, reach.clone(), selection, &command);
                }
            }
            for target in [BlockType::Quote, BlockType::Bullets, BlockType::Numbers] {
                if selection.range().end <= reach.end
                    && let Some(command) = convert::toggle(&text, &blocks, selection, target)
                {
                    check_round_trip(
                        &path,
                        &format!("toggle {target:?}"),
                        &text,
                        reach.clone(),
                        selection,
                        &command,
                    );
                }
            }
        }
    }
}

#[test]
fn moving_any_block_round_trips_and_keeps_every_other_block() {
    for (path, text) in documents() {
        let blocks = Blocks::parse(&text);
        let list = blocks.as_slice();
        for (index, selection) in samples_of(&text, &blocks) {
            if !selection.is_empty() || !moves::can_move(list[index].kind, &text[list[index].range.clone()]) {
                continue;
            }
            for before in [0, list.len() / 2, list.len()] {
                let Some(command) = moves::move_block(&text, &blocks, index, before, selection) else {
                    continue;
                };
                check_round_trip(&path, "move", &text, 0..text.len(), selection, &command);
                let result = applied(&text, &command.edits);
                let parsed = Blocks::parse(&result);
                // A block can only merge with one it touched before, which keeps its text in the joined result.
                let all_new = text_of(&result, &parsed).concat();
                for block in text_of(&text, &blocks) {
                    assert!(
                        all_new.contains(block.trim_end()),
                        "{}",
                        named(&path, "move", selection)
                    );
                }
            }
        }
    }
}

fn text_of<'a>(text: &'a str, blocks: &Blocks) -> Vec<&'a str> {
    blocks
        .as_slice()
        .iter()
        .filter(|block| block.kind != BlockKind::Blank)
        .map(|block| &text[block.range.clone()])
        .collect()
}

#[test]
fn every_slash_entry_round_trips_in_every_kind_of_block() {
    for (path, text) in documents() {
        let blocks = Blocks::parse(&text);
        let context = Context {
            has_works: true,
            has_labels: true,
            ..Context::default()
        };
        let items = slash::items(&context, Language::English);
        for (index, selection) in samples_of(&text, &blocks) {
            let kind = blocks.as_slice()[index].kind;
            if !selection.is_empty() || !slash::opens_at(kind, " /", 1) {
                continue;
            }
            // Types a slash at the caret, as the menu sees it.
            let caret = selection.head;
            let typed = format!("{}/ {}", &text[..caret], &text[caret..]);
            let typed_blocks = Blocks::parse(&typed);
            let block = typed_blocks.index_at(caret);
            for item in &items {
                let Some(command) =
                    slash::apply(&typed, &typed_blocks, caret..caret + 1, &item.action, Language::English)
                else {
                    continue;
                };
                let reach = typed_blocks.as_slice()[block].range.clone();
                let reach = reach.start..reach.end;
                check_round_trip(&path, item.label, &typed, reach, Selection::caret(caret + 1), &command);
            }
        }
    }
}

#[test]
fn a_note_edit_round_trips_on_every_leading_comment() {
    let mut seen = 0;
    for (path, text) in documents() {
        let blocks = Blocks::parse(&text);
        for note in notes::leading_notes(&text, &blocks) {
            seen += 1;
            let edit = notes::note_edit(&text, &note, 0..0, "x").expect("plain text is allowed");
            let caret = Selection::caret(0);
            let command = Command {
                edits: vec![edit],
                selection: caret,
            };
            check_round_trip(&path, "note", &text, note.comment.clone(), caret, &command);
        }
    }
    assert!(
        seen > 0 || std::env::var_os("HOME").is_none(),
        "no document opens with a note"
    );
}

/// A document with every kind of block, as one file in a project that has a picture and a bibliography.
struct Scratch {
    dir: PathBuf,
}

const DOCUMENT: &str = "---\ntitle: T\nbibliography: references.bib\n---\n\nFirst paragraph with some words.\n\n# A heading\n\nA paragraph /QUERY\n\nThe last paragraph.\n";
const EMPTY_LINE: &str = "---\ntitle: T\nbibliography: references.bib\n---\n\nFirst paragraph with some words.\n\n/QUERY\n\nThe last paragraph.\n";
const IN_LIST: &str = "---\ntitle: T\nbibliography: references.bib\n---\n\n- One\n- /QUERY\n\nThe last paragraph.\n";
const IN_HEADING: &str =
    "---\ntitle: T\nbibliography: references.bib\n---\n\n# A heading /QUERY\n\nThe last paragraph.\n";
const IN_QUOTE: &str =
    "---\ntitle: T\nbibliography: references.bib\n---\n\n> A quotation /QUERY\n\nThe last paragraph.\n";
const IN_KEEP: &str =
    "---\ntitle: T\nbibliography: references.bib\n---\n\n::: keep\nHeld /QUERY\n:::\n\nThe last paragraph.\n";
const IN_NOTE: &str =
    "---\ntitle: T\nbibliography: references.bib\n---\n\nText.[^a]\n\n[^a]: A note /QUERY\n\nThe last paragraph.\n";

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("druck-commands-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("images")).expect("folder");
        let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples");
        fs::copy(samples.join("images/chart.svg"), dir.join("images/chart.svg")).expect("image");
        fs::write(
            dir.join("references.bib"),
            "@book{key,\n  author = {A. Writer},\n  title = {A Book},\n  publisher = {A Press},\n  year = {2020},\n}\n",
        )
        .expect("bibliography");
        Self { dir }
    }

    /// Renders `text`, returning what the engine objects to.
    fn problem(&self, text: &str) -> Option<String> {
        let file = self.dir.join("doc.md");
        fs::write(&file, text).expect("file");
        let session = Session::open(&file).expect("opens");
        let mut typesetter = Typesetter::new(session.kind().clone(), None);
        typesetter
            .run(session.joined(), &Cancel::default())
            .err()
            .map(|failure| failure.to_string())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

/// The menu's context for `text` at the slash, as the view builds it.
fn context_for(text: &str, blocks: &Blocks, slash_at: usize) -> Context {
    Context {
        open_directives: slash::open_directives(text, blocks, slash_at),
        has_toc: slash::holds_toc(text),
        has_works: true,
        has_labels: true,
        block: Some(blocks.as_slice()[blocks.index_at(slash_at)].kind),
    }
}

fn check_inserts(document: &str, name: &str, language: Language) {
    let scratch = Scratch::new(name);
    let text = document.replace("/QUERY", "/");
    let slash_at = text.find("/\n").expect("a slash");
    let blocks = Blocks::parse(&text);
    let context = context_for(&text, &blocks, slash_at);
    let base = scratch.problem(&text.replace("/\n", "\n"));
    assert!(base.is_none(), "the base document renders: {base:?}");
    let mut tried = 0;
    for item in slash::items(&context, language) {
        if matches!(
            item.action,
            Action::Section(_)
                | Action::OpenFootnote
                | Action::OpenCitation
                | Action::OpenReference
                | Action::OpenImage
        ) {
            continue;
        }
        let Some(command) = slash::apply(&text, &blocks, slash_at..slash_at + 1, &item.action, language) else {
            panic!("{}: no command in {name}", item.label);
        };
        let result = applied(&text, &command.edits);
        if let Some(problem) = scratch.problem(&result) {
            panic!("{} in {name} gives invalid Druck: {problem}\n{result}", item.label);
        }
        tried += 1;
    }
    assert!(tried > 0, "{name}: nothing was offered");
}

#[test]
fn every_insert_renders_through_the_engine_with_text_on_the_line() {
    check_inserts(DOCUMENT, "paragraph", Language::English);
}

#[test]
fn every_insert_renders_through_the_engine_on_an_empty_line_in_both_languages() {
    check_inserts(EMPTY_LINE, "empty", Language::English);
    check_inserts(EMPTY_LINE, "empty-de", Language::German);
}

#[test]
fn every_insert_renders_through_the_engine_inside_other_blocks() {
    check_inserts(IN_LIST, "list", Language::English);
    check_inserts(IN_HEADING, "heading", Language::English);
    check_inserts(IN_QUOTE, "quote", Language::English);
    check_inserts(IN_KEEP, "keep", Language::English);
    check_inserts(IN_NOTE, "note", Language::English);
}

#[test]
fn a_second_table_of_contents_is_not_offered() {
    let text = "::: toc\n\n/\n";
    let blocks = Blocks::parse(text);
    let context = context_for(text, &blocks, 9);
    assert!(
        !slash::items(&context, Language::English)
            .iter()
            .any(|item| item.action == Action::Insert(Standalone::Toc))
    );
}

#[test]
fn an_image_from_a_projects_assets_renders_in_a_chapter() {
    let scratch = Scratch::new("project-image");
    let dir = &scratch.dir;
    fs::create_dir_all(dir.join("manuscript")).expect("folder");
    fs::create_dir_all(dir.join("assets")).expect("folder");
    fs::copy(dir.join("images/chart.svg"), dir.join("assets/chart.svg")).expect("image");
    fs::write(
        dir.join("druck.yaml"),
        "title: T\nbibliography: references.bib\nchapters:\n  - manuscript/one.md\n",
    )
    .expect("manifest");
    fs::write(dir.join("manuscript/one.md"), "First.\n\n/\n\nLast.\n").expect("chapter");
    let mut session = Session::open(dir).expect("opens");
    let path = crate::editor::rich::images::bring_in(&session.base_dir(), true, &dir.join("images/chart.svg"))
        .expect("the image comes in");
    assert_eq!(path, "images/chart.svg");
    let text = session.active_buffer().text().to_owned();
    let blocks = Blocks::parse(&text);
    let at = text.find('/').expect("a slash");
    let figure = crate::editor::rich::images::write(&path, "Caption", "", crate::editor::rich::images::Placement::Here);
    let command = crate::editor::rich::image_command(&text, &blocks, at..at + 1, &figure);
    session
        .active_buffer_mut()
        .transact(
            EditKind::Discrete,
            command.edits,
            Selection::caret(at + 1),
            command.selection,
            Instant::now(),
        )
        .expect("valid");
    let mut typesetter = Typesetter::new(session.kind().clone(), None);
    if let Err(failure) = typesetter.run(session.joined(), &Cancel::default()) {
        panic!("the image does not render: {failure}");
    }
}

#[test]
fn every_template_is_on_the_list() {
    let items = slash::items(&Context::default(), Language::English);
    for template in TEMPLATES {
        assert!(items.iter().any(|item| item.action == Action::Section(template.slug)));
    }
}

#[test]
fn conversions_of_ordinary_text_render_through_the_engine() {
    let scratch = Scratch::new("convert");
    for source in [
        "A paragraph\nof two lines with *emphasis*.\n",
        "# A heading\n",
        "- One\n- Two\n  - Nested\n",
        "1. One\n2. Two\n",
        "> A quotation\n> of two lines.\n",
        "> [!NOTE]\n> A callout.\n",
        "```\nsome code\n```\n",
    ] {
        let text = format!("Before.\n\n{source}\nAfter.\n");
        let blocks = Blocks::parse(&text);
        assert!(scratch.problem(&text).is_none(), "{source}");
        for target in TARGETS {
            let Some(command) = convert::convert(&text, &blocks, 2, target, Selection::caret(0)) else {
                continue;
            };
            let result = applied(&text, &command.edits);
            if let Some(problem) = scratch.problem(&result) {
                panic!("{source:?} to {target:?}: {problem}\n{result}");
            }
        }
    }
}

#[test]
fn conversions_of_text_with_references_render_through_the_engine() {
    let scratch = Scratch::new("convert-references");
    let front = "---\ntitle: T\nbibliography: references.bib\n---\n\n";
    for source in ["See [@key] here.\n\nAfter.\n", "A note.[^a] More.\n\n[^a]: Its text.\n"] {
        let text = format!("{front}{source}");
        let blocks = Blocks::parse(&text);
        let base = scratch.problem(&text);
        assert!(base.is_none(), "{base:?}");
        for target in TARGETS {
            let Some(command) = convert::convert(&text, &blocks, 2, target, Selection::caret(0)) else {
                continue;
            };
            let result = applied(&text, &command.edits);
            if let Some(problem) = scratch.problem(&result) {
                panic!("{source:?} to {target:?}: {problem}\n{result}");
            }
        }
    }
}

#[test]
fn marks_render_through_the_engine() {
    let scratch = Scratch::new("marks");
    let text = "A paragraph with some words.\n\n# A heading\n";
    let blocks = Blocks::parse(text);
    let selection = Selection { anchor: 2, head: 11 };
    for mark in Mark::ALL {
        let command = marks::toggle(text, &blocks, selection, mark).expect("a change");
        let result = applied(text, &command.edits);
        if let Some(problem) = scratch.problem(&result) {
            panic!("{mark:?}: {problem}\n{result}");
        }
    }
    let command = marks::set_link(text, &blocks, selection, "https://example.org").expect("a change");
    assert!(scratch.problem(&applied(text, &command.edits)).is_none());
}
