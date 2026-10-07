//! PDF output: embeds and subsets fonts, writes text as selectable glyph runs, adds links.
//!
//! krilla uses the same coordinates as [`Page`]: points from the top-left corner, y growing down.

use krilla::Document;
use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect as PdfRect, Size, Transform};
use krilla::metadata::Metadata as PdfMetadata;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::text::{GlyphId, KrillaGlyph};
use krilla_svg::{SurfaceExt, SvgSettings};

use crate::config::front_matter::Metadata;
use crate::config::theme::Lang;
use crate::config::values::Color;
use crate::image::{Image, Pixels};
use crate::page::{Item, Page, Rect};
use crate::text::{Fonts, ShapedRun};

/// Writes `pages` as a PDF document. Title, authors, and language go into the document info.
/// No creation date is written, so the same input gives the same bytes.
pub fn write(pages: &[Page], fonts: &Fonts, metadata: &Metadata, lang: Lang) -> Result<Vec<u8>, String> {
    let mut document = Document::new();
    document.set_metadata(document_info(metadata, lang));
    for page in pages {
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
                Item::Rect { rect, color } => draw_rect(&mut surface, rect, *color)?,
                Item::Link { rect, url } => links.push((rect, url)),
                Item::Image { rect, image } => draw_image(&mut surface, rect, image)?,
            }
        }
        surface.finish();
        for (rect, url) in links {
            let rect = pdf_rect(rect)?;
            let target = Target::Action(LinkAction::new(url.clone()).into());
            pdf_page.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, target), None));
        }
        pdf_page.finish();
    }
    document.finish().map_err(|e| format!("cannot write PDF: {e}"))
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
    use crate::config::resolved::FontFiles;
    use crate::config::source::{Origin, Resource};
    use crate::config::theme::{FontStyle, Weight};
    use crate::config::values::Pt;

    fn fonts() -> Fonts {
        let regular = Resource {
            origin: Origin::Bundled,
            path: "fonts/LibertinusSerif-Regular.otf".to_owned(),
        };
        let files = FontFiles {
            regular,
            italic: None,
            bold: None,
            bold_italic: None,
        };
        Fonts::from_files(&BTreeMap::from([("Serif".to_owned(), files)])).expect("bundled font")
    }

    #[test]
    fn writes_a_pdf_with_text_and_link() {
        let fonts = fonts();
        let face = fonts.face("Serif", Weight::Regular, FontStyle::Normal);
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
                    url: "https://example.com".to_owned(),
                },
            ],
        };
        let metadata = Metadata {
            title: Some("Titel".to_owned()),
            authors: vec!["Mike".to_owned()],
            ..Metadata::default()
        };
        let bytes = write(&[page], &fonts, &metadata, Lang::De).expect("pdf");
        assert!(bytes.starts_with(b"%PDF"));
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
        let first = write(std::slice::from_ref(&page), &fonts(), &Metadata::default(), Lang::En).expect("pdf");
        let second = write(std::slice::from_ref(&page), &fonts(), &Metadata::default(), Lang::En).expect("pdf");
        assert!(first.starts_with(b"%PDF"));
        assert_eq!(first, second);
    }
}
