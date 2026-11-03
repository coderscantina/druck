//! PDF output: embeds and subsets fonts, writes text as selectable glyph runs, adds links to URLs and
//! anchors, and writes the heading outline as bookmarks and page numbers as page labels. For print, it adds
//! a bleed and crop marks around each page.
//!
//! Every PDF is tagged: the [elements](crate::tags) layout marked become the structure tree in document
//! order, mapped to standard structure types by [`structure_type`], and everything else is an artifact.
//! With `document.conformance`, krilla checks the file against PDF/UA-1 or PDF/A-2 and declares it.
//!
//! krilla uses the same coordinates as [`Page`]: points from the top-left corner, y growing down.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::color::separation::{self, SeparationColorant, SeparationSpace};
use krilla::configure::{Accessibility, Archival, ConfigurationBuilder, ValidationError, Validator};
use krilla::destination::XyzDestination;
use krilla::error::KrillaError;
use krilla::geom::{PathBuilder, Point, Rect as PdfRect, Size, Transform};
use krilla::metadata::{DateTime, Metadata as PdfMetadata};
use krilla::outline::{Outline, OutlineNode};
use krilla::page::{NumberingStyle, PageLabel, PageSettings};
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::tagging::{
    Artifact, ArtifactType, ContentTag, Identifier, ListNumbering, Node, SpanTag, TableHeaderScope, Tag, TagGroup,
    TagKind, TagTree,
};
use krilla::text::{GlyphId, KrillaGlyph};
use krilla::{Document, SerializeSettings};
use krilla_svg::{SurfaceExt, SvgSettings};

use crate::config::front_matter::Metadata;
use crate::config::source::Source;
use crate::config::theme::{Conformance, Lang, NumberFormat, Standard};
use crate::config::values::{Color, Pt};
use crate::date::Date;
use crate::diagnostic::Diagnostic;
use crate::document::Link;
use crate::image::{Image, Pixels};
use crate::page::{Bookmark, Item, Output, PageNumber, Rect};
use crate::tags::{Id, Kind, Mark, Node as TreeNode, Tree};
use crate::text::{FaceId, Fonts, ShapedRun};

/// The application, as named in the creator and producer fields of every PDF.
const CREATOR: &str = concat!("Druck ", env!("CARGO_PKG_VERSION"));
const PRODUCER: &str = concat!("Coder's Cantina Druck ", env!("CARGO_PKG_VERSION"));

/// The least distance between the trim edge and a crop mark, as InDesign's default.
const MARK_OFFSET: f32 = 6.0;
const MARK_LENGTH: f32 = 15.0;
const MARK_THICKNESS: f32 = 0.25;

/// What a print shop needs around each page, from the command line. A zero bleed without marks adds nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Print {
    /// How far the bleed box reaches past the trim box on each side.
    pub bleed: Pt,
    /// Whether crop marks show the trim box at the corners, outside the bleed.
    pub crop_marks: bool,
}

impl Print {
    fn bleed(self) -> f32 {
        self.bleed.0 as f32
    }

    /// The distance from the trim box to a crop mark.
    fn mark_offset(self) -> f32 {
        self.bleed().max(MARK_OFFSET)
    }

    /// The space around the trim box on each side of the media box.
    fn margin(self) -> f32 {
        if self.crop_marks {
            self.mark_offset() + MARK_LENGTH
        } else {
            self.bleed()
        }
    }
}

/// What a PDF is written for besides its pages.
pub struct Settings<'a> {
    pub lang: Lang,
    pub print: Print,
    /// The standards the file is checked against and declares.
    pub conformance: &'a Conformance,
    /// The day of the run: the creation date of a PDF/A file whose `date` is not `YYYY-MM-DD`, since PDF/A
    /// needs one.
    pub today: Date,
    /// The document, which diagnostics about its content name.
    pub source: &'a Source,
}

/// Writes the pages of `output` as a tagged PDF document, with links, anchors, and bookmarks. The document
/// info holds the title, authors, subject, language, and application, and a creation date only from an
/// ISO `date`, so the same input gives the same bytes. With a bleed or crop marks, each page is the trim
/// box inside a larger media box. A file that breaks a standard it must conform to is not written; each
/// problem is a diagnostic, at the content it concerns where that has a place in the document.
pub fn write(
    output: &Output,
    fonts: &Fonts,
    metadata: &Metadata,
    settings: &Settings,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let (document, faces) =
        compose(output, fonts, metadata, settings).map_err(|message| vec![Diagnostic::new(None, message)])?;
    document
        .finish()
        .map_err(|error| problems(error, output, fonts, &faces, settings))
}

/// The document ready to be finished, with the faces its text uses.
fn compose(
    output: &Output,
    fonts: &Fonts,
    metadata: &Metadata,
    settings: &Settings,
) -> Result<(Document, BTreeSet<FaceId>), String> {
    let print = settings.print;
    let margin = print.margin();
    let destination = |anchor: usize| {
        let position = output
            .anchors
            .get(anchor)
            .filter(|position| position.page < output.pages.len())
            .ok_or_else(|| format!("link to anchor {anchor}, which is on no page"))?;
        Ok::<_, String>(XyzDestination::new(
            position.page,
            Point::from_xy(position.x.0 as f32 + margin, position.y.0 as f32 + margin),
        ))
    };
    let standards = settings.conformance.standards();
    let mut configuration = ConfigurationBuilder::new();
    for standard in standards {
        configuration = configuration.set_validator(match standard {
            Standard::Ua1 => Validator::Ua(Accessibility::UA1),
            Standard::A2b => Validator::A(Archival::A2_B),
            Standard::A2u => Validator::A(Archival::A2_U),
            Standard::A2a => Validator::A(Archival::A2_A),
        });
    }
    let configuration = configuration
        .finish()
        .map_err(|error| format!("cannot combine these PDF standards: {error:?}"))?;
    let mut document = Document::new_with(SerializeSettings {
        configuration,
        enable_tagging: true,
        ..SerializeSettings::default()
    });
    let archival = standards.iter().any(|standard| *standard != Standard::Ua1);
    let date = archival.then_some(settings.today);
    document.set_metadata(document_info(
        metadata,
        output.heading_title.as_deref(),
        settings.lang,
        date,
    ));
    let mut contents = Contents::new(&output.tags);
    let mut faces = BTreeSet::new();
    let mut spaces = HashMap::new();
    for (index, page) in output.pages.iter().enumerate() {
        let (width, height) = (page.width.0 as f32, page.height.0 as f32);
        let invalid = || format!("invalid page size {} x {} pt", page.width.0, page.height.0);
        let mut page_settings =
            PageSettings::from_wh(width + 2.0 * margin, height + 2.0 * margin).ok_or_else(invalid)?;
        if margin > 0.0 {
            let bleed = print.bleed();
            let trim = PdfRect::from_xywh(margin, margin, width, height).ok_or_else(invalid)?;
            let bleed_box = PdfRect::from_xywh(
                margin - bleed,
                margin - bleed,
                width + 2.0 * bleed,
                height + 2.0 * bleed,
            )
            .ok_or_else(invalid)?;
            page_settings = page_settings.with_trim_box(Some(trim)).with_bleed_box(Some(bleed_box));
        }
        if let Some(number) = output.page_numbers.get(index) {
            page_settings = page_settings.with_page_label(page_label(*number));
        }
        let mut pdf_page = document.start_page_with(page_settings);
        let mut surface = pdf_page.surface();
        if print.crop_marks {
            surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(ArtifactType::Page)));
            draw_crop_marks(&mut surface, print, width, height)?;
            surface.end_tagged();
        }
        if margin > 0.0 {
            surface.push_transform(&Transform::from_translate(margin, margin));
        }
        let mut links = Vec::new();
        // The elements begun and not ended, `None` for decoration inside one, and whether the next content
        // joins the content of the mark.
        let mut marks: Vec<(Option<Id>, bool)> = Vec::new();
        let mut open = None;
        for item in &page.items {
            let element = marks.last().and_then(|mark| mark.0);
            match item {
                Item::Begin(mark) => {
                    let begun = match mark {
                        Mark::Element(id) => Some(*id),
                        Mark::Joined => element,
                        Mark::Link(_) | Mark::Note(_) | Mark::Artifact => None,
                    };
                    if let (Some(parent), Mark::Element(child)) = (element, mark) {
                        contents.begin(parent, *child);
                    }
                    marks.push((begun, *mark == Mark::Joined));
                }
                Item::End => {
                    if marks.pop().is_some_and(|(_, joined)| joined) {
                        contents.last = None;
                    }
                }
                Item::Link { rect, link } => links.push((rect, link, element)),
                Item::Anchor { .. } => {}
                Item::Text { x, y, run, color } => {
                    contents.switch(&mut surface, &mut open, element.map(|id| (id, true)));
                    contents.read(element, &run.text);
                    faces.insert(run.face);
                    let space = match contents.breaks_word(element, x.0, y.0, run) {
                        true => *spaces
                            .entry(run.face)
                            .or_insert_with(|| space_glyph(fonts, run, settings.lang)),
                        false => None,
                    };
                    let start = Point::from_xy(x.0 as f32, y.0 as f32);
                    draw_run(&mut surface, fonts, run, start, *color, space)
                }
                Item::TurnedText {
                    x,
                    y,
                    angle,
                    run,
                    color,
                } => {
                    contents.switch(&mut surface, &mut open, element.map(|id| (id, true)));
                    faces.insert(run.face);
                    let start = Point::from_xy(x.0 as f32, y.0 as f32);
                    // y grows downward, so a counterclockwise turn is a negative angle.
                    surface.push_transform(&Transform::from_rotate_at(-*angle as f32, start.x, start.y));
                    draw_run(&mut surface, fonts, run, start, *color, None);
                    surface.pop();
                }
                // Rectangles are underlines, code backgrounds, and rules: decoration.
                Item::Rect { rect, color } => {
                    contents.switch(&mut surface, &mut open, None);
                    draw_rect(&mut surface, rect, *color)?
                }
                Item::Image { rect, image } => {
                    contents.switch(&mut surface, &mut open, element.map(|id| (id, false)));
                    draw_image(&mut surface, rect, image)?
                }
            }
        }
        if open.is_some() {
            surface.end_tagged();
        }
        if margin > 0.0 {
            surface.pop();
        }
        surface.finish();
        for (rect, link, element) in links {
            let rect = pdf_rect(&Rect {
                x: Pt(rect.x.0 + f64::from(margin)),
                y: Pt(rect.y.0 + f64::from(margin)),
                ..*rect
            })?;
            let (target, fallback) = match link {
                Link::Url(url) => (Target::Action(LinkAction::new(url.clone()).into()), url.as_str()),
                Link::Anchor(anchor) => (Target::Destination(destination(*anchor)?.into()), "Link"),
            };
            let link_element = element.filter(|id| output.tags.nodes[id.index()].kind == Kind::Link);
            // The link text describes the annotation for assistive technology.
            let alt = link_element
                .map(|id| contents.text(id))
                .filter(|text| !text.is_empty())
                .unwrap_or(fallback)
                .to_owned();
            let annotation = Annotation::new_link(LinkAnnotation::new(rect, target), Some(alt));
            match link_element {
                Some(id) => {
                    let identifier = pdf_page.add_tagged_annotation(annotation);
                    contents.add(id, identifier);
                }
                None => pdf_page.add_annotation(annotation),
            }
        }
        pdf_page.finish();
    }
    if !output.outline.is_empty() {
        document.set_outline(outline(&output.outline, destination)?);
    }
    let lang = language(settings.lang).to_owned();
    document.set_tag_tree(contents.tree().with_lang(Some(lang)));
    Ok((document, faces))
}

/// The content of each element in the order it is drawn: the marked content, annotations, and elements
/// begun first inside it, such as the links of a paragraph.
struct Contents<'a> {
    tags: &'a Tree,
    entries: Vec<Vec<Entry>>,
    begun: Vec<bool>,
    /// The text of each element, which describes the annotations of links.
    texts: Vec<String>,
    /// The last text of an element drawn: its block, where it ends, its baseline and size, and whether it
    /// ends in a hyphen.
    last: Option<(Id, f64, f64, f64, bool)>,
}

enum Entry {
    Content(Identifier),
    Element(Id),
}

impl<'a> Contents<'a> {
    fn new(tags: &'a Tree) -> Self {
        let count = tags.nodes.len();
        Self {
            tags,
            entries: (0..count).map(|_| Vec::new()).collect(),
            begun: vec![false; count],
            texts: vec![String::new(); count],
            last: None,
        }
    }

    /// Whether a word break comes before `run` of `element` at (`x`, `y`). Layout sets words apart
    /// without space characters, so text read from the structure would run words together. A break is a
    /// gap after the block's last run on the same line, or a new line after one that does not end in a
    /// hyphen.
    fn breaks_word(&mut self, element: Option<Id>, x: f64, y: f64, run: &ShapedRun) -> bool {
        let Some(block) = element.map(|id| self.block(id)) else {
            return false;
        };
        let size = run.size.0;
        let next = (block, x + run.width.0, y, size, run.text.ends_with('-'));
        let Some((_, end, baseline, last_size, hyphen)) = self.last.replace(next).filter(|last| last.0 == block) else {
            return false;
        };
        let same_line = (y - baseline).abs() < 0.5 * size.max(last_size) && x > end - 0.5;
        if same_line { x - end > 0.05 * size } else { !hyphen }
    }

    /// The element that `id` is inline in, or `id` itself.
    fn block(&self, mut id: Id) -> Id {
        while let TreeNode {
            kind: Kind::Link | Kind::Reference,
            parent: Some(parent),
        } = &self.tags.nodes[id.index()]
        {
            id = *parent;
        }
        id
    }

    /// Notes that `child` is begun inside `parent`, which orders it among the parent's content.
    fn begin(&mut self, parent: Id, child: Id) {
        if self.tags.nodes[child.index()].parent == Some(parent) && !self.begun[child.index()] {
            self.begun[child.index()] = true;
            self.entries[parent.index()].push(Entry::Element(child));
        }
    }

    fn add(&mut self, id: Id, identifier: Identifier) {
        self.entries[id.index()].push(Entry::Content(identifier));
    }

    fn read(&mut self, element: Option<Id>, text: &str) {
        if let Some(id) = element {
            let read = &mut self.texts[id.index()];
            if !read.is_empty() {
                read.push(' ');
            }
            read.push_str(text);
        }
    }

    fn text(&self, id: Id) -> &str {
        &self.texts[id.index()]
    }

    /// Makes `next` the marked content open on the surface: an element's text or other content, or
    /// decoration for `None`. Consecutive items of the same element share one marked content sequence.
    fn switch(&mut self, surface: &mut Surface, open: &mut Option<Option<(Id, bool)>>, next: Option<(Id, bool)>) {
        if *open == Some(next) {
            return;
        }
        if open.is_some() {
            surface.end_tagged();
        }
        *open = Some(next);
        match next {
            Some((id, text)) => {
                let tag = if text {
                    ContentTag::Span(SpanTag::empty())
                } else {
                    ContentTag::Other
                };
                let identifier = surface.start_tagged(tag);
                self.add(id, identifier);
            }
            None => {
                surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(ArtifactType::Other)));
            }
        }
    }

    /// The tag tree in document order. An element's content and the elements begun inside it keep the
    /// order they were drawn in. Its other elements follow the element before them in the document, as a
    /// note follows its reference, or come in document order. Elements without content are left out.
    fn tree(mut self) -> TagTree {
        let mut children: Vec<Vec<Id>> = vec![Vec::new(); self.tags.nodes.len()];
        let mut roots = Vec::new();
        for (index, node) in self.tags.nodes.iter().enumerate() {
            match node.parent {
                Some(parent) => children[parent.index()].push(Id::new(index)),
                None => roots.push(Id::new(index)),
            }
        }
        let nodes: Vec<Node> = roots.into_iter().filter_map(|id| self.group(id, &children)).collect();
        TagTree::from(nodes)
    }

    fn group(&mut self, id: Id, children: &[Vec<Id>]) -> Option<Node> {
        let entries = std::mem::take(&mut self.entries[id.index()]);
        let begun = |entry: &Entry| match entry {
            Entry::Element(child) => Some(*child),
            Entry::Content(_) => None,
        };
        let walked: Vec<Id> = entries.iter().filter_map(begun).collect();
        let after = |child: Id| walked.iter().copied().filter(|begun| *begun < child).max();
        let rest: Vec<Id> = children[id.index()]
            .iter()
            .copied()
            .filter(|child| !self.begun[child.index()])
            .collect();
        let mut ordered: Vec<Entry> = rest
            .iter()
            .filter(|child| after(**child).is_none())
            .map(|child| Entry::Element(*child))
            .collect();
        for entry in entries {
            let begun = begun(&entry);
            ordered.push(entry);
            if let Some(begun) = begun {
                let following = rest.iter().filter(|child| after(**child) == Some(begun));
                ordered.extend(following.map(|child| Entry::Element(*child)));
            }
        }
        let nodes: Vec<Node> = ordered
            .into_iter()
            .filter_map(|entry| match entry {
                Entry::Content(identifier) => Some(identifier.into()),
                Entry::Element(child) => self.group(child, children),
            })
            .collect();
        let kind = &self.tags.nodes[id.index()].kind;
        (!nodes.is_empty()).then(|| TagGroup::with_children(structure_type(kind, id), nodes).into())
    }
}

/// The standard structure type of each kind of element. A new kind of block or inline gets its type here.
fn structure_type(kind: &Kind, id: Id) -> TagKind {
    let span = |columns: usize| NonZeroU32::new(u32::try_from(columns).unwrap_or(u32::MAX)).filter(|n| n.get() > 1);
    match kind {
        Kind::Title => Tag::Title.into(),
        Kind::Paragraph => Tag::P.into(),
        Kind::Heading { level, text } => {
            let level = NonZeroU16::new(u16::from(*level)).unwrap_or(NonZeroU16::MIN);
            Tag::Hn(level, Some(text.clone())).into()
        }
        Kind::List { ordered: true } => Tag::L(ListNumbering::Decimal).into(),
        Kind::List { ordered: false } => Tag::L(ListNumbering::Disc).into(),
        Kind::ListItem => Tag::LI.into(),
        Kind::Label => Tag::Lbl.into(),
        Kind::ListBody => Tag::LBody.into(),
        Kind::Quote => Tag::BlockQuote.into(),
        Kind::Code => Tag::Code.into(),
        Kind::Figure { alt, .. } => {
            // The location finds the image again when a standard rejects it.
            let location = NonZeroU64::new(id.index() as u64 + 1);
            Tag::Figure((!alt.is_empty()).then(|| alt.clone()))
                .with_location(location)
                .into()
        }
        Kind::Caption => Tag::Caption.into(),
        Kind::Table => Tag::Table.into(),
        Kind::TableRow => Tag::TR.into(),
        Kind::TableCell {
            header: true,
            span: columns,
        } => Tag::TH(TableHeaderScope::Column).with_col_span(span(*columns)).into(),
        Kind::TableCell {
            header: false,
            span: columns,
        } => Tag::TD.with_col_span(span(*columns)).into(),
        Kind::Note => Tag::Note.into(),
        Kind::Reference => Tag::Reference.into(),
        Kind::Link => Tag::Link.into(),
        Kind::Contents => Tag::TOC.into(),
        Kind::ContentsEntry => Tag::TOCI.into(),
        Kind::BibEntry => Tag::BibEntry.into(),
        Kind::Callout => Tag::Div.into(),
        Kind::Index => Tag::Index.into(),
    }
}

/// Diagnostics for an error krilla returns instead of the document. An image without a description is
/// reported where it is written.
fn problems(
    error: KrillaError,
    output: &Output,
    fonts: &Fonts,
    faces: &BTreeSet<FaceId>,
    settings: &Settings,
) -> Vec<Diagnostic> {
    let KrillaError::Validation(errors) = error else {
        return vec![Diagnostic::new(None, format!("cannot write PDF: {error}"))];
    };
    let font = |font: &krilla::text::Font| {
        let face = faces.iter().find(|face| fonts.pdf_font(**face) == font);
        face.map_or("a font", |face| fonts.file(*face)).to_owned()
    };
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (error, validators) in errors {
        let standards: Vec<&str> = validators.into_iter().map(Validator::as_str).collect();
        let standards = standards.join(" and ");
        let (message, location) = match &error {
            ValidationError::MissingAltText(location) => (
                format!(
                    "{standards} needs a description of this image; write it between the brackets, as in \
                     ![Sales by month](chart.png)"
                ),
                *location,
            ),
            ValidationError::NoDocumentTitle => (
                format!("{standards} needs a title: set title in the front matter or start with a level 1 heading"),
                None,
            ),
            ValidationError::MissingDocumentOutline => (
                format!("{standards} needs bookmarks, so the document needs at least one heading"),
                None,
            ),
            ValidationError::MissingHeadingTitle => (format!("{standards} needs text in every heading"), None),
            ValidationError::RestrictedLicense(face) => (
                format!(
                    "{standards} needs fonts whose license allows embedding, and {} restricts it (OS/2 fsType)",
                    font(face)
                ),
                None,
            ),
            ValidationError::NoCodepointMapping(face, ..)
            | ValidationError::InvalidCodepointMapping(face, ..)
            | ValidationError::UnicodePrivateArea(face, ..) => (
                format!(
                    "{standards} needs every glyph to stand for text, and a glyph of {} has none or a private \
                     use character",
                    font(face)
                ),
                None,
            ),
            other => (format!("{standards} does not allow this document: {other:?}"), None),
        };
        let at = location
            .and_then(|location| output.tags.nodes.get(usize::try_from(location.get()).ok()? - 1))
            .and_then(|node| match &node.kind {
                Kind::Figure { at, .. } => Some(*at),
                _ => None,
            });
        let diagnostic = match at {
            Some(at) => Diagnostic::new(Some(settings.source.clone()), message).at(at.line, at.column),
            None => Diagnostic::new(None, message),
        };
        if !diagnostics.contains(&diagnostic) {
            diagnostics.push(diagnostic);
        }
    }
    diagnostics
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

fn language(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "en",
        Lang::De => "de",
    }
}

/// The front matter title, else `heading_title`. The subject is the abstract, else the subtitle, on one
/// line. The creation date is an ISO `date`, else `fallback`.
fn document_info(metadata: &Metadata, heading_title: Option<&str>, lang: Lang, fallback: Option<Date>) -> PdfMetadata {
    let mut info = PdfMetadata::new()
        .language(language(lang).to_owned())
        .creator(CREATOR.to_owned())
        .producer(PRODUCER.to_owned());
    if let Some(title) = metadata.title.as_deref().or(heading_title) {
        info = info.title(title.to_owned());
    }
    if !metadata.authors.is_empty() {
        info = info.authors(metadata.authors.clone());
    }
    let subject = metadata.abstract_.as_deref().or(metadata.subtitle.as_deref());
    let subject = subject.map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "));
    if let Some(subject) = subject.filter(|subject| !subject.is_empty()) {
        info = info.description(subject);
    }
    let date = metadata.date.as_deref().and_then(|text| Date::iso(text.trim()));
    if let Some(date) = date.or(fallback) {
        info = info.creation_date(DateTime::new(date.year as u16).month(date.month).day(date.day));
    }
    info
}

/// The label of a page: its number, or an empty label for a page without one. A leading zero is a prefix.
fn page_label(number: Option<PageNumber>) -> PageLabel {
    let Some(PageNumber { format, value }) = number else {
        return PageLabel::new(None, None, None);
    };
    let (style, prefix) = match format {
        NumberFormat::Decimal => (NumberingStyle::Arabic, None),
        NumberFormat::DecimalLeadingZero => (NumberingStyle::Arabic, (value < 10).then(|| "0".to_owned())),
        NumberFormat::LowerRoman => (NumberingStyle::LowerRoman, None),
        NumberFormat::UpperRoman => (NumberingStyle::UpperRoman, None),
    };
    let start = u32::try_from(value).ok().and_then(NonZeroU32::new);
    PageLabel::new(Some(style), prefix, start)
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
/// With `space`, a space glyph without advance comes first, so the text reads as a new word.
fn draw_run(surface: &mut Surface, fonts: &Fonts, run: &ShapedRun, start: Point, color: Color, space: Option<u16>) {
    if run.glyphs.is_empty() {
        return;
    }
    let size = run.size.0 as f32;
    let shift = usize::from(space.is_some());
    let space = space.map(|id| KrillaGlyph::new(GlyphId::new(id.into()), 0.0, 0.0, 0.0, 0.0, 0..1, None));
    let glyphs: Vec<KrillaGlyph> = space
        .into_iter()
        .chain(run.glyphs.iter().map(|glyph| {
            KrillaGlyph::new(
                GlyphId::new(glyph.id.into()),
                glyph.x_advance.0 as f32 / size,
                glyph.x_offset.0 as f32 / size,
                glyph.y_offset.0 as f32 / size,
                0.0,
                glyph.text.start + shift..glyph.text.end + shift,
                None,
            )
        }))
        .collect();
    let text = if shift > 0 {
        Cow::Owned(format!(" {}", run.text))
    } else {
        Cow::Borrowed(run.text.as_str())
    };
    surface.set_fill(Some(fill(color)));
    surface.draw_glyphs(start, &glyphs, fonts.pdf_font(run.face).clone(), &text, size, false);
}

/// The space glyph of `run`'s face, if it has one.
fn space_glyph(fonts: &Fonts, run: &ShapedRun, lang: Lang) -> Option<u16> {
    let space = fonts.shape(" ", run.face, run.size, lang);
    space.glyphs.first().map(|glyph| glyph.id).filter(|id| *id != 0)
}

/// Draws two marks at each corner of the `width` by `height` trim box, which starts `print.margin()` from
/// the media box's top left. They are in the registration color, which prints on every plate.
fn draw_crop_marks(surface: &mut Surface, print: Print, width: f32, height: f32) -> Result<(), String> {
    let (margin, offset) = (print.margin(), print.mark_offset());
    let (left, top) = (margin, margin);
    let (right, bottom) = (margin + width, margin + height);
    let half = MARK_THICKNESS / 2.0;
    let mut path = PathBuilder::new();
    for y in [top, bottom] {
        for x in [left - offset - MARK_LENGTH, right + offset] {
            path.push_rect(PdfRect::from_xywh(x, y - half, MARK_LENGTH, MARK_THICKNESS).ok_or("invalid crop mark")?);
        }
    }
    for x in [left, right] {
        for y in [top - offset - MARK_LENGTH, bottom + offset] {
            path.push_rect(PdfRect::from_xywh(x - half, y, MARK_THICKNESS, MARK_LENGTH).ok_or("invalid crop mark")?);
        }
    }
    let path = path.finish().ok_or("no crop marks")?;
    let registration = SeparationSpace::new(SeparationColorant::AllColorants, rgb::Color::black().into());
    surface.set_fill(Some(Fill {
        paint: krilla::color::Color::from(separation::Color::new(255, registration)).into(),
        ..Fill::default()
    }));
    surface.draw_path(&path);
    Ok(())
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

    const PLAIN: Print = Print {
        bleed: Pt(0.0),
        crop_marks: false,
    };

    /// Writes a PDF without a standard to conform to.
    fn write_plain(
        output: &Output,
        fonts: &Fonts,
        metadata: &Metadata,
        lang: Lang,
        print: Print,
    ) -> Result<Vec<u8>, Vec<Diagnostic>> {
        let settings = Settings {
            lang,
            print,
            conformance: &Conformance::default(),
            today: Date::from_unix(0),
            source: &Source::Document("doc.md".into()),
        };
        write(output, fonts, metadata, &settings)
    }

    fn fonts() -> Fonts<'static> {
        let regular = FaceFile {
            resource: Resource {
                origin: Origin::Bundled,
                path: "fonts/LibertinusSerif-Regular.otf".to_owned(),
            },
            index: 0,
            variable: false,
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
        let bytes = write_plain(&output(vec![page]), &fonts, &metadata, Lang::De, PLAIN).expect("pdf");
        assert!(bytes.starts_with(b"%PDF"));
    }

    fn output(pages: Vec<Page>) -> Output {
        Output {
            pages,
            anchors: Vec::new(),
            outline: Vec::new(),
            heading_title: None,
            page_numbers: Vec::new(),
            tags: Tree::default(),
            asides: Vec::new(),
            warnings: Vec::new(),
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
            heading_title: None,
            page_numbers: Vec::new(),
            tags: Tree::default(),
            asides: Vec::new(),
            warnings: Vec::new(),
        };
        let bytes = write_plain(&output, &fonts(), &Metadata::default(), Lang::En, PLAIN).expect("pdf");
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
        assert!(write_plain(&missing, &fonts(), &Metadata::default(), Lang::En, PLAIN).is_err());
    }

    #[test]
    fn writes_document_info_from_metadata_and_the_first_heading() {
        let info = |metadata: &Metadata, heading_title: Option<&str>| {
            let output = Output {
                heading_title: heading_title.map(str::to_owned),
                ..output(Vec::new())
            };
            let bytes = write_plain(&output, &fonts(), metadata, Lang::En, PLAIN).expect("pdf");
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let producer = text.find("/Producer").expect("document info");
            let start = text[..producer].rfind("<<").expect("dictionary start");
            let end = producer + text[producer..].find(">>").expect("dictionary end");
            text[start..end].to_owned()
        };
        let metadata = Metadata {
            subtitle: Some("Sub".to_owned()),
            abstract_: Some("First  line\n\nSecond".to_owned()),
            date: Some("2026-10-07".to_owned()),
            ..Metadata::default()
        };
        let dict = info(&metadata, Some("Heading"));
        let version = env!("CARGO_PKG_VERSION");
        assert!(dict.contains("/Title(Heading)"), "{dict}");
        assert!(dict.contains("/Subject(First line Second)"), "{dict}");
        assert!(dict.contains("/CreationDate(D:20261007"), "{dict}");
        assert!(dict.contains(&format!("/Creator(Druck {version})")), "{dict}");
        assert!(
            dict.contains(&format!("/Producer(Coder's Cantina Druck {version})")),
            "{dict}"
        );

        let metadata = Metadata {
            title: Some("Front".to_owned()),
            subtitle: Some("Sub".to_owned()),
            date: Some("Spring 2026".to_owned()),
            ..Metadata::default()
        };
        let dict = info(&metadata, Some("Heading"));
        assert!(
            dict.contains("/Title(Front)") && dict.contains("/Subject(Sub)"),
            "{dict}"
        );
        assert!(!dict.contains("/CreationDate"), "{dict}");
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
        let first = write_plain(&output, &fonts(), &Metadata::default(), Lang::En, PLAIN).expect("pdf");
        let second = write_plain(&output, &fonts(), &Metadata::default(), Lang::En, PLAIN).expect("pdf");
        assert!(first.starts_with(b"%PDF"));
        assert_eq!(first, second);
    }

    #[test]
    fn a_bleed_and_crop_marks_put_the_trim_box_inside_a_larger_media_box() {
        let link = Rect {
            x: Pt(10.0),
            y: Pt(10.0),
            width: Pt(50.0),
            height: Pt(12.0),
        };
        let page = Page {
            width: Pt(200.0),
            height: Pt(100.0),
            items: vec![Item::Link {
                rect: link,
                link: Link::Url("https://example.com".to_owned()),
            }],
        };
        let print = Print {
            bleed: Pt(10.0),
            crop_marks: true,
        };
        let bytes = write_plain(
            &output(vec![page.clone()]),
            &fonts(),
            &Metadata::default(),
            Lang::En,
            print,
        )
        .expect("pdf");
        let pdf = String::from_utf8_lossy(&bytes);

        // The marks start 10pt out and are 15pt long, so the media box grows by 25pt on each side.
        assert!(pdf.contains("/MediaBox[0 0 250 150]"), "{pdf}");
        assert!(pdf.contains("/TrimBox[25 25 225 125]"), "{pdf}");
        assert!(pdf.contains("/BleedBox[15 15 235 135]"), "{pdf}");
        assert!(pdf.contains("/All"), "registration color");
        assert!(
            pdf.contains("/Rect[35 103 85 115]"),
            "the link moves with the content: {pdf}"
        );

        let bleed_only = Print {
            crop_marks: false,
            ..print
        };
        let bytes = write_plain(
            &output(vec![page.clone()]),
            &fonts(),
            &Metadata::default(),
            Lang::En,
            bleed_only,
        )
        .expect("pdf");
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/MediaBox[0 0 220 120]") && pdf.contains("/TrimBox[10 10 210 110]"));
        assert!(!pdf.contains("/All"));

        let bytes = write_plain(&output(vec![page]), &fonts(), &Metadata::default(), Lang::En, PLAIN).expect("pdf");
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/MediaBox[0 0 200 100]") && !pdf.contains("/TrimBox"));
    }
}
