//! The text element: paints the visible blocks on the app's surface, the selection, the caret, the block gutter, and the
//! scrollbar, and hands what floats over them to [`super::float`].

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, LayoutId, MouseMoveEvent, MouseUpEvent, Pixels,
    Style, TextAlign, TextRun, Window, fill, point, px, relative, size,
};

use super::display::ChipLook;
use super::float::prepaint_float;
use super::gutter::{CommentMarker, block_icon, gutter_cursor, paint_gutter};
use super::layout::{BlockLayout, Column};
use super::margin::{Wanted, flow};
use super::{EditorView, Scrollbar};
use crate::source::BlockKind;
use crate::tokens::{self, Tokens};

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
    pub hovered: bool,
    pub icon: Option<&'static str>,
}

pub struct Frame {
    pub(super) column: Column,
    pub(super) blocks: Vec<FrameBlock>,
    selection: Vec<Bounds<Pixels>>,
    /// What an agent's changes brought in, tinted.
    marks: Vec<Bounds<Pixels>>,
    /// The thin lines under the passages notes are about.
    note_lines: Vec<Bounds<Pixels>>,
    /// The line under the passage a conflict is about.
    conflict_lines: Vec<Bounds<Pixels>>,
    /// Misspelled words, from the rectangles a selection of them would have.
    misspelled: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
    hitbox: Hitbox,
    gutter_hitbox: Hitbox,
    pub(super) markers: Vec<CommentMarker>,
    pub(super) drop_line: Option<Bounds<Pixels>>,
    /// The pointer is in the left margin, so the gutter shows.
    pub(super) gutter_shown: bool,
    scrollbar: Option<Scrollbar>,
    scrollbar_strong: bool,
    floats: Vec<AnyElement>,
    /// The images of visible figures, drawn inside the text's bounds.
    images: Vec<AnyElement>,
    /// The chapter's notes, drawn inside the same bounds, so they scroll under the title bar with the text.
    margin: Option<AnyElement>,
    /// A passage's buttons and the agent's notes, drawn inside the same bounds for the same reason.
    review: Vec<AnyElement>,
    source: bool,
    pub(super) ink: Hsla,
    pub(super) quiet: Hsla,
    pub(super) rule: Hsla,
    link: Hsla,
    error: Hsla,
    code_background: Hsla,
    removed: Hsla,
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
        let (mut frame, floats, margin, rows, advice) = self.view.update(cx, |view, cx| {
            view.trace.prepaint_started();
            view.set_viewport(bounds, cx);
            view.reveal_float(window, cx);
            if view.autoscroll {
                view.autoscroll = false;
                view.scroll_to_caret(window);
            }
            view.lay_out_visible(window);
            view.spelling_wanted(window, cx);
            view.update_scrollbar();
            let gutter_left = view.column.text_left - tokens::GUTTER_WIDTH;
            let gutter_hitbox = window.insert_hitbox(
                Bounds::new(
                    point(gutter_left, bounds.top()),
                    size(tokens::GUTTER_WIDTH, bounds.size.height),
                ),
                HitboxBehavior::Normal,
            );
            let hitbox = window.insert_hitbox(
                Bounds::new(
                    point(view.column.text_left, bounds.top()),
                    size(bounds.right() - view.column.text_left, bounds.size.height),
                ),
                HitboxBehavior::Normal,
            );
            let frame = view.frame(hitbox, gutter_hitbox, window);
            let selection = frame.selection.clone();
            let caret = frame.caret.or_else(|| view.caret_bounds(window));
            let floats = view.floats(&selection, caret, window, cx);
            let margin = view.margin_element();
            let rows = view.review_rows(cx);
            let mut advice = view.advice_elements(cx);
            advice.extend(view.comment_notes(window, cx));
            advice.sort_by_key(|(_, y)| *y);
            (frame, floats, margin, rows, advice)
        });
        frame.images = self.prepaint_images(&frame, bounds, window, cx);
        let note_left = frame.column.note_left.unwrap_or_default();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (mut element, origin) in rows {
                element.prepaint_as_root(origin, AvailableSpace::min_size(), window, cx);
                frame.review.push(element);
            }
            let measured: Vec<_> = advice
                .into_iter()
                .map(|(mut element, y)| {
                    let height = element.layout_as_root(AvailableSpace::min_size(), window, cx).height;
                    (element, Wanted { y, height })
                })
                .collect();
            let wanted: Vec<Wanted> = measured.iter().map(|(_, wanted)| *wanted).collect();
            for ((mut element, _), y) in measured.into_iter().zip(flow(&wanted, bounds.top())) {
                element.prepaint_at(point(note_left, y), window, cx);
                frame.review.push(element);
            }
        });
        // The chapter's opening notes stand in the flow, above its first text.
        frame.margin = margin.map(|(mut element, origin)| {
            let height = window.with_content_mask(Some(ContentMask { bounds }), |window| {
                let height = element.layout_as_root(AvailableSpace::min_size(), window, cx).height;
                element.prepaint_at(origin, window, cx);
                height
            });
            // Without asking for the next frame, notes unfolded by keyboard focus drew over the text until something else
            // drew, as the caret's blink does in the text but not in a note's field.
            if self.view.update(cx, |view, _| view.set_note_room(height)) {
                let view = self.view.clone();
                window.on_next_frame(move |_, cx| view.update(cx, |_, cx| cx.notify()));
            }
            element
        });
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

        let colors = Tokens::get(cx);
        let accent = colors.accent;
        let selection_color = Hsla::from(accent).opacity(tokens::SELECTION_TINT);
        let thumb_color = Hsla::from(colors.text_faint).opacity(if frame.scrollbar_strong {
            tokens::THUMB_STRONG
        } else {
            tokens::THUMB_FAINT
        });
        let mark_color = Hsla::from(colors.added).opacity(tokens::REVIEW_TINT);
        let misspelled_color = Hsla::from(accent).opacity(tokens::MISSPELLED_TINT);
        let note_line_color = colors.text_faint;
        let focused = focus.is_focused(window);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for block in &frame.blocks {
                paint_block_background(block, frame, window);
            }
            for rectangle in &frame.marks {
                window.paint_quad(fill(*rectangle, mark_color));
            }
            for line in &frame.note_lines {
                window.paint_quad(fill(*line, note_line_color));
            }
            for line in &frame.conflict_lines {
                window.paint_quad(fill(*line, accent));
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
                    size(word.size.width, tokens::HAIRLINE),
                );
                window.paint_quad(fill(line, misspelled_color));
            }
            for image in &mut frame.images {
                image.paint(window, cx);
            }
            if let Some(margin) = &mut frame.margin {
                margin.paint(window, cx);
            }
            for element in &mut frame.review {
                element.paint(window, cx);
            }
            paint_gutter(frame, window, cx);
            if let Some(bar) = frame.scrollbar {
                window.paint_quad(fill(bar.thumb, thumb_color).corner_radii(tokens::BOX_RADIUS));
            }
            if focused
                && caret_on
                && let Some(caret) = frame.caret
            {
                window.paint_quad(fill(caret, accent));
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
            #[cfg(any(debug_assertions, feature = "measure"))]
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
    let column = &frame.column;
    for line in &block.layout.lines {
        for (chip, look) in &line.chips {
            let color = match look {
                ChipLook::Mark => continue,
                ChipLook::Label => frame.link.opacity(tokens::CHIP_TINT),
                ChipLook::Error => frame.error.opacity(tokens::CHIP_ERROR_TINT),
                ChipLook::Removed => frame.removed.opacity(tokens::REVIEW_TINT),
            };
            let bounds = Bounds::new(
                point(column.text_left + chip.origin.x, block.top + chip.origin.y),
                chip.size,
            );
            window.paint_quad(fill(bounds, color).corner_radii(tokens::BOX_RADIUS));
        }
    }
    if frame.source {
        return;
    }
    let height = block.layout.height;
    match block.kind {
        BlockKind::Code => {
            let bounds = Bounds::new(
                point(column.text_left - tokens::CODE_OUTDENT, block.top),
                size(column.text_width + tokens::CODE_OUTDENT * 2., height),
            );
            window.paint_quad(fill(bounds, frame.code_background));
        }
        BlockKind::Quote => {
            let bounds = Bounds::new(
                point(column.text_left + tokens::QUOTE_RULE_X, block.top),
                size(tokens::QUOTE_RULE_WIDTH, height),
            );
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
    }
}

impl EditorView {
    /// Everything paint needs, gathered while the view is mutably at hand.
    fn frame(&mut self, hitbox: Hitbox, gutter_hitbox: Hitbox, window: &Window) -> Frame {
        let column = self.column;
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
                hovered: self.hovered == Some(index),
                icon,
            });
        }
        let selection = self.range_rectangles(self.selection.range(), &blocks, false);
        let marks = (self.review.iter())
            .flat_map(|passage| &passage.added)
            .flat_map(|added| self.range_rectangles(added.clone(), &blocks, true))
            .collect();
        let rule_under = |passage: Bounds<Pixels>| {
            Bounds::new(
                point(passage.left(), passage.bottom() - tokens::NOTE_RULE),
                size(passage.size.width, tokens::NOTE_RULE),
            )
        };
        let note_lines = (self.advice_anchors().into_iter())
            .flat_map(|anchor| self.range_rectangles(anchor, &blocks, true))
            .map(rule_under)
            .collect();
        let conflict_lines = (self.conflict_mark.clone().into_iter())
            .flat_map(|mark| self.range_rectangles(mark, &blocks, true))
            .map(rule_under)
            .collect();
        let misspelled = self
            .spelling_ranges()
            .into_iter()
            .flat_map(|word| self.range_rectangles(word, &blocks, false))
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
            selection,
            marks,
            note_lines,
            conflict_lines,
            misspelled,
            caret,
            hitbox,
            gutter_hitbox,
            markers: self.markers.clone(),
            drop_line: self.drop_line(),
            gutter_shown: self.gutter_shown || self.grab.is_some(),
            scrollbar: self.scrollbar,
            scrollbar_strong: self.thumb_hovered || self.thumb_grab.is_some(),
            floats: Vec::new(),
            images: Vec::new(),
            margin: None,
            review: Vec::new(),
            source: self.source,
            ink: self.style.ink,
            quiet: self.style.quiet,
            rule: self.style.rule,
            link: self.style.link,
            error: self.style.error,
            code_background: self.style.code_background,
            removed: self.style.removed,
            body_size: self.style.body_size,
            body_font: self.style.body.clone(),
        }
    }

    /// The rectangles that cover `selection` in the visible blocks. `past_chip` starts them after removed words standing
    /// at the range's start.
    fn range_rectangles(
        &self,
        selection: std::ops::Range<usize>,
        blocks: &[FrameBlock],
        past_chip: bool,
    ) -> Vec<Bounds<Pixels>> {
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
                let start = selection
                    .start
                    .saturating_sub(block_start)
                    .max(line.display.source.start);
                let from = if past_chip {
                    line.display.to_display_past_chip(start)
                } else {
                    line.display.to_display(start)
                };
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
