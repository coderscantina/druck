//! Cancellation: each check point stops a job where it stands, nothing partial comes back, and the renderer stays
//! good. A probing token cancels at a chosen visit of one check point and counts the visits of all.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::cancel::{Cancel, CheckPoint};
use crate::input::{Date, DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use crate::output::Layout;
use crate::renderer::{EpubOptions, Failure, LayoutOptions, PdfOptions, Renderer};
use crate::resource::{MemoryResolver, Origin};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
/// Layout in a debug build needs more stack than a test thread has by default.
const STACK: usize = 16 * 1024 * 1024;

fn on_big_stack<T: Send>(run: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        let thread = std::thread::Builder::new().stack_size(STACK);
        thread
            .spawn_scoped(scope, run)
            .expect("thread")
            .join()
            .expect("no panic")
    })
}

fn dir() -> PathBuf {
    PathBuf::from("/memory")
}

fn input(text: &str) -> Input {
    Input {
        text: DocumentText::Single {
            text: text.to_owned(),
            origin: Origin::Document(dir()),
            name: "doc.md".to_owned(),
        },
        theme: ThemeSelection::Document {
            dirs: ThemeDirs::default(),
        },
        overrides: Overrides::default(),
        today: Date::from_unix(0),
    }
}

fn layout(renderer: &Renderer, resolver: &MemoryResolver, input: &Input, cancel: &Cancel) -> Result<Layout, Failure> {
    on_big_stack(|| (renderer.job(resolver, input, cancel).configure()?).layout(&LayoutOptions::default()))
}

fn pdf(renderer: &Renderer, resolver: &MemoryResolver, input: &Input) -> Vec<u8> {
    on_big_stack(|| {
        let options = PdfOptions::default();
        let rendered = renderer.render(resolver, input, &options, &Cancel::default());
        rendered.unwrap_or_else(|failure| panic!("{failure:?}")).bytes
    })
}

fn is_cancelled<T>(result: Result<T, Failure>) -> bool {
    matches!(result, Err(Failure::Cancelled))
}

/// Paragraphs of about sixty words: `count` of them take about `count / 3` pages.
fn paragraphs(count: usize) -> String {
    let paragraph = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
                     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
                     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
                     voluptate velit esse cillum dolore eu fugiat nulla pariatur.\n\n";
    paragraph.repeat(count)
}

/// A resolver with the PNG fixture under the given names.
fn with_images(names: &[&str]) -> MemoryResolver {
    let data = std::fs::read(format!("{FIXTURES}/images/pixel.png")).expect("fixture");
    let mut resolver = MemoryResolver::new();
    for name in names {
        let copy: Vec<u8> = data.iter().copied().chain(name.bytes()).collect();
        resolver.insert(Origin::Document(dir()), name, copy);
    }
    resolver
}

const BIBLIOGRAPHY: &str = "@misc{a, title = {A}, author = {B}, url = {https://example.com}, year = {2020}}\n";

/// A resolver and input with a bibliography, for the stages before layout.
fn cited() -> (MemoryResolver, Input) {
    let mut resolver = MemoryResolver::new();
    resolver.insert(Origin::Document(dir()), "refs.bib", BIBLIOGRAPHY.as_bytes());
    let text = "---\nbibliography: refs.bib\n---\n\nSee [@a] and [@a].\n";
    (resolver, input(text))
}

#[test]
fn the_resource_check_point_stops_before_the_next_read() {
    let (resolver, input) = cited();
    let cancel = Cancel::at(CheckPoint::Resource, 1);
    let renderer = Renderer::new();
    assert!(is_cancelled(layout(&renderer, &resolver, &input, &cancel)));
    assert_eq!(cancel.visits(CheckPoint::Resource), 1);
    assert_eq!(cancel.visits(CheckPoint::Face), 0);
}

#[test]
fn the_face_check_point_stops_before_the_next_face() {
    let cancel = Cancel::at(CheckPoint::Face, 3);
    let renderer = Renderer::new();
    assert!(is_cancelled(layout(
        &renderer,
        &MemoryResolver::new(),
        &input("Text.\n"),
        &cancel
    )));
    assert_eq!(cancel.visits(CheckPoint::Face), 3);
    assert_eq!(renderer.faces.builds.load(Ordering::Relaxed), 2, "only finished faces");
    assert_eq!(cancel.visits(CheckPoint::Block), 0);
}

#[test]
fn the_bibliography_check_point_stops_before_the_parse() {
    let (resolver, input) = cited();
    let cancel = Cancel::at(CheckPoint::Bibliography, 1);
    assert!(is_cancelled(layout(&Renderer::new(), &resolver, &input, &cancel)));
    assert_eq!(cancel.visits(CheckPoint::Citation), 0);
}

#[test]
fn the_citation_check_point_stops_before_the_next_citation() {
    let (resolver, input) = cited();
    let cancel = Cancel::at(CheckPoint::Citation, 2);
    assert!(is_cancelled(layout(&Renderer::new(), &resolver, &input, &cancel)));
    assert_eq!(cancel.visits(CheckPoint::Citation), 2);
    assert_eq!(cancel.visits(CheckPoint::Image), 0);
}

#[test]
fn the_image_check_point_stops_before_the_next_decode() {
    let resolver = with_images(&["one.png", "two.png", "three.png"]);
    let input = input("![A](one.png)\n\n![B](two.png)\n\n![C](three.png)\n");
    let cancel = Cancel::at(CheckPoint::Image, 2);
    let renderer = Renderer::new();
    assert!(is_cancelled(layout(&renderer, &resolver, &input, &cancel)));
    assert_eq!(
        renderer.images.decodes.load(Ordering::Relaxed),
        1,
        "the second image is not decoded"
    );
    assert_eq!(cancel.visits(CheckPoint::Pass), 0);
}

#[test]
fn the_pass_check_point_stops_before_the_next_pass() {
    // Contents that show a page other than the 1 the first pass assumes need a second pass.
    let text = format!("---\ntoc: true\n---\n\n# One\n\n{}# Two\n\nText.\n", paragraphs(30));
    let cancel = Cancel::at(CheckPoint::Pass, 2);
    assert!(is_cancelled(layout(
        &Renderer::new(),
        &MemoryResolver::new(),
        &input(&text),
        &cancel
    )));
    assert_eq!(cancel.visits(CheckPoint::Pass), 2);
    let one_pass = cancel.visits(CheckPoint::Block);
    assert!(one_pass >= 32, "a whole pass ran first: {one_pass}");
    assert!(
        cancel.visits(CheckPoint::Pagination) >= 1,
        "the search of the first pass ran"
    );
}

#[test]
fn the_block_check_point_stops_before_the_next_block() {
    let cancel = Cancel::at(CheckPoint::Block, 5);
    let text = paragraphs(20);
    assert!(is_cancelled(layout(
        &Renderer::new(),
        &MemoryResolver::new(),
        &input(&text),
        &cancel
    )));
    assert_eq!(cancel.visits(CheckPoint::Block), 5);
    assert_eq!(
        cancel.visits(CheckPoint::Pagination),
        0,
        "no page breaks for a cancelled flow"
    );
}

#[test]
fn the_pagination_check_point_stops_the_search_every_64_lines() {
    let cancel = Cancel::at(CheckPoint::Pagination, 2);
    let text = paragraphs(60);
    assert!(is_cancelled(layout(
        &Renderer::new(),
        &MemoryResolver::new(),
        &input(&text),
        &cancel
    )));
    assert_eq!(cancel.visits(CheckPoint::Pagination), 2);
    assert_eq!(cancel.visits(CheckPoint::Pass), 1);

    let all = Cancel::at(CheckPoint::Pagination, usize::MAX);
    let laid = layout(&Renderer::new(), &MemoryResolver::new(), &input(&text), &all);
    assert!(laid.is_ok());
    assert!(
        all.visits(CheckPoint::Pagination) > 3,
        "a search this long looks more than twice"
    );
}

#[test]
fn a_cancelled_flow_with_nested_blocks_stops_cleanly_at_every_block() {
    let text = "# Head\n\nText with a note.[^a]\n\n[^a]: The note.\n\n    \
                More of it.\n\n> A quote\n>\n> > nested\n\n- One\n\n  Two\n- ![A](one.png)\n\n| A | B |\n|---|---|\n| \
                1 | 2 |\n\n```rust\nfn main() {}\n```\n\n::: keep\nKept.\n\nAnd more.\n:::\n\n> [!NOTE]\n> A callout.\n\n\
                ::: columns\nOne.\n\n::: full-width\nWide.\n:::\n\nTwo.\n:::\n\n::: bottom\nLow.\n:::\n\nEnd.\n";
    let resolver = with_images(&["one.png"]);
    let input = input(text);
    let renderer = Renderer::new();
    let mut stopped = 0;
    for nth in 1.. {
        let cancel = Cancel::at(CheckPoint::Block, nth);
        match layout(&renderer, &resolver, &input, &cancel) {
            Err(Failure::Cancelled) => stopped += 1,
            Err(Failure::Errors(errors)) => panic!("{errors:?}"),
            Ok(_) => break,
        }
    }
    assert!(stopped > 15, "the document has nested blocks: {stopped}");
}

#[test]
fn the_page_check_point_stops_an_export_with_no_bytes() {
    let resolver = MemoryResolver::new();
    let input = input(&paragraphs(80));
    let renderer = Renderer::new();
    let laid = layout(&renderer, &resolver, &input, &Cancel::default()).unwrap_or_else(|f| panic!("{f:?}"));
    let cancel = Cancel::at(CheckPoint::Page, 3);
    let options = PdfOptions::default();
    assert!(is_cancelled(on_big_stack(|| laid.pdf(&options, &cancel))));
    assert_eq!(cancel.visits(CheckPoint::Page), 3);
    let whole = on_big_stack(|| laid.pdf(&options, &Cancel::default())).unwrap_or_else(|f| panic!("{f:?}"));
    assert!(whole.parts > 3, "the document has more pages");
    assert_eq!(
        whole.bytes,
        pdf(&renderer, &resolver, &input),
        "the layout is not harmed"
    );
}

#[test]
fn a_cancel_while_finishing_a_pdf_returns_no_bytes() {
    let resolver = MemoryResolver::new();
    let input = input("Text.\n");
    let laid = layout(&Renderer::new(), &resolver, &input, &Cancel::default()).unwrap_or_else(|f| panic!("{f:?}"));
    // One page visits once before it is drawn and once after finishing, the second of which trips.
    let cancel = Cancel::at(CheckPoint::Page, 2);
    assert!(is_cancelled(on_big_stack(|| laid.pdf(&PdfOptions::default(), &cancel))));
}

#[test]
fn the_chapter_check_point_stops_an_epub_with_no_bytes() {
    let resolver = MemoryResolver::new();
    let input = input("# One\n\nText.\n\n# Two\n\nText.\n\n# Three\n\nText.\n");
    let renderer = Renderer::new();
    let epub = |cancel: &Cancel| {
        on_big_stack(|| (renderer.job(&resolver, &input, cancel).configure()?).epub(&EpubOptions::default()))
    };
    let cancel = Cancel::at(CheckPoint::Chapter, 2);
    assert!(is_cancelled(epub(&cancel)));
    assert_eq!(cancel.visits(CheckPoint::Chapter), 2);
    let last = Cancel::at(CheckPoint::Chapter, usize::MAX);
    let whole = epub(&last).unwrap_or_else(|f| panic!("{f:?}"));
    assert_eq!(whole.parts, 3);
    assert_eq!(
        last.visits(CheckPoint::Chapter),
        9,
        "two passes, then each file written"
    );
}

#[test]
fn cancelled_jobs_keep_what_the_renderer_holds() {
    let resolver = with_images(&["one.png"]);
    let input = input("![A](one.png)\n");
    let renderer = Renderer::new();
    pdf(&renderer, &resolver, &input);
    let (faces, decodes) = (
        renderer.faces.builds.load(Ordering::Relaxed),
        renderer.images.decodes.load(Ordering::Relaxed),
    );
    for _ in 0..4 {
        let cancel = Cancel::at(CheckPoint::Face, 1);
        assert!(is_cancelled(layout(&renderer, &resolver, &input, &cancel)));
    }
    pdf(&renderer, &resolver, &input);
    assert_eq!(
        renderer.faces.builds.load(Ordering::Relaxed),
        faces,
        "no face is built again"
    );
    assert_eq!(
        renderer.images.decodes.load(Ordering::Relaxed),
        decodes,
        "no image is decoded again"
    );
}

/// What the design promises in a release build: 30 ms from a cancel to `Cancelled`, given about 20 ms for the longest
/// block. A debug build runs about ten times slower.
const BOUND: Duration = Duration::from_millis(if cfg!(debug_assertions) { 300 } else { 30 });

#[test]
fn a_cancel_from_another_thread_stops_a_large_layout_promptly() {
    // Long enough that layout is still running after the wait in either build.
    let count = if cfg!(debug_assertions) { 400 } else { 2000 };
    let wait = Duration::from_millis(if cfg!(debug_assertions) { 300 } else { 50 });
    let resolver = MemoryResolver::new();
    let large = input(&paragraphs(count));
    let renderer = Renderer::new();
    let mut latencies = Vec::new();
    for _ in 0..7 {
        let cancel = Cancel::default();
        let canceller = cancel.clone();
        let stop = std::thread::spawn(move || {
            std::thread::sleep(wait);
            canceller.cancel();
            Instant::now()
        });
        let result = layout(&renderer, &resolver, &large, &cancel);
        let returned = Instant::now();
        assert!(is_cancelled(result), "the layout outlasts the wait");
        latencies.push(returned.saturating_duration_since(stop.join().expect("no panic")));
    }
    latencies.sort();
    let median = latencies[latencies.len() / 2];
    eprintln!("cancel to Cancelled: median {median:?}, all {latencies:?}");
    assert!(median <= BOUND, "{latencies:?}");

    let small = input("The same renderer.\n");
    assert_eq!(
        pdf(&renderer, &resolver, &small),
        pdf(&Renderer::new(), &resolver, &small)
    );
}
