//! PDF output: embeds and subsets fonts, writes text as selectable glyph runs, adds links to URLs and
//! anchors, and writes the heading outline as bookmarks.
//!
//! krilla uses the same coordinates as [`Page`]: points from the top-left corner, y growing down.

use krilla::Document;
use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::destination::XyzDestination;
use krilla::geom::{PathBuilder, Point, Rect as PdfRect, Size, Transform};
use krilla::metadata::Metadata as PdfMetadata;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::text::{GlyphId, KrillaGlyph};
use krilla_svg::{SurfaceExt, SvgSettings};

use crate::config::front_matter::Metadata;
use crate::config::theme::Lang;
use crate::config::values::Color;
use crate::document::Link;
use crate::image::{Image, Pixels};
use crate::page::{Bookmark, Item, Output, Rect};
use crate::text::{Fonts, ShapedRun};

/// Writes the pages of `output` as a PDF document, with links, anchors, and bookmarks. Title, authors,
/// and language go into the document info. No creation date is written, so the same input gives the
/// same bytes.
pub fn write(output: &Output, fonts: &Fonts, metadata: &Metadata, lang: Lang) -> Result<Vec<u8>, String> {
    let destination = |anchor: usize| {
        let position = output
            .anchors
            .get(anchor)
            .filter(|position| position.page < output.pages.len())
            .ok_or_else(|| format!("link to anchor {anchor}, which is on no page"))?;
        Ok::<_, String>(XyzDestination::new(
            position.page,
            Point::from_xy(position.x.0 as f32, position.y.0 as f32),
        ))
    };
    let mut document = Document::new();
    document.set_metadata(document_info(metadata, lang));
    for page in &output.pages {
        let settings = PageSettings::from_wh(page.width.0 as f32, page.height.0 as f32)
            .ok_or_else(|| format!("invalid page size {} x {} pt", page.width.0, page.height.0))?;
        let mut pdf_page = document.start_page_with(settings);
        let mut surface = pdf_page.surface();
        let mut links = Vec::new();
        for item in &page.items {
            match item {
                Item::Text { x, y, run, color } => {
                    draw_run(&mut surface, fonts, run, Point::from_xy(x.0 as f32, y.0 as f32), *color)
                }
                Item::TurnedText {
                    x,
                    y,
                    angle,
                    run,
                    color,
                } => {
                    let start = Point::from_xy(x.0 as f32, y.0 as f32);
                    // y grows downward, so a counterclockwise turn is a negative angle.
                    surface.push_transform(&Transform::from_rotate_at(-*angle as f32, start.x, start.y));
                    draw_run(&mut surface, fonts, run, start, *color);
                    surface.pop();
                }
                Item::Rect { rect, color } => draw_rect(&mut surface, rect, *color)?,
                Item::Link { rect, link } => links.push((rect, link)),
                Item::Image { rect, image } => draw_image(&mut surface, rect, image)?,
                Item::Anchor { .. } => {}
            }
        }
        surface.finish();
        for (rect, link) in links {
            let rect = pdf_rect(rect)?;
            let target = match link {
                Link::Url(url) => Target::Action(LinkAction::new(url.clone()).into()),
                Link::Anchor(anchor) => Target::Destination(destination(*anchor)?.into()),
            };
            pdf_page.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, target), None));
        }
        pdf_page.finish();
    }
    if !output.outline.is_empty() {
        document.set_outline(outline(&output.outline, destination)?);
    }
    document.finish().map_err(|e| format!("cannot write PDF: {e}"))
}

/// Nests bookmarks by level: each belongs to the nearest earlier bookmark of a lower level.
fn outline(
    bookmarks: &[Bookmark],
    destination: impl Fn(usize) -> Result<XyzDestination, String>,
) -> Result<Outline, String> {
    let mut outline = Outline::new();
    let mut open: Vec<(u8, OutlineNode)> = Vec::new();
    let close = |open: &mut Vec<(u8, OutlineNode)>, outline: &mut Outline, level: u8| {
        while let Some((_, node)) = open.pop_if(|(open, _)| *open >= level) {
            match open.last_mut() {
                Some((_, parent)) => parent.push_child(node),
                None => outline.push_child(node),
            }
        }
    };
    for bookmark in bookmarks {
        close(&mut open, &mut outline, bookmark.level);
        let node = OutlineNode::new(bookmark.title.clone(), destination(bookmark.anchor)?);
        open.push((bookmark.level, node));
    }
    close(&mut open, &mut outline, 0);
    Ok(outline)
}

fn document_info(metadata: &Metadata, lang: Lang) -> PdfMetadata {
    let language = match lang {
        Lang::En => "en",
        Lang::De => "de",
    };
    let mut info = PdfMetadata::new().language(language.to_owned());
    if let Some(title) = &metadata.title {
        info = info.title(title.clone());
    }
    if !metadata.authors.is_empty() {
        info = info.authors(metadata.authors.clone());
    }
    info
}

fn fill(color: Color) -> Fill {
    let [r, g, b] = color.0;
    Fill {
        paint: rgb::Color::new(r, g, b).into(),
        ..Fill::default()
    }
}

/// Draws a shaped run with its baseline at `start`. krilla takes glyph metrics as fractions of the
/// font size, and the run text so the PDF can map glyphs back to text for search and copy.
fn draw_run(surface: &mut Surface, fonts: &Fonts, run: &ShapedRun, start: Point, color: Color) {
    if run.glyphs.is_empty() {
        return;
    }
    let size = run.size.0 as f32;
    let glyphs: Vec<KrillaGlyph> = run
        .glyphs
        .iter()
        .map(|glyph| {
            KrillaGlyph::new(
                GlyphId::new(glyph.id.into()),
                glyph.x_advance.0 as f32 / size,
                glyph.x_offset.0 as f32 / size,
                glyph.y_offset.0 as f32 / size,
                0.0,
                glyph.text.clone(),
                None,
            )
        })
        .collect();
    surface.set_fill(Some(fill(color)));
    surface.draw_glyphs(start, &glyphs, fonts.pdf_font(run.face).clone(), &run.text, size, false);
}

fn draw_rect(surface: &mut Surface, rect: &Rect, color: Color) -> Result<(), String> {
    let mut path = PathBuilder::new();
    path.push_rect(pdf_rect(rect)?);
    let path = path.finish().ok_or("empty rectangle")?;
    surface.set_fill(Some(fill(color)));
    surface.draw_path(&path);
    Ok(())
}

/// Draws `image` scaled to fill `rect` exactly.
fn draw_image(surface: &mut Surface, rect: &Rect, image: &Image) -> Result<(), String> {
    let size = Size::from_wh(rect.width.0 as f32, rect.height.0 as f32)
        .ok_or_else(|| format!("invalid image size {} x {} pt", rect.width.0, rect.height.0))?;
    surface.push_transform(&Transform::from_translate(rect.x.0 as f32, rect.y.0 as f32));
    match image.pixels() {
        Pixels::Raster(raster) => surface.draw_image(raster.clone(), size),
        Pixels::Svg(tree) => {
            surface
                .draw_svg(tree, size, SvgSettings::default())
                .ok_or("cannot draw SVG image")?;
        }
    }
    surface.pop();
    Ok(())
}

fn pdf_rect(rect: &Rect) -> Result<PdfRect, String> {
    PdfRect::from_xywh(
        rect.x.0 as f32,
        rect.y.0 as f32,
        rect.width.0 as f32,
        rect.height.0 as f32,
    )
    .ok_or_else(|| format!("invalid rectangle {rect:?}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::config::resolved::FaceFile;
    use crate::config::source::{Origin, Resource};
    use crate::config::theme::{Face, FontStyle, Weight};
    use crate::config::values::Pt;
    use crate::page::{Page, Position};

    fn fonts() -> Fonts {
        let regular = FaceFile {
            resource: Resource {
                origin: Origin::Bundled,
                path: "fonts/LibertinusSerif-Regular.otf".to_owned(),
            },
            index: 0,
        };
        let files = BTreeMap::from([(Face::REGULAR, regular)]);
        Fonts::from_files(&BTreeMap::from([("Serif".to_owned(), files)])).expect("bundled font")
    }

    #[test]
    fn writes_a_pdf_with_text_and_link() {
        let fonts = fonts();
        let face = fonts.face("Serif", Weight::REGULAR, FontStyle::Normal);
        let run = fonts.shape("Effizienz finden für Straße", face, Pt(12.0), Lang::De);
        let rect = Rect {
            x: Pt(10.0),
            y: Pt(10.0),
            width: run.width,
            height: Pt(14.0),
        };
        let page = Page {
            width: Pt(300.0),
            height: Pt(100.0),
            items: vec![
                Item::Rect {
                    rect,
                    color: Color([250, 230, 230]),
                },
                Item::Text {
                    x: Pt(10.0),
                    y: Pt(20.0),
                    run,
                    color: Color([0, 0, 0]),
                },
                Item::Link {
                    rect,
                    link: Link::Url("https://example.com".to_owned()),
                },
            ],
        };
        let metadata = Metadata {
            title: Some("Titel".to_owned()),
            authors: vec!["Mike".to_owned()],
            ..Metadata::default()
        };
        let bytes = write(&output(vec![page]), &fonts, &metadata, Lang::De).expect("pdf");
        assert!(bytes.starts_with(b"%PDF"));
    }

    fn output(pages: Vec<Page>) -> Output {
        Output {
            pages,
            anchors: Vec::new(),
            outline: Vec::new(),
        }
    }

    #[test]
    fn links_to_anchors_and_nests_bookmarks_by_level() {
        let page = || Page {
            width: Pt(200.0),
            height: Pt(200.0),
            items: Vec::new(),
        };
        let mut first = page();
        first.items.push(Item::Link {
            rect: Rect {
                x: Pt(10.0),
                y: Pt(10.0),
                width: Pt(50.0),
                height: Pt(12.0),
            },
            link: Link::Anchor(1),
        });
        let at = |page: usize, y: f64| Position {
            page,
            x: Pt(20.0),
            y: Pt(y),
        };
        let bookmark = |level: u8, title: &str, anchor: usize| Bookmark {
            level,
            title: title.to_owned(),
            anchor,
        };
        let output = Output {
            pages: vec![first, page()],
            anchors: vec![at(0, 30.0), at(1, 50.0), at(1, 120.0)],
            outline: vec![
                bookmark(1, "Intro", 0),
                bookmark(2, "Detail", 1),
                bookmark(1, "Close", 2),
            ],
        };
        let bytes = write(&output, &fonts(), &Metadata::default(), Lang::En).expect("pdf");
        let pdf = String::from_utf8_lossy(&bytes);

        // Each dictionary up to its first nested one, found by a key it holds.
        let dict = |key: &str| pdf.split("<<").find(|dict| dict.contains(key)).expect(key).to_owned();

        assert!(dict("/Type/Outlines").contains("/Count 2"), "two top-level bookmarks");
        assert!(dict("/Title(Intro)").contains("/First"), "Detail is nested under Intro");
        assert!(!dict("/Title(Close)").contains("/First"));
        // The link and the bookmarks go to destinations on pages, not to URLs.
        assert!(dict("/Subtype/Link").contains("/Dest"));
        assert!(!pdf.contains("/URI"));
        assert_eq!(pdf.matches("/XYZ").count(), 3, "one destination per anchor");

        let missing = Output {
            outline: vec![bookmark(1, "Lost", 7)],
            ..output
        };
        assert!(write(&missing, &fonts(), &Metadata::default(), Lang::En).is_err());
    }

    #[test]
    fn writes_the_same_bytes_for_png_jpeg_and_svg_images() {
        let image = |name: &str, y: f64| {
            let path = format!("{}/tests/fixtures/images/{name}", env!("CARGO_MANIFEST_DIR"));
            let extension = name.rsplit('.').next().unwrap_or_default();
            let image = Image::decode(std::fs::read(path).expect("fixture"), extension).expect("image");
            let rect = Rect {
                x: Pt(10.0),
                y: Pt(y),
                width: Pt(80.0),
                height: Pt(40.0),
            };
            Item::Image { rect, image }
        };
        let page = Page {
            width: Pt(100.0),
            height: Pt(160.0),
            items: vec![
                image("pixel.png", 5.0),
                image("photo.jpg", 55.0),
                image("drawing.svg", 105.0),
            ],
        };
        let output = output(vec![page]);
        let first = write(&output, &fonts(), &Metadata::default(), Lang::En).expect("pdf");
        let second = write(&output, &fonts(), &Metadata::default(), Lang::En).expect("pdf");
        assert!(first.starts_with(b"%PDF"));
        assert_eq!(first, second);
    }
}
