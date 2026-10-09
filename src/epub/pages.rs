//! The XHTML documents around the body: the cover, the title page, and the navigation document, which is
//! also the contents page where the document asks for one.

use std::collections::HashMap;
use std::fmt::Write;

use super::content::file_name;
use super::{css, xml};
use crate::config::resolved::{Config, SlotContent};
use crate::config::source::Resource;
use crate::config::theme::{Align, SlotStyle, TemplateStyle};
use crate::config::values::Pt;
use crate::layout::fields::Fields;
use crate::layout::structure::Structure;
use crate::layout::titles;

/// A complete XHTML content document. `kind` is the `epub:type` of its body.
pub fn xhtml(lang: &str, title: &str, kind: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" \
         xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{lang}\" xml:lang=\"{lang}\">\n<head>\n<meta \
         charset=\"UTF-8\"/>\n<title>{}</title>\n<link rel=\"stylesheet\" type=\"text/css\" \
         href=\"style.css\"/>\n</head>\n<body epub:type=\"{kind}\">\n{body}\n</body>\n</html>\n",
        xml::text(title)
    )
}

/// The cover page around the cover image at `path`.
pub fn cover(path: &str, title: &str) -> String {
    format!(
        "<section class=\"cover\" epub:type=\"cover\"><img src=\"{}\" alt=\"{}\"/></section>",
        xml::attribute(path),
        xml::attribute(title)
    )
}

/// The title page from the slots of the title layout in use, or `None` if none has a value. Slots are
/// stacked in order, each in its style with its `space-before`; anchors and group positions are dropped.
pub fn title_page(config: &Config, fields: &Fields, theme_images: &HashMap<Resource, String>) -> Option<String> {
    let mut body = String::new();
    for (slot, align) in titles::slots_in_use(config, fields) {
        let style = config.slot_style(&slot.style);
        let mut classes = css::class(css::slot_name(&slot.style));
        let align = match align {
            Some(Align::Left) => " align-left",
            Some(Align::Center) => " align-center",
            Some(Align::Right) => " align-right",
            Some(Align::Justify) | None => "",
        };
        classes.push_str(align);
        let space = em(slot.space_before, style.size);
        let mut margin = (slot.space_before.0 > 0.0).then(|| format!(" style=\"margin-top: {space}\""));
        let mut paragraph = |body: &mut String, classes: &str, content: String| {
            let margin = margin.take().unwrap_or_default();
            write!(body, "<p class=\"{classes}\"{margin}>{content}</p>").expect("writing to a string succeeds");
        };
        match &slot.content {
            SlotContent::Text(text) => {
                let Ok(lines) = text.fill(|placeholder| fields.value(placeholder)) else {
                    continue;
                };
                if slot.style == SlotStyle::Template(TemplateStyle::Abstract) {
                    let heading = format!("{}{align}", css::class("abstract-heading"));
                    paragraph(&mut body, &heading, xml::text(&config.labels.abstract_));
                }
                let paragraphs = lines.iter().flat_map(|line| line.split("\n\n"));
                for text in paragraphs.filter(|text| !text.trim().is_empty()) {
                    paragraph(&mut body, &classes, xml::text(&text.replace('\n', " ")));
                }
            }
            SlotContent::Image { image, width } => {
                let image = format!(
                    "<img src=\"{}\" alt=\"\" style=\"width: {}\"/>",
                    xml::attribute(&theme_images[image]),
                    em(*width, style.size)
                );
                paragraph(&mut body, &classes, image);
            }
        }
    }
    (!body.is_empty()).then(|| format!("<section class=\"titlepage\" epub:type=\"titlepage\">{body}</section>"))
}

/// A landmark of the navigation document, such as the start of the body matter.
pub struct Landmark {
    pub kind: &'static str,
    pub href: String,
    pub label: String,
}

/// The body of the navigation document: the table of contents with the listed headings up to
/// `document.toc-depth`, nested by level, and the landmarks. `anchors` holds the file of each anchor.
/// Without listed headings the contents hold `start`, a link to the first document and its title.
pub fn navigation(
    config: &Config,
    structure: &Structure,
    anchors: &[Option<usize>],
    landmarks: &[Landmark],
    start: (&str, &str),
) -> String {
    let depth = config.document.toc_depth.get();
    let mut body = format!(
        "<nav epub:type=\"toc\" id=\"toc\"><h1>{}</h1><ol>",
        xml::text(&config.labels.contents)
    );
    let empty = body.len();
    // The open entries by level, each with whether it has a nested list open.
    let mut open: Vec<(u8, bool)> = Vec::new();
    for heading in structure
        .headings
        .iter()
        .filter(|heading| heading.listed && heading.level <= depth)
    {
        let title = heading.title();
        if title.trim().is_empty() {
            continue;
        }
        while let Some((level, nested)) = open.last_mut() {
            if *level < heading.level {
                if !*nested {
                    body.push_str("<ol>");
                    *nested = true;
                }
                break;
            }
            if *nested {
                body.push_str("</ol>");
            }
            body.push_str("</li>");
            open.pop();
        }
        let file = anchors[heading.anchor].expect("every heading of the body is written");
        write!(
            body,
            "<li><a href=\"{}#a{}\">{}</a>",
            file_name(file),
            heading.anchor,
            xml::text(&title)
        )
        .expect("writing to a string succeeds");
        open.push((heading.level, false));
    }
    for (_, nested) in open.into_iter().rev() {
        if nested {
            body.push_str("</ol>");
        }
        body.push_str("</li>");
    }
    // A list needs an entry.
    if body.len() == empty {
        let (href, title) = start;
        write!(
            body,
            "<li><a href=\"{}\">{}</a></li>",
            xml::attribute(href),
            xml::text(title)
        )
        .expect("writing to a string succeeds");
    }
    body.push_str("</ol></nav>");
    if landmarks.is_empty() {
        return body;
    }
    body.push_str("\n<nav epub:type=\"landmarks\" hidden=\"hidden\"><ol>");
    for landmark in landmarks {
        write!(
            body,
            "<li><a epub:type=\"{}\" href=\"{}\">{}</a></li>",
            landmark.kind,
            xml::attribute(&landmark.href),
            xml::text(&landmark.label)
        )
        .expect("writing to a string succeeds");
    }
    body.push_str("</ol></nav>");
    body
}

fn em(length: Pt, size: Pt) -> String {
    format!("{}em", (length.0 / size.0 * 1000.0).round() / 1000.0)
}
