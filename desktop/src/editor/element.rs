//! The galley element: paints the sheet, the visible blocks, the selection, the caret, and the block gutter.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, LayoutId, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Style,
    TextAlign, TextRun, Window, fill, font, point, px, relative, size,
};

use super::layout::BlockLayout;
use super::{Column, EditorView, SHEET_PADDING_Y};
use crate::source::BlockKind;
use crate::tokens::{self, Tokens};

pub struct Galley {
    view: Entity<EditorView>,
}

impl Galley {
    pub fn new(view: Entity<EditorView>) -> Self {
        Self { view }
    }
}

struct FrameBlock {
    kind: BlockKind,
    top: Pixels,
    layout: Rc<BlockLayout>,
    active: bool,
    hovered: bool,
}

pub struct Frame {
    column: Column,
    blocks: Vec<FrameBlock>,
    sheet: Bounds<Pixels>,
    selection: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
    hitbox: Hitbox,
    ink: Hsla,
    quiet: Hsla,
    rule: Hsla,
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
        self.view.update(cx, |view, _| {
            view.trace.prepaint_started();
            view.set_viewport(bounds);
            if view.autoscroll {
                view.autoscroll = false;
                view.scroll_to_caret(window);
            }
            view.lay_out_visible(window);
            let hitbox = window.insert_hitbox(
                Bounds::new(
                    point(view.column.sheet_left, bounds.top()),
                    size(view.column.sheet_width, bounds.size.height),
                ),
                HitboxBehavior::Normal,
            );
            view.frame(hitbox, window)
        })
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

        let tokens = Tokens::get(cx);
        let sheet_color = tokens.sheet;
        let selection_color = Hsla::from(tokens.accent).opacity(0.25);
        let label_color = tokens.text_faint;
        let focused = focus.is_focused(window);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(frame.sheet, sheet_color));
            for block in &frame.blocks {
                paint_block_background(block, frame, window);
            }
            for rectangle in &frame.selection {
                window.paint_quad(fill(*rectangle, selection_color));
            }
            for block in &frame.blocks {
                paint_block_text(block, frame, label_color.into(), window, cx);
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
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase.bubble() {
                view.update(cx, |view, cx| view.mouse_up(event, cx));
            }
        });
        self.view.update(cx, |view, _| view.trace.painted());
    }
}

fn paint_block_background(block: &FrameBlock, frame: &Frame, window: &mut Window) {
    let column = &frame.column;
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

fn paint_block_text(block: &FrameBlock, frame: &Frame, label_color: Hsla, window: &mut Window, cx: &mut App) {
    let column = &frame.column;
    for line in &block.layout.lines {
        if line.height == px(0.) {
            continue;
        }
        let top = block.top + line.y;
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
            let origin = point(column.text_left + *x, top);
            shaped
                .paint(origin, line.line_height, TextAlign::Left, None, window, cx)
                .ok();
        }
        let origin = point(column.text_left + line.x, top);
        line.shaped
            .paint(origin, line.line_height, TextAlign::Left, None, window, cx)
            .ok();
    }
    if (block.active || block.hovered)
        && let Some(label) = block_label(block.kind)
    {
        let label: SharedString = label.into();
        let run = TextRun {
            len: label.len(),
            font: font(tokens::FONT_UI),
            color: label_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(label, tokens::TEXT_SMALL, &[run], None);
        let line_height = block
            .layout
            .lines
            .first()
            .map_or(tokens::TEXT_SMALL, |line| line.line_height);
        let origin = point(column.sheet_left - tokens::SPACE_2 - shaped.width(), block.top);
        shaped
            .paint(origin, line_height, TextAlign::Left, None, window, cx)
            .ok();
    }
}

fn block_label(kind: BlockKind) -> Option<&'static str> {
    Some(match kind {
        BlockKind::FrontMatter => "Meta",
        BlockKind::Heading { level: 1 } => "H1",
        BlockKind::Heading { level: 2 } => "H2",
        BlockKind::Heading { level: 3 } => "H3",
        BlockKind::Heading { .. } => "H4+",
        BlockKind::Paragraph => "Text",
        BlockKind::List => "List",
        BlockKind::Quote => "Quote",
        BlockKind::Code => "Code",
        BlockKind::Table => "Table",
        BlockKind::Directive => "Dir",
        BlockKind::Comment => "Note",
        BlockKind::SceneBreak => "Break",
        BlockKind::FootnoteDefinition => "Foot",
        BlockKind::Raw => "Raw",
        BlockKind::Blank => return None,
    })
}

impl EditorView {
    /// Everything paint needs, gathered while the view is mutably at hand.
    fn frame(&mut self, hitbox: Hitbox, window: &Window) -> Frame {
        let column = self.column;
        let viewport = column.viewport;
        let active = self.blocks.index_at(self.selection.head);
        let mut blocks = Vec::with_capacity(self.visible.len());
        for (index, top) in self.visible.clone() {
            blocks.push(FrameBlock {
                kind: self.blocks.as_slice()[index].kind,
                top,
                layout: self.layout_block(index, window),
                active: index == active,
                hovered: self.hovered == Some(index),
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
        let selection = self.selection_rectangles(&blocks);
        let caret = if self.selection.is_empty() {
            self.caret_bounds(window)
        } else {
            None
        };
        Frame {
            column,
            blocks,
            sheet,
            selection,
            caret,
            hitbox,
            ink: self.style.ink,
            quiet: self.style.quiet,
            rule: self.style.rule,
            code_background: self.style.code_background,
            body_size: self.style.body_size,
            body_font: self.style.body.clone(),
        }
    }

    fn selection_rectangles(&self, blocks: &[FrameBlock]) -> Vec<Bounds<Pixels>> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let selection = self.selection.range();
        let newline_width = self.style.body_size * 0.4;
        let mut rectangles = Vec::new();
        for (block, (index, _)) in blocks.iter().zip(&self.visible) {
            let block_start = self.blocks.as_slice()[*index].range.start;
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
