//! Publish's pages: the engine's pages painted natively, never through PDF, in spreads or, zoomed out past fit, as a
//! contact sheet of every page. The status line has the zoom.
//!
//! The workspace calls [`PreviewView::request`] with the session's joined text and hears [`Typeset`] when pages change.
//! In Write they are not on screen, and [`PreviewView::ready_ahead`] keeps what Publish would show ready to paint.
//!
//! A click on a line opens the [`slip::ProofSlip`] through the workspace and [`slip_control`], which this view shows at its paragraph. Problems
//! come in through [`PreviewView::set_problems`], and only these and the slip's paragraph are ever marked on a page.

mod bench;
mod cache;
pub mod directives;
pub mod fonts;
pub(crate) mod job;
pub mod proof;
mod raster;
mod sheets;
pub mod slip;
pub mod slip_control;
mod strings;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use druck::Failure;
use druck::diagnostic::Diagnostic;
use druck::output::{ChapterId, Item, SourcePos};
use gpui::prelude::*;
use gpui::{
    Anchor, AnyWindowHandle, App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    MouseDownEvent, Pixels, RenderImage, Role, ScrollEvent, SharedString, Window, actions, anchored, canvas, deferred,
    div, fill, point, px, size,
};
use image::RgbaImage;
use resvg::tiny_skia::Pixmap;

use self::bench::Bench;
pub use self::bench::memory;
use self::cache::{PageCache, PageKey};
pub use self::fonts::FontUse;
pub use self::job::BookType;
use self::job::{Job, Layout, Scheduler, Started, Typesetter};
use self::proof::{BlockAt, Mark, Marks, Problem};
use self::raster::pixel_size;
use self::sheets::{Arrangement, Sheets, clamp_scroll};
use self::slip::ProofSlip;
use self::strings::Key;
use crate::a11y::Control as _;
use crate::session::{Joined, Kind};
use crate::theme_file::blocks::{self, Target};
use crate::tokens::{self, Tokens};

actions!(preview, [NextScreen, PreviousScreen, CloseSlip]);

/// The bundled specimen: every element of a document, to see a theme on.
use crate::theme_file::SPECIMEN;
/// Where the specimen pretends to be, beside the document, so relative paths in a theme resolve as for the document.
const SPECIMEN_FILE: &str = "specimen.md";

/// Zoom levels above fit, as logical pixels per point.
const ZOOMS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
/// Zoom levels below fit, which show a contact sheet. Small enough that it reads as one, whatever the window.
const CONTACT_ZOOMS: [f32; 4] = [0.14, 0.2, 0.3, 0.45];
/// Fit follows the view below every fixed level, so a narrow window shows a whole spread. This only keeps a sliver of a
/// view from asking for sheets of no size.
const SMALLEST_FIT: f32 = 0.02;
/// Sharper than this costs too much memory per page; higher zoom enlarges these pixels.
const MAX_RESOLUTION: f32 = 4.0;
/// Rasterized pages kept in memory. Painted ones also have a copy in the GPU atlas.
const CACHE_BYTES: usize = 128 * 1024 * 1024;
/// Pages rasterized at the same time.
const RASTER_THREADS: usize = 3;
/// Pages rasterized beyond each end of the view, so normal scrolling shows finished pages.
const PREFETCH_PAGES: usize = 2;

/// How large sheets are drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Zoom {
    /// A whole spread fills the view.
    Fit,
    Fixed(f32),
}

/// The next step of the one zoom scale from `now`, which is `current` pixels per point, where a spread fits at `fit`:
/// contact sheets below fit, then fit, then larger. `None` at either end.
fn next_zoom(now: Zoom, current: f32, fit: f32, direction: isize) -> Option<Zoom> {
    let below_fit = CONTACT_ZOOMS.iter().copied().filter(|zoom| *zoom < fit * 0.99);
    let above_fit = ZOOMS.iter().copied().filter(|zoom| *zoom > fit * 1.01);
    let levels = below_fit
        .map(Zoom::Fixed)
        .chain([Zoom::Fit])
        .chain(above_fit.map(Zoom::Fixed));
    let value = |zoom: &Zoom| match zoom {
        Zoom::Fit => fit,
        Zoom::Fixed(zoom) => *zoom,
    };
    let current = if now == Zoom::Fit { fit } else { current };
    if direction > 0 {
        levels.into_iter().find(|zoom| value(zoom) > current * 1.01)
    } else {
        levels.into_iter().rev().find(|zoom| value(zoom) < current * 0.99)
    }
}

/// A sheet to paint this frame.
struct Placed {
    bounds: Bounds<Pixels>,
    page: usize,
    image: Option<Arc<RenderImage>>,
    /// Rastered for this zoom and these pages, not an older image standing in.
    exact: bool,
}

/// New pages were published, with the theme's type they were set in.
pub struct Typeset(pub BookType);

impl EventEmitter<Typeset> for PreviewView {}

/// A layout finished and its result counts, whether it made pages or only errors. The shell reads the diagnostics.
pub struct Laid;

impl EventEmitter<Laid> for PreviewView {}

/// A click on a page landed on this block, or on nothing that has one.
pub struct PageClicked(pub Option<BlockAt>);

impl EventEmitter<PageClicked> for PreviewView {}

/// The zoom changed, to keep between runs.
pub struct ZoomChanged;

impl EventEmitter<ZoomChanged> for PreviewView {}

/// The author asked to see the list of problems from the status line.
pub struct ProblemsWanted;

impl EventEmitter<ProblemsWanted> for PreviewView {}

pub struct PreviewView {
    focus: FocusHandle,
    scheduler: Scheduler<Job>,
    typesetter: Arc<Mutex<Typesetter>>,
    /// Typesets the bundled specimen in the same theme, for a view of every element.
    specimen_typesetter: Arc<Mutex<Typesetter>>,
    /// The theme to typeset with, if not the document's, which the next layout follows.
    theme: Option<PathBuf>,
    /// The document's text as last asked for, to typeset again when the theme changes or the specimen goes.
    latest: Option<Joined>,
    /// Whether the pages show the specimen instead of the document.
    specimen: bool,
    layout: Option<Arc<Layout>>,
    /// Why the latest text has no pages. The last good pages stay on screen meanwhile.
    diagnostics: Vec<Diagnostic>,
    cache: PageCache<Arc<RenderImage>>,
    rastering: HashSet<PageKey>,
    /// Evicted images whose atlas tiles to free on the next frame.
    dropped: Vec<Arc<RenderImage>>,
    zoom: Zoom,
    /// The zoom of the last frame, for moves between zoom levels.
    current_zoom: f32,
    /// The zoom at which a spread fills the view. Below it, sheets show as a contact sheet.
    fit: f32,
    /// The page to bring to the middle of the view on the next frame, once the new zoom is known.
    centre_on: Option<usize>,
    scroll: (f32, f32),
    view: (f32, f32),
    /// The first page on screen in the last frame.
    page: usize,
    bench: Option<Bench>,
    /// The request whose pages are published.
    published: u64,
    /// The revision of the text those pages were set from, which their source map's offsets refer to.
    revision: Option<u64>,
    /// Where the canvas of pages is in the window.
    origin: (f32, f32),
    /// The newest request whose visible pages were all exact, and when.
    ready: (u64, Instant),
    /// The last frame in which every visible page was exact.
    exact_frame: Option<Instant>,
    /// While the pages are not on screen: where they would paint, to raster for.
    ahead: Option<Ahead>,
    slip: Entity<ProofSlip>,
    /// The file and bytes of the slip's paragraph, as the pages on screen have them, and where that is on them.
    slip_source: Option<(PathBuf, std::ops::Range<usize>)>,
    slip_marks: Vec<Mark>,
    /// The page of the paragraph the slip stands on, which is where it was clicked when it runs over two pages.
    slip_page: Option<usize>,
    problems: Vec<Problem>,
    marks: Marks,
    /// The Proof tab is open, so warnings are marked.
    proof_open: bool,
    /// The problem being looked at.
    current_problem: Option<usize>,
}

/// Where the pages would paint if they were on screen.
#[derive(Clone, Copy)]
pub struct Ahead {
    pub bounds: Bounds<Pixels>,
    pub scale: f32,
    /// The window whose atlas holds what they painted before, which a page evicted meanwhile is freed from.
    pub window: AnyWindowHandle,
}

pub fn bind_keys(cx: &mut App) {
    let context = Some("Preview");
    cx.bind_keys([
        KeyBinding::new("pagedown", NextScreen, context),
        KeyBinding::new("pageup", PreviousScreen, context),
        KeyBinding::new("escape", CloseSlip, context),
    ]);
}

impl PreviewView {
    /// A view of the document in `theme`, or the theme it names.
    pub fn new(kind: Kind, theme: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_keys(cx);
        let mut view = Self {
            focus: cx.focus_handle().tab_stop(true),
            scheduler: Scheduler::new(),
            specimen_typesetter: Arc::new(Mutex::new(Typesetter::new(
                Kind::Single {
                    path: kind.base_dir().join(SPECIMEN_FILE),
                },
                None,
            ))),
            typesetter: Arc::new(Mutex::new(Typesetter::new(kind, theme.clone()))),
            theme,
            latest: None,
            specimen: false,
            layout: None,
            diagnostics: Vec::new(),
            cache: PageCache::new(CACHE_BYTES),
            rastering: HashSet::new(),
            dropped: Vec::new(),
            zoom: Zoom::Fit,
            current_zoom: 1.0,
            fit: 1.0,
            centre_on: None,
            scroll: (0.0, 0.0),
            view: (0.0, 0.0),
            page: 0,
            bench: Bench::from_env(),
            published: 0,
            revision: None,
            origin: (0.0, 0.0),
            ready: (0, Instant::now()),
            exact_frame: None,
            ahead: None,
            slip: cx.new(|cx| ProofSlip::new(window, cx)),
            slip_source: None,
            slip_marks: Vec::new(),
            slip_page: None,
            problems: Vec::new(),
            marks: Marks::default(),
            proof_open: false,
            current_problem: None,
        };
        if let Some(fixed) = view.bench.as_ref().and_then(Bench::zoom) {
            view.zoom = Zoom::Fixed(fixed);
        }
        view
    }

    /// Lays out a copy of the document on a background thread and publishes the pages when done. A newer request
    /// cancels this one.
    pub fn request(&mut self, joined: Joined, cx: &mut Context<Self>) {
        if let Some(bench) = &mut self.bench {
            bench.requested(&joined.text);
        }
        self.latest = Some(joined);
        // The specimen stays as it is while the document changes. A theme edit asks for it again with `refresh`.
        if !self.specimen || self.layout.is_none() {
            self.typeset_latest(cx);
        }
    }

    /// Typesets again what the pages show, since the theme file changed.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.typeset_latest(cx);
    }

    /// The font families the pages on screen use.
    pub fn fonts(&self) -> &[FontUse] {
        self.layout.as_ref().map_or(&[], |layout| &layout.fonts)
    }

    /// Typesets what the pages show: the specimen, or the document as last asked for.
    fn typeset_latest(&mut self, cx: &mut Context<Self>) {
        let Some(joined) = self.latest.clone() else {
            return;
        };
        let joined = if self.specimen {
            Joined::single(SPECIMEN.to_owned(), joined.revision)
        } else {
            joined
        };
        let job = Job {
            joined,
            theme: self.theme.clone(),
            specimen: self.specimen,
        };
        if let Some(started) = self.scheduler.request(job) {
            self.start_layout(started, cx);
        }
    }

    /// Typesets with another theme file, or with the document's own when `None`. The next layout takes it.
    pub fn set_theme(&mut self, theme: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.theme != theme {
            self.theme = theme;
            self.typeset_latest(cx);
        }
    }

    /// Shows the bundled specimen in pages, or the document again.
    pub fn set_specimen(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.specimen != on {
            self.specimen = on;
            self.typeset_latest(cx);
        }
    }

    pub fn is_specimen(&self) -> bool {
        self.specimen
    }

    /// Scrolls to the top of a page.
    pub fn show_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.centre_on = Some(page);
        cx.notify();
    }

    /// Where the first block set in `style` is written, or that the style is on the first page. `text_of` reads a
    /// block's text as written, by file and byte range.
    pub fn find_style(
        &self,
        style: &str,
        prefer: Option<&Path>,
        text_of: &dyn Fn(&Path, std::ops::Range<usize>) -> Option<String>,
    ) -> Option<Target> {
        let map = self.layout.as_ref()?.laid.source_map()?;
        blocks::find(map, style, prefer, text_of)
    }

    /// The file and offset a block's place stands for, if it is in the pages on screen.
    pub fn place_of(&self, at: SourcePos) -> Option<PathBuf> {
        let chapters = self.layout.as_ref()?.laid.source_map()?.chapters();
        chapters.get(at.chapter.0 as usize).map(|chapter| chapter.path.clone())
    }

    /// Whether the pages on screen were set from `revision` of the text, so offsets in one are offsets in the other.
    pub fn shows(&self, revision: u64) -> bool {
        self.revision == Some(revision)
    }

    /// Scrolls to the text written at byte `offset` of `file`, if it is off screen.
    pub fn show_source(&mut self, file: &Path, offset: usize, cx: &mut Context<Self>) {
        let chapter = self.layout.as_ref().and_then(|layout| {
            let chapters = layout.laid.source_map()?.chapters();
            chapters.iter().position(|chapter| chapter.path == file)
        });
        let Some(chapter) = chapter else {
            return;
        };
        let at = SourcePos {
            chapter: ChapterId(chapter as u32),
            offset: offset as u32,
        };
        let Some(located) = self.layout.as_ref().and_then(|layout| layout.laid.locate(at)) else {
            return;
        };
        let (y, height) = (located.rect.y.0 as f32, located.rect.height.0 as f32);
        if let Some(sheets) = self.sheets(self.current_zoom) {
            self.scroll.1 = sheets.scroll_to_show(located.page, (y, y + height), self.view.1, self.scroll.1);
        }
        cx.notify();
    }

    /// A click on the pages. On a contact sheet it zooms into that page's spread. On a spread it reports the block under
    /// the pointer, if any.
    fn page_clicked(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let at = (
            f32::from(event.position.x) - self.origin.0,
            f32::from(event.position.y) - self.origin.1,
        );
        let hit = self
            .sheets(self.current_zoom)
            .and_then(|sheets| sheets.point_at(at, self.view.0, self.scroll));
        if self.is_contact_sheet() {
            if let Some((page, ..)) = hit {
                self.centre_on = Some(page);
                self.set_zoom_by_author(Zoom::Fit, cx);
            }
            return;
        }
        self.slip_page = hit.map(|(page, ..)| page);
        let block = hit.and_then(|(page, x, y)| {
            let layout = self.layout.as_ref()?;
            proof::block_under(&layout.laid, page, x, y)
        });
        cx.emit(PageClicked(block));
    }

    /// Whether the sheets show as a contact sheet.
    pub fn is_contact_sheet(&self) -> bool {
        self.current_zoom < self.fit * 0.999
    }

    /// The proof slip this view shows.
    pub fn slip(&self) -> &Entity<ProofSlip> {
        &self.slip
    }

    /// Shows the slip at these bytes of a file, on the pages on screen, or nowhere. Pages behind the text say nothing about
    /// where the text is, so the workspace calls this once they are set from it. A paragraph that left the view brings the
    /// view with it.
    pub fn set_slip(&mut self, source: Option<(PathBuf, std::ops::Range<usize>)>, cx: &mut Context<Self>) {
        self.slip_source = source;
        let found = match (&self.slip_source, &self.layout) {
            (Some((file, range)), Some(layout)) => proof::marks_of(&layout.laid, file, range.clone()),
            _ => Vec::new(),
        };
        // A paragraph emptied in the slip has no text on the pages. The slip stays where it was.
        if !found.is_empty() || self.slip_source.is_none() {
            self.slip_marks = found;
        }
        if let Some(anchor) = self.slip_anchor().copied() {
            self.slip_page = Some(anchor.page);
            self.bring_into_view(anchor, cx);
        }
        cx.notify();
    }

    /// The part of the slip's paragraph the slip stands at: on the page it has been on, or where the paragraph starts.
    fn slip_anchor(&self) -> Option<&Mark> {
        let on_page = self.slip_marks.iter().find(|mark| Some(mark.page) == self.slip_page);
        on_page.or(self.slip_marks.first())
    }

    /// Scrolls so a mark is on screen, if it is not.
    fn bring_into_view(&mut self, mark: Mark, cx: &mut Context<Self>) {
        if let Some(sheets) = self.sheets(self.current_zoom) {
            let (top, height) = (mark.rect.y.0 as f32, mark.rect.height.0 as f32);
            self.scroll.1 = sheets.scroll_to_show(mark.page, (top, top + height), self.view.1, self.scroll.1);
            cx.notify();
        }
    }

    /// Says whether the Proof tab is open.
    pub fn set_proof_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.proof_open != open {
            self.proof_open = open;
            cx.notify();
        }
    }

    /// The problems to mark on the pages, replacing the earlier ones.
    pub fn set_problems(&mut self, problems: Vec<Problem>, cx: &mut Context<Self>) {
        self.problems = problems;
        self.mark_problems();
        cx.notify();
    }

    /// Marks the problems again and says how many and how long it took. For measurements.
    pub fn mark_again(&mut self, cx: &mut Context<Self>) -> (usize, std::time::Duration) {
        let started = Instant::now();
        self.mark_problems();
        cx.notify();
        (self.problems.len(), started.elapsed())
    }

    fn mark_problems(&mut self) {
        self.marks = match &self.layout {
            Some(layout) => proof::resolve(&layout.laid, &self.problems),
            None => Marks::default(),
        };
    }

    /// Goes to a problem's place on the pages, and marks it as the one being looked at. A problem with no place on them,
    /// such as one about a theme, moves nothing.
    pub fn show_problem(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        self.current_problem = at;
        let first = at.and_then(|at| self.marks.of_problem.get(at)?.first().copied());
        if let Some(first) = first {
            if self.is_contact_sheet() {
                self.centre_on = Some(first.page);
                self.set_zoom_by_author(Zoom::Fit, cx);
            } else {
                self.bring_into_view(first, cx);
            }
        }
        cx.notify();
    }

    /// The block of the first text that holds `words`. For scripted runs, which have no pointer.
    pub fn block_with_text(&self, words: &str) -> Option<BlockAt> {
        proof::block_with_text(&self.layout.as_ref()?.laid, words)
    }

    /// A paragraph on a page, or the next page that has one. For scripted runs.
    pub fn paragraph_on_page(&self, page: usize) -> Option<(usize, BlockAt)> {
        proof::paragraph_on_page(&self.layout.as_ref()?.laid, page)
    }

    /// The window position of a line on the page in the middle of the view. For scripted runs.
    pub fn point_on_a_line(&self) -> Option<gpui::Point<Pixels>> {
        let sheets = self.sheets(self.current_zoom)?;
        let page = sheets.page_at_middle(self.scroll.1, self.view.1)?;
        let (page, block) = self.paragraph_on_page(page)?;
        let marks = proof::marks_of(&self.layout.as_ref()?.laid, &block.file, block.range);
        let mark = marks.iter().find(|mark| mark.page == page).or(marks.first())?;
        let zoom = self.current_zoom;
        let x = sheets.left(mark.page, self.view.0, self.scroll.0)
            + (mark.rect.x.0 + mark.rect.width.0 / 2.0) as f32 * zoom;
        let y = sheets.top(mark.page) - self.scroll.1 + (mark.rect.y.0 + mark.rect.height.0 / 2.0) as f32 * zoom;
        Some(point(px(self.origin.0 + x), px(self.origin.1 + y)))
    }

    /// The window position of the middle of the view. For scripted runs.
    pub fn middle_of_view(&self) -> gpui::Point<Pixels> {
        point(
            px(self.origin.0 + self.view.0 / 2.0),
            px(self.origin.1 + self.view.1 / 2.0),
        )
    }

    /// The page the slip is on. For scripted runs.
    pub fn slip_page(&self) -> Option<usize> {
        self.slip_anchor().map(|mark| mark.page)
    }

    /// Whether the slip's paragraph starts within the view.
    pub fn slip_in_view(&self) -> bool {
        let Some(mark) = self.slip_anchor() else {
            return false;
        };
        let Some(sheets) = self.sheets(self.current_zoom) else {
            return false;
        };
        let top = sheets.top(mark.page) + mark.rect.y.0 as f32 * self.current_zoom;
        top >= self.scroll.1 && top <= self.scroll.1 + self.view.1
    }

    /// Whether the slip is open and no part of its paragraph is in the view any more. The author scrolled away from it: a
    /// change of the text brings the view back with it through [`Self::set_slip`] first.
    fn slip_left_the_view(&self, sheets: &Sheets, cx: &App) -> bool {
        self.slip.read(cx).opened().is_some()
            && !self.slip_marks.is_empty()
            && !self.slip_marks.iter().any(|mark| {
                let top = sheets.top(mark.page) + mark.rect.y.0 as f32 * self.current_zoom;
                let bottom = top + mark.rect.height.0 as f32 * self.current_zoom;
                bottom > self.scroll.1 && top < self.scroll.1 + self.view.1
            })
    }

    /// The written place of the first text on a page.
    pub fn first_place(&self, page: usize) -> Option<(PathBuf, usize)> {
        proof::first_place(&self.layout.as_ref()?.laid, page)
    }

    /// Whether a layout is running, which the next request would cancel.
    pub fn is_laying_out(&self) -> bool {
        self.scheduler.is_running()
    }

    /// The moment the pages of the newest request were first ready to paint, if they have been.
    pub fn ready_at(&self) -> Option<Instant> {
        (self.ready.0 == self.scheduler.latest()).then_some(self.ready.1)
    }

    /// The last frame that showed every visible page exact.
    pub fn exact_frame(&self) -> Option<Instant> {
        self.exact_frame
    }

    /// Keeps rastering what would show at `ahead` while the pages are not on screen, so that entering Publish finds them
    /// finished. `None` once they are on screen.
    pub fn ready_ahead(&mut self, ahead: Option<Ahead>, cx: &mut Context<Self>) {
        self.ahead = ahead;
        self.prepare_ahead(cx);
    }

    fn prepare_ahead(&mut self, cx: &mut Context<Self>) {
        if let Some(ahead) = self.ahead {
            self.prepare(ahead.bounds, ahead.scale, cx);
        }
    }

    /// The page a run of text containing `needle` is on, and shows it.
    pub fn show_text(&mut self, needle: &str, cx: &mut Context<Self>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let page = layout.laid.pages().iter().position(|page| {
            page.items
                .iter()
                .any(|item| matches!(item, Item::Text { run, .. } if run.text.contains(needle)))
        })?;
        let sheets = self.sheets(self.current_zoom)?;
        self.scroll.1 = sheets.top(page);
        cx.notify();
        Some(page)
    }

    /// Why the latest text has no pages; empty when the pages on screen are current.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// What the pages on screen broke, such as typesetting rules.
    pub fn warnings(&self) -> &[Diagnostic] {
        self.layout.as_ref().map_or(&[], |layout| &layout.warnings)
    }

    fn start_layout(&mut self, (number, job, cancel): Started<Job>, cx: &mut Context<Self>) {
        let Job {
            joined,
            theme,
            specimen,
        } = job;
        let typesetter = if specimen {
            self.specimen_typesetter.clone()
        } else {
            self.typesetter.clone()
        };
        let revision = joined.revision;
        let layout = cx.background_spawn(async move {
            let mut typesetter = typesetter.lock().expect("no panic in layout");
            typesetter.set_theme(theme);
            typesetter.run(joined, &cancel)
        });
        cx.spawn(async move |this, cx| {
            let result = layout.await;
            this.update(cx, |view, cx| {
                let (publish, next) = view.scheduler.finished(number);
                if publish {
                    view.publish(number, revision, specimen, result, cx);
                }
                if let Some(next) = next {
                    view.start_layout(next, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn publish(
        &mut self,
        number: u64,
        revision: u64,
        specimen: bool,
        result: Result<Layout, Failure>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(layout) => {
                self.published = number;
                // Offsets in the specimen are not offsets in any chapter.
                self.revision = (!specimen).then_some(revision);
                self.diagnostics.clear();
                if let Some(bench) = &mut self.bench {
                    bench.published(&layout);
                }
                let book_type = layout.book_type.clone();
                // Freeing a whole document's runs takes long enough to drop a frame.
                let old = self.layout.replace(Arc::new(layout));
                cx.background_spawn(async move { drop(old) }).detach();
                self.mark_problems();
                cx.emit(Typeset(book_type));
            }
            Err(Failure::Errors(diagnostics)) => self.diagnostics = diagnostics,
            Err(Failure::Cancelled) => {}
        }
        // The specimen's warnings and errors are about the specimen, not the document the diagnostics list shows.
        if !specimen {
            cx.emit(Laid);
        }
        cx.notify();
    }

    /// The zoom to keep between runs. `None` fits the pages to the pane.
    pub fn zoom(&self) -> Option<f32> {
        match self.zoom {
            Zoom::Fit => None,
            Zoom::Fixed(zoom) => Some(zoom),
        }
    }

    /// Sets the zoom from a past run. `None` fits the pages to the pane.
    pub fn set_zoom(&mut self, zoom: Option<f32>, cx: &mut Context<Self>) {
        self.zoom = zoom.map_or(Zoom::Fit, Zoom::Fixed);
        self.prepare_ahead(cx);
        cx.notify();
    }

    fn set_zoom_by_author(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        self.prepare_ahead(cx);
        cx.emit(ZoomChanged);
        cx.notify();
    }

    /// Fits a spread to the pane.
    pub fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        self.set_zoom_by_author(Zoom::Fit, cx);
    }

    /// One step along the zoom scale: larger for a positive `direction`, smaller for a negative one. Past fit it becomes a
    /// contact sheet.
    pub fn zoom_step(&mut self, direction: isize, cx: &mut Context<Self>) {
        if let Some(next) = next_zoom(self.zoom, self.current_zoom, self.fit, direction) {
            self.set_zoom_by_author(next, cx);
        }
    }

    fn arrangement(&self, zoom: f32) -> Arrangement {
        if zoom < self.fit * 0.999 {
            Arrangement::Contact { view: self.view.0 }
        } else {
            Arrangement::Spreads {
                gutter: f32::from(tokens::SPREAD_GUTTER),
            }
        }
    }

    fn sheets(&self, zoom: f32) -> Option<Sheets> {
        let layout = self.layout.as_ref()?;
        let pages = (layout.laid.pages().iter()).map(|page| (page.width.0 as f32, page.height.0 as f32));
        let arrangement = self.arrangement(zoom);
        let gap = match arrangement {
            Arrangement::Spreads { .. } => tokens::SPACE_4,
            Arrangement::Contact { .. } => tokens::CONTACT_GAP,
        };
        Some(Sheets::new(pages, zoom, f32::from(gap), arrangement))
    }

    /// The zoom at which a whole spread fills a view this large.
    fn fit_zoom(&self, (width, height): (f32, f32)) -> f32 {
        let Some(layout) = self.layout.as_ref() else {
            return self.fit;
        };
        let pages = layout.laid.pages();
        let widest = pages.iter().map(|page| page.width.0 as f32).fold(1.0, f32::max);
        let tallest = pages.iter().map(|page| page.height.0 as f32).fold(1.0, f32::max);
        let margins = 2.0 * f32::from(tokens::SPACE_4);
        let gutter = f32::from(tokens::SPREAD_GUTTER);
        ((width - margins - gutter) / (2.0 * widest))
            .min((height - margins) / tallest)
            .clamp(SMALLEST_FIT, ZOOMS[ZOOMS.len() - 1])
    }

    /// A frame on screen: frees what the atlas no longer needs, and places the sheets.
    fn frame(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> Vec<Placed> {
        if let Some(bench) = &mut self.bench {
            bench.frame_started();
        }
        for image in self.dropped.drain(..) {
            window.drop_image(image).ok();
        }
        let (placed, all_exact) = self.prepare(bounds, window.scale_factor(), cx);
        if all_exact {
            self.exact_frame = Some(Instant::now());
        }
        if self.bench.is_some() {
            let waiting = placed.len() - placed.iter().filter(|sheet| sheet.exact).count();
            let height = self.sheets(self.current_zoom).map_or(0.0, |sheets| sheets.height());
            bench::frame(self, all_exact, waiting, height, window, cx);
        }
        placed
    }

    /// Places the visible sheets and starts rasterizing what they lack, and says whether every one is exact. Only these
    /// pages cost anything.
    fn prepare(&mut self, bounds: Bounds<Pixels>, device_scale: f32, cx: &mut Context<Self>) -> (Vec<Placed>, bool) {
        self.view = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        self.origin = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let Some(layout) = self.layout.clone() else {
            return (Vec::new(), false);
        };
        let fit = self.fit_zoom(self.view);
        let zoom = match self.zoom {
            Zoom::Fit => fit,
            Zoom::Fixed(zoom) => zoom,
        };
        // The page at the middle stays there through a change of size or arrangement.
        let keep = (zoom != self.current_zoom || fit != self.fit)
            .then(|| {
                self.sheets(self.current_zoom)?
                    .page_at_middle(self.scroll.1, self.view.1)
            })
            .flatten();
        self.fit = fit;
        self.current_zoom = zoom;
        let Some(sheets) = self.sheets(zoom) else {
            return (Vec::new(), false);
        };
        if let Some(page) = self.centre_on.take().or(keep) {
            self.scroll.1 = sheets.scroll_to_centre(page.min(sheets.len().saturating_sub(1)), self.view.1);
        }
        if let Some(page) = self.bench.as_mut().and_then(Bench::take_page) {
            self.scroll.1 = sheets.scroll_to_centre(page.min(sheets.len().saturating_sub(1)), self.view.1);
        }
        self.scroll.0 = clamp_scroll(self.scroll.0, sheets.width(), self.view.0);
        self.scroll.1 = clamp_scroll(self.scroll.1, sheets.height(), self.view.1);

        let visible = sheets.visible(self.scroll.1, self.view.1);
        self.page = visible.start;
        let resolution = (zoom * device_scale).min(MAX_RESOLUTION);
        let key = |page: usize| PageKey {
            painter: layout.painter.id(),
            fingerprint: layout.fingerprints[page],
            resolution: resolution.to_bits(),
        };
        let before = visible.start.saturating_sub(PREFETCH_PAGES)..visible.start;
        let after = visible.end..(visible.end + PREFETCH_PAGES).min(sheets.len());
        let wanted: Vec<usize> = visible.clone().chain(before).chain(after).collect();
        // What is on screen and next to it always fits, or pages would evict each other forever.
        let bytes_each = |page: &usize| {
            let (width, height) = pixel_size(&layout.laid.pages()[*page], resolution);
            width as usize * height as usize * 4
        };
        self.cache
            .set_budget(CACHE_BYTES.max(wanted.iter().map(bytes_each).sum()));
        for page in wanted {
            let key = key(page);
            if self.rastering.len() < RASTER_THREADS
                && !self.rastering.contains(&key)
                && self.cache.get(key, page).is_none()
            {
                self.start_raster(layout.clone(), page, key, resolution, cx);
            }
        }

        let mut placed = Vec::with_capacity(visible.len());
        let mut exact = 0;
        for page in visible.clone() {
            let (width, height) = sheets.size(page);
            let centred = sheets.left(page, self.view.0, self.scroll.0);
            let top = sheets.top(page) - self.scroll.1;
            // Whole device pixels, so an image at full resolution maps one to one.
            let snap = |length: f32| (length * device_scale).round() / device_scale;
            let found = self.cache.get(key(page), page);
            let exact_here = found.is_some();
            exact += usize::from(exact_here);
            placed.push(Placed {
                page,
                bounds: Bounds::new(
                    point(bounds.origin.x + px(snap(centred)), bounds.origin.y + px(snap(top))),
                    size(px(snap(width)), px(snap(height))),
                ),
                image: found.or_else(|| self.cache.stale(page)),
                exact: exact_here,
            });
        }
        if self.slip_left_the_view(&sheets, cx) {
            // A click on nothing, which closes the slip. Not during this frame, which is still placing it.
            let this = cx.entity();
            cx.defer(move |cx| this.update(cx, |_, cx| cx.emit(PageClicked(None))));
        }
        let all_exact = exact == visible.len();
        if all_exact && self.published > self.ready.0 && self.published == self.scheduler.latest() {
            self.ready = (self.published, Instant::now());
        }
        (placed, all_exact)
    }

    fn start_raster(
        &mut self,
        layout: Arc<Layout>,
        page: usize,
        key: PageKey,
        resolution: f32,
        cx: &mut Context<Self>,
    ) {
        self.rastering.insert(key);
        let raster =
            cx.background_spawn(
                async move { render_image(layout.painter.paint(&layout.laid.pages()[page], resolution)) },
            );
        cx.spawn(async move |this, cx| {
            let (image, bytes) = raster.await;
            let off_screen = this.update(cx, |view, cx| {
                view.rastering.remove(&key);
                let evicted = view.cache.insert(key, page, image, bytes);
                view.dropped.extend(evicted);
                view.prepare_ahead(cx);
                cx.notify();
                view.ahead
                    .map(|ahead| (ahead.window, std::mem::take(&mut view.dropped)))
            });
            // No frame comes to free what was evicted while the pages are not on screen.
            if let Ok(Some((window, dropped))) = off_screen {
                window
                    .update(cx, |_, window, _| {
                        for image in dropped {
                            window.drop_image(image).ok();
                        }
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(20.));
        self.scroll.0 -= f32::from(delta.x);
        self.scroll.1 -= f32::from(delta.y);
        cx.notify();
    }

    fn status(&self) -> String {
        let Some(layout) = &self.layout else {
            return if self.diagnostics.is_empty() {
                crate::t!(Key::NoPagesYet).to_owned()
            } else {
                crate::t!(Key::NoPages).to_owned()
            };
        };
        let pages = layout.laid.pages().len();
        if self.is_contact_sheet() {
            return crate::t!(Key::PageCount, pages = pages);
        }
        let shown = self.sheets(self.current_zoom).and_then(|sheets| {
            let page = sheets.page_at_middle(self.scroll.1, self.view.1)?;
            Some(sheets.row_pages(page))
        });
        let shown = shown.unwrap_or(self.page..self.page + 1);
        let number = |page: usize| page_label(&layout.laid, page);
        if shown.len() > 1 {
            crate::t!(
                Key::PagesOf,
                first = number(shown.start),
                last = number(shown.end - 1),
                pages = pages
            )
        } else {
            crate::t!(Key::PageOf, page = number(shown.start.min(pages - 1)), pages = pages)
        }
    }

    /// The window rectangles of what is marked on a sheet, with how strong the tint is and whether it is a warning. A
    /// warning shows during a proof pass, and only the one being looked at has a tint.
    fn marks_on(&self, sheet: &Placed, warnings_shown: bool) -> Vec<(Bounds<Pixels>, f32, bool)> {
        if self.is_contact_sheet() {
            return Vec::new();
        }
        let to_bounds = |mark: &Mark| {
            let padding = tokens::MARK_PADDING;
            let (x, y) = (mark.rect.x.0 as f32 - padding, mark.rect.y.0 as f32 - padding);
            let (width, height) = (
                mark.rect.width.0 as f32 + 2.0 * padding,
                mark.rect.height.0 as f32 + 2.0 * padding,
            );
            let zoom = self.current_zoom;
            Bounds::new(
                sheet.bounds.origin + point(px(x * zoom), px(y * zoom)),
                size(px(width * zoom), px(height * zoom)),
            )
        };
        let problems = self.marks.of_problem.iter().enumerate().flat_map(|(at, marks)| {
            let warning = self.problems.get(at).is_some_and(|problem| problem.warning);
            let current = Some(at) == self.current_problem;
            let tint = match (current, warning) {
                (true, _) => tokens::MARK_TINT_CURRENT,
                (false, true) => 0.,
                (false, false) => tokens::MARK_TINT,
            };
            let shown = !warning || warnings_shown;
            marks.iter().filter(move |_| shown).map(move |mark| (mark, tint, warning))
        });
        let slip = self.slip_marks.iter().map(|mark| (mark, tokens::MARK_TINT, false));
        problems
            .chain(slip)
            .filter(|(mark, ..)| mark.page == sheet.page)
            .map(|(mark, tint, warning)| (to_bounds(mark), tint, warning))
            .collect()
    }

    /// Where the slip stands: under its paragraph's first part on the pages, within the view. `None` while the slip is closed.
    fn slip_position(&self) -> Option<gpui::Point<Pixels>> {
        let mark = self.slip_anchor()?;
        let sheets = self.sheets(self.current_zoom)?;
        let zoom = self.current_zoom;
        let left = sheets.left(mark.page, self.view.0, self.scroll.0) + mark.rect.x.0 as f32 * zoom;
        let bottom = sheets.top(mark.page) - self.scroll.1 + (mark.rect.y.0 + mark.rect.height.0) as f32 * zoom;
        let width = f32::from(tokens::SLIP_WIDTH);
        let room = f32::from(tokens::SPACE_2);
        let x = left.clamp(room, (self.view.0 - width - room).max(room));
        let y = bottom + (tokens::MARK_PADDING + tokens::MARK_PADDING) * zoom;
        Some(point(px(self.origin.0 + x), px(self.origin.1 + y).max(tokens::TITLEBAR_HEIGHT)))
    }

    /// The status line's zoom: smaller, the value, larger, and fit.
    fn zoom_controls(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let button = |id: &'static str, label: SharedString, aria: &'static str, on: bool| {
            div()
                .id(id)
                .button(tokens, aria)
                .flex_none()
                .min_w(tokens::ZOOM_BUTTON)
                .h(tokens::ZOOM_BUTTON)
                .px(tokens::SPACE_1)
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::RADIUS)
                .cursor_pointer()
                .text_color(if on { tokens.text } else { tokens.text_quiet })
                .hover(|button| button.bg(tokens.raised))
                .child(label)
        };
        let percent = format!("{}%", (self.current_zoom * 100.0).round());
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                button("zoom-out", "\u{2212}".into(), crate::t!(Key::ZoomOut), false)
                    .on_click(cx.listener(|view, _, _, cx| view.zoom_step(-1, cx))),
            )
            .child(
                div()
                    .flex_none()
                    .px(tokens::SPACE_1)
                    .text_color(tokens.text)
                    .child(percent),
            )
            .child(
                button("zoom-in", "+".into(), crate::t!(Key::ZoomIn), false)
                    .on_click(cx.listener(|view, _, _, cx| view.zoom_step(1, cx))),
            )
            .child(
                button(
                    "zoom-fit",
                    crate::t!(Key::ZoomFit).into(),
                    crate::t!(Key::ZoomFit),
                    self.zoom == Zoom::Fit,
                )
                .on_click(cx.listener(|view, _, _, cx| view.zoom_to_fit(cx))),
            )
    }
}

/// A page as the book numbers it, which is what warnings say, or its place from the front where it has no number.
fn page_label(laid: &druck::output::Layout, page: usize) -> String {
    match laid.page_numbers().get(page).copied().flatten() {
        Some(number) => number.format.format(number.value),
        None => (page + 1).to_string(),
    }
}

/// Turns a finished page into what the GPU atlas takes: BGRA, with its size in bytes.
fn render_image(pixmap: Pixmap) -> (Arc<RenderImage>, usize) {
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut pixels = pixmap.take();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let bytes = pixels.len();
    let buffer = RgbaImage::from_raw(width, height, pixels).expect("the buffer matches its size");
    let image = RenderImage::new(vec![image::Frame::new(buffer)]);
    (Arc::new(image), bytes)
}

impl Focusable for PreviewView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PreviewView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let this = cx.entity();
        let painted = this.clone();
        let (edge, accent) = (tokens.divider, tokens.accent);
        let warning_color = gpui::rgb(tokens::WARNING_MARK);
        let pages = canvas(
            move |bounds, window, cx| this.update(cx, |view, cx| view.frame(bounds, window, cx)),
            move |bounds, placed, window, cx| {
                let view = painted.read(cx);
                let warnings_shown = view.proof_open || crate::settings::get(cx).warning_marks;
                let drawn: Vec<_> = placed
                    .into_iter()
                    .map(|sheet| {
                        let marks = view.marks_on(&sheet, warnings_shown);
                        let dot = view.is_contact_sheet() && view.marks.pages.contains(&sheet.page);
                        (sheet, marks, dot)
                    })
                    .collect();
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    for (sheet, marks, dot) in drawn {
                        window.paint_quad(gpui::outline(
                            sheet.bounds.dilate(tokens::PAGE_EDGE),
                            edge,
                            gpui::BorderStyle::Solid,
                        ));
                        window.paint_quad(fill(sheet.bounds, gpui::white()));
                        if let Some(image) = sheet.image {
                            let corners = gpui::Corners::default();
                            window
                                .paint_image(sheet.bounds, sheet.bounds, corners, image, 0, false)
                                .ok();
                        }
                        for (mark, tint, warning) in marks {
                            let color = if warning { warning_color } else { accent };
                            window.paint_quad(fill(mark, gpui::Hsla::from(color).opacity(tint)));
                            window.paint_quad(gpui::outline(mark, color, gpui::BorderStyle::Solid));
                        }
                        if dot {
                            let origin = point(
                                sheet.bounds.right() - tokens::CONTACT_DOT_INSET - tokens::DOT_SIZE,
                                sheet.bounds.top() + tokens::CONTACT_DOT_INSET,
                            );
                            window.paint_quad(
                                fill(Bounds::new(origin, size(tokens::DOT_SIZE, tokens::DOT_SIZE)), accent)
                                    .corner_radii(tokens::DOT_SIZE / 2.),
                            );
                        }
                    }
                });
                painted.update(cx, |view, _| {
                    if let Some(bench) = &mut view.bench {
                        bench.frame_painted();
                    }
                });
            },
        )
        .size_full();
        let problem = self.diagnostics.first().map(|first| {
            let more = self.diagnostics.len() - 1;
            let suffix = if more > 0 { format!(" (+{more})") } else { String::new() };
            let first = if first.code.is_some() {
                crate::shell::engine_text::message(first)
            } else {
                first.to_string()
            };
            format!("{first}{suffix}")
        });
        let warnings = self.problems.iter().filter(|problem| problem.warning).count();
        let count = self.problems.len() - warnings;
        let to_look_at = (count > 0).then(|| {
            let label = crate::t!(Key::ToLookAt, count = count);
            div()
                .id("to-look-at")
                .control(&tokens, Role::Link, SharedString::from(label.clone()))
                .flex_none()
                .text_color(tokens.accent)
                .cursor_pointer()
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ProblemsWanted)))
                .child(label)
        });
        let warning_count = (warnings > 0).then(|| {
            let label = crate::t!(Key::Warnings, count = warnings);
            let marks_on = crate::settings::get(cx).warning_marks;
            let toggle = crate::t!(Key::WarningMarks);
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(tokens::SPACE_1)
                .child(
                    div()
                        .id("warnings")
                        .control(&tokens, Role::Link, SharedString::from(label.clone()))
                        .cursor_pointer()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ProblemsWanted)))
                        .child(label),
                )
                .child(
                    div()
                        .id("warning-marks")
                        .toggle(&tokens, toggle, marks_on)
                        .flex_none()
                        .h(tokens::ZOOM_BUTTON)
                        .px(tokens::SPACE_1)
                        .flex()
                        .items_center()
                        .rounded(tokens::RADIUS)
                        .cursor_pointer()
                        .when(marks_on, |button| button.bg(tokens.raised).text_color(tokens.text))
                        .hover(|button| button.bg(tokens.raised))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| {
                            crate::settings::change(cx, |settings| settings.warning_marks = !settings.warning_marks);
                        })
                        .child(toggle),
                )
        });
        let strip = div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_3)
            .border_t_1()
            .border_color(tokens.divider)
            .bg(tokens.panel)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .child(div().flex_none().child(self.status()))
            .children(to_look_at)
            .children(warning_count)
            .child(div().flex_1().min_w_0().when_some(problem, |slot, problem| {
                slot.truncate().text_color(tokens.accent).child(problem)
            }))
            .child(self.zoom_controls(&tokens, cx));
        let slip = self.slip.read(cx).opened().and(self.slip_position()).map(|position| {
            deferred(
                anchored()
                    .position(position)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(tokens::SPACE_2)
                    .child(self.slip.clone()),
            )
            .with_priority(1)
        });
        div()
            .id("preview")
            .role(Role::Group)
            .aria_label(crate::t!(crate::shell::text::Key::Publish))
            .key_context("Preview")
            .track_focus(&self.focus)
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|view, event, window, cx| {
                    window.focus(&view.focus, cx);
                    view.page_clicked(event, cx);
                }),
            )
            .on_scroll(cx.listener(Self::on_scroll))
            .on_action(cx.listener(|view, _: &NextScreen, _, cx| {
                view.scroll.1 += view.view.1 * 0.9;
                cx.notify();
            }))
            .on_action(cx.listener(|view, _: &PreviousScreen, _, cx| {
                view.scroll.1 -= view.view.1 * 0.9;
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &CloseSlip, _, cx| cx.emit(PageClicked(None))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(pages))
            .child(strip)
            .children(slip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_zoom_scale_runs_from_contact_sheets_through_fit_to_larger() {
        let step = |now, current, direction| next_zoom(now, current, 1.53, direction);
        assert_eq!(step(Zoom::Fit, 1.53, -1), Some(Zoom::Fixed(0.45)));
        assert_eq!(step(Zoom::Fixed(0.45), 0.45, -1), Some(Zoom::Fixed(0.3)));
        assert_eq!(step(Zoom::Fixed(0.14), 0.14, -1), None);
        assert_eq!(step(Zoom::Fixed(0.45), 0.45, 1), Some(Zoom::Fit));
        assert_eq!(step(Zoom::Fit, 1.53, 1), Some(Zoom::Fixed(2.0)));
        assert_eq!(step(Zoom::Fixed(2.0), 2.0, -1), Some(Zoom::Fit));
        assert_eq!(step(Zoom::Fixed(3.0), 3.0, 1), None);
    }

    #[test]
    fn fit_goes_below_every_fixed_level() {
        assert_eq!(next_zoom(Zoom::Fit, 0.1, 0.1, -1), None);
        assert_eq!(next_zoom(Zoom::Fit, 0.1, 0.1, 1), Some(Zoom::Fixed(0.5)));
    }

    #[test]
    fn a_level_on_the_wrong_side_of_fit_is_skipped() {
        // In a small window a spread fits at 0.4, so the largest contact level would be larger than fit.
        assert_eq!(next_zoom(Zoom::Fit, 0.4, 0.4, -1), Some(Zoom::Fixed(0.3)));
        assert_eq!(next_zoom(Zoom::Fit, 0.4, 0.4, 1), Some(Zoom::Fixed(0.5)));
    }
}
