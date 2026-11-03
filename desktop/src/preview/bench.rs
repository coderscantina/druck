//! Measurements and fixed views of the preview, driven by the environment and off by default.
//!
//! `DRUCK_PREVIEW_ZOOM=1.5` and `DRUCK_PREVIEW_PAGE=12` open at a zoom and a page, for comparing screenshots.
//! `DRUCK_PREVIEW_BENCH=1` then times the first layout and paint, painting every page, scrolling the whole
//! document, and 50 refreshes that change one word, prints the numbers to stderr, and quits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{Context, Window};

use super::job::Layout;
use super::raster::Painter;
use super::{CACHE_BYTES, PreviewView};

const REFRESHES: usize = 50;
/// Pixels per frame unless `DRUCK_PREVIEW_SCROLL` says otherwise.
const SCROLL_PER_FRAME: f32 = 60.0;
/// Asks for frames faster than any display, so frame intervals show the display's rate or the stalls.
const TICK: Duration = Duration::from_millis(4);

enum Phase {
    Off,
    FirstPaint,
    Painting,
    Scrolling,
    Refreshing { done: usize },
    Finished,
}

pub struct Bench {
    zoom: Option<f32>,
    page: Option<usize>,
    phase: Phase,
    requested: Instant,
    published: Option<Instant>,
    last_frame: Option<Instant>,
    /// Frame intervals of the running phase. gpui caps a window that is not frontmost at 30 frames per
    /// second, so these only show stalls beyond that. `work` shows the cost.
    intervals: Vec<Duration>,
    /// Main thread time from placing sheets to painting them, per frame of the running phase.
    work: Vec<Duration>,
    frame_started: Option<Instant>,
    /// Frames of the running phase that lacked a page's exact image.
    unfinished: usize,
    /// Request to published, and to painted.
    refreshes: Vec<(Duration, Duration)>,
    text: String,
    awaiting_paint: bool,
    scroll_per_frame: f32,
    ticking: bool,
}

impl Bench {
    pub fn from_env() -> Option<Self> {
        let var = |name| std::env::var(name).ok();
        let run = var("DRUCK_PREVIEW_BENCH").is_some();
        let zoom = var("DRUCK_PREVIEW_ZOOM").and_then(|zoom| zoom.parse().ok());
        let page = var("DRUCK_PREVIEW_PAGE").and_then(|page| page.parse().ok());
        (run || zoom.is_some() || page.is_some()).then(|| Self {
            zoom,
            page,
            phase: if run { Phase::FirstPaint } else { Phase::Off },
            requested: Instant::now(),
            published: None,
            last_frame: None,
            intervals: Vec::new(),
            work: Vec::new(),
            frame_started: None,
            unfinished: 0,
            refreshes: Vec::new(),
            text: String::new(),
            awaiting_paint: run,
            scroll_per_frame: var("DRUCK_PREVIEW_SCROLL")
                .and_then(|speed| speed.parse().ok())
                .unwrap_or(SCROLL_PER_FRAME),
            ticking: false,
        })
    }

    pub fn frame_started(&mut self) {
        self.frame_started = Some(Instant::now());
    }

    pub fn frame_painted(&mut self) {
        if let Some(started) = self.frame_started.take() {
            self.work.push(started.elapsed());
        }
    }

    pub fn zoom(&self) -> Option<f32> {
        self.zoom
    }

    pub fn take_page(&mut self) -> Option<usize> {
        self.page.take()
    }

    /// Notes a request. The first one's text is what the refreshes edit.
    pub fn requested(&mut self, text: &str) {
        if self.text.is_empty() && !matches!(self.phase, Phase::Off) {
            self.text = text.to_owned();
        }
        self.requested = Instant::now();
        self.published = None;
        self.awaiting_paint = !matches!(self.phase, Phase::Off);
    }

    pub fn published(&mut self, layout: &Layout) {
        if matches!(self.phase, Phase::Off) {
            return;
        }
        self.published = Some(Instant::now());
        let timing = layout.timing;
        eprintln!(
            "layout: {} pages in {:?} (configure {:?}, fonts {:?}, typeset {:?}, fingerprints {:?})",
            layout.output.pages.len(),
            self.requested.elapsed(),
            timing.configure,
            timing.fonts,
            timing.typeset,
            timing.fingerprints,
        );
    }
}

/// Called once per frame while a bench is set up. `complete` says the visible pages have their exact images.
pub fn frame(
    view: &mut PreviewView,
    complete: bool,
    missing: usize,
    content: f32,
    window: &mut Window,
    cx: &mut Context<PreviewView>,
) {
    let Some(bench) = view.bench.as_mut() else {
        return;
    };
    if matches!(bench.phase, Phase::Off) {
        return;
    }
    let now = Instant::now();
    if let Some(last) = bench.last_frame {
        bench.intervals.push(now - last);
    }
    bench.last_frame = Some(now);
    bench.unfinished += usize::from(missing > 0);

    let painted = complete && bench.published.is_some() && bench.awaiting_paint;
    if painted {
        bench.awaiting_paint = false;
    }
    let mut next_text = None;
    match bench.phase {
        Phase::FirstPaint if painted => {
            eprintln!(
                "first paint: {:?} after the request; {}",
                bench.requested.elapsed(),
                memory()
            );
            bench.phase = Phase::Painting;
            if let Some(layout) = view.layout.clone() {
                let resolution = (view.current_zoom * window.scale_factor()).min(super::MAX_RESOLUTION);
                paint_all(layout, resolution, cx);
            }
        }
        Phase::Scrolling => {
            view.scroll.1 += bench.scroll_per_frame;
            if view.scroll.1 + view.view.1 >= content {
                report_frames("scrolling", bench);
                eprintln!("after scrolling: {}", memory());
                bench.phase = Phase::Refreshing { done: 0 };
                bench.awaiting_paint = false;
                // At the edit, so each refresh repaints the pages it changed.
                view.scroll.1 = content / 2.0;
                next_text = Some(0);
            }
        }
        Phase::Refreshing { done } if painted => {
            let published = bench.published.map_or(Duration::ZERO, |at| at - bench.requested);
            bench.refreshes.push((published, bench.requested.elapsed()));
            let done = done + 1;
            eprintln!("refresh {done}: {}", memory());
            if done == REFRESHES {
                report_frames("during refreshes", bench);
                report_refreshes(bench);
                bench.phase = Phase::Finished;
                cx.defer(|cx| cx.quit());
            } else {
                bench.phase = Phase::Refreshing { done };
                next_text = Some(done);
            }
        }
        _ => {}
    }
    let start_ticking = !bench.ticking;
    bench.ticking = true;
    if let Some(round) = next_text {
        let text = edited(&bench.text, round);
        view.request(text, cx);
    }
    if start_ticking {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        })
        .detach();
    }
}

/// Paints every page on one thread, then on the view's raster threads, and starts scrolling after.
fn paint_all(layout: Arc<Layout>, resolution: f32, cx: &mut Context<PreviewView>) {
    let timed = cx.background_spawn(async move {
        let count = layout.output.pages.len();
        let start = Instant::now();
        paint_range(&layout.painter, &layout, 0, 1, resolution);
        let one = start.elapsed();
        let start = Instant::now();
        std::thread::scope(|scope| {
            for thread in 0..super::RASTER_THREADS {
                let layout = &layout;
                scope.spawn(move || paint_range(&layout.painter, layout, thread, super::RASTER_THREADS, resolution));
            }
        });
        let many = start.elapsed();
        eprintln!(
            "painting {count} pages at {resolution} px/pt: {:?} on one thread ({:.0} pages/s), {:?} on {} threads ({:.0} pages/s)",
            one,
            count as f64 / one.as_secs_f64(),
            many,
            super::RASTER_THREADS,
            count as f64 / many.as_secs_f64(),
        );
        eprintln!("after painting: {}", memory());
    });
    cx.spawn(async move |this, cx| {
        timed.await;
        this.update(cx, |view, cx| {
            if let Some(bench) = &mut view.bench {
                bench.phase = Phase::Scrolling;
                bench.intervals.clear();
                bench.work.clear();
                bench.unfinished = 0;
                bench.last_frame = None;
            }
            cx.notify();
        })
        .ok();
    })
    .detach();
}

fn paint_range(painter: &Painter, layout: &Layout, offset: usize, every: usize, resolution: f32) {
    for page in layout.output.pages.iter().skip(offset).step_by(every) {
        std::hint::black_box(painter.paint(page, resolution));
    }
}

/// The document with a changing word in the middle of its body, which reflows part of it.
fn edited(text: &str, round: usize) -> String {
    let middle = text.len() / 2;
    let at = text[middle..].find(". ").map_or(middle, |found| middle + found + 2);
    format!("{}Wort{round} {}", &text[..at], &text[at..])
}

fn report_frames(name: &str, bench: &mut Bench) {
    fn at(sorted: &[Duration], share: f64) -> Duration {
        let index = ((sorted.len() as f64 - 1.0) * share) as usize;
        sorted.get(index).copied().unwrap_or_default()
    }
    bench.intervals.sort();
    bench.work.sort();
    let (intervals, work) = (&bench.intervals, &bench.work);
    eprintln!(
        "frames {name}: {} frames, interval median {:?} max {:?}; work median {:?}, p95 {:?}, p99 {:?}, max {:?}; {} with a page not yet exact",
        intervals.len(),
        at(intervals, 0.5),
        at(intervals, 1.0),
        at(work, 0.5),
        at(work, 0.95),
        at(work, 0.99),
        at(work, 1.0),
        bench.unfinished,
    );
    bench.intervals.clear();
    bench.work.clear();
    bench.unfinished = 0;
}

fn report_refreshes(bench: &Bench) {
    let mut published: Vec<_> = bench.refreshes.iter().map(|refresh| refresh.0).collect();
    let mut painted: Vec<_> = bench.refreshes.iter().map(|refresh| refresh.1).collect();
    published.sort();
    painted.sort();
    eprintln!(
        "refreshes: {} requests, to published median {:?} max {:?}, to painted median {:?} max {:?}; cache budget {} MB",
        bench.refreshes.len(),
        published[published.len() / 2],
        published[published.len() - 1],
        painted[painted.len() / 2],
        painted[painted.len() - 1],
        CACHE_BYTES / 1024 / 1024,
    );
}

/// Resident memory, and the live heap where the platform says.
pub fn memory() -> String {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output();
    let resident = output
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|rss| rss.trim().parse::<u64>().ok())
        .map_or(0, |kb| kb / 1024);
    format!("resident {resident} MB{}", heap())
}

/// What the allocator has handed out and not got back.
#[cfg(target_os = "macos")]
fn heap() -> String {
    #[repr(C)]
    struct MemoryStats {
        bytes_total: usize,
        chunks_used: usize,
        bytes_used: usize,
        chunks_free: usize,
        bytes_free: usize,
    }
    unsafe extern "C" {
        fn mstats() -> MemoryStats;
    }
    // SAFETY: mstats only reads the allocator's counters.
    let used = unsafe { mstats() }.bytes_used;
    format!(", heap in use {} MB", used / 1024 / 1024)
}

#[cfg(not(target_os = "macos"))]
fn heap() -> String {
    String::new()
}
