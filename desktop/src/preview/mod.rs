//! Centre pane, pages side: the engine's pages painted natively, never through PDF.
//!
//! The workspace calls [`PreviewView::request`] with the session's joined text and hears [`Typeset`] when pages change.

mod bench;
mod cache;
pub(crate) mod job;
mod raster;
mod sheets;
mod strings;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use druck::Failure;
use druck::diagnostic::Diagnostic;
use druck::output::{ChapterId, Item, Pt, SourcePos};
use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, EventEmitter, FocusHandle, Focusable, KeyBinding, MouseDownEvent, Pixels, RenderImage,
    ScrollEvent, Window, actions, canvas, div, fill, point, px, size,
};
use image::RgbaImage;
use resvg::tiny_skia::Pixmap;

use self::bench::Bench;
pub use self::bench::memory;
use self::cache::{PageCache, PageKey};
pub use self::job::BodyStyle;
use self::job::{Layout, Scheduler, Started, Typesetter};
use self::raster::pixel_size;
use self::sheets::{Sheets, clamp_scroll, rescale_scroll};
use self::strings::Key;
use crate::session::{Joined, Kind};
use crate::tokens::{self, Tokens};

actions!(preview, [ZoomIn, ZoomOut, ZoomToFit, NextScreen, PreviousScreen]);

/// Fixed zoom levels, as logical pixels per point.
const ZOOMS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
/// Sharper than this costs too much memory per page; higher zoom enlarges these pixels.
const MAX_RESOLUTION: f32 = 4.0;
/// Rasterized pages kept in memory. Painted ones also have a copy in the GPU atlas.
const CACHE_BYTES: usize = 128 * 1024 * 1024;
/// Pages rasterized at the same time.
const RASTER_THREADS: usize = 3;
/// Pages rasterized beyond each end of the view, so normal scrolling shows finished pages.
const PREFETCH_PAGES: usize = 2;
/// The line that shows the editor's caret on a page.
const CARET_WIDTH: Pixels = px(2.);

/// How large sheets are drawn.
#[derive(Clone, Copy, PartialEq)]
enum Zoom {
    FitWidth,
    Fixed(f32),
}

/// A sheet to paint this frame.
struct Placed {
    bounds: Bounds<Pixels>,
    image: Option<Arc<RenderImage>>,
    /// Where the editor's caret is on this sheet, in window pixels.
    caret: Option<Bounds<Pixels>>,
}

/// New pages were published, with the body style they were set in.
pub struct Typeset(pub BodyStyle);

impl EventEmitter<Typeset> for PreviewView {}

/// A layout finished and its result counts, whether it made pages or only errors. The shell reads the diagnostics.
pub struct Laid;

impl EventEmitter<Laid> for PreviewView {}

/// A click on a page landed on text written at this byte of this file.
pub struct PageClicked {
    pub file: PathBuf,
    pub offset: usize,
}

impl EventEmitter<PageClicked> for PreviewView {}

/// A line on a sheet, in the page's points, that shows the editor's caret.
#[derive(Clone, Copy)]
struct CaretMark {
    page: usize,
    x: f32,
    y: f32,
    height: f32,
}

pub struct PreviewView {
    focus: FocusHandle,
    scheduler: Scheduler<Joined>,
    typesetter: Arc<Mutex<Typesetter>>,
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
    caret: Option<CaretMark>,
    /// The newest request whose visible pages were all exact in a frame, and when.
    painted: (u64, Instant),
}

pub fn bind_keys(cx: &mut App) {
    let context = Some("Preview");
    cx.bind_keys([
        KeyBinding::new("cmd-=", ZoomIn, context),
        KeyBinding::new("cmd--", ZoomOut, context),
        KeyBinding::new("cmd-0", ZoomToFit, context),
        KeyBinding::new("pagedown", NextScreen, context),
        KeyBinding::new("pageup", PreviousScreen, context),
    ]);
}

impl PreviewView {
    /// A view of the document in `theme`, or the theme it names.
    pub fn new(kind: Kind, theme: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        bind_keys(cx);
        let mut view = Self {
            focus: cx.focus_handle(),
            scheduler: Scheduler::new(),
            typesetter: Arc::new(Mutex::new(Typesetter::new(kind, theme))),
            layout: None,
            diagnostics: Vec::new(),
            cache: PageCache::new(CACHE_BYTES),
            rastering: HashSet::new(),
            dropped: Vec::new(),
            zoom: Zoom::FitWidth,
            current_zoom: 1.0,
            scroll: (0.0, 0.0),
            view: (0.0, 0.0),
            page: 0,
            bench: Bench::from_env(),
            published: 0,
            revision: None,
            origin: (0.0, 0.0),
            caret: None,
            painted: (0, Instant::now()),
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
        if let Some(started) = self.scheduler.request(joined) {
            self.start_layout(started, cx);
        }
    }

    /// Whether the pages on screen were set from `revision` of the text, so offsets in one are offsets in the other.
    pub fn shows(&self, revision: u64) -> bool {
        self.revision == Some(revision)
    }

    /// Marks the text written at byte `offset` of `file`, and scrolls to it if it is off screen.
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
        self.caret = Some(CaretMark {
            page: located.page,
            x: located.rect.x.0 as f32,
            y,
            height,
        });
        if let Some(sheets) = self.sheets(self.current_zoom) {
            self.scroll.1 = sheets.scroll_to_show(located.page, (y, y + height), self.view.1, self.scroll.1);
        }
        cx.notify();
    }

    /// Reports the written place of the text at a window position, if the position is on a page.
    fn page_clicked(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let at = (
            f32::from(event.position.x) - self.origin.0,
            f32::from(event.position.y) - self.origin.1,
        );
        let Some(sheets) = self.sheets(self.current_zoom) else {
            return;
        };
        let Some((page, x, y)) = sheets.point_at(at, self.view.0, self.scroll) else {
            return;
        };
        let clicked = self.layout.as_ref().and_then(|layout| {
            let position = layout.laid.position_at(page, Pt(f64::from(x)), Pt(f64::from(y)))?;
            let chapter = layout.laid.source_map()?.chapters().get(position.chapter.0 as usize)?;
            Some(PageClicked {
                file: chapter.path.clone(),
                offset: position.offset as usize,
            })
        });
        if let Some(clicked) = clicked {
            cx.emit(clicked);
        }
    }

    /// Whether a layout is running, which the next request would cancel.
    pub fn is_laying_out(&self) -> bool {
        self.scheduler.is_running()
    }

    /// The moment the pages of the newest request were first on screen, if they have been.
    pub fn painted_at(&self) -> Option<Instant> {
        (self.painted.0 == self.scheduler.latest()).then_some(self.painted.1)
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

    fn start_layout(&mut self, (number, joined, cancel): Started<Joined>, cx: &mut Context<Self>) {
        let typesetter = self.typesetter.clone();
        let revision = joined.revision;
        let layout =
            cx.background_spawn(async move { typesetter.lock().expect("no panic in layout").run(joined, &cancel) });
        cx.spawn(async move |this, cx| {
            let result = layout.await;
            this.update(cx, |view, cx| {
                let (publish, next) = view.scheduler.finished(number);
                if publish {
                    view.publish(number, revision, result, cx);
                }
                if let Some(next) = next {
                    view.start_layout(next, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn publish(&mut self, number: u64, revision: u64, result: Result<Layout, Failure>, cx: &mut Context<Self>) {
        match result {
            Ok(layout) => {
                self.published = number;
                self.revision = Some(revision);
                self.caret = None;
                self.diagnostics.clear();
                if let Some(bench) = &mut self.bench {
                    bench.published(&layout);
                }
                cx.emit(Typeset(layout.body.clone()));
                // Freeing a whole document's runs takes long enough to drop a frame.
                let old = self.layout.replace(Arc::new(layout));
                cx.background_spawn(async move { drop(old) }).detach();
            }
            Err(Failure::Errors(diagnostics)) => self.diagnostics = diagnostics,
            Err(Failure::Cancelled) => {}
        }
        cx.emit(Laid);
        cx.notify();
    }

    /// One zoom level larger for a positive `direction`, smaller for a negative one. For the View menu.
    pub fn zoom_by(&mut self, direction: isize, cx: &mut Context<Self>) {
        self.zoom_step(direction, cx);
    }

    /// Fits the sheets to the width of the pane. For the View menu.
    pub fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        self.zoom = Zoom::FitWidth;
        cx.notify();
    }

    fn zoom_step(&mut self, direction: isize, cx: &mut Context<Self>) {
        let current = self.current_zoom;
        let next = if direction > 0 {
            ZOOMS.iter().find(|zoom| **zoom > current * 1.01)
        } else {
            ZOOMS.iter().rev().find(|zoom| **zoom < current * 0.99)
        };
        if let Some(next) = next {
            self.zoom = Zoom::Fixed(*next);
            cx.notify();
        }
    }

    fn sheets(&self, zoom: f32) -> Option<Sheets> {
        let layout = self.layout.as_ref()?;
        let pages = (layout.laid.pages().iter()).map(|page| (page.width.0 as f32, page.height.0 as f32));
        Some(Sheets::new(pages, zoom, f32::from(tokens::SPACE_4)))
    }

    /// The zoom in effect for a view `width` pixels wide.
    fn resolve_zoom(&self, width: f32) -> f32 {
        if let Zoom::Fixed(zoom) = self.zoom {
            return zoom;
        }
        let Some(unit) = self.sheets(1.0) else {
            return self.current_zoom;
        };
        let margins = 2.0 * f32::from(tokens::SPACE_4);
        let widest = unit.width() - margins;
        ((width - margins) / widest).clamp(ZOOMS[0] / 2.0, ZOOMS[ZOOMS.len() - 1])
    }

    /// Places the visible sheets and starts rasterizing what they lack. Only these pages cost anything.
    fn frame(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> Vec<Placed> {
        if let Some(bench) = &mut self.bench {
            bench.frame_started();
        }
        for image in self.dropped.drain(..) {
            window.drop_image(image).ok();
        }
        self.view = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        self.origin = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let device_scale = window.scale_factor();
        let zoom = self.resolve_zoom(self.view.0);
        let Some(layout) = self.layout.clone() else {
            return Vec::new();
        };
        let Some(sheets) = self.sheets(zoom) else {
            return Vec::new();
        };
        if zoom != self.current_zoom {
            if let Some(old) = self.sheets(self.current_zoom) {
                self.scroll.1 = rescale_scroll(self.scroll.1, self.view.1, old.height(), sheets.height());
            }
            self.current_zoom = zoom;
        }
        if let Some(page) = self.bench.as_mut().and_then(Bench::take_page) {
            self.scroll.1 = sheets.top(page.min(sheets.len().saturating_sub(1)));
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
            let image = match self.cache.get(key(page), page) {
                Some(image) => {
                    exact += 1;
                    Some(image)
                }
                None => self.cache.stale(page),
            };
            placed.push(Placed {
                bounds: Bounds::new(
                    point(bounds.origin.x + px(snap(centred)), bounds.origin.y + px(snap(top))),
                    size(px(snap(width)), px(snap(height))),
                ),
                image,
                caret: self.caret.filter(|mark| mark.page == page).map(|mark| {
                    Bounds::new(
                        point(
                            bounds.origin.x + px(snap(centred + mark.x * zoom)),
                            bounds.origin.y + px(snap(top + mark.y * zoom)),
                        ),
                        size(CARET_WIDTH, px(mark.height * zoom)),
                    )
                }),
            });
        }
        if exact == visible.len() && self.published > self.painted.0 && self.published == self.scheduler.latest() {
            self.painted = (self.published, Instant::now());
        }
        if self.bench.is_some() {
            bench::frame(
                self,
                exact == visible.len(),
                visible.len() - exact,
                sheets.height(),
                window,
                cx,
            );
        }
        placed
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
            this.update(cx, |view, cx| {
                view.rastering.remove(&key);
                let evicted = view.cache.insert(key, page, image, bytes);
                view.dropped.extend(evicted);
                cx.notify();
            })
            .ok();
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
        let page = (self.page + 1).min(pages);
        let percent = (self.current_zoom * 100.0).round();
        crate::t!(Key::PageOf, page = page, pages = pages, percent = percent)
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
        let tokens = Tokens::get(cx);
        let this = cx.entity();
        let painted = this.clone();
        let caret_color = tokens.accent;
        let pages = canvas(
            move |bounds, window, cx| this.update(cx, |view, cx| view.frame(bounds, window, cx)),
            move |bounds, placed, window, cx| {
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    for sheet in placed {
                        window.paint_quad(fill(sheet.bounds, gpui::white()));
                        if let Some(image) = sheet.image {
                            let corners = gpui::Corners::default();
                            window
                                .paint_image(sheet.bounds, sheet.bounds, corners, image, 0, false)
                                .ok();
                        }
                        if let Some(caret) = sheet.caret {
                            window.paint_quad(fill(caret, caret_color));
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
            format!("{first}{suffix}")
        });
        let strip = div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .justify_between()
            .gap(tokens::SPACE_3)
            .border_t_1()
            .border_color(tokens.divider)
            .bg(tokens.panel)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .child(div().flex_none().child(self.status()))
            .when_some(problem, |strip, problem| {
                strip.child(div().min_w_0().truncate().text_color(tokens.accent).child(problem))
            });
        div()
            .id("preview")
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
            .on_action(cx.listener(|view, _: &ZoomIn, _, cx| view.zoom_step(1, cx)))
            .on_action(cx.listener(|view, _: &ZoomOut, _, cx| view.zoom_step(-1, cx)))
            .on_action(cx.listener(|view, _: &ZoomToFit, _, cx| view.zoom_to_fit(cx)))
            .on_action(cx.listener(|view, _: &NextScreen, _, cx| {
                view.scroll.1 += view.view.1 * 0.9;
                cx.notify();
            }))
            .on_action(cx.listener(|view, _: &PreviousScreen, _, cx| {
                view.scroll.1 -= view.view.1 * 0.9;
                cx.notify();
            }))
            .child(div().flex_1().min_h_0().overflow_hidden().child(pages))
            .child(strip)
    }
}
