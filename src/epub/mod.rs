//! EPUB 3 output: a reflowable book from the same document, configuration, and citations as the PDF.
//!
//! Nothing is laid out. The body becomes XHTML [content] documents, the theme a [stylesheet](css), and the
//! front matter the [package] metadata. Bundled and file fonts are [embedded](fonts); installed ones are
//! only named. The container is a [zip] written in a fixed order with fixed timestamps, so the same input
//! gives the same bytes.

mod content;
mod css;
mod fonts;
mod package;
mod pages;
#[cfg(test)]
mod tests;
mod xml;
mod zip;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use self::content::{Context, Item, Picture, file_name};
use self::pages::Landmark;
use crate::citations::Cited;
use crate::config::resolved::Config;
use crate::config::source::{Resource, Source};
use crate::config::theme::{Lang, WideBlock};
use crate::date::Date;
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Document};
use crate::image::Image;
use crate::layout::fields::Fields;
use crate::layout::structure::{Structure, plain};

/// What an EPUB is made from.
pub struct Book<'a> {
    pub document: &'a Document,
    pub cited: &'a Cited,
    pub config: &'a Config,
    pub source: &'a Source,
    /// The decoded [`Document::images`], whose files are relative to `dir`.
    pub images: &'a [Image],
    pub dir: &'a Path,
    /// The build date, for `{build-date}` and `{year}`.
    pub today: Date,
    /// The day of `dcterms:modified`.
    pub modified: Date,
    /// The title for a document without a front matter title or level 1 heading, such as its file name.
    pub fallback_title: &'a str,
}

/// An EPUB file and the warnings about what it leaves out.
pub struct Written {
    pub bytes: Vec<u8>,
    /// The content documents in the reading order.
    pub documents: usize,
    pub warnings: Vec<String>,
}

/// A file in the EPUB, relative to the package document.
pub struct Asset {
    pub path: String,
    pub media_type: &'static str,
    pub data: Cow<'static, [u8]>,
}

/// Writes `book` as an EPUB. The content is checked as layout checks it; images and fonts are read again
/// to be embedded as they are.
pub fn write(book: &Book) -> Result<Written, Vec<Diagnostic>> {
    let Book { document, config, .. } = *book;
    let fields = Fields::new(document, config, book.today);
    crate::layout::check(document, config, &fields, book.source)?;
    let metadata = &config.metadata;
    let lang = match config.document.lang {
        Lang::En => "en",
        Lang::De => "de",
    };
    let heading = document.blocks.iter().find_map(|block| match block {
        Block::Heading { level: 1, content, .. } => Some(plain(content).trim().to_owned()),
        _ => None,
    });
    let title = (metadata.title.clone().filter(|title| !title.trim().is_empty()))
        .or(heading.filter(|title| !title.is_empty()))
        .unwrap_or_else(|| book.fallback_title.to_owned());
    let seed = [title.as_str(), lang]
        .into_iter()
        .chain(metadata.authors.iter().map(String::as_str));
    let identifier = package::identifier(&metadata.meta, &seed.collect::<Vec<_>>().join("\n")).map_err(|message| {
        vec![Diagnostic::new(Some(book.source.clone()), message).property(Some("meta.isbn".to_owned()))]
    })?;

    let (pictures, mut assets) = images(book)?;
    let (theme_images, theme_assets) = theme_images(config)?;
    assets.extend(theme_assets);
    let cover = cover(config)?;
    let (rules, families) = css::rules(config);
    let fonts = fonts::embed(config, &families)?;
    let stylesheet = css::font_faces(&fonts.faces) + &rules;
    assets.extend(fonts.files);

    let structure = Structure::new(document, config, book.cited.references());
    let content = content::write(&Context {
        document,
        cited: book.cited,
        config,
        structure: &structure,
        fields: &fields,
        images: &pictures,
        theme_images: &theme_images,
    });
    let title_page = pages::title_page(config, &fields, &theme_images);

    let mut documents = Vec::new();
    let mut spine = Vec::new();
    let mut items = vec![package::Item {
        id: "style".to_owned(),
        href: "style.css".to_owned(),
        media_type: "text/css",
        properties: None,
    }];
    let mut landmarks = Vec::new();
    let mut add = |id: &str, name: String, text: String, documents: &mut Vec<(String, String)>| {
        items.push(package::Item {
            id: id.to_owned(),
            href: name.clone(),
            media_type: "application/xhtml+xml",
            properties: None,
        });
        documents.push((name, text));
    };
    if let Some(cover) = &cover {
        let body = pages::cover(&cover.path, &title);
        add(
            "cover",
            "cover.xhtml".to_owned(),
            pages::xhtml(lang, &title, "cover", &body),
            &mut documents,
        );
        spine.push("cover".to_owned());
        landmarks.push(Landmark {
            kind: "cover",
            href: "cover.xhtml".to_owned(),
            label: "Cover".to_owned(),
        });
    }
    if let Some(body) = title_page {
        let page = pages::xhtml(lang, &title, "frontmatter", &body);
        add("titlepage", "titlepage.xhtml".to_owned(), page, &mut documents);
        spine.push("titlepage".to_owned());
    }
    // `toc` places the contents after the title; `::: toc` places them itself.
    let mut order = content.order.clone();
    if config.document.toc && !structure.contents {
        order.insert(0, Item::Contents);
    }
    let mut contents = false;
    for item in &order {
        match item {
            Item::Contents if !contents => {
                contents = true;
                spine.push("nav".to_owned());
                landmarks.push(Landmark {
                    kind: "toc",
                    href: "nav.xhtml#toc".to_owned(),
                    label: config.labels.contents.clone(),
                });
            }
            Item::Contents => {}
            Item::File(index) => {
                let file = &content.files[*index];
                let id = format!("text-{:03}", index + 1);
                let page = pages::xhtml(lang, file.title.as_deref().unwrap_or(&title), file.matter, &file.body);
                add(&id, file_name(*index), page, &mut documents);
                spine.push(id);
            }
        }
    }
    let start = (content.files.iter().position(|file| file.matter == "bodymatter")).map(|index| Landmark {
        kind: "bodymatter",
        href: file_name(index),
        label: content.files[index].title.clone().unwrap_or_else(|| title.clone()),
    });
    landmarks.extend(start);
    let first = file_name(0);
    let navigation = pages::navigation(config, &structure, &content.anchors, &landmarks, (&first, &title));
    let navigation = pages::xhtml(lang, &config.labels.contents, "frontmatter", &navigation);
    items.insert(
        0,
        package::Item {
            id: "nav".to_owned(),
            href: "nav.xhtml".to_owned(),
            media_type: "application/xhtml+xml",
            properties: Some("nav"),
        },
    );
    for (index, asset) in cover.iter().chain(&assets).enumerate() {
        let is_cover = index == 0 && cover.is_some();
        items.push(package::Item {
            id: if is_cover {
                "cover-image".to_owned()
            } else {
                format!("asset-{index}")
            },
            href: asset.path.clone(),
            media_type: asset.media_type,
            properties: is_cover.then_some("cover-image"),
        });
    }

    let date = metadata.date.as_deref().and_then(|date| Date::iso(date.trim()));
    let description = (metadata.abstract_.as_deref())
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| !text.is_empty());
    let opf = package::document(
        &package::Metadata {
            identifier,
            title: &title,
            subtitle: metadata
                .subtitle
                .as_deref()
                .filter(|subtitle| !subtitle.trim().is_empty()),
            authors: &metadata.authors,
            lang,
            date,
            description,
            modified: book.modified,
        },
        &items,
        &spine.iter().map(String::as_str).collect::<Vec<_>>(),
    );

    let fail = |message: String| vec![Diagnostic::new(None, message)];
    let mut zip = zip::Zip::default();
    zip.add("mimetype", b"application/epub+zip", false).map_err(fail)?;
    zip.add("META-INF/container.xml", package::CONTAINER.as_bytes(), true)
        .map_err(fail)?;
    zip.add("EPUB/package.opf", opf.as_bytes(), true).map_err(fail)?;
    zip.add("EPUB/nav.xhtml", navigation.as_bytes(), true).map_err(fail)?;
    zip.add("EPUB/style.css", stylesheet.as_bytes(), true).map_err(fail)?;
    for (name, text) in &documents {
        zip.add(&format!("EPUB/{name}"), text.as_bytes(), true).map_err(fail)?;
    }
    for asset in cover.iter().chain(&assets) {
        // PNG and JPEG are compressed already.
        let compress = !matches!(asset.media_type, "image/png" | "image/jpeg");
        zip.add(&format!("EPUB/{}", asset.path), &asset.data, compress)
            .map_err(fail)?;
    }
    Ok(Written {
        bytes: zip.finish().map_err(fail)?,
        documents: spine.len(),
        warnings: fonts.warnings,
    })
}

/// The document images as pictures and files. A picture is as wide as its natural width in the PDF's text
/// width, so images keep their size relative to the text.
fn images(book: &Book) -> Result<(Vec<Picture>, Vec<Asset>), Vec<Diagnostic>> {
    let page = &book.config.page;
    let frame = if page.wide.contains(&WideBlock::Figure) {
        page.text_width()
    } else {
        page.prose_width
    };
    let mut pictures = Vec::new();
    let mut assets = Vec::new();
    let mut errors = Vec::new();
    for (index, (file, image)) in book.document.images.iter().zip(book.images).enumerate() {
        match std::fs::read(book.dir.join(&file.path)) {
            Ok(data) => {
                let (media_type, extension) = image_type(&data);
                let path = format!("images/image-{}.{extension}", index + 1);
                pictures.push(Picture {
                    path: path.clone(),
                    width: (image.size().0.0 / frame.0 * 100.0).min(100.0),
                });
                assets.push(Asset {
                    path,
                    media_type,
                    data: Cow::Owned(data),
                });
            }
            Err(e) => {
                let message = format!("cannot read image {}: {e}", file.path);
                errors.push(Diagnostic::new(Some(book.source.clone()), message).at(file.at.line, file.at.column));
            }
        }
    }
    if errors.is_empty() {
        Ok((pictures, assets))
    } else {
        Err(errors)
    }
}

/// The EPUB path of each theme image by resource.
type ThemePaths = HashMap<Resource, String>;

/// The theme images the book shows, with their EPUB paths.
fn theme_images(config: &Config) -> Result<(ThemePaths, Vec<Asset>), Vec<Diagnostic>> {
    let used = crate::layout::theme_images(config);
    let mut paths = HashMap::new();
    let mut assets = Vec::new();
    for (name, resource) in config.images.iter().filter(|(_, resource)| used.contains(resource)) {
        if paths.contains_key(resource) {
            continue;
        }
        let data = read(resource, &format!("images.{name}"))?;
        let (media_type, extension) = image_type(&data);
        let path = format!("images/theme-{}.{extension}", paths.len() + 1);
        paths.insert(resource.clone(), path.clone());
        assets.push(Asset {
            path,
            media_type,
            data: Cow::Owned(data),
        });
    }
    Ok((paths, assets))
}

/// The cover image from front matter `cover`, checked to be a PNG, JPEG, or SVG image.
fn cover(config: &Config) -> Result<Option<Asset>, Vec<Diagnostic>> {
    let Some(resource) = &config.cover_file else {
        return Ok(None);
    };
    let data = read(resource, "cover")?;
    let extension = Path::new(&resource.path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    Image::decode(data.clone(), extension).map_err(|message| {
        let message = format!("image {resource}: {message}");
        vec![Diagnostic::new(None, message).property(Some("cover".to_owned()))]
    })?;
    let (media_type, extension) = image_type(&data);
    Ok(Some(Asset {
        path: format!("images/cover.{extension}"),
        media_type,
        data: Cow::Owned(data),
    }))
}

fn read(resource: &Resource, property: &str) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let path = resource.file().ok_or_else(|| {
        vec![Diagnostic::new(None, format!("{resource} is not bundled")).property(Some(property.to_owned()))]
    })?;
    std::fs::read(path).map_err(|e| {
        let message = format!("cannot read image {resource}: {e}");
        vec![Diagnostic::new(None, message).property(Some(property.to_owned()))]
    })
}

/// The media type and extension of an image that [`Image::decode`] accepted.
fn image_type(data: &[u8]) -> (&'static str, &'static str) {
    if data.starts_with(b"\x89PNG") {
        ("image/png", "png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        ("image/jpeg", "jpg")
    } else {
        ("image/svg+xml", "svg")
    }
}
