//! A block shaped for the galley, and the geometry questions asked of it. Only blocks the view asks for get shaped.

use std::ops::Range;

use gpui::{
    Font, FontStyle, FontWeight, Hsla, Pixels, SharedString, TextRun, UnderlineStyle, Window, WrappedLine, font, px,
    rgb,
};

use super::display::{DisplayLine, RunStyle};
use crate::source::BlockKind;

/// The galley's look. Fixed values until the theme drives them: the sheet shows document styling, not app chrome.
pub struct GalleyStyle {
    pub body: Font,
    pub mono: Font,
    pub body_size: Pixels,
    pub leading: f32,
    pub ink: Hsla,
    pub quiet: Hsla,
    pub link: Hsla,
    pub rule: Hsla,
    pub code_background: Hsla,
}

impl GalleyStyle {
    pub fn new() -> Self {
        Self {
            body: font("Georgia"),
            mono: font("Menlo"),
            body_size: px(15.),
            leading: 1.45,
            ink: rgb(0x1a1a18).into(),
            quiet: rgb(0x8d8a84).into(),
            link: rgb(0x2f5d8a).into(),
            rule: rgb(0xc9c6c0).into(),
            code_background: rgb(0xf0eee9).into(),
        }
    }

    /// The width of a word's worth of body text per character, for estimating blocks not yet shaped.
    fn estimated_char_width(&self) -> Pixels {
        self.body_size * 0.47
    }

    fn blank_gap(&self) -> Pixels {
        self.body_size * 0.7
    }

    fn line_style(&self, kind: BlockKind) -> LineStyle {
        let body = LineStyle {
            font: self.body.clone(),
            size: self.body_size,
            leading: self.leading,
            color: self.ink,
        };
        let mono = LineStyle {
            font: self.mono.clone(),
            size: self.body_size * 0.8,
            leading: 1.4,
            color: self.ink,
        };
        let quiet_mono = LineStyle {
            color: self.quiet,
            ..mono.clone()
        };
        match kind {
            BlockKind::Heading { level } => LineStyle {
                font: Font {
                    weight: FontWeight::BOLD,
                    ..self.body.clone()
                },
                size: self.body_size
                    * match level {
                        1 => 1.7,
                        2 => 1.35,
                        3 => 1.15,
                        _ => 1.0,
                    },
                leading: 1.25,
                color: self.ink,
            },
            BlockKind::FootnoteDefinition => LineStyle {
                size: self.body_size * 0.87,
                ..body
            },
            BlockKind::Paragraph | BlockKind::List | BlockKind::Quote | BlockKind::Blank => body,
            BlockKind::Code | BlockKind::Table => mono,
            BlockKind::FrontMatter
            | BlockKind::Directive
            | BlockKind::Comment
            | BlockKind::SceneBreak
            | BlockKind::Raw => quiet_mono,
        }
    }

    fn run(&self, base: &LineStyle, len: usize, style: RunStyle) -> TextRun {
        let family = if style.code { &self.mono } else { &base.font };
        let font = Font {
            weight: if style.bold { FontWeight::BOLD } else { base.font.weight },
            style: if style.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            ..family.clone()
        };
        let color = if style.quiet {
            self.quiet
        } else if style.link {
            self.link
        } else {
            base.color
        };
        TextRun {
            len,
            font,
            color,
            background_color: style.code.then_some(self.code_background),
            underline: (style.marked || style.link).then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: None,
        }
    }

    /// A height for a block not yet shaped. Close enough for scrolling, corrected when it comes into view.
    pub fn estimate_height(&self, kind: BlockKind, text: &str, width: Pixels) -> Pixels {
        if kind == BlockKind::Blank {
            return self
                .blank_gap()
                .max(self.body_size * self.leading * (text.matches('\n').count() as f32 - 1.));
        }
        let style = self.line_style(kind);
        let scale = style.size / self.body_size;
        let per_line = (width / (self.estimated_char_width() * scale)).max(1.);
        let rows: f32 = text
            .lines()
            .map(|line| (line.chars().count() as f32 / per_line).ceil().max(1.))
            .sum();
        style.size * style.leading * rows
    }
}

#[derive(Clone)]
struct LineStyle {
    font: Font,
    size: Pixels,
    leading: f32,
    color: Hsla,
}

/// One source line, shaped and wrapped.
pub struct ShapedSourceLine {
    pub display: DisplayLine,
    pub shaped: WrappedLine,
    pub rows: Vec<Range<usize>>,
    /// Distance from the block's top.
    pub y: Pixels,
    /// Distance from the text column's left edge.
    pub x: Pixels,
    pub line_height: Pixels,
    pub height: Pixels,
    /// A line the caret passes over: the blank between two blocks, or a hidden underline.
    pub skipped: bool,
    pub bullet: Option<(SharedString, Pixels)>,
}

impl ShapedSourceLine {
    /// The row showing a displayed offset. At a wrap, `upstream` means the end of the upper row.
    pub fn row_of(&self, display: usize, upstream: bool) -> usize {
        let row = self
            .rows
            .iter()
            .position(|row| display < row.end)
            .unwrap_or(self.rows.len() - 1);
        if upstream && row > 0 && display == self.rows[row].start {
            row - 1
        } else {
            row
        }
    }

    /// The distance of a displayed offset from the start of its row.
    pub fn x_in_row(&self, display: usize, row: usize) -> Pixels {
        let layout = &self.shaped.unwrapped_layout;
        layout.x_for_index(display) - layout.x_for_index(self.rows[row].start)
    }

    /// The displayed offset closest to `x` from the start of a row.
    pub fn display_at_x(&self, row: usize, x: Pixels) -> usize {
        let range = &self.rows[row];
        let layout = &self.shaped.unwrapped_layout;
        let start_x = layout.x_for_index(range.start);
        layout.closest_index_for_x(start_x + x).clamp(range.start, range.end)
    }

    pub fn is_last_row(&self, row: usize) -> bool {
        row + 1 == self.rows.len()
    }
}

pub struct BlockLayout {
    pub lines: Vec<ShapedSourceLine>,
    pub height: Pixels,
    /// What this was shaped for. A change to either means shaping again.
    pub hidden: Vec<Range<usize>>,
    pub marked: Option<Range<usize>>,
    pub width: Pixels,
}

/// The arrangement of one block's lines. Whether each is a skipped separator comes from the caller, which has the
/// text around it.
pub struct ShapeRequest<'a> {
    pub kind: BlockKind,
    pub lines: Vec<DisplayLine>,
    pub separator: bool,
    pub hidden: Vec<Range<usize>>,
    pub marked: Option<Range<usize>>,
    pub width: Pixels,
    pub style: &'a GalleyStyle,
}

pub fn shape_block(request: ShapeRequest<'_>, window: &Window) -> BlockLayout {
    let ShapeRequest {
        kind,
        lines,
        separator,
        hidden,
        marked,
        width,
        style,
    } = request;
    let base = style.line_style(kind);
    let line_height = base.size * base.leading;
    let hang = style.body_size * 1.25;
    let unit = hang * 0.5;
    let mut shaped_lines = Vec::with_capacity(lines.len());
    let mut y = px(0.);
    for display in lines {
        let quote_indent = if kind == BlockKind::Quote { hang } else { px(0.) };
        let x = match kind {
            BlockKind::List => unit * display.nesting as f32 + if display.bullet.is_some() { hang } else { px(0.) },
            _ => quote_indent,
        };
        let wrap = (width - x).max(px(40.));
        let mut runs: Vec<TextRun> = display
            .runs
            .iter()
            .map(|(len, run)| style.run(&base, *len, *run))
            .collect();
        if runs.is_empty() {
            runs.push(style.run(&base, 0, RunStyle::default()));
        }
        let text: SharedString = display.text.clone().into();
        let shaped = window
            .text_system()
            .shape_text(text, base.size, &runs, Some(wrap), None)
            .ok()
            .and_then(|mut lines| lines.pop())
            .unwrap_or_default();
        let rows = rows_of(&shaped);
        let skipped = separator || display.is_collapsed();
        let row_count = rows.len() as f32;
        let height = if display.is_collapsed() {
            px(0.)
        } else if separator {
            style.blank_gap()
        } else {
            line_height * row_count
        };
        let bullet = display
            .bullet
            .clone()
            .map(|text| (SharedString::from(text), unit * display.nesting as f32));
        shaped_lines.push(ShapedSourceLine {
            display,
            shaped,
            rows,
            y,
            x,
            line_height: if separator { style.blank_gap() } else { line_height },
            height,
            skipped,
            bullet,
        });
        y += height;
    }
    BlockLayout {
        lines: shaped_lines,
        height: y,
        hidden,
        marked,
        width,
    }
}

fn rows_of(shaped: &WrappedLine) -> Vec<Range<usize>> {
    let layout = &shaped.unwrapped_layout;
    let mut rows = Vec::with_capacity(shaped.wrap_boundaries.len() + 1);
    let mut start = 0;
    for boundary in &shaped.wrap_boundaries {
        let end = layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index;
        rows.push(start..end);
        start = end;
    }
    rows.push(start..shaped.len());
    rows
}
