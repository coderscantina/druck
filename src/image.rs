//! Image decoding and validation for PNG, JPEG, and SVG.
//!
//! Images are fully decoded here, so a malformed file is reported with the layout error and never
//! surfaces while the PDF is written. SVG text uses only the bundled fonts, and an SVG cannot read files.

use std::fmt;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use krilla::image::Image as RasterImage;
use png::Transformations;
use usvg::fontdb::{Database, Source};
use usvg::{ImageHrefResolver, Options, Tree};
use zune_jpeg::JpegDecoder;

use crate::config::values::Pt;
use crate::text::bundled;

/// CSS pixels are 96 per inch and PDF points 72 per inch.
const PT_PER_SVG_PX: f64 = 0.75;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// The bundled fonts SVG text can use.
const SVG_FONTS: [&str; 5] = [
    "fonts/LibertinusSerif-Regular.otf",
    "fonts/LibertinusSerif-Italic.otf",
    "fonts/LibertinusSerif-Bold.otf",
    "fonts/LibertinusSerif-BoldItalic.otf",
    "fonts/LibertinusMono-Regular.otf",
];
const UNSUPPORTED: &str = "unsupported image format; use PNG, JPEG, or SVG";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Png,
    Jpeg,
    Svg,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Svg => "SVG",
        }
    }

    /// The format a file name extension claims, in any case.
    fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpeg),
            "svg" => Some(Format::Svg),
            _ => None,
        }
    }
}

/// What the magic bytes say, for formats that have them.
enum Sniffed {
    Known(Format),
    Gif,
    WebP,
    Unknown,
}

fn sniff(data: &[u8]) -> Sniffed {
    if data.starts_with(PNG_SIGNATURE) {
        Sniffed::Known(Format::Png)
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Sniffed::Known(Format::Jpeg)
    } else if data.starts_with(b"GIF8") {
        Sniffed::Gif
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        Sniffed::WebP
    } else {
        Sniffed::Unknown
    }
}

/// A decoded, validated image. Cheap to clone.
#[derive(Clone)]
pub struct Image {
    pixels: Pixels,
    width: f64,
    height: f64,
}

/// The decoded content, as the PDF writer draws it.
#[derive(Clone)]
pub enum Pixels {
    Raster(RasterImage),
    Svg(Arc<Tree>),
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let format = match self.pixels {
            Pixels::Raster(_) => "raster",
            Pixels::Svg(_) => "SVG",
        };
        write!(f, "Image({format} {} x {} pt)", self.width, self.height)
    }
}

impl Image {
    /// Decodes and validates `data`. `extension` is the file name extension as written, without the dot, any case
    /// (may be empty). Errors are plain messages without file name or location.
    ///
    /// PNG and JPEG are decoded once here to validate them. krilla decodes PNG again when the PDF is written.
    pub fn decode(data: Vec<u8>, extension: &str) -> Result<Image, String> {
        let claimed = Format::from_extension(extension);
        let format = match (sniff(&data), claimed) {
            (Sniffed::Gif, _) => return Err("GIF images are not supported; use PNG, JPEG, or SVG".to_owned()),
            (Sniffed::WebP, _) => return Err("WebP images are not supported; use PNG, JPEG, or SVG".to_owned()),
            (_, None) => return Err(UNSUPPORTED.to_owned()),
            (Sniffed::Known(found), Some(claimed)) if found != claimed => return Err(mismatch(found, extension)),
            (Sniffed::Known(found), Some(_)) => found,
            (Sniffed::Unknown, Some(Format::Svg)) => Format::Svg,
            (Sniffed::Unknown, Some(claimed)) => {
                return match parse_svg(&data) {
                    Ok(_) => Err(mismatch(Format::Svg, extension)),
                    Err(_) => Err(malformed(claimed, "invalid file signature")),
                };
            }
        };
        match format {
            Format::Png => decode_png(data),
            Format::Jpeg => decode_jpeg(data),
            Format::Svg => decode_svg(&data),
        }
    }

    /// The natural size in points. Raster images count one pixel as one point (72 dpi); SVG uses CSS units,
    /// 96 px per inch, so 0.75 pt per px.
    pub fn size(&self) -> (Pt, Pt) {
        (Pt(self.width), Pt(self.height))
    }

    pub fn pixels(&self) -> &Pixels {
        &self.pixels
    }
}

fn mismatch(found: Format, extension: &str) -> String {
    format!(
        "the file contains {} data but its name ends in .{extension}",
        found.name()
    )
}

fn malformed(format: Format, detail: impl fmt::Display) -> String {
    format!("malformed {} image: {detail}", format.name())
}

fn decode_png(data: Vec<u8>) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(&data));
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| malformed(Format::Png, e))?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("image is too large")?];
    reader.next_frame(&mut buffer).map_err(|e| malformed(Format::Png, e))?;
    let image = RasterImage::from_png(data.into(), false).map_err(|e| malformed(Format::Png, e))?;
    raster(image)
}

fn decode_jpeg(data: Vec<u8>) -> Result<Image, String> {
    // krilla embeds JPEG data as is and reads only the headers, so decode all of it here.
    JpegDecoder::new(Cursor::new(&data))
        .decode()
        .map_err(|e| malformed(Format::Jpeg, e))?;
    let image = RasterImage::from_jpeg(data.into(), false).map_err(|e| malformed(Format::Jpeg, e))?;
    raster(image)
}

fn raster(image: RasterImage) -> Result<Image, String> {
    let (width, height) = image.size();
    if width == 0 || height == 0 {
        return Err("image has no pixels".to_owned());
    }
    Ok(Image {
        width: f64::from(width),
        height: f64::from(height),
        pixels: Pixels::Raster(image),
    })
}

fn decode_svg(data: &[u8]) -> Result<Image, String> {
    let tree = parse_svg(data)?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("image has no area".to_owned());
    }
    Ok(Image {
        width: f64::from(size.width()) * PT_PER_SVG_PX,
        height: f64::from(size.height()) * PT_PER_SVG_PX,
        pixels: Pixels::Svg(Arc::new(tree)),
    })
}

/// Parses SVG with only the bundled fonts, so output is independent of the machine. Images embedded as
/// data URLs work; a reference to another file is an error, since it would neither be read nor drawn.
fn parse_svg(data: &[u8]) -> Result<Tree, String> {
    let external = AtomicBool::new(false);
    let options = Options {
        font_family: "Libertinus Serif".to_owned(),
        fontdb: bundled_fontdb().clone(),
        image_href_resolver: ImageHrefResolver {
            resolve_string: Box::new(|_, _| {
                external.store(true, Ordering::Relaxed);
                None
            }),
            ..ImageHrefResolver::default()
        },
        ..Options::default()
    };
    let tree = Tree::from_data(data, &options).map_err(|e| malformed(Format::Svg, e))?;
    if external.load(Ordering::Relaxed) {
        return Err("an SVG image cannot refer to other files; embed its images as data URLs".to_owned());
    }
    Ok(tree)
}

fn bundled_fontdb() -> &'static Arc<Database> {
    static DATABASE: OnceLock<Arc<Database>> = OnceLock::new();
    DATABASE.get_or_init(|| {
        let mut database = Database::new();
        for bytes in SVG_FONTS.into_iter().filter_map(bundled::font) {
            database.load_font_source(Source::Binary(Arc::new(bytes)));
        }
        database.set_serif_family("Libertinus Serif");
        database.set_monospace_family("Libertinus Mono");
        Arc::new(database)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (Vec<u8>, &str) {
        let path = format!("{}/tests/fixtures/images/{name}", env!("CARGO_MANIFEST_DIR"));
        let data = std::fs::read(&path).expect("fixture");
        (data, name.rsplit('.').next().unwrap_or_default())
    }

    fn decode(name: &str) -> Result<Image, String> {
        let (data, extension) = fixture(name);
        Image::decode(data, extension)
    }

    #[test]
    fn decodes_each_format_with_its_size_in_points() {
        assert_eq!(decode("pixel.png").expect("png").size(), (Pt(40.0), Pt(20.0)));
        assert_eq!(decode("photo.jpg").expect("jpeg").size(), (Pt(30.0), Pt(20.0)));
        assert_eq!(decode("drawing.svg").expect("svg").size(), (Pt(60.0), Pt(30.0)));
    }

    #[test]
    fn rejects_malformed_content() {
        assert!(
            decode("truncated.png")
                .unwrap_err()
                .starts_with("malformed PNG image: ")
        );
        assert!(decode("broken.svg").unwrap_err().starts_with("malformed SVG image: "));
    }

    #[test]
    fn rejects_svg_that_refers_to_other_files() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="a.png" width="5" height="5"/></svg>"#;
        assert_eq!(
            Image::decode(svg.as_bytes().to_vec(), "svg").unwrap_err(),
            "an SVG image cannot refer to other files; embed its images as data URLs"
        );
    }

    #[test]
    fn rejects_content_that_contradicts_the_extension() {
        assert_eq!(
            decode("jpeg-data.png").unwrap_err(),
            "the file contains JPEG data but its name ends in .png"
        );
    }

    #[test]
    fn rejects_gif_and_unknown_extensions() {
        assert_eq!(
            decode("still.gif").unwrap_err(),
            "GIF images are not supported; use PNG, JPEG, or SVG"
        );
        let (data, _) = fixture("pixel.png");
        assert_eq!(Image::decode(data, "bmp").unwrap_err(), UNSUPPORTED);
    }
}
