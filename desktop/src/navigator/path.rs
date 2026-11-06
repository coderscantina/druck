//! The path bar at the title bar's left in Write: part, chapter, and section where the caret is, faint while typing.
//! It never runs into the mode switch at the centre: the segments at the left give up their room first.

use gpui::prelude::*;
use gpui::{Context, ElementId, Entity, MouseButton, Pixels, Role, Window, div, px, svg};

use super::Navigator;
use super::strings::Key;
use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

pub struct PathBar {
    navigator: Entity<Navigator>,
}

impl PathBar {
    pub fn new(navigator: Entity<Navigator>, cx: &mut Context<Self>) -> Self {
        cx.observe(&navigator, |_, _, cx| cx.notify()).detach();
        Self { navigator }
    }
}

/// The widths the segments get in `room`: the segment at the left gives up its room first, down to `least`, then the
/// next, so the one nearest the caret keeps its words longest.
pub fn fit(wanted: &[f32], room: f32, least: f32) -> Vec<f32> {
    let mut widths = wanted.to_vec();
    let mut over = wanted.iter().sum::<f32>() - room;
    for width in &mut widths {
        if over <= 0. {
            break;
        }
        let cut = over.min((*width - least).max(0.));
        *width -= cut;
        over -= cut;
    }
    widths
}

impl Render for PathBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let navigator = self.navigator.read(cx);
        let segments = navigator.segments();
        let open = navigator.open_level();
        let overview = navigator.overview_open();
        let color = if navigator.is_typing() {
            tokens.text_faint
        } else {
            tokens.text_quiet
        };
        // The room between the traffic lights and the mode switch at the centre.
        let width = window.viewport_size().width;
        let room =
            width / 2. - crate::workspace::mode_switch_width(window) / 2. - tokens::NAV_PATH_LEFT - tokens::SPACE_3;
        let chevrons = (tokens::NAV_CHEVRON + tokens::HAIR_GAP * 2.) * segments.len().saturating_sub(1) as f32;
        let padding = tokens::NAV_SEGMENT_X * 2.;
        let wanted: Vec<f32> = segments
            .iter()
            .map(|segment| f32::from(crate::shell::ui::small_text_width(&segment.label, window) + padding))
            .collect();
        let widths = fit(
            &wanted,
            f32::from(room - chevrons),
            f32::from(tokens::NAV_SEGMENT_LEAST),
        );
        let count = segments.len();
        let mut bar = div()
            .id("path-bar")
            .absolute()
            .left(tokens::NAV_PATH_LEFT)
            .top_0()
            .h_full()
            .max_w(room.max(Pixels::ZERO))
            .flex()
            .items_center()
            .gap(tokens::HAIR_GAP)
            .overflow_hidden()
            .role(Role::Navigation)
            .aria_label(crate::t!(Key::Path))
            .text_color(color);
        for (at, (segment, width)) in segments.into_iter().zip(widths).enumerate() {
            let level = segment.level;
            // The overview's own word stands as plain quiet text, like a segment at rest.
            let on = !overview && open == Some(level);
            bar = bar.child(
                div()
                    .id(ElementId::from(("path-segment", at)))
                    .button(&tokens, segment.label.clone())
                    .flex_none()
                    .w(px(width))
                    .px(tokens::NAV_SEGMENT_X)
                    .rounded(tokens::CHIP_RADIUS)
                    .truncate()
                    .cursor_pointer()
                    .when(on, |segment| segment.bg(tokens.raised).text_color(tokens.text))
                    .hover(|segment| segment.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |path, _, window, cx| {
                        path.navigator.update(cx, |navigator, cx| {
                            if overview {
                                navigator.toggle_overview(window, cx);
                            } else {
                                navigator.segment_clicked(level, window, cx);
                            }
                        });
                    }))
                    .child(segment.label),
            );
            if at + 1 < count {
                bar = bar.child(
                    svg()
                        .flex_none()
                        .path("icons/chevron-right.svg")
                        .size(tokens::NAV_CHEVRON)
                        .text_color(tokens.text_faint),
                );
            }
        }
        bar
    }
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn the_left_segments_give_up_their_room_first() {
        assert_eq!(fit(&[80., 120., 100.], 400., 24.), [80., 120., 100.]);
        assert_eq!(fit(&[80., 120., 100.], 250., 24.), [30., 120., 100.]);
        assert_eq!(fit(&[80., 120., 100.], 180., 24.), [24., 56., 100.]);
        assert_eq!(fit(&[80., 120., 100.], 60., 24.), [24., 24., 24.]);
    }
}
