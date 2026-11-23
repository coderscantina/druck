//! A scene break in Write: a short centred rule with about a body line of room above and below it. While the caret is
//! on its line it shows its source there instead, quiet, in the same room, so nothing moves.

use gpui::{Bounds, Pixels, point, px, size};

use super::EditorView;
use super::element::FrameBlock;
use crate::source::BlockKind;
use crate::tokens;

/// The body lines a scene break takes, with its rule in the middle.
const SCENE_LINES: f32 = 2.;
/// The rule's width in body sizes: 48 px at Write's starting zoom, where the body is about 18 px.
const RULE_EMS: f32 = 48. / 18.;

/// The room a scene break keeps above and below its line, which shows only while the caret is on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SceneRoom {
    pub at_rest: bool,
    pub around: Pixels,
}

/// The room around a scene break's line: the same height whether its source shows or not.
pub(super) fn scene_room(line: Pixels, at_rest: bool) -> SceneRoom {
    let shown = if at_rest { px(0.) } else { line };
    SceneRoom {
        at_rest,
        around: (line * SCENE_LINES - shown) * 0.5,
    }
}

impl EditorView {
    /// The room of a scene break at block `index`, which the source view shows as written.
    pub(super) fn scene_break_room(&self, index: usize) -> Option<SceneRoom> {
        if self.source || self.blocks.as_slice()[index].kind != BlockKind::SceneBreak {
            return None;
        }
        let at_rest = self.reveal_offset().is_none_or(|at| self.blocks.index_at(at) != index);
        Some(scene_room(self.style.body_size * self.style.leading, at_rest))
    }

    /// The rules of the scene breaks at rest on screen, in the middle of their room.
    pub(super) fn scene_rules(&self, blocks: &[FrameBlock]) -> Vec<Bounds<Pixels>> {
        if self.source {
            return Vec::new();
        }
        let width = self.style.body_size * RULE_EMS;
        (blocks.iter())
            .filter(|block| block.kind == BlockKind::SceneBreak)
            .filter(|block| block.layout.lines.iter().all(|line| line.height == px(0.)))
            .map(|block| {
                let left = block.left + (block.width - width) * 0.5;
                Bounds::new(point(left, block.top + block.layout.above), size(width, tokens::HAIRLINE))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_break_keeps_its_height_when_the_caret_shows_its_source() {
        let line = px(24.);
        let rest = scene_room(line, true);
        let shown = scene_room(line, false);
        assert_eq!(rest.around * 2., shown.around * 2. + line);
        assert_eq!(rest.around, line);
    }
}
