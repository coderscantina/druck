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
//! page may precede it. A document that cites has a [bibliography] section. Page numbers shown in the
//! text, in the table of contents and in page references, are only known after layout, so layout
//! repeats until they agree with the pages it produced. Headers and footers are drawn on the final
//! pages; see [`bands`].

mod bands;
mod bibliography;
mod classes;
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

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use self::structure::Structure;
use crate::citations::Cited;
use crate::config::resolved::{Config, CustomStyle, PageGeometry, Style};
use crate::config::source::{Resource, Source};
use crate::config::theme::{Align, FontStyle, Weight, WideBlock};
use crate::config::values::{Color, Pt};
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Class, Document, Footnote, Inline, InlineStyle, Link, Location};
use crate::image::Image;
use crate::page::{Bookmark, Item, Output, Page, Position, Rect};
use crate::text::{Fonts, ShapedRun};

pub use self::titles::check as check_title;

/// The most layout passes spent on page numbers that change the layout they come from.
const PASSES: usize = 5;

/// Lays out `document` on pages with its title, table of contents, bibliography, headers, and footers.
/// `cited` holds its formatted citations, `images` the loaded [`Document::images`], and `theme_images`
/// the loaded theme images by resource. Errors name the source location of content that cannot fit.
pub fn layout(
    document: &Document,
    cited: &Cited,
    images: &[Image],
    theme_images: &HashMap<Resource, Image>,
    config: &Config,
    fonts: &Fonts,
    source: &Source,
) -> Result<Output, Vec<Diagnostic>> {
    titles::check(config, source)?;
    classes::check(document, config, source)?;
    let structure = Structure::new(document, config, cited.references());
    let title_page = if config.document.title_page {
        Some(titles::page(config, fonts, theme_images, source)?)
    } else {
        None
    };
    let pass = Pass {
        document,
        cited,
        images,
        theme_images,
        config,
        fonts,
        source,
        structure: &structure,
        first: usize::from(title_page.is_some()),
    };
    let shown = structure.shown_pages(config);
    let (body, anchors) = settle(&structure.anchors, &shown, source, |assumed| pass.run(assumed))?;
    let mut pages: Vec<Page> = title_page.into_iter().chain(body).collect();
    bands::draw(&mut pages, &structure, &anchors, config, fonts)?;
    let outline = structure
        .headings
        .iter()
        .map(|heading| Bookmark {
            level: heading.level,
            title: heading.title(),
            anchor: heading.anchor,
        })
        .collect();
    Ok(Output {
        pages,
        anchors,
        outline,
    })
}

/// Repeats `pass` until the page of every anchor in `shown` is the page the pass assumed for it, and
/// returns the pages and anchor positions of that pass. The first pass assumes page 1 everywhere.
/// Without shown pages one pass is enough. Page numbers that keep moving are reported at the anchor
/// that moved, after [`PASSES`] passes.
fn settle<T>(
    anchors: &[(Location, String)],
    shown: &[usize],
    source: &Source,
    mut pass: impl FnMut(&[usize]) -> Result<(T, Vec<Position>), Vec<Diagnostic>>,
) -> Result<(T, Vec<Position>), Vec<Diagnostic>> {
    let mut assumed = vec![1; anchors.len()];
    let mut count = 0;
    loop {
        let (laid, positions) = pass(&assumed)?;
        count += 1;
        let moved = shown
            .iter()
            .copied()
            .find(|&anchor| positions[anchor].page + 1 != assumed[anchor]);
        let Some(anchor) = moved else {
            return Ok((laid, positions));
        };
        if count == PASSES {
            let (at, what) = &anchors[anchor];
            let message = format!(
                "page numbers did not settle after {PASSES} layout passes: {what} moves between page {} and \
                 page {}",
                assumed[anchor],
                positions[anchor].page + 1
            );
            return Err(vec![
                Diagnostic::new(Some(source.clone()), message).at(at.line, at.column),
            ]);
        }
        assumed = positions.iter().map(|position| position.page + 1).collect();
    }
}

/// What every layout pass shares.
struct Pass<'a> {
    document: &'a Document,
    cited: &'a Cited,
    images: &'a [Image],
    theme_images: &'a HashMap<Resource, Image>,
    config: &'a Config,
    fonts: &'a Fonts,
    source: &'a Source,
    structure: &'a Structure,
    /// The physical index of the first body page: 1 after a title page.
    first: usize,
}

impl Pass<'_> {
    /// Lays out the body with `assumed` page numbers for the anchors, and returns the body pages and
    /// where each anchor is in the whole document.
    fn run(&self, assumed: &[usize]) -> Result<(Vec<Page>, Vec<Position>), Vec<Diagnostic>> {
        let content = self.content(assumed)?;
        let pages = pages::compose(content, self.first, self.config, self.source)?;
        let mut positions = vec![None; self.structure.anchors.len()];
        for (index, page) in pages.iter().enumerate() {
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
        Ok((pages, positions))
    }

    /// Sets the body and the footnotes as lines with their break rules, with `assumed` page numbers for
    /// the anchors.
    fn content(&self, assumed: &[usize]) -> Result<pages::Content, Vec<Diagnostic>> {
        let config = self.config;
        let mut flow = Flow {
            config,
            fonts: self.fonts,
            source: self.source,
            cited: self.cited,
            images: self.images,
            theme_images: self.theme_images,
            structure: self.structure,
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
            errors: Vec::new(),
        };
        let frame = Frame::prose(&config.styles.body, &config.page);
        if !config.document.title_page
            && let Some(slots) = titles::block(config)
        {
            flow.title(slots, frame);
        }
        if config.document.toc {
            flow.contents(frame);
        }
        flow.blocks(&self.document.blocks, frame);
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
        Ok(pages::Content {
            body,
            keeps,
            columns,
            tables,
            notes,
            continued,
        })
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
    fonts: &'a Fonts,
    source: &'a Source,
    cited: &'a Cited,
    images: &'a [Image],
    theme_images: &'a HashMap<Resource, Image>,
    structure: &'a Structure,
    /// The page number this pass assumes for each anchor.
    assumed: &'a [usize],
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
            match block {
                Block::Paragraph { at, content, class } => {
                    let style = self.paragraph_style(class.as_ref(), &frame);
                    // Only a paragraph that continues another one gets a first-line indent.
                    let indent = if self.after_paragraph {
                        style.first_line_indent.0
                    } else {
                        0.0
                    };
                    self.text_block(*at, content, &style, frame, indent, Role::Text);
                }
                Block::Heading {
                    at,
                    level,
                    content,
                    class,
                    ..
                } => {
                    let (style, number_gap) = match class.as_ref().map(|class| self.custom(class)) {
                        None => (heading_style(self.config, *level), None),
                        Some(CustomStyle::Heading { style, number_gap }) => (style, *number_gap),
                        Some(_) => unreachable!("classes are checked before layout"),
                    };
                    if self.in_notes {
                        self.heading(*at, content, style, frame, None, number_gap);
                    } else {
                        let heading = &self.structure.headings[self.headings];
                        self.headings += 1;
                        let content = match &heading.number {
                            Some(number) => Cow::Owned(numbered(number, content)),
                            None => Cow::Borrowed(content.as_slice()),
                        };
                        self.heading(*at, &content, style, frame, Some(heading.anchor), number_gap);
                    }
                }
                Block::List {
                    at,
                    start,
                    items,
                    class,
                } => self.list(*at, *start, items, class.as_ref(), frame),
                Block::Quote { blocks, .. } => {
                    let style = &self.config.styles.quote;
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
                    self.blocks(blocks, frame);
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
                    if let Some(line) = self.lines.last_mut() {
                        line.after = Break::Forced;
                    }
                    continue;
                }
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

    /// The custom style of a class, which [`classes::check`] has found in the theme.
    fn custom(&self, class: &Class) -> &'a CustomStyle {
        &self.config.custom_styles[&class.name]
    }

    /// The style of a paragraph in `frame`: its custom style, or the frame's with the frame's alignment.
    fn paragraph_style(&self, class: Option<&Class>, frame: &Frame<'a>) -> Cow<'a, Style> {
        match (class, frame.align) {
            (Some(class), _) => Cow::Borrowed(self.custom(class).style()),
            (None, Some(align)) => Cow::Owned(Style {
                align,
                ..frame.style.clone()
            }),
            (None, None) => Cow::Borrowed(frame.style),
        }
    }

    /// The style and bullets of a list: its custom style's, or the theme's.
    fn list_style(&self, class: Option<&Class>) -> (&'a Style, &'a [String]) {
        let lists = &self.config.lists;
        match class.map(|class| self.custom(class)) {
            None => (&self.config.styles.list, &lists.bullets),
            Some(CustomStyle::List { style, bullets }) => (style, bullets.as_ref().unwrap_or(&lists.bullets)),
            Some(_) => unreachable!("classes are checked before layout"),
        }
    }

    /// Sets a heading. With a number gap, a number such as "2." that the author typed at its start
    /// hangs before the text, which starts the gap after it on every line.
    fn heading(
        &mut self,
        at: Location,
        content: &[Inline],
        style: &Style,
        frame: Frame<'a>,
        anchor: Option<usize>,
        number_gap: Option<Pt>,
    ) {
        let typed = number_gap.and_then(|gap| Some((gap, typed_number(content)?)));
        let Some((gap, (number, rest))) = typed else {
            self.text_block(at, content, style, frame, 0.0, Role::Heading(anchor));
            return;
        };
        let face = self.fonts.face(&style.font, style.weight, style.style);
        let run = (self.fonts).shape_tracked(&number, face, style.size, self.config.document.lang, style.tracking);
        if let Some(problem) = paragraph::missing_glyph(&run) {
            self.errors.push(self.error(at, problem));
        }
        let x = frame.left + style.indent.0;
        let text = Frame {
            left: frame.left + run.width.0 + gap.0,
            ..frame
        };
        let first = self.lines.len();
        self.text_block(at, &rest, style, text, 0.0, Role::Heading(anchor));
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
        let kept = self.lines.last().is_some_and(|line| line.after == Break::Never);
        self.space(style.space_before.0);
        match self.set(content, style, width, first_indent) {
            Ok(lines) => {
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
            Err(problem) => self.errors.push(self.error(at, problem)),
        }
        self.space(style.space_after.0);
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

    /// Replaces cross-references and citations with their text, linked to their anchor. A page reference
    /// shows the page this pass assumes. A citation links to the bibliography entry of the first work it shows.
    fn resolve<'c>(&self, content: &'c [Inline]) -> Cow<'c, [Inline]> {
        if !content
            .iter()
            .any(|inline| matches!(inline, Inline::Ref(_) | Inline::Citation { .. }))
        {
            return Cow::Borrowed(content);
        }
        let resolved = content
            .iter()
            .map(|inline| match inline {
                Inline::Ref(reference) => {
                    let (anchor, number) = self.structure.reference(&reference.label);
                    let text = if reference.page {
                        format!("{}\u{a0}{}", self.config.labels.page, self.assumed[anchor])
                    } else {
                        number.to_owned()
                    };
                    let style = InlineStyle {
                        link: Some(Link::Anchor(anchor)),
                        ..reference.style.clone()
                    };
                    Inline::Text { text, style }
                }
                Inline::Citation { index, style } => {
                    let anchor = self.structure.entries[self.cited.targets[*index]];
                    Inline::Text {
                        text: self.cited.texts[*index].clone(),
                        style: InlineStyle {
                            link: Some(Link::Anchor(anchor)),
                            ..style.clone()
                        },
                    }
                }
                inline => inline.clone(),
            })
            .collect();
        Cow::Owned(resolved)
    }

    /// Sets a list in the `list` style or its custom style, with markers before the items. A custom
    /// style's bullets replace the theme's.
    fn list(
        &mut self,
        at: Location,
        start: Option<u64>,
        items: &[Vec<Block>],
        class: Option<&Class>,
        frame: Frame<'a>,
    ) {
        let lists = &self.config.lists;
        let (style, bullets) = self.list_style(class);
        let start_line = self.lines.len();
        let inner = Frame {
            left: frame.left + lists.indent.0,
            style,
            list_depth: frame.list_depth + 1,
            widen: 0.0,
            ..frame
        };
        let face = self.fonts.face(&style.font, style.weight, style.style);
        self.space(style.space_before.0);
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                self.space(lists.item_spacing.0);
            }
            let marker = match start {
                Some(first) => format!("{}.", first + index as u64),
                None => bullets[frame.list_depth.min(bullets.len() - 1)].clone(),
            };
            let run = self.fonts.shape(&marker, face, style.size, self.config.document.lang);
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
            line.items.push(text_item(x, y, run, style.color));
        }
        if style.keep_with_next && self.lines.len() > start_line {
            self.keep_last();
        }
        self.space(style.space_after.0);
    }

    /// Sets a column section at the column width, which divides the prose width. Full-width blocks inside
    /// it are set across `frame`, where wide blocks widen as outside columns, and split the section into
    /// runs. Each layout change starts a new paragraph sequence and leaves at least the column gap above
    /// and below the columns.
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
        self.space(self.config.page.column_gap.0);
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
            Some(figure) => match self.caption_lines(at, &self.config.labels.figure, figure.number, caption, width) {
                Some(lines) => lines,
                None => return,
            },
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
    /// `None` after reporting why it cannot be set.
    fn caption_lines(
        &mut self,
        at: Location,
        label: &str,
        number: usize,
        caption: &[Inline],
        width: f64,
    ) -> Option<Vec<Line>> {
        let style = &self.config.styles.caption;
        let label = Inline::Text {
            text: format!("{label} {number}{}", self.config.caption_separator),
            style: InlineStyle::default(),
        };
        let content: Vec<Inline> = std::iter::once(label).chain(caption.iter().cloned()).collect();
        match self.set(&content, style, width - 2.0 * style.indent.0, 0.0) {
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
        let style = &self.config.styles.code_block;
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
