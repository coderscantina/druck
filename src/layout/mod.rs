//! Layout: places the document's blocks on pages using resolved styles and shaped text.
//!
//! Blocks become a vertical flow of lines and spaces, which is then cut into pages.
//! Milestone 02 fills lines greedily ([`paragraph`]) and breaks pages at the first line that
//! does not fit ([`paginate`]). Milestones 03 and 04 replace these two steps.

mod paragraph;
#[cfg(test)]
mod tests;

use crate::config::resolved::{Config, Style};
use crate::config::source::Source;
use crate::config::theme::{FontStyle, Weight};
use crate::config::values::{Color, Pt};
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Document, Inline, Location};
use crate::page::{Item, Page, Rect};
use crate::text::{Fonts, ShapedRun};

/// Lays out `document` on pages. Errors name the source location of content that cannot fit.
pub fn layout(
    document: &Document,
    config: &Config,
    fonts: &Fonts,
    source: &Source,
) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let mut flow = Flow {
        config,
        fonts,
        source,
        items: Vec::new(),
        errors: Vec::new(),
    };
    let frame = Frame {
        left: 0.0,
        right: 0.0,
        style: &config.styles.body,
        list_depth: 0,
    };
    flow.blocks(&document.blocks, frame);
    if !flow.errors.is_empty() {
        return Err(flow.errors);
    }
    paginate(flow.items, config)
}

/// A line of content. Item coordinates are relative to the text area's left edge and the line's top.
#[derive(Debug)]
struct Line {
    height: f64,
    baseline: f64,
    items: Vec<Item>,
}

#[derive(Debug)]
enum FlowItem {
    Line(Line),
    /// Vertical space, dropped at the top of a page.
    Space(f64),
}

/// The horizontal extent and paragraph style of the blocks being laid out.
#[derive(Clone, Copy)]
struct Frame<'a> {
    left: f64,
    right: f64,
    style: &'a Style,
    list_depth: usize,
}

struct Flow<'a> {
    config: &'a Config,
    fonts: &'a Fonts,
    source: &'a Source,
    items: Vec<FlowItem>,
    errors: Vec<Diagnostic>,
}

impl<'a> Flow<'a> {
    fn blocks(&mut self, blocks: &[Block], frame: Frame<'a>) {
        let mut after_paragraph = false;
        for block in blocks {
            match block {
                Block::Paragraph { at, content } => {
                    // Only a paragraph that continues another one gets a first-line indent.
                    let indent = if after_paragraph {
                        frame.style.first_line_indent.0
                    } else {
                        0.0
                    };
                    self.text_block(*at, content, frame.style, frame, indent);
                }
                Block::Heading { at, level, content } => {
                    self.text_block(*at, content, heading_style(self.config, *level), frame, 0.0);
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
                    self.blocks(blocks, inner);
                    self.space(style.space_after.0);
                }
                Block::Code { first_line, lines, .. } => self.code(*first_line, lines, frame),
            }
            after_paragraph = matches!(block, Block::Paragraph { .. });
        }
    }

    fn text_block(&mut self, at: Location, content: &[Inline], style: &Style, frame: Frame<'a>, first_indent: f64) {
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
                self.items
                    .extend(lines.into_iter().map(|line| FlowItem::Line(translate_line(line, dx))));
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
            let first = self.items.len();
            self.blocks(item, inner);
            let line = match self.items[first..].iter_mut().find_map(|item| match item {
                FlowItem::Line(line) => Some(line),
                FlowItem::Space(_) => None,
            }) {
                Some(line) => line,
                None => {
                    self.items.push(FlowItem::Line(self.empty_line(style)));
                    let Some(FlowItem::Line(line)) = self.items.last_mut() else {
                        unreachable!()
                    };
                    line
                }
            };
            let y = line.baseline;
            line.items.push(text_item(x, y, run, style.color));
        }
        self.space(style.space_after.0);
    }

    /// Code lines are set as they are. A line wider than the available width is an error.
    fn code(&mut self, first_line: u64, lines: &[String], frame: Frame<'a>) {
        let style = &self.config.styles.code_block;
        let face = self.fonts.face(&style.font, style.weight, style.style);
        let width = self.width(frame) - 2.0 * style.indent.0;
        let x = frame.left + style.indent.0;
        self.space(style.space_before.0);
        for (index, text) in lines.iter().enumerate() {
            let at = Location {
                line: first_line + index as u64,
                column: 1,
            };
            let mut line = self.empty_line(style);
            let text = text.replace('\t', "    ");
            if !text.trim().is_empty() {
                let run = self.fonts.shape(&text, face, style.size, self.config.document.lang);
                if let Some(problem) = paragraph::missing_glyph(&run) {
                    self.errors.push(self.error(at, problem.to_string()));
                }
                if run.width.0 > width {
                    let message = format!(
                        "code line is {:.1}pt wider than the available {:.1}pt; code is never wrapped, so shorten the line",
                        run.width.0 - width,
                        width
                    );
                    self.errors.push(self.error(at, message));
                }
                line.items.push(text_item(x, line.baseline, run, style.color));
            }
            self.items.push(FlowItem::Line(line));
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
        }
    }

    /// Adds vertical space. Adjacent spaces collapse to the larger one.
    fn space(&mut self, amount: f64) {
        match self.items.last_mut() {
            Some(FlowItem::Space(space)) => *space = space.max(amount),
            _ => self.items.push(FlowItem::Space(amount)),
        }
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

/// Cuts the flow into pages, starting a page when the next line does not fit.
/// Odd pages have the inner margin on the left.
fn paginate(flow: Vec<FlowItem>, config: &Config) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let geometry = &config.page;
    let text_height = geometry.text_height().0;
    let new_page = || Page {
        width: geometry.width,
        height: geometry.height,
        items: Vec::new(),
    };
    let mut pages = Vec::new();
    let mut page = new_page();
    let mut y = 0.0;
    for item in flow {
        match item {
            FlowItem::Space(space) if !page.items.is_empty() => y += space,
            FlowItem::Space(_) => {}
            FlowItem::Line(line) => {
                if line.height > text_height {
                    let message = format!(
                        "a line is {:.1}pt high, taller than the {:.1}pt text area",
                        line.height, text_height
                    );
                    return Err(vec![Diagnostic::new(None, message)]);
                }
                if y + line.height > text_height && !page.items.is_empty() {
                    pages.push(std::mem::replace(&mut page, new_page()));
                    y = 0.0;
                }
                let left = if pages.len() % 2 == 0 {
                    geometry.margin_inner.0
                } else {
                    geometry.margin_outer.0
                };
                let top = geometry.margin_top.0 + y;
                page.items
                    .extend(line.items.into_iter().map(|item| translate(item, left, top)));
                y += line.height;
            }
        }
    }
    pages.push(page);
    Ok(pages)
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
    }
}

/// The face for inline text in a block style. Emphasis toggles italic; strong sets bold.
fn inline_face(style: &Style, emphasis: bool, strong: bool) -> (Weight, FontStyle) {
    let weight = if strong { Weight::Bold } else { style.weight };
    let italic = (style.style == FontStyle::Italic) != emphasis;
    (weight, if italic { FontStyle::Italic } else { FontStyle::Normal })
}
