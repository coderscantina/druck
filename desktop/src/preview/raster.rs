//! Paints the engine's pages to pixels on the CPU.
//!
//! gpui's `paint_glyph` cannot draw these pages: its `Font` has no variation coordinates, it finds fonts by
//! family name rather than by file, and its glyph sprites cannot rotate, which the watermark needs. So glyphs
//! are drawn from the font outlines by their shaped ids with skrifa and tiny-skia, and SVG with resvg.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use druck::editor::engine::{FaceId, Fonts, Image, Item, Page, Pixels, Rect, ShapedRun};
use druck::editor::resolved_config::Color;
use image::RgbaImage;
use image::imageops::{self, FilterType};
use resvg::tiny_skia::{self, FillRule, IntSize, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Transform};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// Decoded images kept between pages. A page reuses a photo only as long as its neighbours do.
const DECODED_IMAGES: usize = 16;

/// A font's family name: the typographic one if it has one, as the system's font lookup also prefers.
pub fn family_name(data: &[u8], index: u32) -> Option<String> {
    let font = FontRef::from_index(data, index).ok()?;
    [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| font.localized_strings(id).english_or_first())
        .map(|name| name.chars().collect())
}

/// A glyph's outline in font units, y up.
struct Outline {
    path: Option<Path>,
    units_per_em: f32,
}

/// Paints pages with one set of loaded fonts. Shared by the threads that rasterize pages.
pub struct Painter {
    id: u64,
    fonts: Fonts,
    outlines: RwLock<HashMap<(FaceId, u16), Arc<Outline>>>,
    decoded: Mutex<HashMap<u64, Arc<Option<RgbaImage>>>>,
}

impl Painter {
    pub fn new(fonts: Fonts) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            fonts,
            outlines: RwLock::default(),
            decoded: Mutex::default(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn fonts(&self) -> &Fonts {
        &self.fonts
    }

    /// The whole `page` at `resolution` pixels per point, on white as on paper. Links, anchors, and tag marks
    /// are not drawn.
    pub fn paint(&self, page: &Page, resolution: f32) -> Pixmap {
        let (width, height) = pixel_size(page, resolution);
        let mut pixmap = Pixmap::new(width, height).expect("a page has an area");
        pixmap.fill(tiny_skia::Color::WHITE);
        let base = Transform::from_scale(resolution, resolution);
        for item in &page.items {
            match item {
                Item::Text { x, y, run, color } => {
                    self.draw_run(&mut pixmap, base, run, x.0 as f32, y.0 as f32, *color);
                }
                Item::TurnedText {
                    x,
                    y,
                    angle,
                    run,
                    color,
                } => {
                    let (x, y) = (x.0 as f32, y.0 as f32);
                    // y grows downward, so a counterclockwise turn is a negative angle.
                    let turned = base.pre_concat(Transform::from_rotate_at(-*angle as f32, x, y));
                    self.draw_run(&mut pixmap, turned, run, x, y, *color);
                }
                Item::Rect { rect, color } => {
                    let area = tiny_skia::Rect::from_xywh(
                        rect.x.0 as f32,
                        rect.y.0 as f32,
                        rect.width.0 as f32,
                        rect.height.0 as f32,
                    );
                    if let Some(area) = area {
                        pixmap.fill_rect(area, &solid(*color), base, None);
                    }
                }
                Item::Image { rect, image } => self.draw_image(&mut pixmap, base, resolution, rect, image),
                Item::Link { .. } | Item::Anchor { .. } | Item::Begin(_) | Item::End => {}
            }
        }
        pixmap
    }

    fn draw_run(&self, pixmap: &mut Pixmap, base: Transform, run: &ShapedRun, x: f32, y: f32, color: Color) {
        let paint = solid(color);
        let size = run.size.0 as f32;
        let mut pen = x;
        for glyph in &run.glyphs {
            let outline = self.outline(run.face, glyph.id);
            if let Some(path) = &outline.path {
                let scale = size / outline.units_per_em;
                let at = base
                    .pre_translate(pen + glyph.x_offset.0 as f32, y - glyph.y_offset.0 as f32)
                    .pre_scale(scale, -scale);
                pixmap.fill_path(path, &paint, FillRule::Winding, at, None);
            }
            pen += glyph.x_advance.0 as f32;
        }
    }

    fn outline(&self, face: FaceId, glyph: u16) -> Arc<Outline> {
        let known = self
            .outlines
            .read()
            .expect("no panic while reading")
            .get(&(face, glyph))
            .cloned();
        if let Some(outline) = known {
            return outline;
        }
        let outline = Arc::new(self.load_outline(face, glyph));
        self.outlines
            .write()
            .expect("no panic while writing")
            .insert((face, glyph), outline.clone());
        outline
    }

    fn load_outline(&self, face: FaceId, glyph: u16) -> Outline {
        let source = self.fonts.outlines(face);
        let font = FontRef::from_index(source.data, source.index).expect("the engine has parsed this face");
        let units_per_em = f32::from(font.metrics(Size::unscaled(), LocationRef::default()).units_per_em);
        let location = font.axes().location(
            source
                .coordinates
                .iter()
                .map(|(tag, value)| (skrifa::Tag::new(tag), *value)),
        );
        let mut pen = Pen(PathBuilder::new());
        let drawn = font.outline_glyphs().get(GlyphId::new(glyph.into())).map(|outline| {
            let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::from(&location));
            outline.draw(settings, &mut pen)
        });
        let path = matches!(drawn, Some(Ok(_))).then(|| pen.0.finish()).flatten();
        Outline { path, units_per_em }
    }

    fn draw_image(&self, pixmap: &mut Pixmap, base: Transform, resolution: f32, rect: &Rect, image: &Image) {
        let (x, y) = (rect.x.0 as f32, rect.y.0 as f32);
        let (width, height) = (rect.width.0 as f32, rect.height.0 as f32);
        match image.pixels() {
            Pixels::Svg(tree) => {
                let size = tree.size();
                let at = base
                    .pre_translate(x, y)
                    .pre_scale(width / size.width(), height / size.height());
                resvg::render(tree, at, &mut pixmap.as_mut());
            }
            Pixels::Raster(_) => {
                let target = (
                    (width * resolution).round() as u32,
                    (height * resolution).round() as u32,
                );
                let scaled = self.scaled_raster(image, target);
                if let Some(scaled) = scaled {
                    let (left, top) = ((x * resolution).round() as i32, (y * resolution).round() as i32);
                    let identity = Transform::identity();
                    pixmap.draw_pixmap(left, top, scaled.as_ref(), &PixmapPaint::default(), identity, None);
                }
            }
        }
    }

    /// The image at exactly `target` pixels, premultiplied. The engine has already validated the file.
    fn scaled_raster(&self, image: &Image, (width, height): (u32, u32)) -> Option<Pixmap> {
        let decoded = self.decoded(image.source());
        let source = decoded.as_ref().as_ref()?;
        let size = IntSize::from_wh(width, height)?;
        let mut pixels = imageops::resize(source, width, height, FilterType::Triangle).into_raw();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = (u16::from(*channel) * alpha / 255) as u8;
            }
        }
        Pixmap::from_vec(pixels, size)
    }

    fn decoded(&self, source: &[u8]) -> Arc<Option<RgbaImage>> {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let key = hasher.finish();
        if let Some(decoded) = self.decoded.lock().expect("no panic while decoding").get(&key) {
            return decoded.clone();
        }
        let decoded = Arc::new(image::load_from_memory(source).ok().map(|image| image.into_rgba8()));
        let mut cache = self.decoded.lock().expect("no panic while decoding");
        if cache.len() >= DECODED_IMAGES {
            cache.clear();
        }
        cache.insert(key, decoded.clone());
        decoded
    }
}

/// The pixels of a page at `resolution`.
pub fn pixel_size(page: &Page, resolution: f32) -> (u32, u32) {
    (
        (page.width.0 as f32 * resolution).round().max(1.0) as u32,
        (page.height.0 as f32 * resolution).round().max(1.0) as u32,
    )
}

/// A hash of everything a page draws, so an unchanged page keeps its pixels across refreshes.
pub fn fingerprint(page: &Page) -> u64 {
    let mut hasher = DefaultHasher::new();
    page.width.0.to_bits().hash(&mut hasher);
    page.height.0.to_bits().hash(&mut hasher);
    for item in &page.items {
        match item {
            Item::Text { x, y, run, color } => {
                (0_u8, x.0.to_bits(), y.0.to_bits(), color.0).hash(&mut hasher);
                hash_run(run, &mut hasher);
            }
            Item::TurnedText {
                x,
                y,
                angle,
                run,
                color,
            } => {
                (1_u8, x.0.to_bits(), y.0.to_bits(), angle.to_bits(), color.0).hash(&mut hasher);
                hash_run(run, &mut hasher);
            }
            Item::Rect { rect, color } => {
                2_u8.hash(&mut hasher);
                hash_rect(rect, &mut hasher);
                color.0.hash(&mut hasher);
            }
            Item::Image { rect, image } => {
                3_u8.hash(&mut hasher);
                hash_rect(rect, &mut hasher);
                image.source().hash(&mut hasher);
            }
            Item::Link { .. } | Item::Anchor { .. } | Item::Begin(_) | Item::End => {}
        }
    }
    hasher.finish()
}

fn hash_run(run: &ShapedRun, hasher: &mut DefaultHasher) {
    (run.face, run.size.0.to_bits()).hash(hasher);
    for glyph in &run.glyphs {
        (
            glyph.id,
            glyph.x_advance.0.to_bits(),
            glyph.x_offset.0.to_bits(),
            glyph.y_offset.0.to_bits(),
        )
            .hash(hasher);
    }
}

fn hash_rect(rect: &Rect, hasher: &mut DefaultHasher) {
    (
        rect.x.0.to_bits(),
        rect.y.0.to_bits(),
        rect.width.0.to_bits(),
        rect.height.0.to_bits(),
    )
        .hash(hasher);
}

fn solid(color: Color) -> Paint<'static> {
    let [red, green, blue] = color.0;
    let mut paint = Paint::default();
    paint.set_color_rgba8(red, green, blue, 255);
    paint
}

struct Pen(PathBuilder);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(cx0, cy0, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path as FilePath;

    use druck::editor::engine::{self, Request};
    use druck::editor::resolved_config::{FontStyle, Lang, Weight};
    use druck::output::Pt;

    use super::*;
    use crate::preview::job::Typesetter;

    fn sample_layout() -> super::super::job::Layout {
        let path = FilePath::new(env!("CARGO_MANIFEST_DIR")).join("../samples/report.md");
        let text = std::fs::read_to_string(&path).expect("sample");
        let Ok(layout) = Typesetter::new(&path, None).run(text) else {
            panic!("typesets")
        };
        layout
    }

    fn page(items: Vec<Item>) -> Page {
        Page {
            width: Pt(100.0),
            height: Pt(100.0),
            items,
        }
    }

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 3] {
        let pixel = pixmap.pixel(x, y).expect("inside");
        [pixel.red(), pixel.green(), pixel.blue()]
    }

    fn ink_bounds(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32)> {
        let mut bounds = None::<(u32, u32, u32, u32)>;
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                if pixel(pixmap, x, y)[0] < 128 {
                    let (left, top, right, bottom) = bounds.unwrap_or((x, y, x, y));
                    bounds = Some((left.min(x), top.min(y), right.max(x), bottom.max(y)));
                }
            }
        }
        bounds
    }

    #[test]
    fn rects_fill_their_area_on_white() {
        let layout = sample_layout();
        let rect = Rect {
            x: Pt(10.0),
            y: Pt(20.0),
            width: Pt(30.0),
            height: Pt(10.0),
        };
        let pixmap = layout.painter.paint(
            &page(vec![Item::Rect {
                rect,
                color: Color([10, 20, 30]),
            }]),
            2.0,
        );
        assert_eq!((pixmap.width(), pixmap.height()), (200, 200));
        assert_eq!(pixel(&pixmap, 40, 50), [10, 20, 30]);
        assert_eq!(pixel(&pixmap, 10, 50), [255, 255, 255]);
    }

    /// A shaped run from the sample, the first on its first page.
    fn first_run(layout: &super::super::job::Layout) -> ShapedRun {
        let run = layout
            .output
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .find_map(|item| match item {
                Item::Text { run, .. } if run.glyphs.len() > 3 => Some(run.clone()),
                _ => None,
            });
        run.expect("the sample has text")
    }

    #[test]
    fn text_sits_on_its_baseline_and_turned_text_turns_about_its_start() {
        let layout = sample_layout();
        let run = first_run(&layout);
        let descent = (run.size.0 * 0.3 * 4.0) as u32;
        let (x, y) = (Pt(5.0), Pt(50.0));
        let color = Color([0, 0, 0]);
        let straight = layout.painter.paint(
            &page(vec![Item::Text {
                x,
                y,
                run: run.clone(),
                color,
            }]),
            4.0,
        );
        let (left, top, right, bottom) = ink_bounds(&straight).expect("the run draws ink");
        assert!((18..30).contains(&left), "starts at x: {left}");
        assert!(
            top < 200 && bottom <= 200 + descent,
            "stands on the baseline: {top}..{bottom}"
        );
        assert!(right > left);

        let turned = layout.painter.paint(
            &page(vec![Item::TurnedText {
                x,
                y,
                angle: 90.0,
                run,
                color,
            }]),
            4.0,
        );
        let (left, _, right, bottom) = ink_bounds(&turned).expect("the turned run draws ink");
        assert!(
            bottom <= 205,
            "a quarter turn counterclockwise runs up the page: {bottom}"
        );
        assert!(
            right <= 20 + descent && left < 20,
            "and stands left of its start: {left}..{right}"
        );
    }

    #[test]
    fn fingerprints_ignore_marks_and_follow_what_is_drawn() {
        let rect = Rect {
            x: Pt(1.0),
            y: Pt(1.0),
            width: Pt(1.0),
            height: Pt(1.0),
        };
        let drawn = |color| Item::Rect { rect, color };
        let plain = fingerprint(&page(vec![drawn(Color([1, 1, 1]))]));
        let anchored = fingerprint(&page(vec![
            Item::Anchor {
                id: 3,
                x: Pt(0.0),
                y: Pt(0.0),
            },
            drawn(Color([1, 1, 1])),
        ]));
        assert_eq!(plain, anchored);
        assert_ne!(plain, fingerprint(&page(vec![drawn(Color([2, 1, 1]))])));
    }

    #[test]
    fn variable_fonts_draw_at_the_instance_they_were_shaped_for() {
        let font = FilePath::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/fonts/PublicSans.ttf");
        let theme = format!(
            r#"{{"version": 1, "fonts": {{"Var": {{"regular": {{"file": {:?}, "variable": true}}}}}}}}"#,
            font.display()
        );
        let dir = std::env::temp_dir().join(format!("druck-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        let theme_path = dir.join("variable.json");
        std::fs::write(&theme_path, theme).expect("theme");
        let request = Request {
            input: FilePath::new("doc.md"),
            theme: Some(&theme_path),
            overrides: &[],
        };
        let configured = engine::configure(&request, &dir, "Body\n".to_owned());
        std::fs::remove_dir_all(&dir).expect("temp folder");
        let Ok((config, _)) = configured else {
            panic!("the theme is valid")
        };
        let Ok(fonts) = engine::load_fonts(&config) else {
            panic!("the font loads")
        };
        let shaped: Vec<_> = [100, 900]
            .map(|weight| {
                let face = fonts.face("Var", Weight::of_font(weight), FontStyle::Normal);
                fonts.shape("Hamburg", face, Pt(12.0), Lang::En)
            })
            .into();
        let painter = Painter::new(fonts);
        let ink: Vec<usize> = shaped
            .into_iter()
            .map(|run| {
                let text = Item::Text {
                    x: Pt(5.0),
                    y: Pt(50.0),
                    run,
                    color: Color([0, 0, 0]),
                };
                let pixmap = painter.paint(&page(vec![text]), 4.0);
                pixmap.pixels().iter().filter(|pixel| pixel.red() < 128).count()
            })
            .collect();
        assert!(
            ink[1] > ink[0] * 2,
            "weight 900 is much darker than weight 100: {ink:?}"
        );
    }

    #[test]
    fn raster_and_svg_images_fill_their_rect() {
        let layout = sample_layout();
        let fixtures = FilePath::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/images");
        let rect = Rect {
            x: Pt(10.0),
            y: Pt(10.0),
            width: Pt(50.0),
            height: Pt(50.0),
        };
        for name in ["pixel.png", "photo.jpg", "drawing.svg"] {
            let path = fixtures.join(name);
            let extension = path.extension().and_then(|e| e.to_str()).expect("extension");
            let data = std::fs::read(&path).expect("fixture");
            let image = Image::decode(data, extension).expect("decodes");
            let pixmap = layout.painter.paint(&page(vec![Item::Image { rect, image }]), 2.0);
            let inside = (20..120).any(|x| (20..120).any(|y| pixel(&pixmap, x, y) != [255, 255, 255]));
            assert!(inside, "{name} draws inside its rect");
            assert_eq!(pixel(&pixmap, 5, 5), [255, 255, 255], "{name} stays inside its rect");
        }
    }
}
