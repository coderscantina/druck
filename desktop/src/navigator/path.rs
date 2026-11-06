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

/// The widths the segments get in `room`, with `gap` between two: the segment at the left gives up its room first, and
/// goes with its gap where it would keep no more than `least`, then the next, so the one nearest the caret keeps its
/// words longest. The last one stays, at least `least` wide. A segment that went is `None`.
pub fn fit(wanted: &[f32], room: f32, least: f32, gap: f32) -> Vec<Option<f32>> {
    let mut widths: Vec<Option<f32>> = wanted.iter().copied().map(Some).collect();
    let mut over = wanted.iter().sum::<f32>() + gap * wanted.len().saturating_sub(1) as f32 - room;
    let last = widths.len().saturating_sub(1);
    for (at, slot) in widths.iter_mut().enumerate() {
        let Some(width) = *slot else { continue };
        if over <= 0. {
            break;
        }
        if at < last && over >= width - least {
            *slot = None;
            over -= width + gap;
        } else {
            let cut = over.min((width - least).max(0.));
            *slot = Some(width - cut);
            over -= cut;
        }
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
        let padding = tokens::NAV_SEGMENT_X * 2.;
        let wanted: Vec<f32> = segments
            .iter()
            .map(|segment| f32::from(crate::shell::ui::small_text_width(&segment.label, window) + padding).ceil())
            .collect();
        let widths = fit(
            &wanted,
            f32::from(room),
            f32::from(tokens::NAV_SEGMENT_LEAST),
            f32::from(tokens::NAV_CHEVRON + tokens::HAIR_GAP * 2.),
        );
        let shown: Vec<_> = (segments.into_iter().zip(widths))
            .filter_map(|(segment, width)| Some((segment, width?)))
            .collect();
        let count = shown.len();
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
        for (at, (segment, width)) in shown.into_iter().enumerate() {
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
    fn the_left_segments_give_up_their_room_first_and_go_before_they_show_nothing() {
        assert_eq!(
            fit(&[80., 120., 100.], 420., 24., 10.),
            [Some(80.), Some(120.), Some(100.)]
        );
        assert_eq!(
            fit(&[80., 120., 100.], 270., 24., 10.),
            [Some(30.), Some(120.), Some(100.)]
        );
        assert_eq!(fit(&[80., 120., 100.], 200., 24., 10.), [None, Some(90.), Some(100.)]);
        assert_eq!(fit(&[80., 120., 100.], 60., 24., 10.), [None, None, Some(60.)]);
        assert_eq!(fit(&[80., 120., 100.], 10., 24., 10.), [None, None, Some(24.)]);
    }
}
