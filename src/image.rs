//! Image decoding and validation for PNG, JPEG, and SVG.
//!
//! Raster images take their natural size from the density stored in the file, or one point per pixel
//! without one.
//!
//! Images are fully decoded here, so a malformed file is reported with the layout error and never
//! surfaces while the PDF is written. SVG text uses only the bundled fonts, and an SVG cannot read files.

use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use krilla::image::Image as RasterImage;
use png::Transformations;
use usvg::fontdb::{Database, Source};
use usvg::{ImageHrefResolver, Options, Tree};
use zune_jpeg::JpegDecoder;

mod density;

use self::density::Density;
use crate::config::values::Pt;
use crate::resource::Bytes;
use crate::text::bundled;

/// PDF points per inch.
const PT_PER_INCH: f64 = 72.0;
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
/// The bytes of decoded images a renderer keeps between jobs.
const CACHE_BUDGET: usize = 256 * 1024 * 1024;

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

/// A decoded, validated image. Cheap to clone, and shared with whatever else holds it.
#[derive(Clone)]
pub struct Image {
    /// The file as read, for the EPUB and for a painter that draws raster images itself.
    source: Bytes,
    drawn: Drawn,
    width: f64,
    height: f64,
}

/// The decoded content, as the PDF writer draws it.
#[derive(Clone)]
pub(crate) enum Drawn {
    Raster(RasterImage),
    Svg(Arc<Tree>),
}

/// What a painter draws for a [`Image`].
pub enum Pixels<'a> {
    /// A raster image of this many pixels. [`Image::source`] holds the PNG or JPEG file to decode.
    Raster { width: u32, height: u32 },
    /// A parsed SVG, drawn from its tree.
    Svg(&'a Tree),
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let format = match self.drawn {
            Drawn::Raster(_) => "raster",
            Drawn::Svg(_) => "SVG",
        };
        write!(f, "Image({format} {} x {} pt)", self.width, self.height)
    }
}

/// The size of the stand-in for a missing image: four inches wide, in the proportions of a screen.
const MISSING_WIDTH: f64 = 288.0;
const MISSING_HEIGHT: f64 = 162.0;
/// The stand-in's grey, quiet on a white page or a tinted one.
const MISSING_GREY: &str = "#8c8c8c";

impl Image {
    /// A stand-in for an image file that is not there: a dashed box of a usual figure's size with the path written in
    /// it, in a quiet grey, so pages keep their shape and show what is missing.
    pub fn missing(path: &str) -> Image {
        let (width, height) = (MISSING_WIDTH / PT_PER_SVG_PX, MISSING_HEIGHT / PT_PER_SVG_PX);
        let escaped = path
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><rect x="1" y="1" width="{inner_width}" height="{inner_height}" fill="none" stroke="{MISSING_GREY}" stroke-width="1" stroke-dasharray="4 3"/><text x="{middle}" y="{baseline}" font-family="Libertinus Serif" font-size="12" fill="{MISSING_GREY}" text-anchor="middle">{escaped}</text></svg>"#,
            inner_width = width - 2.0,
            inner_height = height - 2.0,
            middle = width / 2.0,
            baseline = height / 2.0 + 4.0,
        );
        decode_as(Format::Svg, Bytes::from(svg.into_bytes())).expect("the stand-in is valid SVG")
    }

    /// Decodes and validates `data`. `extension` is the file name extension as written, without the dot, any case
    /// (may be empty). Errors are plain messages without file name or location.
    ///
    /// PNG and JPEG are decoded once here to validate them. krilla decodes PNG again when the PDF is written.
    pub fn decode(data: impl Into<Bytes>, extension: &str) -> Result<Image, String> {
        let data = data.into();
        let format = check(&data, extension)?;
        decode_as(format, data)
    }

    /// The natural size in points. Raster images use their stored density, else count one pixel as one point
    /// (72 dpi); SVG uses CSS units, 96 px per inch, so 0.75 pt per px.
    pub fn size(&self) -> (Pt, Pt) {
        (Pt(self.width), Pt(self.height))
    }

    /// The bytes of the image file.
    pub fn source(&self) -> &Bytes {
        &self.source
    }

    /// Roughly what keeping this image costs: the file, and the samples krilla decodes a raster image into.
    fn cost(&self) -> usize {
        let samples = match &self.drawn {
            Drawn::Raster(image) => {
                let (width, height) = image.size();
                width as usize * height as usize * 4
            }
            Drawn::Svg(_) => 0,
        };
        self.source.len() + samples
    }

    /// What a painter draws.
    pub fn pixels(&self) -> Pixels<'_> {
        match &self.drawn {
            Drawn::Raster(image) => {
                let (width, height) = image.size();
                Pixels::Raster { width, height }
            }
            Drawn::Svg(tree) => Pixels::Svg(tree),
        }
    }

    pub(crate) fn drawn(&self) -> &Drawn {
        &self.drawn
    }
}

/// What format a file is, if its contents suit the extension it has. A decoded result depends on the bytes alone
/// once this has passed, so a cache keyed by content can answer after it.
fn check(data: &[u8], extension: &str) -> Result<Format, String> {
    let claimed = Format::from_extension(extension);
    match (sniff(data), claimed) {
        (Sniffed::Gif, _) => Err("GIF images are not supported; use PNG, JPEG, or SVG".to_owned()),
        (Sniffed::WebP, _) => Err("WebP images are not supported; use PNG, JPEG, or SVG".to_owned()),
        (_, None) => Err(UNSUPPORTED.to_owned()),
        (Sniffed::Known(found), Some(claimed)) if found != claimed => Err(mismatch(found, extension)),
        (Sniffed::Known(found), Some(_)) => Ok(found),
        (Sniffed::Unknown, Some(Format::Svg)) => Ok(Format::Svg),
        (Sniffed::Unknown, Some(claimed)) => match parse_svg(data) {
            Ok(_) => Err(mismatch(Format::Svg, extension)),
            Err(_) => Err(malformed(claimed, "invalid file signature")),
        },
    }
}

fn decode_as(format: Format, data: Bytes) -> Result<Image, String> {
    match format {
        Format::Png => decode_png(data),
        Format::Jpeg => decode_jpeg(data),
        Format::Svg => decode_svg(data),
    }
}

struct Kept {
    image: Image,
    /// The newest job that used this image.
    used: AtomicU64,
}

#[derive(Default)]
struct Table {
    images: HashMap<u64, Kept>,
    cost: usize,
}

/// The decoded images a renderer keeps between jobs, by content. Only ever gains completed images, so a failed
/// job leaves it sound.
#[derive(Default)]
pub struct ImageCache {
    table: RwLock<Table>,
    #[cfg(test)]
    pub(crate) decodes: AtomicUsize,
}

impl ImageCache {
    /// Like [`Image::decode`], decoding only what no job has decoded yet. An image that would pass the budget
    /// serves this job and is not kept.
    pub fn image(&self, data: &Bytes, extension: &str, job: u64) -> Result<Image, String> {
        let format = check(data, extension)?;
        if let Some(kept) = self
            .table
            .read()
            .expect("no panic while caching")
            .images
            .get(&data.hash())
        {
            kept.used.store(job, Ordering::Relaxed);
            return Ok(kept.image.clone());
        }
        let image = decode_as(format, data.clone())?;
        #[cfg(test)]
        self.decodes.fetch_add(1, Ordering::Relaxed);
        let cost = image.cost();
        let mut table = self.table.write().expect("no panic while caching");
        if table.cost + cost <= CACHE_BUDGET && !table.images.contains_key(&data.hash()) {
            table.cost += cost;
            let used = AtomicU64::new(job);
            let image = image.clone();
            table.images.insert(data.hash(), Kept { image, used });
        }
        Ok(image)
    }

    /// Forgets the images no job since `oldest` used.
    pub fn sweep(&self, oldest: u64) {
        let mut table = self.table.write().expect("no panic while caching");
        table
            .images
            .retain(|_, kept| kept.used.load(Ordering::Relaxed) >= oldest);
        table.cost = table.images.values().map(|kept| kept.image.cost()).sum();
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

fn decode_png(data: Bytes) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(&*data));
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| malformed(Format::Png, e))?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("image is too large")?];
    reader.next_frame(&mut buffer).map_err(|e| malformed(Format::Png, e))?;
    let density = density::png(reader.info());
    let image = RasterImage::from_png(data.data(), false).map_err(|e| malformed(Format::Png, e))?;
    raster(data, image, density)
}

fn decode_jpeg(data: Bytes) -> Result<Image, String> {
    // krilla embeds JPEG data as is and reads only the headers, so decode all of it here.
    JpegDecoder::new(Cursor::new(&*data))
        .decode()
        .map_err(|e| malformed(Format::Jpeg, e))?;
    let density = density::jpeg(&data);
    let image = RasterImage::from_jpeg(data.data(), false).map_err(|e| malformed(Format::Jpeg, e))?;
    raster(data, image, density)
}

fn raster(source: Bytes, image: RasterImage, density: Option<Density>) -> Result<Image, String> {
    let (width, height) = image.size();
    if width == 0 || height == 0 {
        return Err("image has no pixels".to_owned());
    }
    let (per_pixel_x, per_pixel_y) = density.map_or((1.0, 1.0), |d| (PT_PER_INCH / d.x, PT_PER_INCH / d.y));
    Ok(Image {
        source,
        width: f64::from(width) * per_pixel_x,
        height: f64::from(height) * per_pixel_y,
        drawn: Drawn::Raster(image),
    })
}

fn decode_svg(data: Bytes) -> Result<Image, String> {
    let tree = parse_svg(&data)?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("image has no area".to_owned());
    }
    Ok(Image {
        source: data,
        width: f64::from(size.width()) * PT_PER_SVG_PX,
        height: f64::from(size.height()) * PT_PER_SVG_PX,
        drawn: Drawn::Svg(Arc::new(tree)),
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
    fn a_missing_image_stands_in_as_a_box_of_a_figures_size() {
        let image = Image::missing("assets/<missing> & co.png");
        assert_eq!(image.size(), (Pt(MISSING_WIDTH), Pt(MISSING_HEIGHT)));
    }

    #[test]
    fn decodes_each_format_with_its_size_in_points() {
        assert_eq!(decode("pixel.png").expect("png").size(), (Pt(40.0), Pt(20.0)));
        assert_eq!(decode("photo.jpg").expect("jpeg").size(), (Pt(30.0), Pt(20.0)));
        assert_eq!(decode("drawing.svg").expect("svg").size(), (Pt(60.0), Pt(30.0)));
    }

    #[test]
    fn sizes_raster_images_by_their_stored_density() {
        let (mut png, _) = fixture("pixel.png");
        let mut chunk = b"pHYs".to_vec();
        for value in [11811u32, 5906] {
            chunk.extend(value.to_be_bytes());
        }
        chunk.push(1);
        let crc = chunk.iter().fold(u32::MAX, |crc, byte| {
            (0..8).fold(crc ^ u32::from(*byte), |crc, _| (crc >> 1) ^ (0xedb8_8320 * (crc & 1)))
        });
        let mut bytes = 9u32.to_be_bytes().to_vec();
        bytes.extend(chunk);
        bytes.extend((!crc).to_be_bytes());
        png.splice(33..33, bytes);
        let (width, height) = Image::decode(png, "png").expect("png with density").size();
        assert!((width.0 - 40.0 * 72.0 / 300.0).abs() < 0.01 && (height.0 - 20.0 * 72.0 / 150.0).abs() < 0.01);

        let (mut jpeg, _) = fixture("photo.jpg");
        jpeg[13..18].copy_from_slice(&[1, 0, 144, 0, 144]);
        assert_eq!(Image::decode(jpeg, "jpg").expect("jpeg").size(), (Pt(15.0), Pt(10.0)));
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
