//! The body as XHTML content documents.
//!
//! The body is split into files where a level 1 heading, the bibliography, a part of a book, or a page break
//! starts, and where `::: toc` places the contents page. Each file is front, body, or back matter. Headings,
//! figures, and tables carry the numbers of [`Structure`], and cross-references and citations link to their
//! targets in whichever file those are. A page reference has no page in a reflowable book, so it shows the
//! target's reference text, as a plain cross-reference does. Footnotes are EPUB notes at the end of the
//! file that references them.

use std::collections::HashMap;
use std::fmt::Write;

use super::{css, xml};
use crate::citations::Cited;
use crate::config::resolved::{Config, SceneMark};
use crate::config::source::Resource;
use crate::config::theme::NumberPosition;
use crate::document::{Block, Cell, Class, Column, ColumnAlign, Document, Inline, InlineStyle, Link, Matter, Row};
use crate::layout::fields::Fields;
use crate::layout::structure::{Numbered, Structure, plain};
use crate::layout::{NO_DROP_CAP, lead_in};

/// What rendering the body needs besides the document.
pub struct Context<'a> {
    pub document: &'a Document,
    pub cited: &'a Cited,
    pub config: &'a Config,
    pub structure: &'a Structure,
    pub fields: &'a Fields<'a>,
    /// The EPUB path and width of each of [`Document::images`].
    pub images: &'a [Picture],
    /// The EPUB path of each theme image the body shows.
    pub theme_images: &'a HashMap<Resource, String>,
}

/// A document image in the EPUB.
pub struct Picture {
    pub path: String,
    /// The share of the text width it takes, in percent, at most 100.
    pub width: f64,
}

/// The XHTML of the body in reading order.
pub struct Content {
    pub files: Vec<File>,
    /// The body's order of files and the place of the contents page.
    pub order: Vec<Item>,
    /// The file each anchor is in.
    pub anchors: Vec<Option<usize>>,
}

pub struct File {
    /// `frontmatter`, `bodymatter`, or `backmatter`.
    pub matter: &'static str,
    /// The title of its first heading.
    pub title: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    File(usize),
    Contents,
}

/// The name of body file `index` in the EPUB.
pub fn file_name(index: usize) -> String {
    format!("text-{:03}.xhtml", index + 1)
}

/// The body as files. Rendering runs twice: the first pass finds the file of every anchor, which the
/// second links to.
pub fn write(cx: &Context) -> Content {
    let pieces = split(cx);
    let (_, placed) = render(cx, &pieces, &vec![None; cx.structure.anchors.len()]);
    let (files, anchors) = render(cx, &pieces, &placed);
    let mut index = 0;
    let order = pieces
        .iter()
        .map(|piece| match piece {
            Piece::Section(_) => {
                index += 1;
                Item::File(index - 1)
            }
            Piece::Contents => Item::Contents,
        })
        .collect();
    Content { files, order, anchors }
}

enum Piece<'d> {
    Section(Section<'d>),
    Contents,
}

struct Section<'d> {
    matter: Option<Matter>,
    blocks: Vec<&'d Block>,
}

/// Splits the top-level blocks into files.
fn split<'d>(cx: &Context<'d>) -> Vec<Piece<'d>> {
    let mut pieces = Vec::new();
    let mut current = Section {
        matter: None,
        blocks: Vec::new(),
    };
    let flush = |pieces: &mut Vec<Piece<'d>>, current: &mut Section<'d>| {
        if !current.blocks.is_empty() {
            let blocks = std::mem::take(&mut current.blocks);
            pieces.push(Piece::Section(Section {
                matter: current.matter,
                blocks,
            }));
        }
    };
    for block in &cx.document.blocks {
        match block {
            Block::Matter { matter, .. } => {
                flush(&mut pieces, &mut current);
                current.matter = Some(*matter);
            }
            Block::PageBreak { .. } => flush(&mut pieces, &mut current),
            // Readers cannot set text at the bottom of a screen, so a bottom group only ends its file.
            Block::Bottom { .. } => {
                current.blocks.push(block);
                flush(&mut pieces, &mut current);
            }
            Block::Contents { .. } => {
                flush(&mut pieces, &mut current);
                pieces.push(Piece::Contents);
            }
            // Without citations there is no bibliography.
            Block::Bibliography { .. } if cx.cited.section.is_none() => {}
            Block::Heading { level: 1, .. } | Block::Bibliography { .. } => {
                flush(&mut pieces, &mut current);
                current.blocks.push(block);
            }
            block => current.blocks.push(block),
        }
    }
    flush(&mut pieces, &mut current);
    // A book needs a document to read, even an empty one.
    if !pieces.iter().any(|piece| matches!(piece, Piece::Section(_))) {
        pieces.insert(0, Piece::Section(current));
    }
    pieces
}

/// Renders the sections with links to the anchors in `placed` files, and returns the files and where
/// each anchor is.
fn render(cx: &Context, pieces: &[Piece], placed: &[Option<usize>]) -> (Vec<File>, Vec<Option<usize>>) {
    let parts = cx
        .document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Matter { .. }));
    let mut writer = Writer {
        cx,
        placed,
        defined: vec![None; placed.len()],
        file: 0,
        out: String::new(),
        title: None,
        headings: 0,
        figures: 0,
        tables: 0,
        in_notes: false,
        opening: false,
        notes: Vec::new(),
    };
    let mut files = Vec::new();
    for piece in pieces {
        let Piece::Section(section) = piece else {
            continue;
        };
        let matter = match (parts, section.matter) {
            (false, _) | (true, Some(Matter::Main)) => "bodymatter",
            (true, None | Some(Matter::Front)) => "frontmatter",
            (true, Some(Matter::Back)) => "backmatter",
        };
        let wrapper = match section.blocks.first() {
            Some(Block::Bibliography { .. }) => Some("bibliography"),
            Some(Block::Heading { level: 1, .. }) if matter == "bodymatter" => Some("chapter"),
            _ => None,
        };
        writer.opening = false;
        if let Some(kind) = wrapper {
            write!(writer.out, "<section epub:type=\"{kind}\">").expect("writing to a string succeeds");
        }
        for block in &section.blocks {
            writer.block(block);
        }
        if wrapper.is_some() {
            writer.out.push_str("</section>");
        }
        writer.footnotes();
        files.push(File {
            matter,
            title: writer.title.take(),
            body: std::mem::take(&mut writer.out),
        });
        writer.file += 1;
    }
    (files, writer.defined)
}

struct Writer<'a> {
    cx: &'a Context<'a>,
    /// The file of each anchor, from an earlier pass.
    placed: &'a [Option<usize>],
    /// The file of each anchor this pass has written.
    defined: Vec<Option<usize>>,
    /// The index of the file being written.
    file: usize,
    out: String,
    title: Option<String>,
    /// The headings, figures, and tables written so far, which index their entries in the structure.
    headings: usize,
    figures: usize,
    tables: usize,
    in_notes: bool,
    /// Whether a chapter heading waits for its first paragraph.
    opening: bool,
    /// The footnotes the current file references, in order.
    notes: Vec<usize>,
}

impl Writer<'_> {
    fn push(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.block(block);
        }
    }

    fn block(&mut self, block: &Block) {
        let opening = std::mem::take(&mut self.opening);
        match block {
            Block::Paragraph { content, class, .. } => match class {
                // A styled paragraph, such as an epigraph, leaves the opening to the next one.
                Some(class) if class.name != NO_DROP_CAP && opening => {
                    self.opening = true;
                    self.paragraph(content, Some(class), "");
                }
                None if opening => self.opening_paragraph(content),
                class => self.paragraph(content, class.as_ref().filter(|class| class.name != NO_DROP_CAP), ""),
            },
            Block::Heading {
                level, content, class, ..
            } => self.heading(*level, content, class.as_ref()),
            Block::List {
                start, items, class, ..
            } => {
                let tag = if start.is_some() { "ol" } else { "ul" };
                self.push(&format!("<{tag}"));
                self.class_attribute(class.as_ref().map(|class| css::class(&class.name)).as_deref());
                if let Some(start) = start.filter(|&start| start != 1) {
                    self.push(&format!(" start=\"{start}\""));
                }
                self.push(">");
                for item in items {
                    self.push("<li>");
                    self.contained(item);
                    self.push("</li>");
                }
                self.push(&format!("</{tag}>"));
            }
            Block::Quote { blocks, .. } => {
                self.push("<blockquote>");
                self.blocks(blocks);
                self.push("</blockquote>");
            }
            Block::Code { lines, .. } => {
                let code: Vec<String> = lines
                    .iter()
                    .map(|line| xml::text(&line.replace('\t', "    ")))
                    .collect();
                self.push(&format!("<pre><code>{}</code></pre>", code.join("\n")));
            }
            Block::Image { image, caption, .. } => self.image(*image, caption),
            Block::Table {
                columns,
                header,
                rows,
                caption,
                ..
            } => self.table(columns, header, rows, caption),
            Block::Keep { blocks, .. } | Block::Bottom { blocks, .. } => {
                self.opening = opening;
                self.blocks(blocks);
            }
            Block::Columns { blocks, .. } | Block::FullWidth { blocks, .. } => self.blocks(blocks),
            Block::PageBreak { .. } => self.push("<div class=\"page-break\"></div>"),
            Block::SceneBreak { .. } => self.scene_break(),
            Block::Bibliography { .. } => self.bibliography(),
            Block::Matter { .. } | Block::Contents { .. } => {}
        }
    }

    /// The content of a list item or table cell: a single plain paragraph as text, else blocks.
    fn contained(&mut self, blocks: &[Block]) {
        match blocks {
            [
                Block::Paragraph {
                    content, class: None, ..
                },
            ] => self.inlines(content),
            blocks => self.blocks(blocks),
        }
    }

    fn class_attribute(&mut self, classes: Option<&str>) {
        if let Some(classes) = classes.filter(|classes| !classes.is_empty()) {
            self.push(&format!(" class=\"{}\"", xml::attribute(classes)));
        }
    }

    /// A paragraph with `prefix` markup before its text.
    fn paragraph(&mut self, content: &[Inline], class: Option<&Class>, prefix: &str) {
        self.push("<p");
        self.class_attribute(class.map(|class| css::class(&class.name)).as_deref());
        self.push(">");
        self.push(prefix);
        self.inlines(content);
        self.push("</p>");
    }

    /// The first paragraph of a chapter, with its lead-in in small capitals and a drop capital.
    fn opening_paragraph(&mut self, content: &[Inline]) {
        let chapters = &self.cx.config.chapters;
        let content = lead_in(content, chapters.lead_in);
        self.push(if chapters.drop_cap > 0 {
            "<p class=\"opening\">"
        } else {
            "<p>"
        });
        self.inlines(&content);
        self.push("</p>");
    }

    fn heading(&mut self, level: u8, content: &[Inline], class: Option<&Class>) {
        let custom = class.map(|class| css::class(&class.name));
        if self.in_notes {
            self.push(&format!("<h{level}"));
            self.class_attribute(custom.as_deref());
            self.push(">");
            self.inlines(content);
            self.push(&format!("</h{level}>"));
            return;
        }
        let cx = self.cx;
        let heading = &cx.structure.headings[self.headings];
        self.headings += 1;
        let chapter = level == 1;
        let classes: Vec<&str> = chapter
            .then_some("chapter")
            .into_iter()
            .chain(custom.as_deref())
            .collect();
        let id = self.define(heading.anchor);
        self.push(&format!("<h{level} id=\"{id}\""));
        self.class_attribute(Some(&classes.join(" ")));
        self.push(">");
        let chapters = &cx.config.chapters;
        match &heading.number {
            Some(number) if chapter && chapters.number_position == NumberPosition::Above => {
                let text = if chapters.number_label {
                    format!("{} {number}", cx.config.labels.chapter)
                } else {
                    number.clone()
                };
                self.push(&format!(
                    "<span class=\"chapter-number {}\">{}</span> ",
                    css::class(&chapters.number_style),
                    xml::text(&text)
                ));
            }
            Some(number) => self.push(&format!("{} ", xml::text(number))),
            None => {}
        }
        self.inlines(content);
        self.push(&format!("</h{level}>"));
        self.title.get_or_insert_with(|| heading.title());
        if chapter {
            if let Some(ornament) = &chapters.ornament {
                let path = &cx.theme_images[&ornament.image];
                self.push(&format!(
                    "<div class=\"ornament\"><img src=\"{}\" alt=\"\"/></div>",
                    xml::attribute(path)
                ));
            }
            self.opening = chapters.drop_cap > 0 || chapters.lead_in > 0;
        }
    }

    /// An image with its numbered caption below it, or alone without a caption.
    fn image(&mut self, image: usize, caption: &[Inline]) {
        let cx = self.cx;
        let picture = &cx.images[image];
        // Footnotes have no numbered figures or tables.
        let figure = (!self.in_notes).then(|| {
            self.figures += 1;
            cx.structure.figures[self.figures - 1]
        });
        let img = format!(
            "<img src=\"{}\" alt=\"{}\" style=\"width: {}%\"/>",
            xml::attribute(&picture.path),
            xml::attribute(plain(caption).trim()),
            (picture.width * 10.0).round() / 10.0
        );
        match figure.flatten() {
            Some(Numbered { number, anchor }) => {
                let id = self.define(anchor);
                self.push(&format!("<figure class=\"figure\" id=\"{id}\">{img}<figcaption>"));
                self.caption(&cx.config.labels.figure, number, caption);
                self.push("</figcaption></figure>");
            }
            None => self.push(&format!("<div class=\"image\">{img}</div>")),
        }
    }

    /// A caption's label and number, then its text.
    fn caption(&mut self, label: &str, number: usize, caption: &[Inline]) {
        let label = format!("{label} {number}{}", self.cx.config.caption_separator);
        self.push(&xml::text(&label));
        self.inlines(caption);
    }

    fn table(&mut self, columns: &[Column], header: &Row, rows: &[Row], caption: &[Inline]) {
        let cx = self.cx;
        let table = (!self.in_notes).then(|| {
            self.tables += 1;
            cx.structure.tables[self.tables - 1]
        });
        match table.flatten() {
            Some(Numbered { number, anchor }) => {
                let id = self.define(anchor);
                self.push(&format!("<table id=\"{id}\"><caption>"));
                self.caption(&cx.config.labels.table, number, caption);
                self.push("</caption>");
            }
            None => self.push("<table>"),
        }
        self.push("<thead>");
        self.row(header, "th", columns);
        self.push("</thead><tbody>");
        for row in rows {
            self.row(row, "td", columns);
        }
        self.push("</tbody></table>");
    }

    /// A row whose cells take the alignment of their first column unless they have a style of their own.
    fn row(&mut self, row: &Row, tag: &str, columns: &[Column]) {
        let row_class = row.class.as_ref().map(|class| css::class(&class.name));
        self.push("<tr");
        self.class_attribute(row_class.as_deref());
        self.push(">");
        let mut column = 0;
        for cell in &row.cells {
            let Cell {
                span,
                class: own,
                blocks,
                ..
            } = cell;
            let align = columns
                .get(column)
                .and_then(|column| column.align)
                .filter(|_| own.is_none())
                .map(|align| match align {
                    ColumnAlign::Left => "align-left",
                    ColumnAlign::Center => "align-center",
                    ColumnAlign::Right => "align-right",
                });
            // A cell's own style replaces its row's.
            let style = own
                .as_ref()
                .map(|class| css::class(&class.name))
                .or_else(|| row_class.clone());
            let classes: Vec<&str> = style.as_deref().into_iter().chain(align).collect();
            self.push(&format!("<{tag}"));
            self.class_attribute(Some(&classes.join(" ")));
            if *span > 1 {
                self.push(&format!(" colspan=\"{span}\""));
            }
            self.push(">");
            self.contained(blocks);
            self.push(&format!("</{tag}>"));
            column += span;
        }
        self.push("</tr>");
    }

    fn scene_break(&mut self) {
        let cx = self.cx;
        let mark = match &cx.config.scene_break.mark {
            SceneMark::Text(text) => xml::text(text),
            SceneMark::Image { image, .. } => {
                format!("<img src=\"{}\" alt=\"\"/>", xml::attribute(&cx.theme_images[image]))
            }
        };
        self.push(&format!("<div class=\"scene-break\" role=\"separator\">{mark}</div>"));
    }

    fn bibliography(&mut self) {
        let cx = self.cx;
        let Some(section) = &cx.cited.section else {
            return;
        };
        let heading = &cx.structure.headings[self.headings];
        self.headings += 1;
        let id = self.define(heading.anchor);
        self.push(&format!("<h1 id=\"{id}\">{}</h1>", xml::text(&heading.text)));
        self.title.get_or_insert_with(|| heading.text.clone());
        for (index, reference) in section.references.iter().enumerate() {
            let id = self.define(cx.structure.entries[index]);
            self.push(&format!("<p class=\"entry\" id=\"{id}\" epub:type=\"biblioentry\">"));
            if let Some(label) = &reference.label {
                self.push(&format!("{} ", xml::text(label)));
            }
            self.inlines(&reference.content);
            self.push("</p>");
        }
    }

    /// The notes referenced in the current file, each starting with its number linked back to its reference.
    fn footnotes(&mut self) {
        if self.notes.is_empty() {
            return;
        }
        self.in_notes = true;
        self.push("<section class=\"footnotes\" epub:type=\"footnotes\">");
        // A note may reference further notes, which follow it.
        let mut next = 0;
        while let Some(&note) = self.notes.get(next) {
            next += 1;
            let number = note + 1;
            self.push(&format!(
                "<aside class=\"footnote\" epub:type=\"footnote\" id=\"fn-{number}\">"
            ));
            let marker = format!("<a class=\"note-number\" href=\"#fnref-{number}\">{number}</a> ");
            let blocks = &self.cx.document.footnotes[note].blocks;
            match blocks.split_first() {
                Some((Block::Paragraph { content, class, .. }, rest)) => {
                    self.paragraph(content, class.as_ref(), &marker);
                    self.blocks(rest);
                }
                _ => {
                    self.push(&format!("<p>{}</p>", marker.trim_end()));
                    self.blocks(blocks);
                }
            }
            self.push("</aside>");
        }
        self.push("</section>");
        self.notes.clear();
        self.in_notes = false;
    }

    /// Records that `anchor` is in the current file and returns its id.
    fn define(&mut self, anchor: usize) -> String {
        self.defined[anchor] = Some(self.file);
        format!("a{anchor}")
    }

    /// A link to `anchor`, in another file if it is there.
    fn href(&self, anchor: usize) -> String {
        match self.placed[anchor] {
            Some(file) if file != self.file => format!("{}#a{anchor}", file_name(file)),
            _ => format!("#a{anchor}"),
        }
    }

    /// Inline content with its formatting, links, and note references. Cross-references, citations, and
    /// placeholders show their text.
    fn inlines(&mut self, content: &[Inline]) {
        let cx = self.cx;
        let mut runs: Vec<Run> = Vec::with_capacity(content.len());
        let text = |runs: &mut Vec<Run>, text: &str, style: InlineStyle| match runs.last_mut() {
            Some(Run::Text(last, last_style)) if *last_style == style => last.push_str(text),
            _ => runs.push(Run::Text(text.to_owned(), style)),
        };
        for inline in content {
            match inline {
                Inline::Text { text: piece, style } => text(&mut runs, piece, style.clone()),
                Inline::LineBreak => runs.push(Run::LineBreak),
                Inline::FootnoteRef(note) => runs.push(Run::Note(*note)),
                Inline::Ref(reference) => {
                    let (anchor, shown) = cx.structure.reference(&reference.label);
                    let style = InlineStyle {
                        link: Some(Link::Anchor(anchor)),
                        ..reference.style.clone()
                    };
                    text(&mut runs, shown, style);
                }
                Inline::Citation { index, style } => {
                    for part in &cx.cited.citations[*index] {
                        let style = InlineStyle {
                            link: part.target.map(|target| Link::Anchor(cx.structure.entries[target])),
                            ..style.clone()
                        };
                        text(&mut runs, &part.text, style);
                    }
                }
                Inline::Field { placeholder, style, .. } => {
                    let value = cx.fields.text(placeholder).expect("fields are checked before writing");
                    text(&mut runs, &value, style.clone());
                }
            }
        }

        let mut open: Option<Link> = None;
        for run in runs {
            match run {
                Run::Text(text, style) => {
                    if style.link != open {
                        self.close_link(&mut open);
                        if let Some(link) = &style.link {
                            let href = match link {
                                Link::Url(url) => url.clone(),
                                Link::Anchor(anchor) => self.href(*anchor),
                            };
                            self.push(&format!("<a href=\"{}\">", xml::attribute(&href)));
                        }
                        open = style.link.clone();
                    }
                    self.styled(&text, &style);
                }
                Run::LineBreak => self.push("<br/>"),
                Run::Note(note) => {
                    self.close_link(&mut open);
                    self.note_reference(note);
                }
            }
        }
        self.close_link(&mut open);
    }

    fn close_link(&mut self, open: &mut Option<Link>) {
        if open.take().is_some() {
            self.push("</a>");
        }
    }

    fn styled(&mut self, text: &str, style: &InlineStyle) {
        let tags = [
            (style.strong, "<strong>", "</strong>"),
            (style.emphasis, "<em>", "</em>"),
            (style.code, "<code>", "</code>"),
            (style.small_caps, "<span class=\"lead-in\">", "</span>"),
        ];
        for (_, start, _) in tags.iter().filter(|(on, ..)| *on) {
            self.push(start);
        }
        self.push(&xml::text(text));
        for (_, _, end) in tags.iter().rev().filter(|(on, ..)| *on) {
            self.push(end);
        }
    }

    /// A note reference, which places the note in the current file. The parser allows one per note.
    fn note_reference(&mut self, note: usize) {
        let number = note + 1;
        self.notes.push(note);
        self.push(&format!(
            "<a class=\"noteref\" epub:type=\"noteref\" id=\"fnref-{number}\" href=\"#fn-{number}\">{number}</a>"
        ));
    }
}

/// Inline content after cross-references, citations, and placeholders became text.
enum Run {
    Text(String, InlineStyle),
    LineBreak,
    Note(usize),
}
