//! A block shaped for the text, and the geometry questions asked of it. Only blocks the view asks for get shaped.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    Bounds, Font, FontStyle, FontWeight, Hsla, LineLayout, Pixels, SharedString, StrikethroughStyle, TextAlign,
    TextRun, UnderlineStyle, Window, WrapBoundary, WrappedLine, WrappedLineLayout, font, point, px, size,
};

use super::display::{Chip, ChipLook, DisplayLine, RunStyle};
use super::rich::view::shapes::Whole;
use crate::preview::job::{BookType, Face};
use crate::source::BlockKind;
use crate::tokens::{self, Tokens};

/// How the text looks: the theme's type on the app's surface, with colours following the appearance. There is no
/// justification and no hyphenation, on purpose: Publish shows the real lines.
pub struct GalleyStyle {
    /// What the theme says. A stand-in until the first layout has read it.
    book: BookType,
    zoom: f32,
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
    /// Behind words an agent took out.
    pub removed: Hsla,
}

fn font_of(face: &Face) -> Font {
    Font {
        weight: FontWeight(f32::from(face.weight)),
        style: if face.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..font(SharedString::from(face.family.clone()))
    }
}

impl GalleyStyle {
    pub fn new(tokens: &Tokens) -> Self {
        let book = tokens::stand_in_type();
        let mut style = Self {
            zoom: 1.,
            body: font_of(&book.body),
            mono: font_of(&book.code),
            body_size: px(book.body.size * tokens::POINT),
            leading: book.body.line_height,
            book,
            ink: tokens.text.into(),
            quiet: tokens.text_quiet.into(),
            link: tokens.link.into(),
            rule: tokens.divider.into(),
            code_background: tokens.raised.into(),
            error: tokens.error.into(),
            removed: tokens.removed.into(),
        };
        style.set_colors(tokens);
        style
    }

    /// Takes the colours of the appearance.
    pub fn set_colors(&mut self, tokens: &Tokens) {
        self.ink = tokens.text.into();
        self.quiet = tokens.text_quiet.into();
        self.link = tokens.link.into();
        self.rule = tokens.divider.into();
        self.code_background = tokens.raised.into();
        self.error = tokens.error.into();
    }

    /// Takes the theme's type.
    pub fn set_type(&mut self, book: BookType) {
        self.body = font_of(&book.body);
        self.mono = font_of(&book.code);
        self.leading = book.body.line_height;
        self.book = book;
        self.body_size = px(self.book.body.size * self.scale());
    }

    /// Scales the text, which keeps the characters per line.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
        self.body_size = px(self.book.body.size * self.scale());
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Logical pixels per point of the theme's sizes.
    pub fn scale(&self) -> f32 {
        tokens::POINT * self.zoom
    }

    /// The width of the book's prose on the page, at this zoom.
    pub fn book_measure(&self) -> Pixels {
        px(self.book.measure * self.scale())
    }

    /// The room between two columns, as the theme sets it, at this zoom.
    pub fn column_gap(&self) -> Pixels {
        px(self.book.column_gap * self.scale())
    }

    /// The width of about this many characters of body text.
    pub fn width_of_characters(&self, count: u32) -> Pixels {
        self.estimated_char_width() * count as f32
    }

    /// The theme's style for a block of this kind, where it has one.
    fn face(&self, kind: BlockKind) -> &Face {
        match kind {
            BlockKind::Heading { level } => &self.book.headings[usize::from(level.clamp(1, 6)) - 1],
            BlockKind::Quote => &self.book.quote,
            BlockKind::List => &self.book.list,
            BlockKind::FootnoteDefinition => &self.book.footnote,
            BlockKind::Code | BlockKind::Table => &self.book.code,
            _ => &self.book.body,
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
                ..font(tokens::FONT_MONO)
            },
            size: self.body_size * tokens::SOURCE_SCALE,
            leading: tokens::SOURCE_LEADING,
            color,
        }
    }

    /// The width of a word's worth of body text per character, for estimating blocks not yet shaped.
    fn estimated_char_width(&self) -> Pixels {
        self.body_size * tokens::CHAR_WIDTH
    }

    /// The room between two blocks: the larger of what the upper leaves below it and the lower wants above it. Paragraphs
    /// that neither space nor indent would run together, so they keep a little.
    pub fn gap_between(&self, upper: Option<BlockKind>, lower: Option<BlockKind>) -> Pixels {
        let after = upper.map_or(0., |kind| self.face(kind).space_after);
        let before = lower.map_or(0., |kind| self.face(kind).space_before);
        let gap = after.max(before);
        let runs_together = gap == 0. && self.indent_of(lower) == px(0.);
        if runs_together {
            self.body_size * tokens::PARAGRAPH_GAP
        } else {
            px(gap * self.scale())
        }
    }

    /// How far in a paragraph's first line starts when another paragraph comes before it.
    pub fn indent_of(&self, kind: Option<BlockKind>) -> Pixels {
        if kind == Some(BlockKind::Paragraph) {
            px(self.book.body.first_line_indent * self.scale())
        } else {
            px(0.)
        }
    }

    pub(super) fn line_style(&self, kind: BlockKind) -> LineStyle {
        let face = self.face(kind);
        let themed = LineStyle {
            font: font_of(face),
            size: px(face.size * self.scale()),
            leading: face.line_height,
            color: self.ink,
        };
        match kind {
            BlockKind::FrontMatter
            | BlockKind::Directive
            | BlockKind::Comment
            | BlockKind::SceneBreak
            | BlockKind::Raw => LineStyle {
                font: self.mono.clone(),
                size: px(self.book.code.size * self.scale()),
                leading: self.book.code.line_height,
                color: self.quiet,
            },
            _ => themed,
        }
    }

    fn run(&self, base: &LineStyle, len: usize, style: RunStyle, dim: bool) -> TextRun {
        let family = if style.code { &self.mono } else { &base.font };
        let font = Font {
            weight: if style.bold { FontWeight::BOLD } else { base.font.weight },
            style: if style.italic {
                FontStyle::Italic
            } else {
                base.font.style
            },
            ..family.clone()
        };
        let color = if let Some(look) = style.chip {
            match look {
                ChipLook::Mark | ChipLook::Label => self.link,
                ChipLook::Error => self.error,
                ChipLook::Removed => base.color,
            }
        } else if style.quiet {
            self.quiet
        } else if style.link {
            self.link
        } else {
            base.color
        };
        let color = if dim { color.opacity(tokens::DIM_OPACITY) } else { color };
        TextRun {
            len,
            font,
            color,
            background_color: style.code.then_some(self.code_background),
            underline: (style.marked || style.link || style.underline).then_some(UnderlineStyle {
                thickness: tokens::HAIRLINE,
                color: Some(color),
                wavy: false,
            }),
            strikethrough: (style.strike || style.chip == Some(ChipLook::Removed)).then_some(StrikethroughStyle {
                thickness: tokens::HAIRLINE,
                color: Some(color),
            }),
        }
    }

    /// A height for a block not yet shaped. Close enough for scrolling, corrected when it comes into view.
    pub fn estimate_height(&self, kind: BlockKind, text: &str, width: Pixels, source: bool) -> Pixels {
        if kind == BlockKind::Blank && !source {
            return (self.body_size * tokens::PARAGRAPH_GAP)
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

/// Where the text stands in the pane: centred at its measure, keeping room for the gutter on its left. Margin notes stand
/// at its right, and the text moves left for them only if the window is too narrow to hold both. The text keeps its measure
/// before the notes keep their place: below measure plus margin they do not show, and the gutter's marker stands for them.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Column {
    pub viewport: Bounds<Pixels>,
    pub text_left: Pixels,
    pub text_width: Pixels,
    /// Where margin notes stand, when there are any and they fit.
    pub note_left: Option<Pixels>,
}

impl Column {
    pub fn place(viewport: Bounds<Pixels>, measure: Pixels, notes: bool) -> Self {
        let left = viewport.left() + tokens::GUTTER_WIDTH;
        let right = viewport.right() - tokens::TEXT_MARGIN_RIGHT;
        let note_room = tokens::NOTE_GAP + tokens::NOTE_WIDTH;
        let notes_fit = notes && right - left - note_room >= measure.max(tokens::TEXT_MIN_WIDTH);
        let room = right - left - if notes_fit { note_room } else { px(0.) };
        // Never wider than the room, so a line is never cut by the pane's edge or covered by a panel.
        let text_width = measure.min(room).max(tokens::TEXT_MIN_WIDTH.min(room));
        let centred = viewport.left() + (viewport.size.width - text_width) * 0.5;
        let last = right - text_width - if notes_fit { note_room } else { px(0.) };
        let text_left = centred.min(last).max(left);
        Self {
            viewport,
            text_left,
            text_width,
            note_left: notes_fit.then_some(text_left + text_width + tokens::NOTE_GAP),
        }
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
    /// How far in the first row starts, for a paragraph the theme indents.
    pub indent: Pixels,
    /// How each row sits in the width the line was shaped for.
    pub align: TextAlign,
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
    /// Shaped in the faded colour of a paragraph out of focus.
    pub dim: bool,
    /// The room it was shaped with: a separator's gap, a first line's indent, and what stands below its text.
    pub gap: Pixels,
    pub indent: Pixels,
    pub below: Pixels,
}

impl BlockLayout {
    /// The height of the block's first line that shows, for what stands beside it in the gutter.
    pub fn first_line_height(&self) -> Pixels {
        if self.whole.is_some() {
            return tokens::SOURCE_LINE_FALLBACK;
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
    pub dim: bool,
    /// How tall a separator is.
    pub gap: Pixels,
    /// How far in the block's first line starts.
    pub indent: Pixels,
    /// Room kept below the text, for what stands under it.
    pub below: Pixels,
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
        dim,
        gap,
        indent,
        below,
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
        let wrap = (width - x).max(tokens::WRAP_MIN);
        let mut runs: Vec<TextRun> = display
            .runs
            .iter()
            .map(|(len, run)| style.run(&base, *len, *run, dim))
            .collect();
        if runs.is_empty() {
            runs.push(style.run(&base, 0, RunStyle::default(), dim));
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
        // The toolkit wraps every row alike, so a first row that starts further in is broken here.
        let first_row_in =
            (!source && indent > px(0.) && display.source.start == 0 && !display.text.is_empty()).then_some(indent);
        let shaped = match first_row_in {
            Some(indent) => {
                let mut shaped = shape(None);
                let breaks = breaks_within(&shaped, indent, wrap);
                indent_rows(&mut shaped, &breaks, indent);
                shaped
            }
            None => shape(Some(wrap)),
        };
        let row_indent = first_row_in.unwrap_or(px(0.));
        let rows = rows_of(&shaped);
        let hidden_line = collapsed || display.is_collapsed();
        let skipped = separator || hidden_line;
        let row_count = rows.len() as f32;
        let height = if hidden_line {
            px(0.)
        } else if separator {
            gap
        } else {
            line_height * row_count
        };
        let bullet = display
            .bullet
            .clone()
            .map(|text| (SharedString::from(text), unit * display.nesting as f32));
        let chip_boxes = chip_boxes(&display, &shaped, &rows, x, y, line_height, row_indent);
        shaped_lines.push(ShapedSourceLine {
            display,
            shaped,
            rows,
            y,
            x,
            line_height: if separator { gap } else { line_height },
            height,
            skipped,
            bullet,
            chips: chip_boxes,
            indent: row_indent,
            align: TextAlign::Left,
        });
        y += height;
    }
    BlockLayout {
        lines: shaped_lines,
        height: y + below,
        hidden,
        marked,
        width,
        collapsed,
        chips,
        whole,
        dim,
        gap,
        indent,
        below,
    }
}

/// Where a line breaks with its first row starting `indent` in: at the last space that fits each row, or inside a word
/// with no space to break at. A space that runs past the end hangs there, as the toolkit's own wrapping has it.
fn breaks_within(shaped: &WrappedLine, indent: Pixels, width: Pixels) -> Vec<usize> {
    let line = &shaped.unwrapped_layout;
    let bytes = shaped.text.as_bytes();
    let glyphs: Vec<usize> = line
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .map(|glyph| glyph.index)
        .collect();
    let mut breaks = Vec::new();
    let mut start = 0;
    let mut room = width - indent;
    let mut last_space = None;
    for (at, index) in glyphs.iter().copied().enumerate() {
        let is_space = bytes.get(index) == Some(&b' ');
        let end = glyphs.get(at + 1).copied().unwrap_or(line.len);
        if !is_space && index > start && line.x_for_index(end) - line.x_for_index(start) > room {
            let cut = last_space.filter(|space| *space > start).unwrap_or(index);
            breaks.push(cut);
            start = cut;
            room = width;
            last_space = None;
        }
        if is_space {
            last_space = Some(end);
        }
    }
    breaks
}

/// Breaks a shaped line at `breaks`, with the first row starting `indent` in. Positions stay rising from row to row, as the
/// caret's lookups expect, and rows are measured from their first glyph.
fn indent_rows(shaped: &mut WrappedLine, breaks: &[usize], indent: Pixels) {
    let line = &shaped.unwrapped_layout;
    let mut runs = line.runs.clone();
    let mut boundaries = Vec::new();
    let mut row = 0;
    for (run_ix, run) in runs.iter_mut().enumerate() {
        for (glyph_ix, glyph) in run.glyphs.iter_mut().enumerate() {
            if breaks.get(row).is_some_and(|at| glyph.index >= *at) {
                row += 1;
                boundaries.push(WrapBoundary { run_ix, glyph_ix });
            }
            glyph.position.x += indent;
        }
    }
    let layout = WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            font_size: line.font_size,
            width: line.width + indent,
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

/// How far a chip's box stands off its text, and how much of the line it leaves free above and below.
const CHIP_PAD: Pixels = tokens::HAIR_GAP;
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

    fn source_line(text: &str, indent: Pixels, width: Pixels) -> ShapedSourceLine {
        let mut line = shaped(text);
        let breaks = breaks_within(&line, indent, width);
        indent_rows(&mut line, &breaks, indent);
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
            align: TextAlign::Left,
        }
    }

    #[test]
    fn an_indented_first_row_has_less_room_and_the_caret_keeps_its_place_on_both_sides_of_a_break() {
        // 90 px hold nine letters, or six after a 30 px indent.
        let line = source_line("aaaa bbbb cccc", px(30.), px(90.));
        assert_eq!(line.rows, [0..5, 5..14]);
        assert_eq!(line.x_in_row(0, 0), px(30.));
        assert_eq!(line.x_in_row(5, 1), px(0.));
        assert_eq!(line.row_of(5, false), 1);
        assert_eq!(line.row_of(5, true), 0);
        for (at, row) in [(0, 0), (2, 0), (5, 0), (5, 1), (9, 1)] {
            assert_eq!(
                line.display_at_x(row, line.x_in_row(at, row)),
                at,
                "offset {at} in row {row}"
            );
        }
    }

    #[test]
    fn a_word_with_no_space_breaks_where_the_room_ends() {
        let line = shaped("aaaaaaaaaaaa");
        assert_eq!(breaks_within(&line, px(20.), px(50.)), [3, 8]);
    }

    #[test]
    fn positions_rise_from_row_to_row_so_lookups_stay_ordered() {
        let line = source_line("aaaa bbbb cccc", px(30.), px(90.));
        let glyphs: Vec<_> = line.shaped.unwrapped_layout.runs[0]
            .glyphs
            .iter()
            .map(|glyph| glyph.position.x)
            .collect();
        assert!(glyphs.windows(2).all(|pair| pair[0] < pair[1]), "{glyphs:?}");
    }

    fn viewport(width: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(28.)), size(px(width), px(800.)))
    }

    #[test]
    fn the_text_is_centred_and_notes_do_not_move_it_when_both_fit() {
        let bare = Column::place(viewport(1400.), px(460.), false);
        let noted = Column::place(viewport(1400.), px(460.), true);
        assert_eq!(bare.text_left, px(470.));
        assert_eq!(bare.text_width, px(460.));
        assert_eq!(bare.note_left, None);
        assert_eq!(noted.text_left, bare.text_left);
        assert_eq!(noted.note_left, Some(px(958.)));
    }

    #[test]
    fn a_narrow_window_moves_the_text_for_its_notes_then_drops_them_before_narrowing_it() {
        let moved = Column::place(viewport(860.), px(460.), true);
        assert!(moved.text_left < px(200.));
        assert_eq!(moved.text_width, px(460.));
        assert!(moved.note_left.is_some());
        let dropped = Column::place(viewport(620.), px(460.), true);
        assert_eq!(dropped.note_left, None);
        assert_eq!(dropped.text_width, px(460.));
        let narrowed = Column::place(viewport(400.), px(460.), true);
        assert_eq!(narrowed.note_left, None);
        assert!(narrowed.text_width < px(460.));
    }

    #[test]
    fn the_text_never_runs_past_its_pane() {
        // Narrower than the least width: the text takes what there is rather than run under a panel.
        let pane = viewport(208.);
        let column = Column::place(pane, px(460.), false);
        assert!(column.text_left + column.text_width <= pane.right() - tokens::TEXT_MARGIN_RIGHT);
        assert!(column.text_left >= pane.left() + tokens::GUTTER_WIDTH);
    }
}
