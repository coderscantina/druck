//! The block gutter: a type icon beside each block, quiet until it is active or hovered, comment markers, and dragging a
//! block by its icon. It sits inside the sheet, left of the text.

use gpui::{Bounds, Context, CursorStyle, Hsla, Pixels, Point, TransformationMatrix, Window, fill, point, px, size};

use super::commands::convert::{self, BlockType};
use super::commands::{moves, notes, slash};
use super::element::{Frame, FrameBlock};
use super::{EditorView, Tokens};
use crate::source::inline::SpanKind;
use crate::source::{BlockKind, Selection};
use crate::tokens;

const ICON: Pixels = px(14.);
const COMMENT_ICON: Pixels = px(12.);
/// How far the pointer must move before a press on an icon is a drag, not a click.
const DRAG_START: f32 = 4.;
/// The space between a comment marker and the text.
const MARKER_INSET: Pixels = px(20.);
const ICON_INSET: Pixels = px(14.);

/// A press on a block's icon, which a release turns into its menu and a move into a drag.
#[derive(Clone)]
pub(super) struct BlockGrab {
    block: usize,
    origin: Point<Pixels>,
    anchor: Bounds<Pixels>,
    dragging: bool,
    /// The block the dragged one would go above, or the number of blocks for the end.
    pub(super) before: Option<usize>,
}

/// A comment marker in the gutter.
#[derive(Clone, Copy)]
pub(super) struct CommentMarker {
    pub bounds: Bounds<Pixels>,
    target: MarkerTarget,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerTarget {
    /// The chapter's notes, which live in the panel.
    Notes,
    /// The block whose comments it shows or hides.
    Block(usize),
}

pub(super) fn block_icon(kind: BlockKind, text: &str) -> Option<&'static str> {
    Some(match kind {
        BlockKind::Blank => return None,
        BlockKind::FrontMatter => "icons/block-meta.svg",
        BlockKind::Paragraph if text.trim_start().starts_with("![") => "icons/images.svg",
        BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::List | BlockKind::Quote | BlockKind::Code => {
            slash::type_icon(convert::type_of(kind, text).unwrap_or(BlockType::Paragraph))
        }
        BlockKind::Table => "icons/block-table.svg",
        BlockKind::Directive => "icons/block-directive.svg",
        BlockKind::Comment => "icons/block-comment.svg",
        BlockKind::SceneBreak => "icons/block-break.svg",
        BlockKind::FootnoteDefinition => "icons/block-footnote.svg",
        BlockKind::Raw => "icons/block-raw.svg",
    })
}

impl EditorView {
    /// The comment markers of this frame's blocks. A hidden comment block has no height of its own, so its marker stands
    /// beside the next block that shows.
    pub(super) fn comment_markers(&mut self, blocks: &[FrameBlock]) -> Vec<CommentMarker> {
        let x = self.column.text_left - MARKER_INSET;
        let leading = notes::leading_region(self.session.active_buffer().text(), &self.blocks);
        let mut markers = Vec::new();
        let mut waiting: Option<MarkerTarget> = None;
        let at = |top: Pixels, line_height: Pixels| {
            Bounds::new(
                point(x, top + (line_height - COMMENT_ICON) * 0.5),
                size(COMMENT_ICON, COMMENT_ICON),
            )
        };
        for (block, (index, _)) in blocks.iter().zip(self.visible.clone()) {
            let range = self.blocks.as_slice()[index].range.clone();
            let line_height = block.layout.first_line_height();
            if block.layout.collapsed {
                if block.kind == BlockKind::Comment {
                    let in_notes = leading.as_ref().is_some_and(|leading| leading.contains(&range.start));
                    waiting = waiting.or(Some(if in_notes {
                        MarkerTarget::Notes
                    } else {
                        MarkerTarget::Block(index)
                    }));
                }
                continue;
            }
            if self.source {
                continue;
            }
            if let Some(target) = waiting.take() {
                markers.push(CommentMarker {
                    bounds: at(block.top, line_height),
                    target,
                });
            }
            if block.kind == BlockKind::Comment {
                markers.push(CommentMarker {
                    bounds: at(block.top, line_height),
                    target: MarkerTarget::Block(index),
                });
            }
            let comments = self.slots[index].syntax.as_ref().map_or(Vec::new(), |syntax| {
                syntax
                    .spans
                    .iter()
                    .filter(|span| span.kind == SpanKind::Comment)
                    .map(|span| span.range.start)
                    .collect()
            });
            for comment in comments {
                let line = block
                    .layout
                    .lines
                    .iter()
                    .rfind(|line| line.display.source.start <= comment)
                    .filter(|line| line.height > px(0.));
                if let Some(line) = line {
                    let row = line.row_of(line.display.to_display(comment), false);
                    markers.push(CommentMarker {
                        bounds: at(block.top + line.y + line.line_height * row as f32, line.line_height),
                        target: MarkerTarget::Block(index),
                    });
                }
            }
        }
        if let (Some(target), Some(last)) = (waiting, blocks.last()) {
            markers.push(CommentMarker {
                bounds: at(last.top, last.layout.first_line_height()),
                target,
            });
        }
        markers
    }

    /// Handles a press in the gutter. Returns whether it was one.
    pub(super) fn gutter_mouse_down(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        if position.x < self.column.sheet_left || position.x >= self.column.text_left {
            return false;
        }
        if let Some(marker) = self
            .markers
            .iter()
            .find(|marker| marker.bounds.dilate(px(3.)).contains(&position))
        {
            match marker.target {
                MarkerTarget::Notes => cx.emit(ShowNotes),
                MarkerTarget::Block(index) => {
                    let expanded = &mut self.slots[index].expanded;
                    *expanded = !*expanded;
                    cx.notify();
                }
            }
            return true;
        }
        let Some(block) = self.block_at_y(position.y) else {
            return false;
        };
        if self.blocks.as_slice()[block].kind == BlockKind::Blank || self.is_hidden_block(block) {
            return false;
        }
        let top = self
            .visible
            .iter()
            .find(|(index, _)| *index == block)
            .map_or(position.y, |(_, top)| *top);
        self.grab = Some(BlockGrab {
            block,
            origin: position,
            anchor: Bounds::new(
                point(self.column.sheet_left + ICON_INSET, top),
                size(ICON, tokens::SECTION_HEIGHT),
            ),
            dragging: false,
            before: None,
        });
        cx.notify();
        true
    }

    /// Follows the pointer while a block's icon is held.
    pub(super) fn gutter_mouse_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(grab) = &self.grab else {
            return;
        };
        let distance = f32::from((position.x - grab.origin.x).abs()) + f32::from((position.y - grab.origin.y).abs());
        let block = grab.block;
        let was_dragging = grab.dragging;
        let text = self.session.active_buffer().text();
        let block_data = &self.blocks.as_slice()[block];
        let dragging = was_dragging
            || (distance > DRAG_START && moves::can_move(block_data.kind, &text[block_data.range.clone()]));
        let before = dragging.then(|| self.drop_target(position.y));
        if let Some(grab) = &mut self.grab {
            grab.dragging = dragging;
            grab.before = before;
        }
        cx.notify();
    }

    /// The block a drop at this height goes above: the first one whose middle is below it. Nothing goes above a chapter's
    /// notes or front matter.
    pub(super) fn drop_target(&self, y: Pixels) -> usize {
        let list = self.blocks.as_slice();
        let earliest = list
            .iter()
            .position(|block| {
                !matches!(
                    block.kind,
                    BlockKind::FrontMatter | BlockKind::Blank | BlockKind::Comment
                )
            })
            .unwrap_or(0);
        for &(index, top) in &self.visible {
            let height = self.block_height(index);
            if list[index].kind != BlockKind::Blank && y < top + height * 0.5 && !self.is_hidden_block(index) {
                return index.max(earliest);
            }
        }
        match self.visible.last() {
            Some(&(last, _)) if last + 1 < list.len() => (last + 1).max(earliest),
            _ => list.len(),
        }
    }

    /// Ends a press on an icon: a drag drops the block, and a click opens its menu.
    pub(super) fn gutter_mouse_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(grab) = self.grab.take() else {
            return;
        };
        cx.notify();
        if !grab.dragging {
            self.open_convert(grab.block, grab.anchor, cx);
            return;
        }
        let Some(before) = grab.before else {
            return;
        };
        let text = self.session.active_buffer().text();
        let selection = if self.selection.is_empty() {
            self.selection
        } else {
            Selection::caret(self.selection.head)
        };
        if let Some(command) = moves::move_block(text, &self.blocks, grab.block, before, selection) {
            self.run_command(command, window, cx);
        }
    }

    /// The accent line where a dragged block would land, in the same 2 px as a dragged sidebar row.
    pub(super) fn drop_line(&self) -> Option<Bounds<Pixels>> {
        let before = self.grab.as_ref().filter(|grab| grab.dragging)?.before?;
        let y = self
            .visible
            .iter()
            .find(|(index, _)| *index == before)
            .map(|(_, top)| *top)
            .or_else(|| {
                let (last, top) = *self.visible.last()?;
                Some(top + self.block_height(last))
            })?;
        Some(Bounds::new(
            point(self.column.text_left, y - px(1.)),
            size(self.column.text_width, px(2.)),
        ))
    }
}

/// The writer asked for the chapter's notes.
pub struct ShowNotes;

impl gpui::EventEmitter<ShowNotes> for EditorView {}

/// Paints the gutter. Everything shows quietly, as in the mockups, and the active or hovered block's icon in full.
pub(super) fn paint_gutter(frame: &Frame, window: &mut Window, cx: &mut gpui::App) {
    let column = &frame.column;
    let x = column.sheet_left + ICON_INSET;
    for block in &frame.blocks {
        let (Some(icon), false) = (block.icon, block.layout.collapsed) else {
            continue;
        };
        let line_height = block.layout.first_line_height();
        let bounds = Bounds::new(point(x, block.top + (line_height - ICON) * 0.5), size(ICON, ICON));
        let color: Hsla = if block.active || block.hovered {
            frame.ink
        } else {
            frame.quiet.opacity(0.55)
        };
        window
            .paint_svg(bounds, icon.into(), None, TransformationMatrix::unit(), color, cx)
            .ok();
    }
    for marker in &frame.markers {
        window
            .paint_svg(
                marker.bounds,
                "icons/block-comment.svg".into(),
                None,
                TransformationMatrix::unit(),
                frame.quiet,
                cx,
            )
            .ok();
    }
    if let Some(line) = frame.drop_line {
        window.paint_quad(fill(line, Tokens::get(cx).accent));
    }
}

/// The cursor over the gutter: a hand on an icon, closed while one is dragged.
pub(super) fn gutter_cursor(frame: &Frame) -> CursorStyle {
    if frame.drop_line.is_some() {
        CursorStyle::ClosedHand
    } else {
        CursorStyle::PointingHand
    }
}
