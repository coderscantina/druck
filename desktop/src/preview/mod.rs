//! Publish's pages: the engine's pages painted natively, never through PDF, in spreads or, zoomed out past fit, as a
//! contact sheet of every page. The status line has the zoom.
//!
//! The workspace calls [`PreviewView::request`] with the session's joined text and hears [`Typeset`] when pages change.
//! In Write they are not on screen, and [`PreviewView::ready_ahead`] keeps what Publish would show ready to paint.
//!
//! A click on a line opens the [`slip::ProofSlip`] through the workspace and [`slip_control`], which this view shows at its paragraph. Problems
//! come in through [`PreviewView::set_problems`], and only these and the slip's paragraph are ever marked on a page.

#[cfg(any(debug_assertions, feature = "measure"))]
mod bench;
mod cache;
pub mod directives;
pub mod fonts;
pub(crate) mod job;
pub mod proof;
mod raster;
mod scheduling;
mod sheets;
pub mod slip;
pub mod slip_control;
mod status;
mod strings;
mod zoom;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use druck::Failure;
use druck::diagnostic::{Diagnostic, Source, path_from};
#[cfg(any(debug_assertions, feature = "measure"))]
use druck::output::Item;
use druck::output::{ChapterId, SourcePos};
use gpui::prelude::*;
use gpui::{
    Anchor, AnyView, AnyWindowHandle, App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    MouseDownEvent, Pixels, RenderImage, Role, ScrollEvent, SharedString, Toggled, Window, actions, anchored, canvas,
    deferred, div, fill, point, px, size,
};
use image::RgbaImage;
use resvg::tiny_skia::Pixmap;

#[cfg(any(debug_assertions, feature = "measure"))]
use self::bench::Bench;
#[cfg(any(debug_assertions, feature = "measure"))]
pub use self::bench::memory;
use self::cache::{PageCache, PageKey};
pub use self::fonts::FontUse;
pub use self::job::BookType;
use self::job::{Job, Layout, Scheduler, Started, Typesetter};
use self::proof::{BlockAt, Mark, Marks, Problem};
use self::raster::pixel_size;
pub use self::scheduling::Ahead;
use self::sheets::{Arrangement, Sheets, clamp_scroll};
use self::slip::ProofSlip;
use self::strings::Key;
use self::zoom::Zoom;
use crate::a11y::Control as _;
use crate::session::{Joined, Kind};
use crate::theme_file::blocks::{self, Target};
use crate::tokens::{self, Tokens};

actions!(preview, [NextScreen, PreviousScreen, CloseSlip]);

/// The bundled specimen: every element of a document, to see a theme on.
use crate::theme_file::SPECIMEN;
/// Where the specimen pretends to be, beside the document, so relative paths in a theme resolve as for the document.
const SPECIMEN_FILE: &str = "specimen.md";

/// Sharper than this costs too much memory per page; higher zoom enlarges these pixels.
const MAX_RESOLUTION: f32 = 4.0;
/// Rasterized pages kept in memory. Painted ones also have a copy in the GPU atlas.
const CACHE_BYTES: usize = 128 * 1024 * 1024;
/// Pages rasterized at the same time.
const RASTER_THREADS: usize = 3;
/// Pages rasterized beyond each end of the view, so normal scrolling shows finished pages.
const PREFETCH_PAGES: usize = 2;

/// Where the proof slip stands.
struct SlipPlace {
    at: gpui::Point<Pixels>,
    anchor: Anchor,
    width: Pixels,
}

/// A sheet to paint this frame.
struct Placed {
    bounds: Bounds<Pixels>,
    page: usize,
    image: Option<Arc<RenderImage>>,
    /// Rastered for this zoom and these pages, not an older image standing in.
    #[cfg(any(debug_assertions, feature = "measure"))]
    exact: bool,
}

/// New pages were published, with the theme's type they were set in.
pub struct Typeset(pub BookType);

/// The theme's type, read without pages after a layout failed.
pub struct TypeRead(pub BookType);

impl EventEmitter<TypeRead> for PreviewView {}

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

/// The author asked for the book's details from the empty pages.
pub struct DetailsWanted;

impl EventEmitter<DetailsWanted> for PreviewView {}

/// The empty state's Show in text: the written place of what keeps the pages from being set, a file and its line.
pub struct TextPlaceWanted(pub PathBuf, pub usize);

impl EventEmitter<TextPlaceWanted> for PreviewView {}

pub struct PreviewView {
    focus: FocusHandle,
    scheduler: Scheduler<Job>,
    typesetter: Arc<Mutex<Typesetter>>,
    /// The book's folder, which messages name files from.
    base: PathBuf,
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
    /// The page a change of zoom last kept in the middle, with the scroll it left. While the view has not moved since,
    /// the next change keeps the same page, so a step out to the contact sheet and back returns to the starting spread.
    held: Option<(usize, f32)>,
    scroll: (f32, f32),
    view: (f32, f32),
    /// The first page on screen in the last frame.
    page: usize,
    #[cfg(any(debug_assertions, feature = "measure"))]
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
    #[cfg(any(debug_assertions, feature = "measure"))]
    exact_frame: Option<Instant>,
    /// While the pages are not on screen: where they would paint, to raster for.
    ahead: Option<Ahead>,
    slip: Entity<ProofSlip>,
    /// The file and bytes of the slip's paragraph, as the pages on screen have them, and where that is on them.
    slip_source: Option<(PathBuf, std::ops::Range<usize>)>,
    slip_marks: Vec<Mark>,
    /// The page of the paragraph the slip stands on, which is where it was clicked when it runs over two pages.
    slip_page: Option<usize>,
    /// The view is to go to the slip's paragraph on the next frame, once the zoom is known.
    slip_follow: bool,
    problems: Vec<Problem>,
    marks: Marks,
    /// The Proof tab is open, so warnings are marked.
    proof_open: bool,
    /// The problem being looked at.
    current_problem: Option<usize>,
    /// The window's foot line, which stands in the status line here.
    foot: Vec<AnyView>,
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
        let base = kind.base_dir();
        let view = Self {
            base,
            // The keys rest here, but Tab walks only what can be pressed.
            focus: cx.focus_handle(),
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
            held: None,
            scroll: (0.0, 0.0),
            view: (0.0, 0.0),
            page: 0,
            #[cfg(any(debug_assertions, feature = "measure"))]
            bench: Bench::from_env(),
            published: 0,
            revision: None,
            origin: (0.0, 0.0),
            ready: (0, Instant::now()),
            #[cfg(any(debug_assertions, feature = "measure"))]
            exact_frame: None,
            ahead: None,
            slip: cx.new(|cx| ProofSlip::new(window, cx)),
            slip_source: None,
            slip_marks: Vec::new(),
            slip_page: None,
            slip_follow: false,
            problems: Vec::new(),
            marks: Marks::default(),
            proof_open: false,
            current_problem: None,
            foot: Vec::new(),
        };
        #[cfg(any(debug_assertions, feature = "measure"))]
        let mut view = view;
        #[cfg(any(debug_assertions, feature = "measure"))]
        if let Some(fixed) = view.bench.as_ref().and_then(Bench::zoom) {
            view.zoom = Zoom::Fixed(fixed);
        }
        view
    }

    /// The font families the pages on screen use.
    pub fn fonts(&self) -> &[FontUse] {
        self.layout.as_ref().map_or(&[], |layout| &layout.fonts)
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
            self.slip_follow = true;
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
    #[cfg(any(debug_assertions, feature = "measure"))]
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
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn block_with_text(&self, words: &str) -> Option<BlockAt> {
        proof::block_with_text(&self.layout.as_ref()?.laid, words)
    }

    /// A paragraph on a page, or the next page that has one. For scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn paragraph_on_page(&self, page: usize) -> Option<(usize, BlockAt)> {
        proof::paragraph_on_page(&self.layout.as_ref()?.laid, page)
    }

    /// The window position of a line on the page in the middle of the view. For scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
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

    /// The window position of the centre of the page in the middle of the view, never the gutter between two. For
    /// scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn middle_of_a_page(&self) -> Option<gpui::Point<Pixels>> {
        let sheets = self.sheets(self.current_zoom)?;
        let page = sheets.page_at_middle(self.scroll.1, self.view.1)?;
        let (width, height) = sheets.size(page);
        let x = sheets.left(page, self.view.0, self.scroll.0) + width / 2.0;
        let y = sheets.top(page) - self.scroll.1 + height / 2.0;
        Some(point(px(self.origin.0 + x), px(self.origin.1 + y)))
    }

    /// The page the slip is on. For scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn slip_page(&self) -> Option<usize> {
        self.slip_anchor().map(|mark| mark.page)
    }

    /// Whether the slip's paragraph starts within the view. For scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
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

    /// The page a run of text containing `needle` is on, and shows it.
    #[cfg(any(debug_assertions, feature = "measure"))]
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

    /// Whether the latest layout gave errors instead of pages, which a change of a file it lacked may mend.
    pub fn layout_failed(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// What the pages on screen broke, such as typesetting rules.
    pub fn warnings(&self) -> &[Diagnostic] {
        self.layout.as_ref().map_or(&[], |layout| &layout.warnings)
    }

    /// A frame on screen: frees what the atlas no longer needs, and places the sheets.
    fn frame(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> Vec<Placed> {
        #[cfg(any(debug_assertions, feature = "measure"))]
        if let Some(bench) = &mut self.bench {
            bench.frame_started();
        }
        for image in self.dropped.drain(..) {
            window.drop_image(image).ok();
        }
        #[cfg_attr(not(any(debug_assertions, feature = "measure")), allow(unused_variables))]
        let (placed, all_exact) = self.prepare(bounds, window.scale_factor(), cx);
        #[cfg(any(debug_assertions, feature = "measure"))]
        if all_exact {
            self.exact_frame = Some(Instant::now());
        }
        #[cfg(any(debug_assertions, feature = "measure"))]
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
            .then(|| match self.held {
                Some((page, scroll)) if scroll == self.scroll.1 => Some(page),
                _ => (self.sheets(self.current_zoom)?).page_at_middle(self.scroll.1, self.view.1),
            })
            .flatten();
        self.fit = fit;
        self.current_zoom = zoom;
        let Some(sheets) = self.sheets(zoom) else {
            return (Vec::new(), false);
        };
        let kept = self.centre_on.take().or(keep);
        if let Some(page) = kept {
            self.scroll.1 = sheets.scroll_to_centre(page.min(sheets.len().saturating_sub(1)), self.view.1);
        }
        #[cfg(any(debug_assertions, feature = "measure"))]
        if let Some(page) = self.bench.as_mut().and_then(Bench::take_page) {
            self.scroll.1 = sheets.scroll_to_centre(page.min(sheets.len().saturating_sub(1)), self.view.1);
        }
        if std::mem::take(&mut self.slip_follow)
            && let Some(mark) = self.slip_anchor().copied()
        {
            let (top, height) = (mark.rect.y.0 as f32, mark.rect.height.0 as f32);
            self.scroll.1 = sheets.scroll_to_show(mark.page, (top, top + height), self.view.1, self.scroll.1);
        }
        self.scroll.0 = clamp_scroll(self.scroll.0, sheets.width(), self.view.0);
        self.scroll.1 = clamp_scroll(self.scroll.1, sheets.height(), self.view.1);
        if let Some(page) = kept {
            self.held = Some((page, self.scroll.1));
        }

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
                #[cfg(any(debug_assertions, feature = "measure"))]
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

    /// The window rectangles of what is marked on a sheet, with how strong the tint is and whether it is a warning. A
    /// warning shows during a proof pass, and only the one being looked at has a tint. On a page of the one being looked
    /// at, only its marks draw.
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
        // The problem being looked at stands alone on its pages, so a page's tint and a paragraph's outline never nest.
        let looked_at = (self.current_problem)
            .and_then(|at| self.marks.of_problem.get(at))
            .is_some_and(|marks| marks.iter().any(|mark| mark.page == sheet.page));
        let problems = self.marks.of_problem.iter().enumerate().flat_map(|(at, marks)| {
            let warning = self.problems.get(at).is_some_and(|problem| problem.warning);
            let current = Some(at) == self.current_problem;
            let tint = match (current, warning) {
                (true, _) => tokens::MARK_TINT_CURRENT,
                (false, true) => 0.,
                (false, false) => tokens::MARK_TINT,
            };
            let shown = (!warning || warnings_shown) && (current || !looked_at);
            marks
                .iter()
                .filter(move |_| shown)
                .map(move |mark| (mark, tint, warning))
        });
        let slip = self.slip_marks.iter().map(|mark| (mark, tokens::MARK_TINT, false));
        problems
            .chain(slip)
            .filter(|(mark, ..)| mark.page == sheet.page)
            .map(|(mark, tint, warning)| (to_bounds(mark), tint, warning))
            .collect()
    }

    /// Where the slip stands and how wide: as wide as its paragraph's column and under its first part on the pages, within
    /// the view, or above it near the view's foot, so it never covers its own paragraph. `None` while the slip is closed.
    fn slip_place(&self) -> Option<SlipPlace> {
        let mark = self.slip_anchor()?;
        let sheets = self.sheets(self.current_zoom)?;
        let zoom = self.current_zoom;
        let padding = tokens::MARK_PADDING * zoom;
        let left = sheets.left(mark.page, self.view.0, self.scroll.0) + mark.rect.x.0 as f32 * zoom - padding;
        let top = sheets.top(mark.page) - self.scroll.1 + mark.rect.y.0 as f32 * zoom;
        let bottom = top + mark.rect.height.0 as f32 * zoom;
        let room = f32::from(tokens::SPACE_2);
        let column = mark.rect.width.0 as f32 * zoom + 2. * padding;
        let width = column
            .max(f32::from(tokens::SLIP_WIDTH))
            .min((self.view.0 - 2. * room).max(f32::from(tokens::SLIP_WIDTH)));
        let x = left.clamp(room, (self.view.0 - width - room).max(room));
        let gap = 2. * padding;
        let below = self.view.1 - bottom - gap;
        let above = top - gap;
        let over = below < f32::from(tokens::SLIP_ROOM_BELOW) && above > below;
        let y = if over { top - gap } else { bottom + gap };
        Some(SlipPlace {
            at: point(
                px(self.origin.0 + x),
                px(self.origin.1 + y).max(tokens::TITLEBAR_HEIGHT),
            ),
            anchor: if over { Anchor::BottomLeft } else { Anchor::TopLeft },
            width: px(width),
        })
    }

    /// What stands in place of the pages when the document has none: every message in full, and where to mend it.
    fn empty_state(&self, tokens: &Tokens, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.layout.is_some() || self.diagnostics.is_empty() {
            return None;
        }
        let messages = (self.diagnostics.iter())
            .map(|problem| div().text_color(tokens.text_quiet).child(in_words(problem, &self.base)));
        let place = self
            .diagnostics
            .iter()
            .find_map(|problem| match (&problem.source, problem.location) {
                (Some(Source::Document(path)), Some(location)) => Some((path.clone(), location.line as usize)),
                _ => None,
            });
        let has_place = place.is_some();
        let show_in_text = place.map(|(path, line)| {
            crate::shell::ui::small_button(tokens, "no-pages-text", crate::t!(Key::ShowInText))
                .text_color(tokens.accent)
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(TextPlaceWanted(path.clone(), line))))
        });
        Some(
            div()
                .id("no-pages")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .overflow_y_scroll()
                .child(
                    div()
                        .w(tokens::EMPTY_STATE_WIDTH)
                        .flex()
                        .flex_col()
                        .gap(tokens::SPACE_2)
                        .text_size(tokens::TEXT_BODY)
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(tokens.text)
                                .child(crate::t!(Key::NoPages)),
                        )
                        .children(messages)
                        .child(
                            div()
                                .pt(tokens::SPACE_1)
                                .flex()
                                .gap(tokens::SPACE_2)
                                .children(show_in_text)
                                .child(
                                    crate::shell::ui::small_button(
                                        tokens,
                                        "no-pages-details",
                                        crate::t!(Key::OpenDetails),
                                    )
                                    .when(!has_place, |button| button.text_color(tokens.accent))
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DetailsWanted))),
                                )
                                .child(
                                    crate::shell::ui::small_button(tokens, "no-pages-proof", crate::t!(Key::OpenProof))
                                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ProblemsWanted))),
                                ),
                        ),
                ),
        )
    }
}

/// A diagnostic in the language now set where it has a code, with its file and line from the book's folder `base`.
fn in_words(problem: &Diagnostic, base: &Path) -> String {
    let message = crate::shell::engine_text::message_in(problem, Some(base));
    let file = match &problem.source {
        Some(Source::Document(path) | Source::Theme(path) | Source::Bibliography(path)) => Some(path_from(path, base)),
        _ => None,
    };
    match (file, problem.location) {
        (Some(file), Some(location)) => crate::t!(Key::AtLine, file = file, line = location.line, message = message),
        (Some(file), None) => format!("{file}: {message}"),
        _ => message,
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                #[cfg(any(debug_assertions, feature = "measure"))]
                painted.update(cx, |view, _| {
                    if let Some(bench) = &mut view.bench {
                        bench.frame_painted();
                    }
                });
            },
        )
        .size_full();
        let empty = self.empty_state(&tokens, cx);
        let strip = self.status_line(&tokens, window, cx);
        let slip = self.slip.read(cx).opened().and(self.slip_place()).map(|place| {
            deferred(
                anchored()
                    .position(place.at)
                    .anchor(place.anchor)
                    .snap_to_window_with_margin(tokens::SPACE_2)
                    .child(div().w(place.width).child(self.slip.clone())),
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
            .on_action(cx.listener(|view, _: &NextScreen, _, cx| view.page_screen(true, cx)))
            .on_action(cx.listener(|view, _: &PreviousScreen, _, cx| view.page_screen(false, cx)))
            .on_action(cx.listener(|_, _: &CloseSlip, _, cx| cx.emit(PageClicked(None))))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .relative()
                    .bg(tokens.pages_canvas)
                    .child(pages)
                    .children(empty),
            )
            .child(strip)
            .children(slip)
    }
}
