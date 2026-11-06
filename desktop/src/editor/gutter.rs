//! The block gutter: a type icon beside each block, comment markers, and dragging a block by its icon. It stands left of
//! the text, and is empty at rest and while typing: it shows when the pointer enters the left margin.

use gpui::{Bounds, Context, CursorStyle, Hsla, Pixels, Point, TransformationMatrix, Window, fill, point, px, size};

use super::commands::convert::{self, BlockType};
use super::commands::{moves, notes, slash};
use super::element::{Frame, FrameBlock};
use super::{EditorView, Tokens};
use crate::source::inline::SpanKind;
use crate::source::{BlockKind, Selection};
use crate::tokens;

/// How far the pointer must move before a press on an icon is a drag, not a click.
const DRAG_START: f32 = 4.;

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
    /// The block whose comments it shows or hides.
    block: usize,
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
    /// beside the next block that shows. A chapter's notes have none, since they stand in the margin.
    pub(super) fn comment_markers(&mut self, blocks: &[FrameBlock]) -> Vec<CommentMarker> {
        let x = self.column.text_left - tokens::COMMENT_MARKER_INSET;
        let leading = notes::leading_region(self.session.active_buffer().text(), &self.blocks);
        let mut markers = Vec::new();
        let mut waiting: Option<usize> = None;
        let at = |top: Pixels, line_height: Pixels| {
            Bounds::new(
                point(x, top + (line_height - tokens::COMMENT_ICON) * 0.5),
                size(tokens::COMMENT_ICON, tokens::COMMENT_ICON),
            )
        };
        for (block, (index, _)) in blocks.iter().zip(self.visible.clone()) {
            let range = self.blocks.as_slice()[index].range.clone();
            let line_height = block.layout.first_line_height();
            // A block in a second column has its marker in the gap between the columns.
            let x = block.left - tokens::COMMENT_MARKER_INSET;
            let at = |top: Pixels, line_height: Pixels| {
                Bounds::new(
                    point(x, top + (line_height - tokens::COMMENT_ICON) * 0.5),
                    size(tokens::COMMENT_ICON, tokens::COMMENT_ICON),
                )
            };
            if block.layout.collapsed {
                if block.kind == BlockKind::Comment {
                    let in_notes = leading.as_ref().is_some_and(|leading| leading.contains(&range.start));
                    if !in_notes {
                        waiting = waiting.or(Some(index));
                    }
                }
                continue;
            }
            if self.source {
                continue;
            }
            if let Some(block_index) = waiting.take() {
                markers.push(CommentMarker {
                    bounds: at(block.top + block.layout.above, line_height),
                    block: block_index,
                });
            }
            if block.kind == BlockKind::Comment
                && !crate::persist::merge::is_marker(&self.session.active_buffer().text()[range.clone()])
            {
                markers.push(CommentMarker {
                    bounds: at(block.top + block.layout.above, line_height),
                    block: index,
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
                        block: index,
                    });
                }
            }
        }
        if let (Some(block), Some(last)) = (waiting, blocks.last()) {
            markers.push(CommentMarker {
                bounds: at(last.top + last.layout.above, last.layout.first_line_height()),
                block,
            });
        }
        markers
    }

    /// Handles a press in the gutter. Returns whether it was one.
    pub(super) fn gutter_mouse_down(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let gutter_left = self.column.text_left - tokens::GUTTER_WIDTH;
        if position.x < gutter_left || position.x >= self.column.text_left {
            return false;
        }
        if let Some(marker) = self
            .markers
            .iter()
            .find(|marker| marker.bounds.dilate(tokens::MARKER_HIT).contains(&position))
        {
            let expanded = &mut self.slots[marker.block].expanded;
            *expanded = !*expanded;
            cx.notify();
            return true;
        }
        let Some(block) = self.block_at(position) else {
            return false;
        };
        if self.blocks.as_slice()[block].kind == BlockKind::Blank || self.takes_no_room(block) {
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
                point(gutter_left + tokens::GUTTER_ICON_INSET, top),
                size(tokens::GUTTER_ICON, tokens::SECTION_HEIGHT),
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
            let height = self.own_height(index);
            if list[index].kind != BlockKind::Blank && y < top + height * 0.5 && !self.takes_no_room(index) {
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
                Some(top + self.own_height(last))
            })?;
        let line = before.min(self.blocks.as_slice().len() - 1);
        Some(Bounds::new(
            point(self.block_left(line), y - tokens::HAIRLINE),
            size(self.block_width(line), tokens::KEY_BAR),
        ))
    }
}

/// Paints the gutter while the pointer is in the left margin: the hovered block's icon in full ink, the others in the
/// quiet color. A dragged block's line shows until it lands.
pub(super) fn paint_gutter(frame: &Frame, window: &mut Window, cx: &mut gpui::App) {
    if let Some(line) = frame.drop_line {
        window.paint_quad(fill(line, Tokens::get(cx).accent));
    }
    if !frame.gutter_shown {
        return;
    }
    for block in &frame.blocks {
        let (Some(icon), false) = (block.icon, block.layout.collapsed) else {
            continue;
        };
        let x = block.icon_x;
        let line_height = block.layout.first_line_height();
        let bounds = Bounds::new(
            point(
                x,
                block.top + block.layout.above + (line_height - tokens::GUTTER_ICON) * 0.5,
            ),
            size(tokens::GUTTER_ICON, tokens::GUTTER_ICON),
        );
        let color: Hsla = if block.hovered {
            frame.ink
        } else {
            frame.quiet.opacity(tokens::GUTTER_REST)
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
}

/// The cursor over the gutter: a hand on an icon, closed while one is dragged.
pub(super) fn gutter_cursor(frame: &Frame) -> CursorStyle {
    if frame.drop_line.is_some() {
        CursorStyle::ClosedHand
    } else {
        CursorStyle::PointingHand
    }
}
