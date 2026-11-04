//! A block shaped for the galley, and the geometry questions asked of it. Only blocks the view asks for get shaped.

use std::ops::Range;

use std::sync::Arc;

use gpui::{
    Bounds, Font, FontStyle, FontWeight, Hsla, LineLayout, Pixels, ShapedLine, SharedString, StrikethroughStyle,
    TextRun, UnderlineStyle, Window, WrapBoundary, WrappedLine, WrappedLineLayout, font, point, px, rgb, size,
};

use super::composed::{ComposedBlock, Forced};
use super::display::{Chip, ChipLook, DisplayLine, RunStyle};
use super::rich::view::shapes::Whole;
use crate::source::BlockKind;

/// Logical pixels per point of the theme's sizes. The galley is for reading on screen, larger than the page.
pub(super) const POINT: f32 = 1.4;

/// The galley's look. The body comes from the theme: the sheet shows document styling, not app chrome.
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
    /// What the book cannot resolve.
    pub error: Hsla,
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
            error: rgb(0xb5442c).into(),
        }
    }

    /// A line of the source view: the same monospace throughout, with color and weight telling blocks apart.
    fn source_line_style(&self, kind: BlockKind) -> LineStyle {
        let (weight, color) = match kind {
            BlockKind::Heading { .. } => (FontWeight::BOLD, self.ink),
            BlockKind::Code => (FontWeight::NORMAL, self.link),
            BlockKind::FrontMatter
            | BlockKind::Directive
            | BlockKind::Comment
            | BlockKind::SceneBreak
            | BlockKind::Raw => (FontWeight::NORMAL, self.quiet),
            _ => (FontWeight::NORMAL, self.ink),
        };
        LineStyle {
            font: Font {
                weight,
                ..self.mono.clone()
            },
            size: self.body_size * 0.85,
            leading: 1.5,
            color,
        }
    }

    /// Takes the theme's body font, its size in points, and its line height.
    pub fn set_body(&mut self, family: &str, size: f32, line_height: f32) {
        self.body = font(SharedString::from(family.to_owned()));
        self.body_size = px(size * POINT);
        self.leading = line_height;
    }

    /// The width of a word's worth of body text per character, for estimating blocks not yet shaped.
    fn estimated_char_width(&self) -> Pixels {
        self.body_size * 0.47
    }

    fn blank_gap(&self) -> Pixels {
        self.body_size * 0.7
    }

    pub(super) fn line_style(&self, kind: BlockKind) -> LineStyle {
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
        let color = if let Some(look) = style.chip {
            match look {
                ChipLook::Mark | ChipLook::Label => self.link,
                ChipLook::Error => self.error,
            }
        } else if style.quiet {
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
            underline: (style.marked || style.link || style.underline).then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: style.strike.then_some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            }),
        }
    }

    /// A height for a block not yet shaped. Close enough for scrolling, corrected when it comes into view.
    pub fn estimate_height(&self, kind: BlockKind, text: &str, width: Pixels, source: bool) -> Pixels {
        if kind == BlockKind::Blank && !source {
            return self
                .blank_gap()
                .max(self.body_size * self.leading * (text.matches('\n').count() as f32 - 1.));
        }
        let style = if source {
            self.source_line_style(kind)
        } else {
            self.line_style(kind)
        };
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
pub(super) struct LineStyle {
    pub font: Font,
    pub size: Pixels,
    pub leading: f32,
    pub color: Hsla,
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
    /// Behind each chip on the line, from the block's top and the column's left edge.
    pub chips: Vec<(Bounds<Pixels>, ChipLook)>,
    /// How far in the first row starts, with the engine's lines.
    pub indent: Pixels,
    /// The rows that end in a hyphen the engine added, with the hyphen to paint.
    pub hyphens: Option<(Vec<usize>, ShapedLine)>,
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
        layout.x_for_index(display) - layout.x_for_index(self.rows[row].start) + self.row_left(row)
    }

    /// How far in a row starts.
    fn row_left(&self, row: usize) -> Pixels {
        if row == 0 { self.indent } else { px(0.) }
    }

    /// The displayed offset closest to `x` from the start of a row.
    pub fn display_at_x(&self, row: usize, x: Pixels) -> usize {
        let range = &self.rows[row];
        let layout = &self.shaped.unwrapped_layout;
        let start_x = layout.x_for_index(range.start);
        let x = (x - self.row_left(row)).max(px(0.));
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
    /// The whole block takes no room.
    pub collapsed: bool,
    /// The chips it was shaped with.
    pub chips: Vec<Chip>,
    /// A table or figure drawn in place of its lines.
    pub whole: Option<Box<Whole>>,
    /// The engine's lines it was shaped with.
    pub composed: Option<Arc<ComposedBlock>>,
}

impl BlockLayout {
    /// The height of the block's first line that shows, for what stands beside it in the gutter.
    pub fn first_line_height(&self) -> Pixels {
        if self.whole.is_some() {
            return px(20.);
        }
        self.lines
            .iter()
            .find(|line| line.height > px(0.))
            .map_or(px(0.), |line| line.line_height)
    }
}

/// The arrangement of one block's lines. Whether each is a skipped separator comes from the caller, which has the
/// text around it.
pub struct ShapeRequest<'a> {
    pub kind: BlockKind,
    pub lines: Vec<DisplayLine>,
    pub separator: bool,
    /// The whole block takes no room, as a hidden comment does.
    pub collapsed: bool,
    /// The source view: every line in one monospace, at the column's left edge.
    pub source: bool,
    pub hidden: Vec<Range<usize>>,
    pub marked: Option<Range<usize>>,
    pub width: Pixels,
    pub style: &'a GalleyStyle,
    pub chips: Vec<Chip>,
    pub whole: Option<Box<Whole>>,
    pub composed: Option<Arc<ComposedBlock>>,
}

pub fn shape_block(request: ShapeRequest<'_>, window: &Window) -> BlockLayout {
    let ShapeRequest {
        kind,
        lines,
        separator,
        collapsed,
        source,
        hidden,
        marked,
        width,
        style,
        chips,
        whole,
        composed,
    } = request;
    let base = if source {
        style.source_line_style(kind)
    } else {
        style.line_style(kind)
    };
    let line_height = base.size * base.leading;
    let hang = style.body_size * 1.25;
    let unit = hang * 0.5;
    let mut shaped_lines = Vec::with_capacity(lines.len());
    let mut y = whole.as_deref().map_or(px(0.), Whole::height);
    for display in lines {
        let quote_indent = if kind == BlockKind::Quote { hang } else { px(0.) };
        let x = match kind {
            _ if source => px(0.),
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
        let shape = |wrap_at| {
            window
                .text_system()
                .shape_text(text.clone(), base.size, &runs, wrap_at, None)
                .ok()
                .and_then(|mut lines| lines.pop())
                .unwrap_or_default()
        };
        let forced = composed.as_ref().map(|composed| composed.breaks(&display));
        let mut shaped = if composed.is_some() {
            shape(None)
        } else {
            shape(Some(wrap))
        };
        // A paragraph in a custom style has the engine's measure for another size than the galley shows.
        let forced = forced.filter(|forced| {
            let fits = rows_fit(&shaped, forced, wrap);
            if !fits {
                shaped = shape(Some(wrap));
            }
            fits
        });
        let mut indent = px(0.);
        let mut hyphens = None;
        if let (Some(composed), Some(forced)) = (&composed, forced.as_ref()) {
            let starts_block = display.source.start == 0 || display.text.is_empty();
            indent = if starts_block {
                px(composed.indent * POINT)
            } else {
                px(0.)
            };
            let hyphen = forced
                .iter()
                .any(|row| row.hyphen)
                .then(|| hyphen_line(&base, style, window));
            let hyphen_width = hyphen.as_ref().map_or(px(0.), |line| line.width);
            force_rows(&mut shaped, forced, indent, wrap, hyphen_width);
            hyphens = hyphen.map(|line| {
                let rows = (forced.iter().enumerate())
                    .filter(|(_, row)| row.hyphen)
                    .map(|(index, _)| index)
                    .collect();
                (rows, line)
            });
        }
        let rows = rows_of(&shaped);
        let hidden_line = collapsed || display.is_collapsed();
        let skipped = separator || hidden_line;
        let row_count = rows.len() as f32;
        let height = if hidden_line {
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
        let chip_boxes = chip_boxes(&display, &shaped, &rows, x, y, line_height, indent);
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
            chips: chip_boxes,
            indent,
            hyphens,
        });
        y += height;
    }
    BlockLayout {
        lines: shaped_lines,
        height: y,
        hidden,
        marked,
        width,
        collapsed,
        chips,
        whole,
        composed,
    }
}

/// The hyphen the engine added at the end of a row, which is painted and is not in the text.
fn hyphen_line(base: &LineStyle, style: &GalleyStyle, window: &Window) -> ShapedLine {
    let run = style.run(base, 1, RunStyle::default());
    window.text_system().shape_line("-".into(), base.size, &[run], None)
}

/// Breaks a shaped line where the engine did, and sets each row's left edge and spaces: the first row starts `indent`
/// in, and one that reaches the measure in the engine's lines reaches `width` here, whatever the toolkit's glyphs measure.
/// Positions stay rising from row to row, as the caret's lookups expect, and rows are measured from their first glyph.
fn force_rows(shaped: &mut WrappedLine, forced: &[Forced], indent: Pixels, width: Pixels, hyphen: Pixels) {
    let text = shaped.text.clone();
    let line = &shaped.unwrapped_layout;
    let mut runs = line.runs.clone();
    let mut boundaries = Vec::new();
    let mut row = 0;
    let mut shift = indent;
    let mut step = row_step(line, &text, forced, row, indent, width, hyphen);
    let mut spaces = px(0.);
    for (run_ix, run) in runs.iter_mut().enumerate() {
        for (glyph_ix, glyph) in run.glyphs.iter_mut().enumerate() {
            if forced.get(row).is_some_and(|next| glyph.index >= next.at) {
                row += 1;
                shift += spaces;
                spaces = px(0.);
                step = row_step(line, &text, forced, row, px(0.), width, hyphen);
                boundaries.push(WrapBoundary { run_ix, glyph_ix });
            }
            glyph.position.x += shift + spaces;
            if text.as_bytes().get(glyph.index) == Some(&b' ') {
                spaces += step;
            }
        }
    }
    let layout = WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            font_size: line.font_size,
            width: line.width + shift + spaces,
            ascent: line.ascent,
            descent: line.descent,
            runs,
            len: line.len,
        }),
        wrap_boundaries: boundaries.into_iter().collect(),
        wrap_width: None,
    };
    **shaped = Arc::new(layout);
}

/// Whether the rows between `forced` breaks hold in `width`, give or take what a line's spaces can give.
fn rows_fit(shaped: &WrappedLine, forced: &[Forced], width: Pixels) -> bool {
    let line = &shaped.unwrapped_layout;
    let mut start = 0;
    for end in forced.iter().map(|row| row.at).chain([line.len]) {
        let natural = line.x_for_index(end) - line.x_for_index(start);
        if natural > width * (1. + OVERFLOW) {
            return false;
        }
        start = end;
    }
    true
}

/// How much wider than the column a row can be before the engine's breaks are taken to be for another size.
const OVERFLOW: f32 = 0.10;

/// What each space of a row gains so that it reaches the measure, or nothing if it need not.
fn row_step(
    line: &LineLayout,
    text: &str,
    forced: &[Forced],
    row: usize,
    left: Pixels,
    width: Pixels,
    hyphen: Pixels,
) -> Pixels {
    let Some(end) = forced.get(row).filter(|end| end.fills) else {
        return px(0.);
    };
    let start = if row == 0 { 0 } else { forced[row - 1].at };
    let shown = text[start..end.at].trim_end_matches(' ');
    let spaces = shown.bytes().filter(|byte| *byte == b' ').count();
    if spaces == 0 {
        return px(0.);
    }
    let natural = line.x_for_index(start + shown.len()) - line.x_for_index(start);
    let room = width - left - if end.hyphen { hyphen } else { px(0.) };
    let step = (room - natural) / spaces as f32;
    // A very different toolkit font should not close the spaces or tear them open.
    step.clamp(line.font_size * -0.12, line.font_size * 0.5)
}

/// How far a chip's box stands off its text, and how much of the line it leaves free above and below.
const CHIP_PAD: Pixels = px(2.);
const CHIP_INSET: f32 = 0.08;

/// The boxes behind a line's chips. A chip that wraps has one per row.
fn chip_boxes(
    display: &DisplayLine,
    shaped: &WrappedLine,
    rows: &[Range<usize>],
    x: Pixels,
    y: Pixels,
    line_height: Pixels,
    indent: Pixels,
) -> Vec<(Bounds<Pixels>, ChipLook)> {
    let layout = &shaped.unwrapped_layout;
    let mut boxes = Vec::new();
    for (range, look) in &display.chips {
        for (row, shown) in rows.iter().enumerate() {
            let (start, end) = (range.start.max(shown.start), range.end.min(shown.end));
            if start >= end {
                continue;
            }
            let row_start = layout.x_for_index(shown.start);
            let row_left = if row == 0 { indent } else { px(0.) };
            let left = x + row_left + layout.x_for_index(start) - row_start - CHIP_PAD;
            let right = x + row_left + layout.x_for_index(end) - row_start + CHIP_PAD;
            let inset = line_height * CHIP_INSET;
            boxes.push((
                Bounds::new(
                    point(left, y + line_height * row as f32 + inset),
                    size(right - left, line_height - inset * 2.),
                ),
                *look,
            ));
        }
    }
    boxes
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

#[cfg(test)]
mod tests {
    use gpui::{FontId, GlyphId, ShapedGlyph, ShapedRun};

    use super::*;
    use crate::editor::display::{Syntax, display_lines};

    /// A line shaped by hand: every byte is a glyph 10 px wide.
    fn shaped(text: &str) -> WrappedLine {
        let glyphs = (0..text.len())
            .map(|index| ShapedGlyph {
                id: GlyphId(0),
                position: point(px(index as f32 * 10.), px(0.)),
                index,
                is_emoji: false,
            })
            .collect();
        let layout = LineLayout {
            font_size: px(20.),
            width: px(text.len() as f32 * 10.),
            runs: vec![ShapedRun {
                font_id: FontId(0),
                glyphs,
            }],
            len: text.len(),
            ..LineLayout::default()
        };
        let mut wrapped = WrappedLine::default();
        wrapped.text = text.to_owned().into();
        *wrapped = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(layout),
            wrap_boundaries: Default::default(),
            wrap_width: None,
        });
        wrapped
    }

    fn source_line(text: &str, forced: &[Forced], indent: Pixels, width: Pixels, hyphen: Pixels) -> ShapedSourceLine {
        let mut line = shaped(text);
        force_rows(&mut line, forced, indent, width, hyphen);
        let display = display_lines(BlockKind::Paragraph, text, &Syntax::default(), &[], None, false, &[]).remove(0);
        ShapedSourceLine {
            rows: rows_of(&line),
            display,
            shaped: line,
            y: px(0.),
            x: px(0.),
            line_height: px(20.),
            height: px(40.),
            skipped: false,
            bullet: None,
            chips: Vec::new(),
            indent,
            hyphens: None,
        }
    }

    const CUT: [Forced; 1] = [Forced {
        at: 9,
        hyphen: true,
        fills: true,
    }];

    #[test]
    fn a_row_ends_where_the_engine_ended_it_and_the_caret_keeps_its_place_on_both_sides() {
        let line = source_line("aaa bbbb cccc", &CUT, px(30.), px(200.), px(10.));
        assert_eq!(line.rows, [0..9, 9..13]);
        // At the cut, the upper row's end and the lower row's start are one offset.
        assert_eq!(line.row_of(9, false), 1);
        assert_eq!(line.row_of(9, true), 0);
        assert_eq!(line.x_in_row(9, 1), px(0.));
        for (at, row) in [(0, 0), (2, 0), (9, 0), (9, 1), (11, 1)] {
            assert_eq!(
                line.display_at_x(row, line.x_in_row(at, row)),
                at,
                "offset {at} in row {row}"
            );
        }
    }

    #[test]
    fn the_first_row_starts_at_its_indent_and_a_row_that_fills_reaches_the_measure() {
        let line = source_line("aaa bbbb cccc", &CUT, px(30.), px(130.), px(10.));
        assert_eq!(line.x_in_row(0, 0), px(30.));
        // "aaa bbbb" is 80 px with one space. It has 130 px, less the indent and the hyphen's 10, so the space gains 10.
        assert_eq!(line.x_in_row(8, 0), px(120.));
    }

    #[test]
    fn positions_rise_from_row_to_row_so_lookups_stay_ordered() {
        let line = source_line("aaa bbbb cccc", &CUT, px(30.), px(200.), px(10.));
        let glyphs: Vec<_> = line.shaped.unwrapped_layout.runs[0]
            .glyphs
            .iter()
            .map(|glyph| glyph.position.x)
            .collect();
        assert!(glyphs.windows(2).all(|pair| pair[0] < pair[1]), "{glyphs:?}");
    }

    #[test]
    fn a_row_too_wide_for_the_column_is_not_forced() {
        let line = shaped("aaa bbbb cccc");
        assert!(rows_fit(&line, &CUT, px(90.)));
        assert!(!rows_fit(&line, &CUT, px(70.)));
    }
}
