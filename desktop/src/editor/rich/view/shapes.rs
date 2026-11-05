//! What the galley draws for a table or a figure at rest: shaped cells and rules, or the image with its caption. Shaped
//! with the block, so it costs nothing until a table's text changes.

use crate::tokens;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Pixels, Point, SharedString, Size, Window, fill, img, point,
    px, size,
};

use crate::editor::EditorView;
use crate::editor::display::{ChipLook, Syntax, display_lines, hidden_ranges};
use crate::editor::element::{Frame, paint_lines};
use crate::editor::layout::{BlockLayout, ShapeRequest, shape_block};
use crate::editor::rich::table::{Align, Grid};
use crate::editor::rich::{Atom, AtomKind, Key};
use crate::source::BlockKind;
use crate::source::inline::inline_spans;

/// The tallest a figure shows, as a share of the column's width.
const MAX_ASPECT: f32 = 0.75;

pub struct GridCell {
    layout: BlockLayout,
    x: Pixels,
}

pub struct GridRow {
    y: Pixels,
    height: Pixels,
    cells: Vec<GridCell>,
}

pub struct GridShape {
    /// What it was shaped from, to tell when the text changed.
    source: Rc<Grid>,
    caption: Option<BlockLayout>,
    top: Pixels,
    rows: Vec<GridRow>,
    width: Pixels,
    height: Pixels,
}

pub struct FigureShape {
    source: Atom,
    file: PathBuf,
    x: Pixels,
    image: Size<Pixels>,
    /// Why there is no image to show.
    problem: Option<BlockLayout>,
    caption: Option<BlockLayout>,
    height: Pixels,
}

/// A table or a figure, drawn in place of its source lines.
#[allow(clippy::large_enum_variant, reason = "a block's layout holds it boxed")]
pub enum Whole {
    Grid(GridShape),
    Figure(FigureShape),
}

/// Whether a block shaped as `shaped` still shows `atom`.
pub fn still_fits(shaped: Option<&Whole>, atom: Option<&Atom>) -> bool {
    match (shaped, atom.map(|atom| &atom.kind)) {
        (None, None) => true,
        (Some(Whole::Grid(grid)), Some(AtomKind::Table(Ok(source)))) => grid.source == *source,
        (Some(Whole::Figure(figure)), Some(_)) => atom == Some(&figure.source),
        _ => false,
    }
}

impl Whole {
    pub fn height(&self) -> Pixels {
        match self {
            Self::Grid(grid) => grid.height,
            Self::Figure(figure) => figure.height,
        }
    }

    pub(in crate::editor) fn paint(&self, origin: Point<Pixels>, frame: &Frame, window: &mut Window, cx: &mut App) {
        match self {
            Self::Grid(grid) => grid.paint(origin, frame, window, cx),
            Self::Figure(figure) => figure.paint(origin, frame, window, cx),
        }
    }

    /// The image of a figure, ready to paint inside the galley.
    pub fn image_element(
        &self,
        origin: Point<Pixels>,
        mask: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let Self::Figure(figure) = self else {
            return None;
        };
        if figure.problem.is_some() {
            return None;
        }
        let mut element = img(figure.file.clone())
            .w(figure.image.width)
            .h(figure.image.height)
            .into_any_element();
        let at = point(origin.x + figure.x, origin.y);
        window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
            element.prepaint_as_root(
                at,
                size(
                    AvailableSpace::Definite(figure.image.width),
                    AvailableSpace::Definite(figure.image.height),
                ),
                window,
                cx,
            );
        });
        Some(element)
    }
}

impl GridShape {
    fn paint(&self, origin: Point<Pixels>, frame: &Frame, window: &mut Window, cx: &mut App) {
        if let Some(caption) = &self.caption {
            paint_lines(caption, origin.x, origin.y, frame, window, cx);
        }
        let rule = |y: Pixels, strong: bool, window: &mut Window| {
            let bounds = Bounds::new(
                point(origin.x, origin.y + y),
                size(self.width, if strong { tokens::HAIRLINE } else { tokens::TABLE_RULE }),
            );
            window.paint_quad(fill(bounds, frame.rule));
        };
        rule(self.top, true, window);
        for (index, row) in self.rows.iter().enumerate() {
            for cell in &row.cells {
                let (x, y) = (origin.x + cell.x + tokens::CELL_X, origin.y + row.y + tokens::CELL_Y);
                paint_lines(&cell.layout, x, y, frame, window, cx);
            }
            rule(row.y + row.height, index == 0 || index + 1 == self.rows.len(), window);
        }
    }
}

impl FigureShape {
    fn paint(&self, origin: Point<Pixels>, frame: &Frame, window: &mut Window, cx: &mut App) {
        let box_origin = point(origin.x + self.x, origin.y);
        if let Some(problem) = &self.problem {
            let bounds = Bounds::new(box_origin, self.image);
            window.paint_quad(
                fill(bounds, frame.rule.opacity(tokens::FIGURE_LOADING_TINT)).corner_radii(tokens::CHIP_RADIUS),
            );
            paint_lines(
                problem,
                box_origin.x + tokens::CELL_X,
                box_origin.y + tokens::CELL_Y,
                frame,
                window,
                cx,
            );
        }
        if let Some(caption) = &self.caption {
            paint_lines(
                caption,
                origin.x,
                origin.y + self.image.height + tokens::CAPTION_GAP,
                frame,
                window,
                cx,
            );
        }
    }
}

impl EditorView {
    /// A short piece of inline Markdown, shaped small with its markers hidden and its chips showing.
    fn shape_inline(
        &self,
        text: &str,
        width: Pixels,
        bold: bool,
        look: Option<ChipLook>,
        dim: bool,
        window: &Window,
    ) -> BlockLayout {
        let kind = BlockKind::FootnoteDefinition;
        let syntax = Syntax {
            spans: inline_spans(kind, text),
            markers: Vec::new(),
        };
        let hidden = hidden_ranges(kind, text, &syntax, None, false, false);
        let chips = self.rich.chips(text, &syntax.spans, None);
        let mut lines = display_lines(kind, text, &syntax, &hidden, None, false, &chips);
        for line in &mut lines {
            for (_, run) in &mut line.runs {
                run.bold |= bold;
                run.chip = run.chip.or(look);
            }
        }
        shape_block(
            ShapeRequest {
                kind,
                lines,
                separator: false,
                collapsed: false,
                source: false,
                hidden,
                marked: None,
                width,
                style: &self.style,
                chips,
                whole: None,
                dim,
                gap: px(0.),
                indent: px(0.),
                below: px(0.),
            },
            window,
        )
    }

    /// Shapes a table or figure for the current width.
    pub(in crate::editor) fn shape_atom(&mut self, atom: &Atom, dim: bool, window: &Window) -> Whole {
        match &atom.kind {
            AtomKind::Table(Ok(grid)) => Whole::Grid(self.shape_grid(grid.clone(), dim, window)),
            _ => Whole::Figure(self.shape_figure(atom, dim, window)),
        }
    }

    fn shape_grid(&self, grid: Rc<Grid>, dim: bool, window: &Window) -> GridShape {
        let width = self.column.text_width;
        let mut weights = vec![4usize; grid.columns];
        for row in &grid.rows {
            let mut column = 0;
            for cell in row {
                if cell.span == 1 && column < weights.len() {
                    let longest = cell.text.lines().map(|line| line.chars().count()).max().unwrap_or(0);
                    weights[column] = weights[column].max(longest.min(40));
                }
                column += cell.span;
            }
        }
        let total: usize = weights.iter().sum();
        let widths: Vec<Pixels> = weights
            .iter()
            .map(|weight| width * (*weight as f32 / total as f32))
            .collect();
        let lefts: Vec<Pixels> = widths
            .iter()
            .scan(px(0.), |left, width| {
                let at = *left;
                *left += *width;
                Some(at)
            })
            .collect();
        let caption = (!grid.caption.text.is_empty())
            .then(|| self.shape_inline(&grid.caption.text, width, false, None, dim, window));
        let top = caption
            .as_ref()
            .map_or(px(0.), |caption| caption.height + tokens::CAPTION_GAP);
        let mut y = top;
        let mut rows = Vec::with_capacity(grid.rows.len());
        for (index, row) in grid.rows.iter().enumerate() {
            let mut column = 0;
            let mut cells = Vec::with_capacity(row.len());
            let mut height = px(0.);
            for cell in row {
                let end = (column + cell.span).min(grid.columns);
                let covered: Pixels = widths[column.min(grid.columns - 1)..end]
                    .iter()
                    .copied()
                    .fold(px(0.), |a, b| a + b);
                let room = (covered - tokens::CELL_X * 2.).max(tokens::CELL_MIN_WIDTH);
                let mut layout = self.shape_inline(&cell_at_rest(&cell.text), room, index == 0, None, dim, window);
                let align = grid.aligns.get(column).copied().unwrap_or_default();
                if matches!(align, Align::Center | Align::Right) {
                    for line in &mut layout.lines {
                        if line.rows.len() == 1 {
                            let slack = room - line.shaped.unwrapped_layout.width;
                            let shift = if align == Align::Center { slack * 0.5 } else { slack };
                            line.x += shift;
                            for (chip, _) in &mut line.chips {
                                chip.origin.x += shift;
                            }
                        }
                    }
                }
                height = height.max(layout.height + tokens::CELL_Y * 2.);
                cells.push(GridCell {
                    layout,
                    x: lefts[column.min(grid.columns - 1)],
                });
                column = end;
            }
            rows.push(GridRow { y, height, cells });
            y += height;
        }
        GridShape {
            source: grid,
            caption,
            top,
            rows,
            width,
            height: y + tokens::HAIRLINE,
        }
    }

    fn shape_figure(&mut self, atom: &Atom, dim: bool, window: &Window) -> FigureShape {
        let AtomKind::Figure(figure) = &atom.kind else {
            unreachable!("a table is shaped as a grid");
        };
        let block = &self.blocks.as_slice()[atom.first];
        let text = self.session.active_buffer().text()[block.range.clone()].to_owned();
        let written = figure.path(&text).to_owned();
        let file = self.session.base_dir().join(&written);
        let width = self.column.text_width;
        let found = self.rich.image_size(&file);
        let (image, problem) = match found {
            Ok((natural_width, natural_height)) if natural_width > 0. && natural_height > 0. => {
                let scale = (self.style.scale())
                    .min(f32::from(width) / natural_width)
                    .min(f32::from(width) * MAX_ASPECT / natural_height);
                (size(px(natural_width * scale), px(natural_height * scale)), None)
            }
            Ok(_) => (size(width, tokens::FIGURE_MISSING_HEIGHT), Some(written.clone())),
            Err(why) => {
                let message = if why.is_empty() {
                    crate::t!(Key::ImageMissing, path = written)
                } else {
                    format!("{written}: {why}")
                };
                (size(width, tokens::FIGURE_MISSING_HEIGHT), Some(message))
            }
        };
        let problem = problem.map(|message| {
            self.shape_inline(
                &message,
                width - tokens::CELL_X * 2.,
                false,
                Some(ChipLook::Error),
                dim,
                window,
            )
        });
        let caption_text = figure.caption(&text);
        let caption =
            (!caption_text.is_empty()).then(|| self.shape_inline(caption_text, width, false, None, dim, window));
        let height = image.height
            + caption
                .as_ref()
                .map_or(px(0.), |caption| caption.height + tokens::CAPTION_GAP);
        FigureShape {
            source: atom.clone(),
            file,
            x: (width - image.width) * 0.5,
            image,
            problem,
            caption,
            height,
        }
    }
}

/// A cell's text as it shows at rest: attribute lines go, and a list's markers read as bullets.
fn cell_at_rest(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let line = line.trim();
            !(line.starts_with('{') && line.ends_with('}'))
        })
        .map(|line| match line.trim_start().strip_prefix("- ") {
            Some(item) => format!("• {item}"),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A file size for a label, whole numbers.
pub fn size_label(size: (f32, f32)) -> SharedString {
    crate::t!(Key::ImageSize, width = size.0.round(), height = size.1.round()).into()
}
