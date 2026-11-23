//! The limits of `docs/DECISIONS.md` (2026-11-03): a 100-page document renders in under 2 s with under 150 MB.
//! The input is the body of `samples/report.md` repeated with unique labels and notes, the heaviest sample per
//! page. Memory is the peak of live heap allocations in this process, counted by the allocator below.
//!
//! The limits hold for release builds, so the test is ignored in debug ones. CI runs it with
//! `cargo test --release --test performance`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use druck::cancel::Cancel;
use druck::input::{Date, DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::resource::{FileResolver, Origin};
use druck::{PdfOptions, Renderer};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples");
const MAX_TIME: Duration = Duration::from_secs(2);
const MAX_HEAP: usize = 150 * 1024 * 1024;
const MIN_PAGES: usize = 100;
const COPIES: usize = 12;
const RUNS: usize = 3;
/// Layout in a debug build needs more stack than a test thread has by default; a release build is given the same.
const STACK: usize = 16 * 1024 * 1024;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY: every call goes to the system allocator unchanged; the counters only observe sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            grew(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            grew(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, size) };
        if !moved.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            grew(size);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The report's front matter once and its body `copies` times, each copy with its own labels and note names.
fn repeated_report(copies: usize) -> String {
    let text = std::fs::read_to_string(format!("{SAMPLES}/report.md")).expect("the report sample");
    let text = text.replace("\r\n", "\n");
    let (front, body) = (text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---\n"))).expect("front matter");
    let mut out = format!("---\n{front}\n---\n");
    for copy in 0..copies {
        let mut body = body.replace("[^", &format!("[^c{copy}-"));
        for kind in ["sec:", "fig:", "tbl:"] {
            body = body.replace(kind, &format!("{kind}c{copy}-"));
        }
        out.push_str(&body);
    }
    out
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the limits apply to release builds")]
fn a_hundred_pages_render_within_the_time_and_memory_limits() {
    let input = Input {
        text: DocumentText::Single {
            text: repeated_report(COPIES),
            origin: Origin::Document(PathBuf::from(SAMPLES)),
            name: "report.md".to_owned(),
        },
        theme: ThemeSelection::Document {
            dirs: ThemeDirs::default(),
        },
        overrides: Overrides::default(),
        today: Date::from_unix(0),
    };
    let render = || {
        let start = Instant::now();
        let rendered = Renderer::new()
            .render(&FileResolver::new(), &input, &PdfOptions::default(), &Cancel::default())
            .unwrap_or_else(|failure| panic!("{failure}"));
        (start.elapsed(), rendered.parts)
    };

    // Like a run of the CLI, every render starts with a new renderer. The first run also loads what the process
    // keeps after it, such as the syntax set, so its peak is the one a single CLI run has.
    let (mut times, pages, peak) = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(STACK)
            .spawn_scoped(scope, || {
                PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
                let (first, pages) = render();
                let peak = PEAK.load(Ordering::Relaxed);
                let mut times = vec![first];
                times.extend((1..RUNS).map(|_| render().0));
                (times, pages, peak)
            })
            .expect("a thread")
            .join()
            .expect("no panic")
    });
    times.sort();
    let median = times[RUNS / 2];
    eprintln!("{pages} pages: median {median:?} of {times:?}, peak heap {} MB", peak / (1024 * 1024));

    assert!(pages >= MIN_PAGES, "the input has {pages} pages, fewer than {MIN_PAGES}");
    assert!(median < MAX_TIME, "{pages} pages took {median:?}, over {MAX_TIME:?}");
    assert!(peak < MAX_HEAP, "{pages} pages used {} MB of heap, over 150 MB", peak / (1024 * 1024));
}
