//! Layout: places the document's blocks on pages using resolved styles and shaped text.
//!
//! Blocks become a flow of lines, each with the space above it and a rule for ending a page after
//! it. Lines of a column section are set at the column width and grouped into runs. [`paragraph`]
//! chooses line breaks for each whole paragraph; [`pages`] chooses page and column breaks for the
//! whole flow and places footnotes.
//!
//! An image is one line as tall as the image, kept with the lines of its caption. A table row is
//! one line; see [`table`].

mod pages;
mod paragraph;
mod table;
#[cfg(test)]
mod tests;

use std::ops::Range;

use crate::config::resolved::{Config, Style};
use crate::config::source::Source;
use crate::config::theme::{FontStyle, Weight};
use crate::config::values::{Color, Pt};
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Document, Footnote, Inline, InlineStyle, Location};
use crate::image::Image;
use crate::page::{Item, Page, Rect};
use crate::text::{Fonts, ShapedRun};

/// Lays out `document` on pages. `images` are the loaded [`Document::images`]. Errors name the source
/// location of content that cannot fit.
pub fn layout(
    document: &Document,
    images: &[Image],
    config: &Config,
    fonts: &Fonts,
    source: &Source,
) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let mut flow = Flow {
        config,
        fonts,
        source,
        images,
        figures: 0,
        tables: 0,
        table_lines: Vec::new(),
        lines: Vec::new(),
        space: 0.0,
        after_paragraph: false,
        keeps: Vec::new(),
        columns: Vec::new(),
        errors: Vec::new(),
    };
    let body = flow.run(&document.blocks, Frame::full(&config.styles.body));
    let keeps = std::mem::take(&mut flow.keeps);
    let columns = std::mem::take(&mut flow.columns);
    let tables = std::mem::take(&mut flow.table_lines);
    let mut notes = Vec::with_capacity(document.footnotes.len());
    let mut continued = Vec::with_capacity(document.footnotes.len());
    for (index, footnote) in document.footnotes.iter().enumerate() {
        notes.push(flow.note(index, footnote));
        continued.push(flow.continued(index, footnote.at));
    }
    if !flow.errors.is_empty() {
        return Err(flow.errors);
    }
    pages::compose(
        pages::Content {
            body,
            keeps,
            columns,
            tables,
            notes,
            continued,
        },
        config,
        source,
    )
}

/// A line of content. Item coordinates are relative to the text area's left edge and the line's top.
#[derive(Debug, Clone)]
struct Line {
    height: f64,
    baseline: f64,
    items: Vec<Item>,
    /// The footnotes referenced on this line.
    notes: Vec<usize>,
}

/// Whether a page may end after a line, and at what cost.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Break {
    Allowed(f64),
    Never,
    Forced,
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
}

/// The horizontal extent and paragraph style of the blocks being laid out.
#[derive(Clone, Copy)]
struct Frame<'a> {
    left: f64,
    right: f64,
    style: &'a Style,
    list_depth: usize,
}

impl<'a> Frame<'a> {
    fn full(style: &'a Style) -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            style,
            list_depth: 0,
        }
    }
}

struct Flow<'a> {
    config: &'a Config,
    fonts: &'a Fonts,
    source: &'a Source,
    images: &'a [Image],
    /// The number of captioned images so far, which numbers the next caption.
    figures: usize,
    /// The number of captioned tables so far.
    tables: usize,
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
        self.unmarked(&[Block::Paragraph { at, content }])
    }

    /// Lays out footnote content. Its own markers do not count as references.
    fn unmarked(&mut self, blocks: &[Block]) -> Vec<FlowLine> {
        let mut lines = self.run(blocks, Frame::full(&self.config.styles.footnote));
        for line in &mut lines {
            line.line.notes.clear();
        }
        lines
    }

    fn blocks(&mut self, blocks: &[Block], frame: Frame<'a>) {
        for block in blocks {
            match block {
                Block::Paragraph { at, content } => {
                    // Only a paragraph that continues another one gets a first-line indent.
                    let indent = if self.after_paragraph {
                        frame.style.first_line_indent.0
                    } else {
                        0.0
                    };
                    self.text_block(*at, content, frame.style, frame, indent, false);
                }
                Block::Heading { at, level, content } => {
                    let style = heading_style(self.config, *level);
                    self.text_block(*at, content, style, frame, 0.0, true);
                }
                Block::List { at, start, items } => self.list(*at, *start, items, frame),
                Block::Quote { blocks, .. } => {
                    let style = &self.config.styles.quote;
                    self.space(style.space_before.0);
                    let indent = style.indent.0;
                    let inner = Frame {
                        left: frame.left + indent,
                        right: frame.right + indent,
                        style,
                        ..frame
                    };
                    self.after_paragraph = false;
                    self.blocks(blocks, inner);
                    self.space(style.space_after.0);
                }
                Block::Code { first_line, lines, .. } => self.code(*first_line, lines, frame),
                Block::Image { at, image, caption } => {
                    let images = self.images;
                    self.image(*at, &images[*image], caption, frame);
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
                    align,
                    header,
                    rows,
                    caption,
                } => {
                    let block = table::TableBlock {
                        at: *at,
                        align,
                        header,
                        rows,
                        caption,
                    };
                    self.table(block, frame);
                }
                Block::Columns { blocks, .. } => self.columns(blocks, frame),
                Block::FullWidth { .. } => unreachable!("the parser allows full-width only directly inside columns"),
            }
            self.after_paragraph = matches!(block, Block::Paragraph { .. });
        }
    }

    /// Sets a paragraph or heading. Headings never end a page, so they stay with what follows.
    fn text_block(
        &mut self,
        at: Location,
        content: &[Inline],
        style: &Style,
        frame: Frame<'a>,
        first_indent: f64,
        heading: bool,
    ) {
        let width = self.width(frame) - 2.0 * style.indent.0;
        self.space(style.space_before.0);
        let lang = self.config.document.lang;
        match paragraph::lines(
            content,
            style,
            &self.config.inline,
            self.fonts,
            lang,
            width,
            first_indent,
        ) {
            Ok(lines) => {
                let dx = frame.left + style.indent.0;
                let count = lines.len();
                for (index, line) in lines.into_iter().enumerate() {
                    let after = if heading {
                        Break::Never
                    } else {
                        pages::line_break(index, count)
                    };
                    self.push(translate_line(line, dx), at, after);
                }
            }
            Err(problem) => self.errors.push(self.error(at, problem.to_string())),
        }
        self.space(style.space_after.0);
    }

    fn list(&mut self, at: Location, start: Option<u64>, items: &[Vec<Block>], frame: Frame<'a>) {
        let style = &self.config.styles.list;
        let lists = &self.config.lists;
        let inner = Frame {
            left: frame.left + lists.indent.0,
            style,
            list_depth: frame.list_depth + 1,
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
                None => lists.bullets[frame.list_depth.min(lists.bullets.len() - 1)].clone(),
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
        self.space(style.space_after.0);
    }

    /// Sets a column section at the column width. Full-width blocks inside it are set across `frame`
    /// and split the section into runs. Each layout change starts a new paragraph sequence and leaves
    /// at least the column gap above and below the columns.
    fn columns(&mut self, blocks: &[Block], frame: Frame<'a>) {
        let page = &self.config.page;
        let column = Frame {
            right: frame.right + page.text_width().0 - pages::column_width(page),
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
    fn image(&mut self, at: Location, image: &Image, caption: &[Inline], frame: Frame<'a>) {
        let style = &self.config.styles.caption;
        let width = self.width(frame);
        let caption = if caption.is_empty() {
            Vec::new()
        } else {
            self.figures += 1;
            match self.caption_lines(at, &self.config.labels.figure, self.figures, caption, width) {
                Some(lines) => lines,
                None => return,
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
        let line = Line {
            height: size.1,
            baseline: size.1,
            items: vec![Item::Image {
                rect,
                image: image.clone(),
            }],
            notes: Vec::new(),
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
        let lang = self.config.document.lang;
        let measure = width - 2.0 * style.indent.0;
        match paragraph::lines(&content, style, &self.config.inline, self.fonts, lang, measure, 0.0) {
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
        }
    }

    fn push(&mut self, line: Line, at: Location, after: Break) {
        self.lines.push(FlowLine {
            line,
            space_before: std::mem::take(&mut self.space),
            after,
            at,
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
        Item::Link { rect: r, url } => Item::Link { rect: rect(r), url },
        Item::Image { rect: r, image } => Item::Image { rect: rect(r), image },
    }
}

/// The face for inline text in a block style. Emphasis toggles italic; strong sets bold.
fn inline_face(style: &Style, emphasis: bool, strong: bool) -> (Weight, FontStyle) {
    let weight = if strong { Weight::Bold } else { style.weight };
    let italic = (style.style == FontStyle::Italic) != emphasis;
    (weight, if italic { FontStyle::Italic } else { FontStyle::Normal })
}
