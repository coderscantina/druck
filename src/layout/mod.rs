//! Layout: places the document's blocks on pages using resolved styles and shaped text.
//!
//! Blocks become a flow of lines, each with the space above it and a rule for ending a page after
//! it. Lines of a column section are set at the column width and grouped into runs. [`paragraph`]
//! chooses line breaks for each whole paragraph; [`pages`] chooses page and column breaks for the
//! whole flow and places footnotes.
//!
//! An image is one line as tall as the image, kept with the lines of its caption. A table row is
//! one line; see [`table`].
//!
//! Before layout, [`structure`] numbers headings, figures, and tables and gives each an anchor. The
//! body may start with a [title block](titles) and a [table of contents](toc), and a separate title
//! page may precede it. A document that cites has a [bibliography] section. Front, main, and back
//! matter start new pages and set how pages are [numbered](numbering). Page numbers shown in the
//! text, in the table of contents and in page references, are only known after layout, so layout
//! repeats until they agree with the pages it produced. Headers and footers are drawn on the final
//! pages; see [`bands`].
//!
//! A level 1 heading opens a chapter as the theme's `chapters` section sets: on a new page, sunk
//! below the top of the text area, with its number on a line above it and an ornament below it. The
//! chapter's first paragraph may start with a drop capital and a lead-in in small capitals.

mod bands;
mod bibliography;
mod classes;
mod fields;
mod notes;
mod numbering;
mod pages;
mod paragraph;
mod structure;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod style_tests;
mod table;
#[cfg(test)]
mod table_tests;
#[cfg(test)]
mod tests;
mod titles;
mod toc;
mod watermark;

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use self::fields::Fields;
use self::notes::NoteStyles;
use self::numbering::Numbering;
use self::structure::Structure;
use crate::citations::Cited;
use crate::config::resolved::{Config, CustomStyle, PageGeometry, SceneMark, Style};
use crate::config::source::{Resource, Source};
use crate::config::theme::{Align, BreakBefore, FontStyle, NumberPosition, Weight, WideBlock};
use crate::config::values::{Color, Pt};
use crate::date::Date;
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Class, Document, Footnote, Inline, InlineStyle, Link, Location, Matter};
use crate::image::Image;
use crate::page::{Bookmark, Item, Output, Page, Position, Rect};
use crate::text::{Fonts, ShapedRun};

pub use self::titles::images as theme_images;

/// The paragraph style that turns off the drop capital and lead-in of a chapter's first paragraph.
const NO_DROP_CAP: &str = "no-drop-cap";
/// Space between a drop capital and the text beside it, in ems of the paragraph.
const DROP_CAP_GAP: f64 = 0.25;

/// The most layout passes spent on page numbers that change the layout they come from.
const PASSES: usize = 5;

/// Lays out `document` on pages with its title, table of contents, bibliography, headers, and footers.
/// `cited` holds its formatted citations, `images` the loaded [`Document::images`], and `theme_images`
/// the loaded theme images by resource. `today` is the build date. Errors name the source location of
/// content that cannot fit.
#[allow(
    clippy::too_many_arguments,
    reason = "the inputs of layout, each prepared separately"
)]
pub fn layout(
    document: &Document,
    cited: &Cited,
    images: &[Image],
    theme_images: &HashMap<Resource, Image>,
    config: &Config,
    fonts: &Fonts,
    source: &Source,
    today: Date,
) -> Result<Output, Vec<Diagnostic>> {
    let fields = Fields::new(document, config, today);
    titles::check(config, &fields, source)?;
    classes::check(document, config, source)?;
    fields.check(document, source)?;
    let structure = Structure::new(document, config, cited.references());
    let notes = NoteStyles::new(config);
    let title_page = if config.document.title_page {
        Some(titles::page(config, &fields, fonts, theme_images, source)?)
    } else {
        None
    };
    // In duplex a blank page follows the title page, so the body starts on an odd page.
    let blank = (title_page.is_some() && config.document.duplex).then(|| blank_page(config));
    let pass = Pass {
        document,
        cited,
        images,
        theme_images,
        config,
        fields: &fields,
        fonts,
        source,
        structure: &structure,
        notes: &notes,
        first: usize::from(title_page.is_some()) + usize::from(blank.is_some()),
    };
    let shown = structure.shown_pages(config);
    let (Laid { body, numbering }, anchors) = settle(&structure.anchors, &shown, source, |assumed| pass.run(assumed))?;
    let unnumbered: Vec<Diagnostic> = structure
        .paged()
        .filter(|&anchor| numbering.page(anchors[anchor].page).is_none())
        .map(|anchor| {
            let (at, what) = &structure.anchors[anchor];
            let message = format!("{what} is on a page without a number, so a page reference to it shows nothing");
            Diagnostic::new(Some(source.clone()), message).at(at.line, at.column)
        })
        .collect();
    if !unnumbered.is_empty() {
        return Err(unnumbered);
    }
    let mut blanks = body.blanks;
    if blank.is_some() {
        blanks.insert(0, 1);
    }
    let mut pages: Vec<Page> = title_page.into_iter().chain(blank).chain(body.pages).collect();
    bands::draw(
        &mut pages, pass.first, &blanks, &structure, &anchors, &numbering, config, &fields, fonts,
    )?;
    let outline = structure
        .headings
        .iter()
        .map(|heading| Bookmark {
            level: heading.level,
            title: heading.title(),
            anchor: heading.anchor,
        })
        .collect();
    let heading_title = document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Heading { level: 1, content, .. } => Some(structure::plain(content).trim().to_owned()),
            _ => None,
        })
        .filter(|title| !title.is_empty());
    Ok(Output {
        pages,
        anchors,
        outline,
        heading_title,
        page_numbers: numbering.labels(),
    })
}

/// Repeats `pass` until the page number of every anchor in `shown` is the one the pass assumed for it,
/// and returns the pages and anchor positions of that pass. A pass also returns the number each page
/// shows, empty for none. The first pass assumes page 1 everywhere. Without shown pages one pass is
/// enough. Page numbers that keep moving are reported at the anchor that moved, after [`PASSES`] passes.
fn settle<T>(
    anchors: &[(Location, String)],
    shown: &[usize],
    source: &Source,
    mut pass: impl FnMut(&[String]) -> Result<(T, Vec<Position>, Vec<String>), Vec<Diagnostic>>,
) -> Result<(T, Vec<Position>), Vec<Diagnostic>> {
    let mut assumed = vec!["1".to_owned(); anchors.len()];
    let mut count = 0;
    loop {
        let (laid, positions, numbers) = pass(&assumed)?;
        count += 1;
        let number = |anchor: usize| &numbers[positions[anchor].page];
        let moved = shown.iter().copied().find(|&anchor| *number(anchor) != assumed[anchor]);
        let Some(anchor) = moved else {
            return Ok((laid, positions));
        };
        if count == PASSES {
            let (at, what) = &anchors[anchor];
            let message = format!(
                "page numbers did not settle after {PASSES} layout passes: {what} moves between page {} and \
                 page {}",
                assumed[anchor],
                number(anchor)
            );
            return Err(vec![
                Diagnostic::new(Some(source.clone()), message).at(at.line, at.column),
            ]);
        }
        assumed = positions
            .iter()
            .map(|position| numbers[position.page].clone())
            .collect();
    }
}

/// What every layout pass shares.
struct Pass<'a> {
    document: &'a Document,
    cited: &'a Cited,
    images: &'a [Image],
    theme_images: &'a HashMap<Resource, Image>,
    config: &'a Config,
    fields: &'a Fields<'a>,
    fonts: &'a Fonts,
    source: &'a Source,
    structure: &'a Structure,
    notes: &'a NoteStyles,
    /// The physical index of the first body page: 1 after a title page.
    first: usize,
}

impl Pass<'_> {
    /// Lays out the body with `assumed` page numbers for the anchors, and returns the body pages with the
    /// numbers of all pages, where each anchor is in the whole document, and the number each page shows.
    fn run(&self, assumed: &[String]) -> Result<(Laid, Vec<Position>, Vec<String>), Vec<Diagnostic>> {
        let (content, matters) = self.content(assumed)?;
        let body = pages::compose(content, self.first, self.config, self.source)?;
        let mut positions = vec![None; self.structure.anchors.len()];
        for (index, page) in body.pages.iter().enumerate() {
            for item in &page.items {
                if let Item::Anchor { id, x, y } = item {
                    positions[*id] = Some(Position {
                        page: self.first + index,
                        x: *x,
                        y: *y,
                    });
                }
            }
        }
        let positions = positions
            .into_iter()
            .map(|position| position.expect("layout places every anchor"))
            .collect();
        // A part starts on the page of its first line, which a forced break put at the top of a page.
        let starts: Vec<(usize, Matter)> = matters
            .into_iter()
            .filter_map(|(line, matter)| {
                let page = body.starts.iter().position(|start| *start == Some(line))?;
                Some((self.first + page, matter))
            })
            .collect();
        let numbering = Numbering::new(self.first + body.pages.len(), &starts, self.config);
        let numbers = numbering.texts();
        Ok((Laid { body, numbering }, positions, numbers))
    }

    /// Sets the body and the footnotes as lines with their break rules, with `assumed` page numbers for
    /// the anchors. Also returns the line each part of the book starts at.
    fn content(&self, assumed: &[String]) -> Result<(pages::Content, Parts), Vec<Diagnostic>> {
        let config = self.config;
        let mut flow = Flow {
            config,
            fields: self.fields,
            fonts: self.fonts,
            source: self.source,
            cited: self.cited,
            images: self.images,
            theme_images: self.theme_images,
            structure: self.structure,
            notes: self.notes,
            assumed,
            headings: 0,
            figures: 0,
            tables: 0,
            in_notes: false,
            table_lines: Vec::new(),
            lines: Vec::new(),
            space: 0.0,
            after_paragraph: false,
            keeps: Vec::new(),
            columns: Vec::new(),
            wide: None,
            rectos: Vec::new(),
            matters: Vec::new(),
            in_keep: false,
            opening: false,
            errors: Vec::new(),
        };
        let frame = Frame::prose(&config.styles.body, &config.page);
        if !config.document.title_page
            && let Some(slots) = titles::block(config, self.fields)
        {
            flow.title(slots, frame);
        }
        // `::: toc` places the contents itself.
        if config.document.toc && !self.structure.contents {
            flow.contents(frame);
            if config.document.duplex {
                flow.lines.last_mut().expect("the contents have a heading").after = Break::Forced;
                flow.rectos.push(flow.lines.len());
            }
        }
        flow.blocks(&self.document.blocks, frame);
        let rectos = std::mem::take(&mut flow.rectos);
        let matters = std::mem::take(&mut flow.matters);
        let body = std::mem::take(&mut flow.lines);
        let keeps = std::mem::take(&mut flow.keeps);
        let columns = std::mem::take(&mut flow.columns);
        let tables = std::mem::take(&mut flow.table_lines);
        flow.in_notes = true;
        let mut notes = Vec::with_capacity(self.document.footnotes.len());
        let mut continued = Vec::with_capacity(self.document.footnotes.len());
        for (index, footnote) in self.document.footnotes.iter().enumerate() {
            notes.push(flow.note(index, footnote));
            continued.push(flow.continued(index, footnote.at));
        }
        if !flow.errors.is_empty() {
            return Err(flow.errors);
        }
        let content = pages::Content {
            body,
            rectos,
            keeps,
            columns,
            tables,
            notes,
            continued,
        };
        Ok((content, matters))
    }
}

/// The body pages of a layout pass and the numbers of all pages.
struct Laid {
    body: pages::Composed,
    numbering: Numbering,
}

/// The line each part of a book starts at, in order.
type Parts = Vec<(usize, Matter)>;

/// A page left blank, without header or footer.
fn blank_page(config: &Config) -> Page {
    Page {
        width: config.page.width,
        height: config.page.height,
        items: Vec::new(),
    }
}

/// A line of content. Item coordinates are relative to the text area's left edge and the line's top.
#[derive(Debug, Clone)]
struct Line {
    height: f64,
    baseline: f64,
    items: Vec<Item>,
    /// The footnotes referenced on this line.
    notes: Vec<usize>,
    /// Whether the line ends inside a word, at a hyphenation point or after an explicit hyphen.
    hyphenated: bool,
}

/// Whether a page may end after a line, and at what cost.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Break {
    Allowed(f64),
    /// A page never ends here, since it would strand a line. A column or a continued footnote may, at
    /// this cost.
    Avoid(f64),
    Never,
    Forced,
}

impl Break {
    /// Whether a page may end after the line.
    fn ends_page(self) -> bool {
        matches!(self, Self::Allowed(_) | Self::Forced)
    }

    /// The rule with `extra` added to its cost, if it has one.
    fn costlier(self, extra: f64) -> Self {
        match self {
            Self::Allowed(cost) => Self::Allowed(cost + extra),
            Self::Avoid(cost) => Self::Avoid(cost + extra),
            other => other,
        }
    }
}

/// A line in the flow with the space above it and the rule for a page break below it.
#[derive(Debug)]
struct FlowLine {
    line: Line,
    /// Space between this line and the one before. Dropped at the top of a page.
    space_before: f64,
    after: Break,
    /// The source of the line's block, for diagnostics.
    at: Location,
    /// `None` for a line in the prose width, which moves to the inner edge with the prose. For a wide line,
    /// how far it may move right with the prose, which is the frame width it leaves free.
    wide: Option<f64>,
    /// Whether the line is only drawn where it starts a page, as the mark of a blank scene break.
    top_only: bool,
}

/// How a text block takes part in page breaking.
#[derive(Debug, Clone, Copy)]
enum Role {
    Text,
    /// A heading, with the anchor its first line marks if it has one.
    Heading(Option<usize>),
}

/// The horizontal extent and paragraph style of the blocks being laid out, as insets from the margin
/// frame.
#[derive(Clone, Copy)]
struct Frame<'a> {
    left: f64,
    right: f64,
    style: &'a Style,
    list_depth: usize,
    /// How much wider a wide block may be, on the right. Zero inside lists, quotations, and columns.
    widen: f64,
    /// The alignment of paragraphs without a custom style, instead of their style's: a table column's.
    align: Option<Align>,
}

impl<'a> Frame<'a> {
    /// The prose width, from the left edge of the margin frame. Wide blocks in it span the frame.
    fn prose(style: &'a Style, page: &PageGeometry) -> Self {
        let inset = page.text_width().0 - page.prose_width.0;
        Self {
            left: 0.0,
            right: inset,
            style,
            list_depth: 0,
            widen: inset,
            align: None,
        }
    }
}

struct Flow<'a> {
    config: &'a Config,
    fields: &'a Fields<'a>,
    fonts: &'a Fonts,
    source: &'a Source,
    cited: &'a Cited,
    images: &'a [Image],
    theme_images: &'a HashMap<Resource, Image>,
    structure: &'a Structure,
    notes: &'a NoteStyles,
    /// The page number this pass assumes for each anchor, empty on a page without one.
    assumed: &'a [String],
    /// The headings, images, and tables set so far, which index their entries in `structure`.
    headings: usize,
    figures: usize,
    tables: usize,
    /// Whether footnotes are being set, whose headings are not part of the structure.
    in_notes: bool,
    /// The tables set so far, whose headers repeat on continuation pages and columns.
    table_lines: Vec<pages::Table>,
    lines: Vec<FlowLine>,
    /// Space requested before the next line. Adjacent spaces collapse to the larger one.
    space: f64,
    /// Whether the previous block was a paragraph, which gives the next one a first-line indent.
    after_paragraph: bool,
    /// Line ranges of keep groups with their directive locations.
    keeps: Vec<(Range<usize>, Location)>,
    /// Line ranges set in two columns. A full-width block ends one run and starts the next.
    columns: Vec<Range<usize>>,
    /// Whether a wide block is being set across the frame, with the frame width its lines leave free.
    wide: Option<f64>,
    /// The lines that start an odd page.
    rectos: Vec<usize>,
    /// The line each part of the book starts at.
    matters: Parts,
    /// Whether a keep group is being set, which a chapter does not break.
    in_keep: bool,
    /// Whether a chapter heading waits for its first paragraph, which may get a drop capital and a lead-in.
    opening: bool,
    errors: Vec<Diagnostic>,
}

impl<'a> Flow<'a> {
    /// Lays out `blocks` from a fresh start and returns their lines.
    fn run(&mut self, blocks: &[Block], frame: Frame<'a>) -> Vec<FlowLine> {
        self.space = 0.0;
        self.after_paragraph = false;
        self.blocks(blocks, frame);
        std::mem::take(&mut self.lines)
    }

    /// The lines of footnote `index`, starting with its raised number.
    fn note(&mut self, index: usize, footnote: &Footnote) -> Vec<FlowLine> {
        let mut blocks = footnote.blocks.clone();
        let marker = Inline::FootnoteRef(index);
        match blocks.first_mut() {
            Some(Block::Paragraph { content, .. }) => {
                let space = Inline::Text {
                    text: " ".to_owned(),
                    style: InlineStyle::default(),
                };
                content.splice(0..0, [marker, space]);
            }
            _ => blocks.insert(
                0,
                Block::Paragraph {
                    at: footnote.at,
                    content: vec![marker],
                    class: None,
                },
            ),
        }
        self.unmarked(&blocks)
    }

    /// The marker above the continued part of footnote `index`: its number and the `continued` label.
    fn continued(&mut self, index: usize, at: Location) -> Vec<FlowLine> {
        let label = Inline::Text {
            text: format!(" {}", self.config.labels.continued),
            style: InlineStyle {
                emphasis: true,
                ..InlineStyle::default()
            },
        };
        let content = vec![Inline::FootnoteRef(index), label];
        self.unmarked(&[Block::Paragraph {
            at,
            content,
            class: None,
        }])
    }

    /// Lays out footnote content at the prose width. Its own markers do not count as references.
    fn unmarked(&mut self, blocks: &[Block]) -> Vec<FlowLine> {
        let frame = Frame {
            widen: 0.0,
            ..Frame::prose(&self.config.styles.footnote, &self.config.page)
        };
        let mut lines = self.run(blocks, frame);
        for line in &mut lines {
            line.line.notes.clear();
        }
        lines
    }

    fn blocks(&mut self, blocks: &[Block], frame: Frame<'a>) {
        let mut previous: Option<&Block> = None;
        for block in blocks {
            let kind = match block {
                Block::Table { .. } => Some(WideBlock::Table),
                Block::Image { .. } => Some(WideBlock::Figure),
                Block::Code { .. } => Some(WideBlock::CodeBlock),
                _ => None,
            };
            let wide = frame.widen > 0.0 && kind.is_some_and(|kind| self.config.page.wide.contains(&kind));
            self.wide = wide.then_some(0.0);
            let frame = if wide {
                Frame {
                    right: frame.right - frame.widen,
                    widen: 0.0,
                    ..frame
                }
            } else {
                frame
            };
            if matches!(block, Block::List { .. } | Block::Code { .. })
                && let Some(Block::Paragraph { content, .. }) = previous
                && introduces(content)
                && let Some(line) = self.lines.last_mut()
            {
                line.after = line.after.costlier(pages::INTRODUCTION);
            }
            previous = Some(block);
            // Only the first paragraph after a chapter heading opens the chapter.
            let opening = std::mem::take(&mut self.opening);
            match block {
                Block::Paragraph { at, content, class } => {
                    let style = self.paragraph_style(class.as_ref(), &frame);
                    // Only a paragraph that continues another one gets a first-line indent.
                    let indent = if self.after_paragraph {
                        style.first_line_indent.0
                    } else {
                        0.0
                    };
                    match class {
                        // A styled paragraph, such as an epigraph, leaves the opening to the next one.
                        Some(class) if class.name != NO_DROP_CAP && opening => {
                            self.opening = true;
                            self.text_block(*at, content, &style, frame, indent, Role::Text);
                        }
                        None if opening => self.opening_paragraph(*at, content, &style, frame),
                        _ => self.text_block(*at, content, &style, frame, indent, Role::Text),
                    }
                }
                Block::Heading {
                    at,
                    level,
                    content,
                    class,
                    ..
                } => {
                    let (mut style, typed_gap) = match class.as_ref().map(|class| self.custom(class)) {
                        None => (Cow::Borrowed(heading_style(self.config, *level)), None),
                        Some(CustomStyle::Heading { style, number_gap }) => (Cow::Borrowed(style), *number_gap),
                        Some(_) => unreachable!("classes are checked before layout"),
                    };
                    let typed = |gap: Option<Pt>| {
                        let (number, rest) = typed_number(content)?;
                        Some((gap?, number, rest))
                    };
                    let chapter = *level == 1 && !self.in_notes;
                    if chapter && self.open_chapter(*at) {
                        style.to_mut().space_before = Pt(0.0);
                    }
                    let heading = &self.structure.headings[self.headings];
                    self.headings += 1;
                    let anchor = Some(heading.anchor);
                    let chapters = &self.config.chapters;
                    match (&heading.number, style.number_gap) {
                        (Some(number), _) if chapter && chapters.number_position == NumberPosition::Above => {
                            let text = if chapters.number_label {
                                format!("{} {number}", self.config.labels.chapter)
                            } else {
                                number.clone()
                            };
                            let number = [Inline::Text {
                                text,
                                style: InlineStyle::default(),
                            }];
                            // The number takes the heading's place at the top, and its own space after
                            // separates it from the heading text.
                            let number_style = Style {
                                space_before: style.space_before,
                                ..self.config.named_style(&chapters.number_style).clone()
                            };
                            self.text_block(*at, &number, &number_style, frame, 0.0, Role::Heading(anchor));
                            style.to_mut().space_before = Pt(0.0);
                            self.heading(*at, content, None, &style, frame, None);
                        }
                        (Some(number), Some(gap)) => {
                            self.heading(*at, content, Some((gap, number)), &style, frame, anchor);
                        }
                        (Some(number), None) => {
                            self.heading(*at, &numbered(number, content), None, &style, frame, anchor);
                        }
                        (None, _) => match typed(typed_gap) {
                            Some((gap, number, rest)) => {
                                self.heading(*at, &rest, Some((gap, &number)), &style, frame, anchor);
                            }
                            None => self.heading(*at, content, None, &style, frame, anchor),
                        },
                    }
                    if chapter {
                        self.ornament(*at, &style, frame);
                        self.opening = chapters.drop_cap > 0 || chapters.lead_in > 0;
                    }
                }
                Block::List {
                    at,
                    start,
                    items,
                    class,
                } => self.list(*at, *start, items, class.as_ref(), frame),
                Block::Quote { blocks, .. } => {
                    let style = if self.in_notes {
                        &self.notes.quote
                    } else {
                        &self.config.styles.quote
                    };
                    self.space(style.space_before.0);
                    let indent = style.indent.0;
                    let inner = Frame {
                        left: frame.left + indent,
                        right: frame.right + indent,
                        style,
                        widen: 0.0,
                        ..frame
                    };
                    self.after_paragraph = false;
                    self.blocks(blocks, inner);
                    self.space(style.space_after.0);
                }
                Block::Code { first_line, lines, .. } => self.code(*first_line, lines, frame),
                Block::Image { at, image, caption, .. } => {
                    let images = self.images;
                    let figure = self.structure.figures[self.figures];
                    self.figures += 1;
                    self.image(*at, &images[*image], caption, figure, frame);
                }
                Block::Keep { at, blocks } => {
                    let start = self.lines.len();
                    let in_keep = std::mem::replace(&mut self.in_keep, true);
                    self.opening = opening;
                    self.blocks(blocks, frame);
                    self.in_keep = in_keep;
                    let end = self.lines.len();
                    if end > start {
                        for line in &mut self.lines[start..end - 1] {
                            line.after = Break::Never;
                        }
                        self.keeps.push((start..end, *at));
                    }
                    // A keep group does not interrupt the paragraph sequence around it.
                    continue;
                }
                Block::PageBreak { .. } => {
                    self.page_break();
                    continue;
                }
                Block::Matter { matter, .. } => {
                    self.page_break();
                    if self.config.document.duplex {
                        self.rectos.push(self.lines.len());
                    }
                    self.matters.push((self.lines.len(), *matter));
                }
                Block::Contents { .. } => self.contents(frame),
                Block::SceneBreak { at } => self.scene_break(*at, frame),
                Block::Table {
                    at,
                    columns,
                    header,
                    rows,
                    caption,
                    ..
                } => {
                    let number = self.structure.tables[self.tables];
                    self.tables += 1;
                    let block = table::TableBlock {
                        at: *at,
                        columns,
                        header,
                        rows,
                        caption,
                        number,
                    };
                    self.table(block, frame);
                }
                Block::Columns { blocks, .. } => self.columns(blocks, frame),
                Block::Bibliography { at } => self.bibliography(*at, frame),
                Block::FullWidth { .. } => unreachable!("the parser allows full-width only directly inside columns"),
            }
            self.after_paragraph = matches!(block, Block::Paragraph { .. });
            self.wide = None;
        }
    }

    /// Ends the page after the last line set so far.
    fn page_break(&mut self) {
        if let Some(line) = self.lines.last_mut() {
            line.after = Break::Forced;
        }
    }

    /// Starts a chapter on a new page or a new odd page, as the theme sets, outside keep groups. A chapter
    /// that starts a page is sunk by an empty line. Returns whether it starts a page, whose space above
    /// the heading the sink replaces.
    fn open_chapter(&mut self, at: Location) -> bool {
        let chapters = &self.config.chapters;
        if chapters.break_before == BreakBefore::None || self.in_keep {
            return false;
        }
        self.page_break();
        if chapters.break_before == BreakBefore::Recto {
            self.rectos.push(self.lines.len());
        }
        if chapters.sink.0 > 0.0 {
            self.space = 0.0;
            let sink = Line {
                height: chapters.sink.0,
                baseline: 0.0,
                items: Vec::new(),
                notes: Vec::new(),
                hyphenated: false,
            };
            self.push(sink, at, Break::Never);
        }
        true
    }

    /// Sets the theme's chapter ornament centered below a chapter heading in `style`, kept with what
    /// follows. The heading's space after goes below the ornament.
    fn ornament(&mut self, at: Location, style: &Style, frame: Frame<'a>) {
        let Some(ornament) = &self.config.chapters.ornament else {
            return;
        };
        let image = &self.theme_images[&ornament.image];
        self.space = 0.0;
        self.space(ornament.space_before.0);
        let width = ornament.width.0.min(self.width(frame));
        let (natural_width, natural_height) = image.size();
        let height = width * natural_height.0 / natural_width.0;
        let rect = Rect {
            x: Pt(frame.left + (self.width(frame) - width) / 2.0),
            y: Pt(0.0),
            width: Pt(width),
            height: Pt(height),
        };
        let line = Line {
            height,
            baseline: height,
            items: vec![Item::Image {
                rect,
                image: image.clone(),
            }],
            notes: Vec::new(),
            hyphenated: false,
        };
        self.push(line, at, Break::Never);
        self.space(style.space_after.0);
    }

    /// Sets a thematic break: the theme's mark in the `scene-break` style, kept with the text after it. A
    /// blank break keeps the mark's room but shows it only where it starts a page, where an empty line
    /// would go unnoticed.
    fn scene_break(&mut self, at: Location, frame: Frame<'a>) {
        let config = self.config;
        let style = &config.styles.scene_break;
        let width = self.width(frame);
        self.space(style.space_before.0);
        let lines = match &config.scene_break.mark {
            SceneMark::Text(text) => {
                let content = [Inline::Text {
                    text: text.clone(),
                    style: InlineStyle::default(),
                }];
                match self.set(&content, style, width - 2.0 * style.indent.0, 0.0) {
                    Ok(lines) => lines
                        .into_iter()
                        .map(|line| translate_line(line, frame.left + style.indent.0))
                        .collect(),
                    Err(problem) => {
                        self.errors.push(self.error(at, format!("scene break: {problem}")));
                        return;
                    }
                }
            }
            SceneMark::Image { image, width: wanted } => {
                let image = &self.theme_images[image];
                let wanted = wanted.0.min(width);
                let (natural_width, natural_height) = image.size();
                let height = wanted * natural_height.0 / natural_width.0;
                let x = match style.align {
                    Align::Left | Align::Justify => 0.0,
                    Align::Center => (width - wanted) / 2.0,
                    Align::Right => width - wanted,
                };
                let rect = Rect {
                    x: Pt(frame.left + x),
                    y: Pt(0.0),
                    width: Pt(wanted),
                    height: Pt(height),
                };
                vec![Line {
                    height,
                    baseline: height,
                    items: vec![Item::Image {
                        rect,
                        image: image.clone(),
                    }],
                    notes: Vec::new(),
                    hyphenated: false,
                }]
            }
        };
        for line in lines {
            self.push(line, at, Break::Never);
            if let Some(line) = self.lines.last_mut() {
                line.top_only = config.scene_break.blank;
            }
        }
        self.space(style.space_after.0);
    }

    /// Sets the first paragraph of a chapter with the theme's lead-in in small capitals and drop capital.
    /// The drop capital is the first letter, with any punctuation before it, as tall as the lines it spans
    /// from the cap height of the first to the baseline of the last, which stay on one page. A paragraph
    /// too short for it, or one that does not start with text, has none.
    fn opening_paragraph(&mut self, at: Location, content: &[Inline], style: &Style, frame: Frame<'a>) {
        let chapters = &self.config.chapters;
        let content = lead_in(content, chapters.lead_in);
        let span = chapters.drop_cap;
        let Some((initial, rest)) = (span > 0).then(|| drop_initial(&content)).flatten() else {
            self.text_block(at, &content, style, frame, 0.0, Role::Text);
            return;
        };
        let fonts = self.fonts;
        let lang = self.config.document.lang;
        let face = fonts.face(&style.font, style.weight, FontStyle::Normal);
        let line_height = style.size.0 * style.line_height;
        let unit = fonts.cap_height(face, Pt(1.0)).0;
        let height = (span - 1) as f64 * line_height + fonts.cap_height(face, style.size).0;
        let run = fonts.shape(&initial, face, Pt(height / unit), lang);
        if let Some(problem) = paragraph::missing_glyph(&run) {
            self.errors.push(self.error(at, problem));
            return;
        }
        let indent = run.width.0 + DROP_CAP_GAP * style.size.0;
        let width = self.width(frame) - 2.0 * style.indent.0;
        let resolved = self.resolve(&rest);
        let lines = paragraph::prepare(&resolved, style, &self.config.inline, fonts, lang)
            .and_then(|prepared| prepared.lines_beside(width, indent, span));
        let mut lines = match lines {
            Ok(lines) if lines.len() >= span => lines,
            Ok(_) => {
                self.text_block(at, &content, style, frame, 0.0, Role::Text);
                return;
            }
            Err(problem) => {
                self.errors.push(self.error(at, problem));
                return;
            }
        };
        let baseline = lines[..span - 1].iter().map(|line| line.height).sum::<f64>() + lines[span - 1].baseline;
        lines[0].items.push(text_item(0.0, baseline, run, style.color));
        let first = self.lines.len();
        self.space(style.space_before.0);
        self.place(at, lines, style, frame, Role::Text);
        for line in &mut self.lines[first..first + span - 1] {
            line.after = Break::Never;
        }
        self.space(style.space_after.0);
    }

    /// The custom style of a class, which [`classes::check`] has found in the theme.
    fn custom(&self, class: &Class) -> &'a CustomStyle {
        &self.config.custom_styles[&class.name]
    }

    /// The style of a paragraph in `frame`: its custom style, or the frame's with the frame's alignment
    /// and, in a list item, without block spacing.
    fn paragraph_style(&self, class: Option<&Class>, frame: &Frame<'a>) -> Cow<'a, Style> {
        let class = class.filter(|class| class.name != NO_DROP_CAP);
        let mut style = match (class, frame.align) {
            (Some(class), _) => return Cow::Borrowed(self.custom(class).style()),
            (None, Some(align)) => Cow::Owned(Style {
                align,
                ..frame.style.clone()
            }),
            (None, None) => Cow::Borrowed(frame.style),
        };
        // A list item's text takes the list style, whose spacing belongs around the whole list.
        if frame.list_depth > 0 {
            let style = style.to_mut();
            style.space_before = Pt(0.0);
            style.space_after = Pt(0.0);
        }
        style
    }

    /// The style, bullets, and marker style of a list: its custom style's, else the theme's. Markers
    /// take the list style unless a marker style is named.
    fn list_style(&self, class: Option<&Class>) -> (&'a Style, &'a [String], &'a Style) {
        let config = self.config;
        let lists = &config.lists;
        let (style, bullets, marker) = match class.map(|class| (class, self.custom(class))) {
            None if self.in_notes => (&self.notes.list, &lists.bullets, &lists.marker),
            None => (&config.styles.list, &lists.bullets, &lists.marker),
            Some((class, CustomStyle::List { style, bullets, marker })) => (
                if self.in_notes {
                    &self.notes.custom_lists[&class.name]
                } else {
                    style
                },
                bullets.as_ref().unwrap_or(&lists.bullets),
                if marker.is_some() { marker } else { &lists.marker },
            ),
            Some(_) => unreachable!("classes are checked before layout"),
        };
        let marker = marker.as_deref().map_or(style, |name| config.named_style(name));
        (style, bullets, marker)
    }

    /// Sets a heading whose first line marks `anchor`. A `hanging` number, with the gap after it, is set
    /// before the text, which starts the gap after the number on every line.
    fn heading(
        &mut self,
        at: Location,
        content: &[Inline],
        hanging: Option<(Pt, &str)>,
        style: &Style,
        frame: Frame<'a>,
        anchor: Option<usize>,
    ) {
        let Some((gap, number)) = hanging else {
            self.text_block(at, content, style, frame, 0.0, Role::Heading(anchor));
            return;
        };
        let face = self.fonts.face(&style.font, style.weight, style.style);
        let run = (self.fonts).shape_tracked(number, face, style.size, self.config.document.lang, style.tracking);
        if let Some(problem) = paragraph::missing_glyph(&run) {
            self.errors.push(self.error(at, problem));
        }
        let x = frame.left + style.indent.0;
        let text = Frame {
            left: frame.left + run.width.0 + gap.0,
            ..frame
        };
        let first = self.lines.len();
        self.text_block(at, content, style, text, 0.0, Role::Heading(anchor));
        if let Some(line) = self.lines.get_mut(first) {
            let y = line.line.baseline;
            line.line.items.push(text_item(x, y, run, style.color));
        }
    }

    /// Sets a paragraph or heading. Headings never end a page, so they stay with what follows, and
    /// neither does a block whose style keeps it with the next one. Such a block keeps the first two
    /// lines of a paragraph after it, also in columns.
    fn text_block(
        &mut self,
        at: Location,
        content: &[Inline],
        style: &Style,
        frame: Frame<'a>,
        first_indent: f64,
        role: Role,
    ) {
        let width = self.width(frame) - 2.0 * style.indent.0;
        self.space(style.space_before.0);
        match self.set(content, style, width, first_indent) {
            Ok(lines) => self.place(at, lines, style, frame, role),
            Err(problem) => self.errors.push(self.error(at, problem)),
        }
        self.space(style.space_after.0);
    }

    /// Adds the lines of a paragraph or heading in `style` to the flow, with the break rules of
    /// [`Flow::text_block`].
    fn place(&mut self, at: Location, lines: Vec<Line>, style: &Style, frame: Frame<'a>, role: Role) {
        let kept = self.lines.last().is_some_and(|line| line.after == Break::Never);
        let dx = frame.left + style.indent.0;
        let count = lines.len();
        for (index, mut line) in lines.into_iter().enumerate() {
            let after = match role {
                Role::Heading(anchor) => {
                    if let Some(id) = anchor.filter(|_| index == 0) {
                        line.items.push(anchor_item(id, 0.0));
                    }
                    Break::Never
                }
                Role::Text if kept && index == 0 && count > 1 => Break::Never,
                Role::Text => pages::line_break(index, count),
            };
            self.push(translate_line(line, dx), at, after);
        }
        if style.keep_with_next && count > 0 {
            self.keep_last();
        }
    }

    /// Keeps the last line set with the line that follows it.
    fn keep_last(&mut self) {
        if let Some(line) = self.lines.last_mut() {
            line.after = Break::Never;
        }
    }

    /// Breaks inline content into lines of `width` in `style`, with cross-references and citations resolved.
    fn set(&self, content: &[Inline], style: &Style, width: f64, first_indent: f64) -> Result<Vec<Line>, String> {
        let content = self.resolve(content);
        let lang = self.config.document.lang;
        paragraph::lines(
            &content,
            style,
            &self.config.inline,
            self.fonts,
            lang,
            width,
            first_indent,
        )
    }

    /// Replaces cross-references, citations, and placeholders with their text, cross-references and
    /// citations linked to their anchor. A page reference shows the page this pass assumes. Each work in
    /// a citation links to its bibliography entry.
    fn resolve<'c>(&self, content: &'c [Inline]) -> Cow<'c, [Inline]> {
        if !content
            .iter()
            .any(|inline| matches!(inline, Inline::Ref(_) | Inline::Citation { .. } | Inline::Field { .. }))
        {
            return Cow::Borrowed(content);
        }
        let mut resolved = Vec::with_capacity(content.len());
        for inline in content {
            match inline {
                Inline::Ref(reference) => {
                    let (anchor, number) = self.structure.reference(&reference.label);
                    let text = if reference.page {
                        format!("{}\u{a0}{}", self.config.labels.page, self.assumed[anchor])
                    } else {
                        number.to_owned()
                    };
                    let style = InlineStyle {
                        link: Some(Link::Anchor(anchor)),
                        unbreakable: true,
                        ..reference.style.clone()
                    };
                    resolved.push(Inline::Text { text, style });
                }
                Inline::Citation { index, style } => {
                    resolved.extend(self.cited.citations[*index].iter().map(|part| Inline::Text {
                        text: part.text.clone(),
                        style: InlineStyle {
                            link: part.target.map(|target| Link::Anchor(self.structure.entries[target])),
                            ..style.clone()
                        },
                    }));
                }
                Inline::Field { placeholder, style, .. } => resolved.push(Inline::Text {
                    text: self.fields.text(placeholder).expect("fields are checked before layout"),
                    style: style.clone(),
                }),
                inline => resolved.push(inline.clone()),
            }
        }
        Cow::Owned(resolved)
    }

    /// Sets a list in the `list` style or its custom style, with markers before the items. A custom
    /// style's bullets replace the theme's. Markers take the font, size, weight, and color of the marker
    /// style and sit on the baseline of their item's first line.
    fn list(
        &mut self,
        at: Location,
        start: Option<u64>,
        items: &[Vec<Block>],
        class: Option<&Class>,
        frame: Frame<'a>,
    ) {
        let (indent, item_spacing) = if self.in_notes {
            (self.notes.list_indent, self.notes.item_spacing)
        } else {
            (self.config.lists.indent, self.config.lists.item_spacing)
        };
        let (style, bullets, marker_style) = self.list_style(class);
        let start_line = self.lines.len();
        let inner = Frame {
            left: frame.left + indent.0,
            style,
            list_depth: frame.list_depth + 1,
            widen: 0.0,
            ..frame
        };
        let face = (self.fonts).face(&marker_style.font, marker_style.weight, marker_style.style);
        let outermost = frame.list_depth == 0;
        if outermost {
            self.space(style.space_before.0);
        }
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                self.space(item_spacing.0);
            }
            let marker = match start {
                Some(first) => format!("{}.", first + index as u64),
                None => bullets[frame.list_depth.min(bullets.len() - 1)].clone(),
            };
            let run = (self.fonts).shape(&marker, face, marker_style.size, self.config.document.lang);
            if let Some(problem) = paragraph::missing_glyph(&run) {
                self.errors.push(self.error(at, problem.to_string()));
            }
            // The marker ends half an em before the item text.
            let x = inner.left - 0.5 * style.size.0 - run.width.0;
            let first = self.lines.len();
            self.after_paragraph = false;
            self.blocks(item, inner);
            if first == self.lines.len() {
                let line = self.empty_line(style);
                self.push(line, at, Break::Allowed(0.0));
            }
            let line = &mut self.lines[first].line;
            let y = line.baseline;
            line.items.push(text_item(x, y, run, marker_style.color));
        }
        if style.keep_with_next && self.lines.len() > start_line {
            self.keep_last();
        }
        if outermost {
            self.space(style.space_after.0);
        }
    }

    /// Sets a column section at the column width, which divides the prose width. Full-width blocks inside
    /// it are set across `frame`, where wide blocks widen as outside columns, and split the section into
    /// runs. Each layout change starts a new paragraph sequence and leaves at least the column change
    /// spacing above and below the columns.
    fn columns(&mut self, blocks: &[Block], frame: Frame<'a>) {
        let page = &self.config.page;
        let column = Frame {
            right: frame.right + page.prose_width.0 - pages::column_width(page),
            widen: 0.0,
            ..frame
        };
        let mut start = self.change_layout();
        for block in blocks {
            match block {
                Block::FullWidth { blocks, .. } => {
                    self.end_run(start);
                    self.blocks(blocks, frame);
                    start = self.change_layout();
                }
                block => self.blocks(std::slice::from_ref(block), column),
            }
        }
        self.end_run(start);
    }

    /// Requests the space at a change between one and two columns and returns the next line's index.
    fn change_layout(&mut self) -> usize {
        self.space(self.config.page.column_change_spacing.0);
        self.after_paragraph = false;
        self.lines.len()
    }

    fn end_run(&mut self, start: usize) {
        if self.lines.len() > start {
            self.columns.push(start..self.lines.len());
        }
        self.change_layout();
    }

    /// Sets an image centered in the frame, followed by its numbered caption, as lines that stay
    /// together. The image keeps its proportions and shrinks to the frame width and to the height the
    /// text area leaves beside its caption and the heading it is kept with. It never grows.
    fn image(
        &mut self,
        at: Location,
        image: &Image,
        caption: &[Inline],
        figure: Option<structure::Numbered>,
        frame: Frame<'a>,
    ) {
        let style = &self.config.styles.caption;
        let width = self.width(frame);
        let caption = match figure {
            None => Vec::new(),
            Some(figure) => {
                match self.caption_lines(at, &self.config.labels.figure, figure.number, caption, width, false) {
                    Some(lines) => lines,
                    None => return,
                }
            }
        };

        // A figure is spaced like its caption: the caption's space after it, and its space before
        // between image and caption.
        self.space(style.space_after.0);
        let mut below = caption.iter().map(|line| line.height).sum::<f64>();
        if !caption.is_empty() {
            below += style.space_before.0;
        }
        let height = self.config.page.text_height().0 - self.kept_above() - below;
        if height <= 0.0 {
            let message = format!(
                "this image has no room: its caption and the heading kept with it fill the {:.1}pt text area",
                self.config.page.text_height().0
            );
            self.errors.push(self.error(at, message));
            return;
        }
        let (natural_width, natural_height) = image.size();
        let scale = 1f64.min(width / natural_width.0).min(height / natural_height.0);
        let size = (natural_width.0 * scale, natural_height.0 * scale);
        let rect = Rect {
            x: Pt(frame.left + (width - size.0) / 2.0),
            y: Pt(0.0),
            width: Pt(size.0),
            height: Pt(size.1),
        };
        let mut items = vec![Item::Image {
            rect,
            image: image.clone(),
        }];
        if let Some(figure) = figure {
            items.push(anchor_item(figure.anchor, frame.left));
        }
        let line = Line {
            height: size.1,
            baseline: size.1,
            items,
            notes: Vec::new(),
            hyphenated: false,
        };
        let count = caption.len();
        let after = if count == 0 { Break::Allowed(0.0) } else { Break::Never };
        self.push(line, at, after);
        self.space(style.space_before.0);
        let dx = frame.left + style.indent.0;
        for (index, line) in caption.into_iter().enumerate() {
            let after = if index + 1 == count {
                Break::Allowed(0.0)
            } else {
                Break::Never
            };
            self.push(translate_line(line, dx), at, after);
        }
        self.space(style.space_after.0);
    }

    /// The lines of a caption numbered `number` with `label`, set at `width` in the caption style, or
    /// `None` after reporting why it cannot be set. With `widen`, a width that is too narrow for the
    /// caption's longest word grows to fit it.
    fn caption_lines(
        &mut self,
        at: Location,
        label: &str,
        number: usize,
        caption: &[Inline],
        width: f64,
        widen: bool,
    ) -> Option<Vec<Line>> {
        let style = &self.config.styles.caption;
        let label = Inline::Text {
            text: format!("{label} {number}{}", self.config.caption_separator),
            style: InlineStyle::default(),
        };
        let content: Vec<Inline> = std::iter::once(label).chain(caption.iter().cloned()).collect();
        let mut width = width - 2.0 * style.indent.0;
        if widen {
            let lang = self.config.document.lang;
            let resolved = self.resolve(&content);
            match paragraph::prepare(&resolved, style, &self.config.inline, self.fonts, lang) {
                Ok(prepared) => width = width.max(prepared.minimum().0 + 1e-6),
                Err(problem) => {
                    self.errors.push(self.error(at, problem));
                    return None;
                }
            }
        }
        match self.set(&content, style, width, 0.0) {
            Ok(lines) => Some(lines),
            Err(problem) => {
                self.errors.push(self.error(at, problem));
                None
            }
        }
    }

    /// The height of the lines at the end of the flow that the next line must stay with, such as a
    /// heading, with the space requested before the next line.
    fn kept_above(&self) -> f64 {
        let kept = self.lines.iter().rev().take_while(|line| line.after == Break::Never);
        let mut height = 0.0;
        let mut space = self.space;
        for line in kept {
            height += space + line.line.height;
            space = line.space_before;
        }
        height
    }

    /// Code lines are set as they are. A line wider than the available width is an error.
    fn code(&mut self, first_line: u64, lines: &[String], frame: Frame<'a>) {
        let style = if self.in_notes {
            &self.notes.code_block
        } else {
            &self.config.styles.code_block
        };
        let face = self.fonts.face(&style.font, style.weight, style.style);
        let width = self.width(frame) - 2.0 * style.indent.0;
        let x = frame.left + style.indent.0;
        self.space(style.space_before.0);
        for (index, text) in lines.iter().enumerate() {
            let line_at = Location {
                line: first_line + index as u64,
                column: 1,
            };
            let mut line = self.empty_line(style);
            let text = text.replace('\t', "    ");
            if !text.trim().is_empty() {
                let run = self.fonts.shape(&text, face, style.size, self.config.document.lang);
                if let Some(problem) = paragraph::missing_glyph(&run) {
                    self.errors.push(self.error(line_at, problem.to_string()));
                }
                if run.width.0 > width {
                    let message = format!(
                        "code line is {:.1}pt wider than the available {:.1}pt; code is never wrapped, so shorten the line",
                        run.width.0 - width,
                        width
                    );
                    self.errors.push(self.error(line_at, message));
                }
                line.items.push(text_item(x, line.baseline, run, style.color));
            }
            self.push(line, line_at, pages::line_break(index, lines.len()));
        }
        self.space(style.space_after.0);
    }

    fn empty_line(&self, style: &Style) -> Line {
        let face = self.fonts.face(&style.font, style.weight, style.style);
        let metrics = self.fonts.metrics(face, style.size);
        let height = style.size.0 * style.line_height;
        Line {
            height,
            baseline: paragraph::baseline(height, &metrics),
            items: Vec::new(),
            notes: Vec::new(),
            hyphenated: false,
        }
    }

    /// Adds a line to the flow. A page or column ending after a hyphenated line costs more.
    fn push(&mut self, line: Line, at: Location, after: Break) {
        let after = if line.hyphenated {
            after.costlier(pages::HYPHENATED)
        } else {
            after
        };
        self.lines.push(FlowLine {
            line,
            space_before: std::mem::take(&mut self.space),
            after,
            at,
            wide: self.wide,
            top_only: false,
        });
    }

    /// Requests vertical space before the next line. Adjacent spaces collapse to the larger one.
    fn space(&mut self, amount: f64) {
        self.space = self.space.max(amount);
    }

    fn width(&self, frame: Frame) -> f64 {
        self.config.page.text_width().0 - frame.left - frame.right
    }

    fn error(&self, at: Location, message: String) -> Diagnostic {
        Diagnostic::new(Some(self.source.clone()), message).at(at.line, at.column)
    }
}

/// Whether a paragraph ends in a colon, introducing what follows.
fn introduces(content: &[Inline]) -> bool {
    matches!(content.last(), Some(Inline::Text { text, .. }) if text.trim_end().ends_with(':'))
}

/// A number such as "2." typed at the start of a heading and followed by a space, and the content
/// after it, or `None` without one.
fn typed_number(content: &[Inline]) -> Option<(String, Vec<Inline>)> {
    let Some(Inline::Text { text, style }) = content.first() else {
        return None;
    };
    let digits = text.find(|c: char| !c.is_ascii_digit())?;
    let after = text[digits..].strip_prefix('.')?;
    let rest = after.trim_start_matches(' ');
    if style.code || digits == 0 || rest.len() == after.len() {
        return None;
    }
    let mut content = content.to_vec();
    if rest.is_empty() {
        content.remove(0);
    } else {
        content[0] = Inline::Text {
            text: rest.to_owned(),
            style: style.clone(),
        };
    }
    (!content.is_empty()).then(|| (text[..=digits].to_owned(), content))
}

/// Content whose first `words` words are set in small capitals.
fn lead_in(content: &[Inline], words: usize) -> Vec<Inline> {
    let mut left = words;
    let mut started = false;
    let mut result = Vec::with_capacity(content.len() + 1);
    for inline in content {
        let Inline::Text { text, style } = inline else {
            result.push(inline.clone());
            continue;
        };
        if left == 0 || style.code {
            result.push(inline.clone());
            continue;
        }
        // The byte where the lead-in ends in this piece: before the space after its last word.
        let mut end = text.len();
        let mut in_word = started;
        for (index, character) in text.char_indices() {
            if character == ' ' {
                if in_word {
                    left -= 1;
                    in_word = false;
                    if left == 0 {
                        end = index;
                        break;
                    }
                }
            } else {
                in_word = true;
            }
        }
        started = in_word;
        let small = InlineStyle {
            small_caps: true,
            ..style.clone()
        };
        if end > 0 {
            result.push(Inline::Text {
                text: text[..end].to_owned(),
                style: small,
            });
        }
        if end < text.len() {
            result.push(Inline::Text {
                text: text[end..].to_owned(),
                style: style.clone(),
            });
        }
    }
    result
}

/// The first letter of content that starts with text, with any punctuation before it, and the content
/// without it. `None` if the content starts otherwise, with code, or without a letter.
fn drop_initial(content: &[Inline]) -> Option<(String, Vec<Inline>)> {
    let Some(Inline::Text { text, style }) = content.first() else {
        return None;
    };
    if style.code {
        return None;
    }
    let letter = text.char_indices().find(|(_, character)| character.is_alphanumeric())?;
    let end = letter.0 + letter.1.len_utf8();
    if text[..letter.0].contains(' ') {
        return None;
    }
    let mut rest = content.to_vec();
    if end == text.len() {
        rest.remove(0);
    } else {
        rest[0] = Inline::Text {
            text: text[end..].to_owned(),
            style: style.clone(),
        };
    }
    Some((text[..end].to_owned(), rest))
}

/// Heading content after its number and a space.
fn numbered(number: &str, content: &[Inline]) -> Vec<Inline> {
    let number = Inline::Text {
        text: format!("{number} "),
        style: InlineStyle::default(),
    };
    std::iter::once(number).chain(content.iter().cloned()).collect()
}

/// The destination of anchor `id` at `x` on the top of a line.
fn anchor_item(id: usize, x: f64) -> Item {
    Item::Anchor {
        id,
        x: Pt(x),
        y: Pt(0.0),
    }
}

fn heading_style(config: &Config, level: u8) -> &Style {
    let styles = &config.styles;
    match level {
        1 => &styles.heading_1,
        2 => &styles.heading_2,
        3 => &styles.heading_3,
        4 => &styles.heading_4,
        5 => &styles.heading_5,
        _ => &styles.heading_6,
    }
}

fn text_item(x: f64, y: f64, run: ShapedRun, color: Color) -> Item {
    Item::Text {
        x: Pt(x),
        y: Pt(y),
        run,
        color,
    }
}

fn translate_line(mut line: Line, dx: f64) -> Line {
    line.items = line.items.into_iter().map(|item| translate(item, dx, 0.0)).collect();
    line
}

fn translate(item: Item, dx: f64, dy: f64) -> Item {
    let rect = |rect: Rect| Rect {
        x: Pt(rect.x.0 + dx),
        y: Pt(rect.y.0 + dy),
        ..rect
    };
    match item {
        Item::Text { x, y, run, color } => text_item(x.0 + dx, y.0 + dy, run, color),
        Item::TurnedText {
            x,
            y,
            angle,
            run,
            color,
        } => Item::TurnedText {
            x: Pt(x.0 + dx),
            y: Pt(y.0 + dy),
            angle,
            run,
            color,
        },
        Item::Rect { rect: r, color } => Item::Rect { rect: rect(r), color },
        Item::Link { rect: r, link } => Item::Link { rect: rect(r), link },
        Item::Image { rect: r, image } => Item::Image { rect: rect(r), image },
        Item::Anchor { id, x, y } => Item::Anchor {
            id,
            x: Pt(x.0 + dx),
            y: Pt(y.0 + dy),
        },
    }
}

/// The face for inline text in a block style. Emphasis toggles italic; strong sets bold, or keeps a
/// heavier weight.
fn inline_face(style: &Style, emphasis: bool, strong: bool) -> (Weight, FontStyle) {
    let weight = if strong {
        style.weight.max(Weight::BOLD)
    } else {
        style.weight
    };
    let italic = (style.style == FontStyle::Italic) != emphasis;
    (weight, if italic { FontStyle::Italic } else { FontStyle::Normal })
}
