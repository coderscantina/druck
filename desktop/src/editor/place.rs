//! Where floating things go, without gpui: beside what they belong to, never over it if there is room, and always on
//! screen.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// The top left of something of `width` by `height`, below `target` or above it if below does not fit. With no room on
/// either side it stands at the top of `area`, the only case where it covers its target. It is centered on `target` and
/// moves sideways to stay inside `area`.
pub fn beside(target: Rect, width: f32, height: f32, area: Rect, gap: f32) -> (f32, f32) {
    let below = target.bottom + gap;
    let above = target.top - gap - height;
    let y = if below + height <= area.bottom - gap {
        below
    } else if above >= area.top + gap {
        above
    } else {
        area.top + gap
    };
    let centered = (target.left + target.right - width) * 0.5;
    let x = centered.min(area.right - gap - width).max(area.left + gap);
    (x, y)
}

/// The same as [`beside`], but above `target` first, and below it only if above does not fit: the selection toolbar.
pub fn above(target: Rect, width: f32, height: f32, area: Rect, gap: f32) -> (f32, f32) {
    let top = target.top - gap - height;
    let (x, y) = beside(target, width, height, area, gap);
    (x, if top >= area.top + gap { top } else { y })
}

/// The top of something `height` tall above a line that starts at `line_top`, in lines `pitch` apart: centred on whole
/// lines above it, so its edges fall in the space between lines and cut no glyphs, and never reaching the line.
pub fn between_lines(line_top: f32, pitch: f32, height: f32) -> f32 {
    let overhang = (height - pitch) * 0.5;
    let lines = (1. + overhang / pitch).ceil().max(1.);
    line_top - lines * pitch - overhang
}

/// The top left of a list opening at a caret: starting at the caret's left edge rather than centered on it.
pub fn at_caret(target: Rect, width: f32, height: f32, area: Rect, gap: f32) -> (f32, f32) {
    let (_, y) = beside(target, width, height, area, gap);
    (target.left.min(area.right - gap - width).max(area.left + gap), y)
}

/// The left edge nearest `left` for something `width` wide inside `reach`, whose sides both stand at least `room` clear
/// of every word, given as its left and right, so it cuts no glyph. `None` when no such place exists.
pub fn clear_of(left: f32, width: f32, words: &[(f32, f32)], reach: (f32, f32), room: f32) -> Option<f32> {
    const SLACK: f32 = 0.01;
    let clear = |x: f32| (words.iter()).all(|&(start, end)| x <= start - room + SLACK || x >= end + room - SLACK);
    let edges = (words.iter()).flat_map(|&(start, end)| [start - room, end + room]);
    std::iter::once(left)
        .chain(edges.flat_map(|edge| [edge, edge - width]))
        .filter(|x| *x >= reach.0 && x + width <= reach.1 && clear(*x) && clear(x + width))
        .min_by(|a, b| (a - left).abs().total_cmp(&(b - left).abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        left: 0.,
        top: 28.,
        right: 800.,
        bottom: 600.,
    };

    fn line(left: f32, top: f32, right: f32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom: top + 20.,
        }
    }

    #[test]
    fn it_sits_below_and_centered_when_there_is_room() {
        assert_eq!(beside(line(300., 100., 500.), 100., 30., AREA, 6.), (350., 126.));
    }

    #[test]
    fn it_goes_above_when_there_is_no_room_below() {
        assert_eq!(beside(line(300., 570., 500.), 100., 30., AREA, 6.), (350., 534.));
    }

    #[test]
    fn the_toolbar_goes_above_and_below_only_with_no_room_above() {
        assert_eq!(above(line(300., 100., 500.), 100., 30., AREA, 6.), (350., 64.));
        assert_eq!(above(line(300., 40., 500.), 100., 30., AREA, 6.), (350., 66.));
    }

    #[test]
    fn the_toolbar_stands_with_its_edges_between_lines() {
        // 36 high over 30 px lines: over the second line up, 3 px into the space on each side of it.
        assert_eq!(between_lines(300., 30., 36.), 237.);
        // Over 45 px lines it fits over the line just above.
        assert_eq!(between_lines(300., 45., 36.), 259.5);
    }

    #[test]
    fn it_stays_inside_the_sides_of_the_window() {
        assert_eq!(beside(line(0., 100., 20.), 100., 30., AREA, 6.).0, 6.);
        assert_eq!(beside(line(780., 100., 800.), 100., 30., AREA, 6.).0, 694.);
    }

    #[test]
    fn a_list_opens_at_the_caret_and_stays_inside_the_window() {
        assert_eq!(at_caret(line(300., 100., 301.), 240., 200., AREA, 6.), (300., 126.));
        assert_eq!(at_caret(line(700., 500., 701.), 240., 200., AREA, 6.), (554., 294.));
    }

    #[test]
    fn a_selection_taller_than_the_room_gets_it_at_the_top() {
        let tall = Rect {
            left: 100.,
            top: 30.,
            right: 300.,
            bottom: 590.,
        };
        assert_eq!(beside(tall, 100., 30., AREA, 6.).1, 34.);
    }

    #[test]
    fn the_toolbar_finds_the_nearest_place_clear_of_glyphs() {
        let words = [(0., 60.), (65., 120.), (125., 200.), (205., 300.), (305., 330.)];
        // Clearing either edge's word alone puts the other edge into one; past the line's end both edges are clear.
        assert_eq!(clear_of(110., 150., &words, (0., 500.), 2.), Some(202.));
        assert_eq!(clear_of(110., 150., &words, (0., 250.), 2.), None);
        assert_eq!(clear_of(340., 150., &words, (0., 500.), 2.), Some(340.));
    }
}
