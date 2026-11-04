//! The layout result stands alone, and its source map is exact or says nothing. Each construct that changes text
//! on its way to the page has a test.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::cancel::Cancel;
use crate::input::{Date, DocumentText, Input, Overrides, Project, ThemeDirs, ThemeSelection};
use crate::output::{Item, Layout, Pt, Run, SourcePos, TextId};
use crate::renderer::{LayoutOptions, PdfOptions, Renderer};
use crate::resource::{Bytes, FileResolver, MemoryResolver, Origin, ResolveError, Resolver};
use crate::text::SHAPED;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");
/// Layout in a debug build needs more stack than a test thread has by default.
const STACK: usize = 16 * 1024 * 1024;
const MAPPED: LayoutOptions = LayoutOptions { source_map: true };

fn on_big_stack<T: Send>(run: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        let thread = std::thread::Builder::new().stack_size(STACK);
        thread
            .spawn_scoped(scope, run)
            .expect("a thread")
            .join()
            .expect("no panic")
    })
}

fn input(text: DocumentText) -> Input {
    Input {
        text,
        theme: ThemeSelection::Document {
            dirs: ThemeDirs::default(),
        },
        overrides: Overrides::default(),
        today: Date::from_unix(0),
    }
}

fn lay_out(resolver: &dyn Resolver, input: &Input, options: LayoutOptions) -> Layout {
    let (renderer, cancel) = (Renderer::new(), Cancel::default());
    let configured = renderer.job(resolver, input, &cancel).configure().expect("configures");
    configured.layout(&options).expect("lays out")
}

fn sample(name: &str, options: LayoutOptions) -> Layout {
    let resolver = FileResolver::new();
    let path = Path::new(SAMPLES).join(name);
    let input = input(DocumentText::read(&resolver, &path).expect("reads"));
    on_big_stack(|| lay_out(&resolver, &input, options))
}

fn single(text: &str, theme: Option<&str>) -> Input {
    let origin = Origin::Document(PathBuf::from("/memory"));
    let mut input = input(DocumentText::Single {
        text: text.to_owned(),
        origin: origin.clone(),
        name: "doc.md".to_owned(),
    });
    if let Some(theme) = theme {
        input.theme = ThemeSelection::Json {
            text: theme.to_owned(),
            origin,
        };
    }
    input
}

/// `text` laid out from memory with a source map, in `theme`.
fn mapped_in(text: &str, theme: Option<&str>, resolver: &MemoryResolver) -> Layout {
    let input = single(text, theme);
    on_big_stack(|| lay_out(resolver, &input, MAPPED))
}

fn mapped(text: &str) -> Layout {
    mapped_in(text, None, &MemoryResolver::default())
}

/// Every text item with a number: page, item, run, and number.
fn texts(layout: &Layout) -> Vec<(usize, usize, &Run, TextId)> {
    let mut found = Vec::new();
    for (page_index, page) in layout.pages().iter().enumerate() {
        for (item_index, item) in page.items.iter().enumerate() {
            if let Item::Text {
                run, source: Some(id), ..
            } = item
            {
                found.push((page_index, item_index, run, *id));
            }
        }
    }
    found
}

/// The text item whose run is `wanted`.
fn run_of<'a>(layout: &'a Layout, wanted: &str) -> (&'a Run, TextId) {
    let found = texts(layout).into_iter().find(|(.., run, _)| run.text == wanted);
    let (.., run, id) = found.unwrap_or_else(|| panic!("no run {wanted:?}"));
    (run, id)
}

/// Where byte `byte` of the run `wanted` is written, in the only chapter.
fn written(layout: &Layout, wanted: &str, byte: usize) -> Option<u32> {
    let (_, id) = run_of(layout, wanted);
    layout
        .source_map()
        .expect("mapped")
        .position(id, byte)
        .map(|at| at.offset)
}

/// Where a click on byte `byte` of the run `wanted` goes, in the only chapter.
fn nearest(layout: &Layout, wanted: &str, byte: usize) -> u32 {
    let (_, id) = run_of(layout, wanted);
    layout.source_map().expect("mapped").nearest(id, byte).offset
}

fn offset(text: &str, needle: &str) -> u32 {
    text.find(needle).unwrap_or_else(|| panic!("no {needle:?} in the text")) as u32
}

/// Every written span holds the same bytes on the page as in `text`, whatever the case a style sets it in, and every
/// byte of it round-trips through `position` then `locate` to its item.
fn check_exact(layout: &Layout, text: &str) {
    let map = layout.source_map().expect("mapped");
    let starts: Vec<u32> = map.blocks().iter().map(|block| block.range.0.offset).collect();
    assert!(starts.is_sorted(), "blocks are in written order");
    // A table header repeats on later pages, and its one text is located where it is first drawn.
    let mut first = HashMap::new();
    for (page, item, run, id) in texts(layout) {
        let (page, item) = *first.entry(id).or_insert((page, item));
        let source = map.text(id, run);
        let mut covered = 0;
        for span in &source.spans {
            assert_eq!(span.run.start, covered, "spans cover {:?}", run.text);
            covered = span.run.end;
            let Some(at) = span.source else { continue };
            let shown = &run.text[span.run.start as usize..span.run.end as usize];
            let start = at.offset as usize;
            let written = &text[start..start + shown.len()];
            assert_eq!(written.to_uppercase(), shown.to_uppercase(), "in {:?}", run.text);
            for byte in span.run.clone() {
                let here = map.position(id, byte as usize).expect("a written byte");
                assert_eq!(here.offset, at.offset + byte - span.run.start);
                assert_eq!(map.locate(here), Some((page, item)), "{:?} at byte {byte}", run.text);
            }
        }
        assert!(covered == run.text.len() as u32 || source.spans.is_empty());
    }
}

#[test]
fn a_layout_exports_alone_and_equals_a_render() {
    for name in ["report.md", "images.md", "typography.md"] {
        let path = Path::new(SAMPLES).join(name);
        let expected = on_big_stack(|| {
            let resolver = FileResolver::new();
            let input = input(DocumentText::read(&resolver, &path).expect("reads"));
            let rendered = Renderer::new().render(&resolver, &input, &PdfOptions::default(), &Cancel::default());
            rendered.expect("renders").bytes
        });
        let layout = sample(name, LayoutOptions::default());
        let bytes = on_big_stack(|| layout.pdf(&PdfOptions::default(), &Cancel::default()));
        assert!(
            bytes.expect("writes").bytes == expected,
            "{name}: same PDF after all else is gone"
        );
    }
}

/// Reads files and counts the reads.
#[derive(Default)]
struct Counting {
    reads: AtomicUsize,
    files: FileResolver,
}

impl Resolver for Counting {
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.files.read(origin, path)
    }
}

#[test]
fn exporting_reads_nothing_and_shapes_nothing() {
    let resolver = Counting::default();
    let path = Path::new(SAMPLES).join("report.md");
    let input = input(DocumentText::read(&resolver, &path).expect("reads"));
    on_big_stack(|| {
        let layout = lay_out(&resolver, &input, LayoutOptions::default());
        let reads = resolver.reads.load(Ordering::Relaxed);
        let shaped = SHAPED.with(|shaped| shaped.get());
        assert!(reads > 0 && shaped > 0, "layout read and shaped");
        layout.pdf(&PdfOptions::default(), &Cancel::default()).expect("writes");
        assert_eq!(resolver.reads.load(Ordering::Relaxed), reads);
        assert_eq!(SHAPED.with(|shaped| shaped.get()), shaped);
    });
}

#[test]
fn a_map_changes_no_byte_of_the_pdf() {
    for name in ["de.md", "report.md", "marks.md", "tables.md", "margins.md"] {
        let export = |layout: Layout| on_big_stack(|| layout.pdf(&PdfOptions::default(), &Cancel::default()));
        let plain = export(sample(name, LayoutOptions::default())).expect("writes").bytes;
        let with_map = export(sample(name, MAPPED)).expect("writes").bytes;
        assert!(plain == with_map, "{name}");
    }
}

#[test]
fn without_a_map_nothing_is_recorded() {
    let layout = sample("de.md", LayoutOptions::default());
    assert!(layout.source_map().is_none());
    assert!(texts(&layout).is_empty());
    assert_eq!(layout.position_at(0, Pt(100.0), Pt(100.0)), None);
    let mut items = layout.pages().iter().flat_map(|page| &page.items);
    assert!(items.any(|item| matches!(item, Item::Text { .. })));
}

#[test]
fn every_text_item_of_the_samples_is_exact_and_round_trips() {
    for name in fs::read_dir(SAMPLES).expect("samples").flatten() {
        let name = name.file_name().into_string().expect("a plain name");
        if !name.ends_with(".md") {
            continue;
        }
        let text = fs::read_to_string(Path::new(SAMPLES).join(&name)).expect("reads");
        let layout = sample(&name, MAPPED);
        assert!(!texts(&layout).is_empty(), "{name}");
        on_big_stack(|| check_exact(&layout, &text));
    }
}

#[test]
fn generated_text_of_a_sample_goes_to_an_item_of_its_block() {
    let layout = sample("de.md", MAPPED);
    let map = layout.source_map().expect("mapped");
    let mut generated = 0;
    for (.., run, id) in texts(&layout) {
        let source = map.text(id, run);
        if source.spans.iter().any(|span| span.source.is_some()) {
            continue;
        }
        generated += 1;
        assert_eq!(map.position(id, 0), None, "{:?}", run.text);
        assert!(map.locate(map.nearest(id, 0)).is_some(), "{:?}", run.text);
    }
    assert!(generated > 0, "markers, numbers, and dashes");
}

#[test]
fn only_a_page_number_is_outside_every_block() {
    let text = "# Head\n\nSome words.\n\n- a list\n\n> a quote\n\n> [!NOTE]\n> a note\n\n```\ncode\n```\n\n\
                | t |\n|---|\n| c |\n\n::: keep\n\nKept.\n\n:::\n\n***\n\nEnd.\n";
    let layout = mapped(text);
    let unmapped: Vec<&str> = (layout.pages().iter().flat_map(|page| &page.items))
        .filter_map(|item| match item {
            Item::Text { run, source: None, .. } => Some(run.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(unmapped, ["1"]);
    check_exact(&layout, text);
}

#[test]
fn a_hyphen_added_at_a_break_maps_to_nothing_and_its_letters_to_the_written_bytes() {
    let text = fs::read_to_string(Path::new(SAMPLES).join("de.md")).expect("reads");
    let layout = sample("de.md", MAPPED);
    let map = layout.source_map().expect("mapped");
    let hyphenated = texts(&layout).into_iter().filter(|(.., run, _)| {
        let word = run.text.trim_end_matches('-');
        run.text.ends_with('-') && word.chars().all(char::is_alphabetic) && !text.contains(&run.text)
    });
    let mut count = 0;
    for (.., run, id) in hyphenated {
        count += 1;
        let last = run.text.len() - 1;
        assert_eq!(map.position(id, last), None, "{:?}", run.text);
        let at = map.position(id, 0).expect("written letters");
        assert_eq!(&text[at.offset as usize..at.offset as usize + last], &run.text[..last]);
        assert_eq!(
            map.nearest(id, last).offset,
            at.offset + last as u32,
            "where the word goes on"
        );
    }
    assert!(count > 0, "the sample hyphenates");
}

#[test]
fn no_break_spaces_map_around_the_space_they_replaced() {
    let text = "---\nno-break-spaces: true\n---\n\nSiehe S. 12 und mehr.\n";
    let layout = mapped(text);
    let word = "S.\u{a0}12";
    assert_eq!(written(&layout, word, 0), Some(offset(text, "S. 12")));
    assert_eq!(written(&layout, word, 2), None, "the no-break space");
    assert_eq!(nearest(&layout, word, 2), offset(text, " 12"));
    assert_eq!(written(&layout, word, 4), Some(offset(text, "12")));
    check_exact(&layout, text);
}

#[test]
fn smart_punctuation_maps_to_nothing_and_stands_where_it_is_written() {
    let text = "Say \"yes\" -- now... it's 'done'.\n";
    let layout = mapped(text);
    assert_eq!(written(&layout, "“yes”", 0), None);
    assert_eq!(nearest(&layout, "“yes”", 0), offset(text, "\"yes"));
    assert_eq!(written(&layout, "“yes”", "“".len()), Some(offset(text, "yes")));
    assert_eq!(written(&layout, "–", 0), None);
    assert_eq!(nearest(&layout, "–", 0), offset(text, "--"));
    assert_eq!(written(&layout, "now…", 0), Some(offset(text, "now")));
    assert_eq!(written(&layout, "now…", 3), None);
    assert_eq!(written(&layout, "it’s", 0), Some(offset(text, "it's")));
    assert_eq!(written(&layout, "it’s", 2), None);
    assert_eq!(written(&layout, "it’s", 2 + "’".len()), Some(offset(text, "s 'done")));
    check_exact(&layout, text);
}

#[test]
fn entities_and_escapes_map_exactly_or_not_at_all() {
    let text = "Fish &amp; chips, a\\*b and a&nbsp;c.\n";
    let layout = mapped(text);
    check_exact(&layout, text);
    assert_eq!(written(&layout, "Fish", 0), Some(0));
}

#[test]
fn comments_are_skipped_on_both_sides() {
    let text = "<!-- first -->\n\nA wor<!-- mid -->ds <!-- gone\n\nlong --> end.\n\n<!-- between -->\n\nNext.\n";
    let layout = mapped(text);
    assert_eq!(written(&layout, "A", 0), Some(offset(text, "A wor")));
    assert_eq!(written(&layout, "words", 2), Some(offset(text, "r<!--")));
    assert_eq!(written(&layout, "words", 3), Some(offset(text, "ds <")));
    assert_eq!(written(&layout, "end.", 0), Some(offset(text, "end.")));
    assert_eq!(written(&layout, "Next.", 0), Some(offset(text, "Next.")));
    check_exact(&layout, text);
    // A place inside a comment goes to the text before it.
    let map = layout.source_map().expect("mapped");
    let inside = SourcePos {
        chapter: crate::output::ChapterId(0),
        offset: offset(text, "mid"),
    };
    let (page, item) = map.locate(inside).expect("located");
    assert!(matches!(&layout.pages()[page].items[item], Item::Text { run, .. } if run.text == "words"));
}

#[test]
fn front_matter_counts_in_offsets() {
    let text = "---\ntitle: T\nauthor: A\n---\n\nHello there.\n";
    let layout = mapped(text);
    assert_eq!(written(&layout, "Hello", 0), Some(offset(text, "Hello")));
    assert_eq!(written(&layout, "there.", 0), Some(offset(text, "there.")));
}

#[test]
fn inline_marks_map_to_their_text_without_their_syntax() {
    let text = "Some ~~struck~~ H~2~O 4 m^2^ [under]{.underline} [small]{.smallcaps} **bold** *em* `co de` \
                [link](https://example.com) {todo: check  this}.\n";
    let layout = mapped(text);
    for word in ["struck", "under", "bold", "em", "link"] {
        assert_eq!(written(&layout, word, 0), Some(offset(text, word)), "{word}");
    }
    assert_eq!(written(&layout, "H", 0), Some(offset(text, "H~2")));
    assert_eq!(written(&layout, "2", 0), Some(offset(text, "2~O")));
    assert_eq!(written(&layout, "O", 0), Some(offset(text, "O 4")));
    assert_eq!(written(&layout, "co", 0), Some(offset(text, "co de")));
    assert_eq!(written(&layout, "de", 0), Some(offset(text, "de`")));
    // A todo's white space is normalized, so its text is inserted at the brace.
    assert_eq!(written(&layout, "TODO:", 0), None);
    assert_eq!(nearest(&layout, "check", 0), offset(text, "{todo"));
    check_exact(&layout, text);
}

#[test]
fn capitals_of_another_length_map_to_nothing() {
    let theme = r#"{"version": 1, "styles": {"heading-1": {"uppercase": true}}}"#;
    let text = "# Straße frei\n\nBody.\n";
    let layout = mapped_in(text, Some(theme), &MemoryResolver::default());
    let map = layout.source_map().expect("mapped");
    let (_, id) = run_of(&layout, "STRASSE");
    assert_eq!(map.position(id, 0).map(|at| at.offset), Some(offset(text, "Straße")));
    assert_eq!(map.position(id, 3).map(|at| at.offset), Some(offset(text, "aße")));
    assert_eq!(map.position(id, 4), None, "ß became SS");
    assert_eq!(map.position(id, 6).map(|at| at.offset), Some(offset(text, "e frei")));
    assert_eq!(written(&layout, "FREI", 1), Some(offset(text, "rei")));
    check_exact(&layout, text);
}

#[test]
fn synthetic_small_capitals_map_to_their_letters() {
    // The bundled mono face has no small capitals of its own, so the layout builds them by reshaping.
    let theme = r#"{"version": 1, "styles": {"body": {"font": "Libertinus Mono"}}}"#;
    let text = "Before [Mixed]{.smallcaps} after.\n";
    let layout = mapped_in(text, Some(theme), &MemoryResolver::default());
    assert_eq!(written(&layout, "M", 0), Some(offset(text, "Mixed")));
    assert_eq!(written(&layout, "IXED", 0), Some(offset(text, "ixed")));
    assert_eq!(written(&layout, "IXED", 3), Some(offset(text, "d]")));
    check_exact(&layout, text);
}

#[test]
fn footnote_markers_stand_at_their_reference_and_notes_map_to_their_definitions() {
    let text = "Text here[^a] and more.\n\n[^a]: The note itself.\n";
    let layout = mapped(text);
    let map = layout.source_map().expect("mapped");
    let markers: Vec<_> = texts(&layout)
        .into_iter()
        .filter(|(.., run, _)| run.text == "1")
        .collect();
    assert_eq!(markers.len(), 2, "the reference and the note's own number");
    for (.., id) in &markers {
        assert_eq!(map.position(*id, 0), None);
    }
    assert_eq!(map.nearest(markers[0].3, 0).offset, offset(text, "[^a]"));
    assert_eq!(written(&layout, "note", 0), Some(offset(text, "note itself")));
    assert_eq!(written(&layout, "more.", 0), Some(offset(text, "more.")));
    check_exact(&layout, text);
}

#[test]
fn margin_notes_map_to_their_definitions_without_their_mark() {
    let theme = r#"{"version": 1, "page": {"margins": {"outer": "60mm"}, "wide": []}, "margin-notes": {"width": "40mm", "gap": "6mm"}}"#;
    let text = "Text[^m] goes on.\n\n[^m]: {-} Beside it.\n";
    let layout = mapped_in(text, Some(theme), &MemoryResolver::default());
    assert_eq!(written(&layout, "Beside", 0), Some(offset(text, "Beside")));
    check_exact(&layout, text);
}

#[test]
fn citations_and_references_map_to_nothing_and_stand_where_they_are_written() {
    let text = "---\nbibliography: refs.bib\n---\n\n# Start {#sec:one}\n\nSeen in [@a] and @sec:one too.\n";
    let mut resolver = MemoryResolver::default();
    let bib = b"@book{a, author = {Ada Example}, title = {The Title}, year = {2020}, publisher = {P}}\n";
    resolver.insert(Origin::Document(PathBuf::from("/memory")), "refs.bib", bib.as_slice());
    let layout = mapped_in(text, None, &resolver);
    let map = layout.source_map().expect("mapped");
    let citation = texts(&layout)
        .into_iter()
        .find(|(.., run, _)| run.text.starts_with("(Example") || run.text.starts_with("Example"))
        .expect("a citation");
    assert_eq!(map.position(citation.3, 0), None);
    assert_eq!(
        map.nearest(citation.3, 0).offset,
        offset(text, " [@a]"),
        "after the word before it"
    );
    assert_eq!(written(&layout, "and", 0), Some(offset(text, "and @sec")));
    assert_eq!(written(&layout, "too.", 0), Some(offset(text, "too.")));
    // A bibliography entry is the engine's text, in the block added for it.
    let entry = texts(&layout)
        .into_iter()
        .find(|(.., run, _)| run.text.starts_with("Title"));
    assert_eq!(map.position(entry.expect("an entry").3, 0), None);
    check_exact(&layout, text);
}

#[test]
fn a_heading_number_goes_to_the_heading_and_its_text_to_the_written_text() {
    let text = "# Intro part\n\nBody.\n\n## Second {#sec:s}\n\nMore, see @sec:s.\n";
    let layout = mapped(text);
    let map = layout.source_map().expect("mapped");
    let number = run_of(&layout, "1").1;
    assert_eq!(map.position(number, 0), None);
    assert_eq!(map.nearest(number, 0).offset, 0, "the start of the heading");
    assert_eq!(written(&layout, "Intro", 0), Some(offset(text, "Intro")));
    assert_eq!(written(&layout, "part", 0), Some(offset(text, "part")));
    assert_eq!(written(&layout, "Second", 0), Some(offset(text, "Second")));
    check_exact(&layout, text);
}

#[test]
fn a_contents_entry_goes_to_its_heading() {
    let text = "---\ntoc: true\n---\n\n# First\n\nBody.\n\n# Second\n\nMore.\n";
    let layout = mapped(text);
    let map = layout.source_map().expect("mapped");
    let seconds: Vec<_> = texts(&layout)
        .into_iter()
        .filter(|(.., run, _)| run.text == "Second")
        .collect();
    assert_eq!(seconds.len(), 2, "the entry and the heading");
    let (entry, heading) = (seconds[0].3, seconds[1].3);
    assert_eq!(map.position(entry, 0), None);
    assert_eq!(map.nearest(entry, 0).offset, offset(text, "# Second"));
    assert_eq!(
        map.position(heading, 0).map(|at| at.offset),
        Some(offset(text, "Second"))
    );
    check_exact(&layout, text);
}

#[test]
fn code_lines_map_by_line_and_tabs_to_nothing() {
    let text = "Before.\n\n```\nfirst line\n\tsecond\n```\n\n> ```\n> quoted\n> ```\n\n- item\n\n      indented\n";
    let layout = mapped(text);
    assert_eq!(written(&layout, "first line", 6), Some(offset(text, "line")));
    let tabbed = "    second";
    assert_eq!(written(&layout, tabbed, 0), None);
    assert_eq!(nearest(&layout, tabbed, 2), offset(text, "\tsecond"));
    assert_eq!(written(&layout, tabbed, 4), Some(offset(text, "second")));
    assert_eq!(written(&layout, "quoted", 0), Some(offset(text, "quoted")));
    assert_eq!(written(&layout, "indented", 0), Some(offset(text, "indented")));
    check_exact(&layout, text);
}

#[test]
fn highlighted_code_maps_each_colored_piece() {
    let text = "```rust\nfn main() { let x = 1; }\n```\n";
    let layout = mapped(text);
    let map = layout.source_map().expect("mapped");
    let line = "fn main() { let x = 1; }";
    let pieces: Vec<_> = texts(&layout)
        .into_iter()
        .filter(|(.., run, _)| line.contains(&run.text))
        .collect();
    assert!(!pieces.is_empty());
    let mut next = offset(text, line);
    for (.., run, id) in pieces {
        assert_eq!(map.position(id, 0).map(|at| at.offset), Some(next), "{:?}", run.text);
        next += run.text.len() as u32;
    }
    check_exact(&layout, text);
}

#[test]
fn table_cells_and_captions_map_to_where_they_are_written() {
    let text = "| Head | Other |\n|------|-------|\n| cell | text  |\n\n: The caption {#tbl:t}\n\nAfter.\n";
    let layout = mapped(text);
    assert_eq!(written(&layout, "Head", 0), Some(offset(text, "Head")));
    assert_eq!(written(&layout, "cell", 1), Some(offset(text, "ell |")));
    assert_eq!(written(&layout, "caption", 0), Some(offset(text, "caption")));
    check_exact(&layout, text);
    let list_table = "::: table\n- - One\n  - Two\n- - three\n  - four\n:::\n";
    let layout = mapped(list_table);
    assert_eq!(written(&layout, "four", 0), Some(offset(list_table, "four")));
    check_exact(&layout, list_table);
}

#[test]
fn a_chapter_opening_maps_its_drop_capital_and_lead_in() {
    let theme = r#"{"version": 1, "chapters": {"drop-cap": 2, "lead-in": 2, "number-position": "above"}}"#;
    let text = "# Opening\n\nDark and stormy was the night that followed the day, and it went on for a long time, long \
                enough to fill the lines beside the initial letter of this paragraph and more lines after it.\n";
    let layout = mapped_in(text, Some(theme), &MemoryResolver::default());
    assert_eq!(written(&layout, "D", 0), Some(offset(text, "Dark")));
    assert_eq!(written(&layout, "ark", 0), Some(offset(text, "ark and")));
    assert_eq!(written(&layout, "and", 0), Some(offset(text, "and stormy")));
    check_exact(&layout, text);
}

#[test]
fn a_place_in_a_blocks_syntax_goes_to_its_first_text() {
    let text = "Before.\n\n## Heading\n\n- first item\n- second item\n";
    let layout = mapped(text);
    let map = layout.source_map().expect("mapped");
    let at = |offset: u32| SourcePos {
        chapter: crate::output::ChapterId(0),
        offset,
    };
    let shown = |offset: u32| {
        let (page, item) = map.locate(at(offset)).expect("located");
        match &layout.pages()[page].items[item] {
            Item::Text { run, .. } => run.text.clone(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(shown(offset(text, "## Heading")), "0.1", "the heading's number");
    assert_eq!(shown(offset(text, "Heading")), "Heading");
    assert_eq!(shown(offset(text, "- second")), "•", "the item's marker");
    assert_eq!(shown(offset(text, "second")), "second");
    assert_eq!(
        shown(offset(text, "\n\n##")),
        "Before.",
        "a blank line goes to the text before it"
    );
    assert_eq!(shown(text.len() as u32 + 50), "item");
}

#[test]
fn a_point_on_a_page_finds_its_text_and_a_place_finds_its_rectangle() {
    let layout = sample("de.md", MAPPED);
    let map = layout.source_map().expect("mapped");
    let mut checked = 0;
    for (page, item_index, run, id) in texts(&layout) {
        let Item::Text { x, y, .. } = &layout.pages()[page].items[item_index] else {
            unreachable!()
        };
        let Some(start) = map.position(id, 0) else { continue };
        let on_glyph = Pt(x.0 + 0.1);
        let Some(hit) = layout.hit(page, on_glyph, Pt(y.0 - 0.3 * run.size.0)) else {
            panic!("a text item is on its page")
        };
        assert_eq!((hit.item, hit.byte), (item_index, 0), "{:?}", run.text);
        assert_eq!(
            layout.position_at(page, on_glyph, Pt(y.0 - 0.3 * run.size.0)),
            Some(start)
        );
        let located = layout.locate(start).expect("located");
        assert_eq!((located.page, located.item), (page, item_index));
        assert_eq!(located.rect.x, *x);
        assert!(located.rect.y.0 < y.0 && located.rect.y.0 + located.rect.height.0 > y.0);
        checked += 1;
    }
    assert!(checked > 100);
    assert_eq!(layout.hit(9999, Pt(0.0), Pt(0.0)), None);
}

#[test]
fn a_project_maps_to_each_chapters_own_file() {
    let dir = PathBuf::from("/book");
    let origin = Origin::Document(dir.clone());
    let mut resolver = MemoryResolver::default();
    let manifest = "title: B\nchapters:\n  - one.md\n  - two.md\n";
    let two = "\u{feff}# Two\n\nSecond <!-- cut --> chapter.\n";
    resolver.insert(origin.clone(), "druck.yaml", manifest.as_bytes());
    resolver.insert(origin.clone(), "one.md", b"# One\n\nFirst chapter.\n".as_slice());
    resolver.insert(origin.clone(), "two.md", two.as_bytes());
    let project = Project::open(&resolver, &dir).unwrap_or_else(|errors| panic!("{errors:?}"));
    let (text, _) = project.export(None).unwrap_or_else(|errors| panic!("{errors:?}"));
    let input = input(text);
    let layout = on_big_stack(|| lay_out(&resolver, &input, MAPPED));
    let map = layout.source_map().expect("mapped");
    let paths: Vec<_> = map.chapters().iter().map(|chapter| chapter.path.clone()).collect();
    assert_eq!(paths, [dir.join("one.md"), dir.join("two.md")]);
    let (_, first) = run_of(&layout, "First");
    let at = map.position(first, 0).expect("written");
    assert_eq!((at.chapter.0, at.offset), (0, 7));
    let (_, second) = run_of(&layout, "chapter.");
    let found: Vec<_> = texts(&layout)
        .into_iter()
        .filter(|(.., run, _)| run.text == "chapter.")
        .collect();
    assert_eq!(found.len(), 2);
    let at = map.position(found[1].3, 0).expect("written");
    assert_eq!((at.chapter.0, at.offset), (1, offset(two, "chapter.")));
    assert_eq!(map.position(second, 0).map(|at| at.chapter.0), Some(0));
    let (page, item) = map.locate(at).expect("located");
    assert_eq!((page, item), (found[1].0, found[1].1));
    let heading = map
        .blocks()
        .iter()
        .filter(|block| block.range.0.chapter.0 == 1)
        .map(|block| block.range.0.offset);
    assert_eq!(heading.min(), Some(3), "after the byte order mark");
}
