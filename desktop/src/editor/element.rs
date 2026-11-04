//! The galley element: paints the sheet, the visible blocks, the selection, the caret, the block gutter, and the
//! scrollbar, and hands what floats over them to [`super::float`].

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, LayoutId, MouseMoveEvent, MouseUpEvent, Pixels,
    Style, TextAlign, TextRun, Window, fill, point, px, relative, size,
};

use super::display::ChipLook;
use super::float::prepaint_float;
use super::gutter::{CommentMarker, block_icon, gutter_cursor, paint_gutter};
use super::layout::BlockLayout;
use super::{Column, EditorView, SHEET_PADDING_Y, Scrollbar};
use crate::source::BlockKind;
use crate::tokens::Tokens;

pub struct Galley {
    view: Entity<EditorView>,
}

/// How far up a line a misspelled word's underline stands, as a share of the line's height.
const MISSPELLED_RISE: f32 = 0.14;

impl Galley {
    pub fn new(view: Entity<EditorView>) -> Self {
        Self { view }
    }
}

pub(super) struct FrameBlock {
    pub kind: BlockKind,
    pub top: Pixels,
    pub layout: Rc<BlockLayout>,
    pub active: bool,
    pub hovered: bool,
    pub icon: Option<&'static str>,
}

pub struct Frame {
    pub(super) column: Column,
    pub(super) blocks: Vec<FrameBlock>,
    sheet: Bounds<Pixels>,
    selection: Vec<Bounds<Pixels>>,
    marks: Vec<Bounds<Pixels>>,
    /// Misspelled words, from the rectangles a selection of them would have.
    misspelled: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
    hitbox: Hitbox,
    gutter_hitbox: Hitbox,
    pub(super) markers: Vec<CommentMarker>,
    pub(super) drop_line: Option<Bounds<Pixels>>,
    scrollbar: Option<Scrollbar>,
    scrollbar_strong: bool,
    floats: Vec<AnyElement>,
    /// The images of visible figures, drawn inside the galley.
    images: Vec<AnyElement>,
    source: bool,
    pub(super) ink: Hsla,
    pub(super) quiet: Hsla,
    pub(super) rule: Hsla,
    link: Hsla,
    error: Hsla,
    code_background: Hsla,
    body_size: Pixels,
    body_font: gpui::Font,
}

impl IntoElement for Galley {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Galley {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.flex_grow = 1.;
        style.min_size.height = px(0.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Frame {
        let (mut frame, floats) = self.view.update(cx, |view, cx| {
            view.trace.prepaint_started();
            view.set_viewport(bounds);
            view.compose_if_resized(window, cx);
            if view.autoscroll {
                view.autoscroll = false;
                view.scroll_to_caret(window);
            }
            view.lay_out_visible(window);
            view.spelling_wanted(window, cx);
            view.update_scrollbar();
            let gutter_hitbox = window.insert_hitbox(
                Bounds::new(
                    point(view.column.sheet_left, bounds.top()),
                    size(view.column.text_left - view.column.sheet_left, bounds.size.height),
                ),
                HitboxBehavior::Normal,
            );
            let hitbox = window.insert_hitbox(
                Bounds::new(
                    point(view.column.text_left, bounds.top()),
                    size(
                        view.column.sheet_left + view.column.sheet_width - view.column.text_left,
                        bounds.size.height,
                    ),
                ),
                HitboxBehavior::Normal,
            );
            let frame = view.frame(hitbox, gutter_hitbox, window);
            let selection = frame.selection.clone();
            let caret = frame.caret.or_else(|| view.caret_bounds(window));
            let floats = view.floats(&selection, caret, window, cx);
            (frame, floats)
        });
        frame.images = self.prepaint_images(&frame, bounds, window, cx);
        frame.floats = floats
            .into_iter()
            .map(|(element, origin)| prepaint_float(element, origin, window, cx))
            .collect();
        frame
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut Frame,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus, caret_on) = {
            let view = self.view.read(cx);
            (view.focus.clone(), view.caret_on)
        };
        self.view.update(cx, |view, _| view.trace.paint_started());
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        window.set_cursor_style(CursorStyle::IBeam, &frame.hitbox);
        window.set_cursor_style(gutter_cursor(frame), &frame.gutter_hitbox);
        if frame.drop_line.is_some() {
            window.set_window_cursor_style(CursorStyle::ClosedHand);
        }

        let tokens = Tokens::get(cx);
        let sheet_color = tokens.sheet;
        let selection_color = Hsla::from(tokens.accent).opacity(0.25);
        let thumb_color = Hsla::from(tokens.text_faint).opacity(if frame.scrollbar_strong { 0.8 } else { 0.4 });
        let mark_color = Hsla::from(tokens.added).opacity(0.2);
        let misspelled_color = Hsla::from(tokens.accent).opacity(0.6);
        let focused = focus.is_focused(window);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(frame.sheet, sheet_color));
            for block in &frame.blocks {
                paint_block_background(block, frame, window);
            }
            for rectangle in &frame.marks {
                window.paint_quad(fill(*rectangle, mark_color));
            }
            for rectangle in &frame.selection {
                window.paint_quad(fill(*rectangle, selection_color));
            }
            for block in &frame.blocks {
                paint_block_text(block, frame, window, cx);
            }
            for word in &frame.misspelled {
                let line = Bounds::new(
                    point(word.left(), word.bottom() - word.size.height * MISSPELLED_RISE),
                    size(word.size.width, px(1.)),
                );
                window.paint_quad(fill(line, misspelled_color));
            }
            for image in &mut frame.images {
                image.paint(window, cx);
            }
            paint_gutter(frame, window, cx);
            if let Some(bar) = frame.scrollbar {
                window.paint_quad(fill(bar.thumb, thumb_color).corner_radii(px(3.)));
            }
            if focused
                && caret_on
                && let Some(caret) = frame.caret
            {
                window.paint_quad(fill(caret, frame.ink));
            }
        });

        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase.bubble() {
                view.update(cx, |view, cx| view.mouse_moved(event, window, cx));
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase.bubble() {
                view.update(cx, |view, cx| view.mouse_up(event, window, cx));
            }
        });
        for float in &mut frame.floats {
            float.paint(window, cx);
        }
        self.view.update(cx, |view, _| {
            view.trace.painted();
            view.note_painted();
        });
    }
}

impl Galley {
    fn prepaint_images(
        &self,
        frame: &Frame,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        let mut images = Vec::new();
        for block in &frame.blocks {
            if let Some(element) = block
                .layout
                .whole
                .as_ref()
                .and_then(|whole| whole.image_element(point(frame.column.text_left, block.top), bounds, window, cx))
            {
                images.push(element);
            }
        }
        images
    }
}

fn paint_block_background(block: &FrameBlock, frame: &Frame, window: &mut Window) {
    if frame.source {
        return;
    }
    let column = &frame.column;
    for line in &block.layout.lines {
        for (chip, look) in &line.chips {
            let color = match look {
                ChipLook::Mark => continue,
                ChipLook::Label => frame.link.opacity(0.12),
                ChipLook::Error => frame.error.opacity(0.14),
            };
            let bounds = Bounds::new(
                point(column.text_left + chip.origin.x, block.top + chip.origin.y),
                chip.size,
            );
            window.paint_quad(fill(bounds, color).corner_radii(px(3.)));
        }
    }
    let height = block.layout.height;
    match block.kind {
        BlockKind::Code => {
            let bounds = Bounds::new(
                point(column.text_left - px(6.), block.top),
                size(column.text_width + px(12.), height),
            );
            window.paint_quad(fill(bounds, frame.code_background));
        }
        BlockKind::Quote => {
            let bounds = Bounds::new(point(column.text_left + px(2.), block.top), size(px(2.), height));
            window.paint_quad(fill(bounds, frame.rule));
        }
        _ => {}
    }
}

fn paint_block_text(block: &FrameBlock, frame: &Frame, window: &mut Window, cx: &mut App) {
    let column = &frame.column;
    if let Some(whole) = &block.layout.whole {
        whole.paint(point(column.text_left, block.top), frame, window, cx);
    }
    paint_lines(&block.layout, column.text_left, block.top, frame, window, cx);
}

/// Paints a shaped block's lines with its top left corner at (`left`, `top`).
pub(super) fn paint_lines(
    layout: &BlockLayout,
    left: Pixels,
    top: Pixels,
    frame: &Frame,
    window: &mut Window,
    cx: &mut App,
) {
    for line in &layout.lines {
        if line.height == px(0.) {
            continue;
        }
        let top = top + line.y;
        if let Some((bullet, x)) = &line.bullet {
            let run = TextRun {
                len: bullet.len(),
                font: frame.body_font.clone(),
                color: frame.quiet,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(bullet.clone(), frame.body_size, &[run], None);
            let origin = point(left + *x, top);
            shaped
                .paint(origin, line.line_height, TextAlign::Left, None, window, cx)
                .ok();
        }
        let origin = point(left + line.x, top);
        line.shaped
            .paint(origin, line.line_height, TextAlign::Left, None, window, cx)
            .ok();
        if let Some((rows, hyphen)) = &line.hyphens {
            for row in rows {
                let at = point(
                    origin.x + line.x_in_row(line.rows[*row].end, *row),
                    origin.y + line.line_height * *row as f32,
                );
                hyphen
                    .paint(at, line.line_height, TextAlign::Left, None, window, cx)
                    .ok();
            }
        }
    }
}

impl EditorView {
    /// Everything paint needs, gathered while the view is mutably at hand.
    fn frame(&mut self, hitbox: Hitbox, gutter_hitbox: Hitbox, window: &Window) -> Frame {
        let column = self.column;
        let viewport = column.viewport;
        let active = self.blocks.index_at(self.selection.head);
        let mut blocks = Vec::with_capacity(self.visible.len());
        for (index, top) in self.visible.clone() {
            let block = self.blocks.as_slice()[index].clone();
            let icon = match self.slots[index].icon {
                Some(icon) => Some(icon),
                None => {
                    let icon = block_icon(block.kind, &self.session.active_buffer().text()[block.range.clone()]);
                    self.slots[index].icon = icon;
                    icon
                }
            };
            // A list table starts with its directive line, and it is a table all the same.
            let icon = if self
                .rich_atom(index)
                .is_some_and(|atom| atom.first == index && atom.is_shown_whole() && block.kind == BlockKind::Directive)
            {
                Some("icons/block-table.svg")
            } else {
                icon
            };
            blocks.push(FrameBlock {
                kind: block.kind,
                top,
                layout: self.layout_block(index, window),
                active: index == active,
                hovered: self.hovered == Some(index),
                icon,
            });
        }
        let starts_here = self.visible.first().is_some_and(|(index, _)| *index == 0);
        let ends_here = self
            .visible
            .last()
            .is_some_and(|(index, _)| *index + 1 == self.blocks.as_slice().len());
        let sheet_top = match (starts_here, blocks.first()) {
            (true, Some(first)) => (first.top - SHEET_PADDING_Y).max(viewport.top()),
            _ => viewport.top(),
        };
        let sheet_bottom = match (ends_here, blocks.last()) {
            (true, Some(last)) => (last.top + last.layout.height + SHEET_PADDING_Y).min(viewport.bottom()),
            _ => viewport.bottom(),
        };
        let sheet = Bounds::new(
            point(column.sheet_left, sheet_top),
            size(column.sheet_width, sheet_bottom - sheet_top),
        );
        let selection = self.range_rectangles(self.selection.range(), &blocks);
        let marks = self
            .marks
            .iter()
            .flat_map(|mark| self.range_rectangles(mark.clone(), &blocks))
            .collect();
        let misspelled = self
            .spelling_ranges()
            .into_iter()
            .flat_map(|word| self.range_rectangles(word, &blocks))
            .collect();
        let caret = if self.selection.is_empty() {
            self.caret_bounds(window)
        } else {
            None
        };
        self.markers = self.comment_markers(&blocks);
        Frame {
            column,
            blocks,
            sheet,
            selection,
            marks,
            misspelled,
            caret,
            hitbox,
            gutter_hitbox,
            markers: self.markers.clone(),
            drop_line: self.drop_line(),
            scrollbar: self.scrollbar,
            scrollbar_strong: self.thumb_hovered || self.thumb_grab.is_some(),
            floats: Vec::new(),
            images: Vec::new(),
            source: self.source,
            ink: self.style.ink,
            quiet: self.style.quiet,
            rule: self.style.rule,
            link: self.style.link,
            error: self.style.error,
            code_background: self.style.code_background,
            body_size: self.style.body_size,
            body_font: self.style.body.clone(),
        }
    }

    /// The rectangles that cover `selection` in the visible blocks.
    fn range_rectangles(&self, selection: std::ops::Range<usize>, blocks: &[FrameBlock]) -> Vec<Bounds<Pixels>> {
        if selection.is_empty() {
            return Vec::new();
        }
        let newline_width = self.style.body_size * 0.4;
        let mut rectangles = Vec::new();
        for (block, (index, _)) in blocks.iter().zip(&self.visible) {
            let block_start = self.blocks.as_slice()[*index].range.start;
            // A table or figure the selection takes in whole shows as one rectangle.
            if let Some(whole) = &block.layout.whole
                && selection.start <= block_start
                && selection.end > block_start
            {
                rectangles.push(Bounds::new(
                    point(self.column.text_left, block.top),
                    size(self.column.text_width, whole.height()),
                ));
            }
            for line in &block.layout.lines {
                if line.height == px(0.) {
                    continue;
                }
                let line_start = block_start + line.display.source.start;
                let line_end = block_start + line.display.source.end;
                if selection.end < line_start || selection.start > line_end {
                    continue;
                }
                let from = line.display.to_display(
                    selection
                        .start
                        .saturating_sub(block_start)
                        .max(line.display.source.start),
                );
                let to = line
                    .display
                    .to_display(selection.end.saturating_sub(block_start).min(line.display.source.end));
                let crosses_line_end = selection.end > line_end;
                // A blank gap between blocks is how Markdown spells the boundary: nothing to show as selected.
                if line.skipped {
                    continue;
                }
                for (row, range) in line.rows.iter().enumerate() {
                    let start = from.max(range.start);
                    let end = to.min(range.end);
                    let last = line.is_last_row(row);
                    if start > end || (start == end && !(last && crosses_line_end)) {
                        continue;
                    }
                    let x0 = line.x_in_row(start, row);
                    let mut x1 = line.x_in_row(end, row);
                    if last && crosses_line_end {
                        x1 += newline_width;
                    }
                    rectangles.push(Bounds::new(
                        point(
                            self.column.text_left + line.x + x0,
                            block.top + line.y + line.line_height * row as f32,
                        ),
                        size(x1 - x0, line.line_height),
                    ));
                }
            }
        }
        rectangles
    }
}
