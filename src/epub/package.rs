//! The package document with the book's metadata, manifest, and spine, and the container file that points
//! to it.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::xml;
use crate::config::front_matter::MetaValue;
use crate::date::Date;

pub const CONTAINER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" \
    xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n<rootfiles>\n<rootfile \
    full-path=\"EPUB/package.opf\" media-type=\"application/oebps-package+xml\"/>\n</rootfiles>\n</container>\n";

pub struct Metadata<'a> {
    pub identifier: String,
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    pub authors: &'a [String],
    pub lang: &'a str,
    /// The publication date, if the front matter writes it as `YYYY-MM-DD`.
    pub date: Option<Date>,
    pub description: Option<String>,
    pub modified: Date,
}

/// A file of the EPUB, relative to the package document.
pub struct Item {
    pub id: String,
    pub href: String,
    pub media_type: &'static str,
    /// Such as `nav` or `cover-image`.
    pub properties: Option<&'static str>,
}

/// The package document. `spine` lists the ids of the content documents in reading order.
pub fn document(metadata: &Metadata, items: &[Item], spine: &[&str]) -> String {
    let mut opf = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" \
         unique-identifier=\"id\"",
    );
    let line = |opf: &mut String, text: String| {
        opf.push_str(&text);
        opf.push('\n');
    };
    line(&mut opf, format!(" xml:lang=\"{}\">", metadata.lang));
    line(
        &mut opf,
        "<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">".to_owned(),
    );
    line(
        &mut opf,
        format!(
            "<dc:identifier id=\"id\">{}</dc:identifier>",
            xml::text(&metadata.identifier)
        ),
    );
    line(
        &mut opf,
        format!("<dc:title id=\"title\">{}</dc:title>", xml::text(metadata.title)),
    );
    if let Some(subtitle) = metadata.subtitle {
        line(
            &mut opf,
            "<meta refines=\"#title\" property=\"title-type\">main</meta>".to_owned(),
        );
        line(
            &mut opf,
            format!("<dc:title id=\"subtitle\">{}</dc:title>", xml::text(subtitle)),
        );
        line(
            &mut opf,
            "<meta refines=\"#subtitle\" property=\"title-type\">subtitle</meta>".to_owned(),
        );
    }
    for (index, author) in metadata.authors.iter().enumerate() {
        let id = format!("author-{}", index + 1);
        line(
            &mut opf,
            format!("<dc:creator id=\"{id}\">{}</dc:creator>", xml::text(author)),
        );
        line(
            &mut opf,
            format!("<meta refines=\"#{id}\" property=\"role\" scheme=\"marc:relators\">aut</meta>"),
        );
    }
    line(&mut opf, format!("<dc:language>{}</dc:language>", metadata.lang));
    if let Some(date) = metadata.date {
        line(&mut opf, format!("<dc:date>{}</dc:date>", day(date)));
    }
    if let Some(description) = &metadata.description {
        line(
            &mut opf,
            format!("<dc:description>{}</dc:description>", xml::text(description)),
        );
    }
    line(
        &mut opf,
        format!(
            "<meta property=\"dcterms:modified\">{}T00:00:00Z</meta>",
            day(metadata.modified)
        ),
    );
    if let Some(cover) = items.iter().find(|item| item.properties == Some("cover-image")) {
        // For readers that only know EPUB 2.
        line(&mut opf, format!("<meta name=\"cover\" content=\"{}\"/>", cover.id));
    }
    line(&mut opf, "</metadata>\n<manifest>".to_owned());
    for item in items {
        let properties = item
            .properties
            .map(|p| format!(" properties=\"{p}\""))
            .unwrap_or_default();
        line(
            &mut opf,
            format!(
                "<item id=\"{}\" href=\"{}\" media-type=\"{}\"{properties}/>",
                item.id,
                xml::attribute(&item.href),
                item.media_type
            ),
        );
    }
    line(&mut opf, "</manifest>\n<spine>".to_owned());
    for id in spine {
        writeln!(opf, "<itemref idref=\"{id}\"/>").expect("writing to a string succeeds");
    }
    opf.push_str("</spine>\n</package>\n");
    opf
}

/// The book's identifier: the ISBN in `meta.isbn` as a URN, else a UUID derived from `seed`, such as the
/// title and authors, so it stays the same while the text changes.
pub fn identifier(meta: &BTreeMap<String, MetaValue>, seed: &str) -> Result<String, String> {
    let Some(value) = meta.get("isbn") else {
        return Ok(format!("urn:uuid:{}", uuid(seed)));
    };
    let invalid = || "meta.isbn must be an ISBN of 10 or 13 digits, such as 978-3-16-148410-0".to_owned();
    let MetaValue::Text(text) = value else {
        return Err(invalid());
    };
    let isbn: String = text.chars().filter(|c| !matches!(c, '-' | ' ')).collect();
    let valid = match isbn.len() {
        13 => isbn.bytes().all(|b| b.is_ascii_digit()),
        10 => isbn
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_digit() || (i == 9 && b == b'X')),
        _ => false,
    };
    if valid {
        Ok(format!("urn:isbn:{isbn}"))
    } else {
        Err(invalid())
    }
}

/// A version 8 UUID from the 128-bit FNV-1a hash of `seed`, which is stable across platforms and releases.
fn uuid(seed: &str) -> String {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let hash = seed
        .bytes()
        .fold(OFFSET, |hash, byte| (hash ^ u128::from(byte)).wrapping_mul(PRIME));
    let mut bytes = hash.to_be_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn day(date: Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_isbn_from_meta_or_derives_a_stable_uuid() {
        let meta = |value: &str| BTreeMap::from([("isbn".to_owned(), MetaValue::Text(value.to_owned()))]);
        assert_eq!(
            identifier(&meta("978-3-16-148410-0"), "x"),
            Ok("urn:isbn:9783161484100".to_owned())
        );
        assert_eq!(
            identifier(&meta("0 306 40615 X"), "x"),
            Ok("urn:isbn:030640615X".to_owned())
        );
        assert!(identifier(&meta("ISBN 978"), "x").is_err());

        let derived = identifier(&BTreeMap::new(), "The Lighthouse Keeper\nMara Ellison").unwrap();
        assert_eq!(
            derived,
            identifier(&BTreeMap::new(), "The Lighthouse Keeper\nMara Ellison").unwrap()
        );
        assert_ne!(derived, identifier(&BTreeMap::new(), "Another book").unwrap());
        let uuid = derived.strip_prefix("urn:uuid:").unwrap();
        assert_eq!(uuid.len(), 36);
        assert_eq!(&uuid[14..15], "8");
    }
}
