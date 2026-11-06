//! The one zoom scale and the view's place: contact sheets below fit, then fit, then larger, how the sheets stand at a
//! zoom, scrolling, and the zoom's controls at the status line's right. [`Place`] owns all of it, and the view asks it.

use gpui::prelude::*;
use gpui::{Context, Pixels, Point, SharedString, div};

use super::PreviewView;
use super::job::Layout;
use super::sheets::{Arrangement, Sheets};
use super::strings::Key;
use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

/// Zoom levels above fit, as logical pixels per point.
pub(super) const ZOOMS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];

/// Zoom levels below fit, which show a contact sheet. Small enough that it reads as one, whatever the window.
pub(super) const CONTACT_ZOOMS: [f32; 4] = [0.14, 0.2, 0.3, 0.45];

/// Fit follows the view below every fixed level, so a narrow window shows a whole spread. This only keeps a sliver of a
/// view from asking for sheets of no size.
pub(super) const SMALLEST_FIT: f32 = 0.02;

/// How large sheets are drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Zoom {
    /// A whole spread fills the view.
    Fit,
    Fixed(f32),
}

/// The next step of the one zoom scale from `now`, which is `current` pixels per point, where a spread fits at `fit`:
/// contact sheets below fit, then fit, then larger. `None` at either end.
pub(super) fn next_zoom(now: Zoom, current: f32, fit: f32, direction: isize) -> Option<Zoom> {
    let below_fit = CONTACT_ZOOMS.iter().copied().filter(|zoom| *zoom < fit * 0.99);
    let above_fit = ZOOMS.iter().copied().filter(|zoom| *zoom > fit * 1.01);
    let levels = below_fit
        .map(Zoom::Fixed)
        .chain([Zoom::Fit])
        .chain(above_fit.map(Zoom::Fixed));
    let value = |zoom: &Zoom| match zoom {
        Zoom::Fit => fit,
        Zoom::Fixed(zoom) => *zoom,
    };
    let current = if now == Zoom::Fit { fit } else { current };
    if direction > 0 {
        levels.into_iter().find(|zoom| value(zoom) > current * 1.01)
    } else {
        levels.into_iter().rev().find(|zoom| value(zoom) < current * 0.99)
    }
}

/// The zoom the author chose and where the view stands among the sheets.
#[derive(Debug)]
pub(super) struct Place {
    pub zoom: Zoom,
    /// The zoom of the last frame, for moves between zoom levels.
    pub current_zoom: f32,
    /// The zoom at which a spread fills the view. Below it, sheets show as a contact sheet.
    pub fit: f32,
    /// The page to bring to the middle of the view on the next frame, once the new zoom is known.
    pub centre_on: Option<usize>,
    /// The page a change of zoom last kept in the middle, with the scroll it left. While the view has not moved since,
    /// the next change keeps the same page, so a step out to the contact sheet and back returns to the starting spread.
    pub held: Option<(usize, f32)>,
    pub scroll: (f32, f32),
    pub view: (f32, f32),
    /// The first page on screen in the last frame.
    pub page: usize,
}

impl Default for Place {
    fn default() -> Self {
        Self {
            zoom: Zoom::Fit,
            current_zoom: 1.0,
            fit: 1.0,
            centre_on: None,
            held: None,
            scroll: (0.0, 0.0),
            view: (0.0, 0.0),
            page: 0,
        }
    }
}

impl Place {
    /// The zoom to keep between runs. `None` fits the pages to the pane.
    pub(super) fn kept_zoom(&self) -> Option<f32> {
        match self.zoom {
            Zoom::Fit => None,
            Zoom::Fixed(zoom) => Some(zoom),
        }
    }

    /// One step along the zoom scale from where it is, if there is one.
    pub(super) fn step(&self, direction: isize) -> Option<Zoom> {
        next_zoom(self.zoom, self.current_zoom, self.fit, direction)
    }

    pub(super) fn is_contact_sheet(&self) -> bool {
        self.current_zoom < self.fit * 0.999
    }

    pub(super) fn arrangement(&self, zoom: f32) -> Arrangement {
        if zoom < self.fit * 0.999 {
            Arrangement::Contact {
                view: self.view.0,
                gutter: f32::from(tokens::SPREAD_GUTTER),
            }
        } else {
            Arrangement::Spreads {
                gutter: f32::from(tokens::SPREAD_GUTTER),
            }
        }
    }

    /// How `layout`'s pages stand at `zoom`.
    pub(super) fn sheets(&self, layout: Option<&Layout>, zoom: f32) -> Option<Sheets> {
        let layout = layout?;
        let pages = (layout.laid.pages().iter()).map(|page| (page.width.0 as f32, page.height.0 as f32));
        let arrangement = self.arrangement(zoom);
        let gap = match arrangement {
            Arrangement::Spreads { .. } => tokens::SPACE_4,
            Arrangement::Contact { .. } => tokens::CONTACT_GAP,
        };
        Some(Sheets::new(pages, zoom, f32::from(gap), arrangement))
    }

    /// The zoom at which a whole spread of `layout` fills a view this large.
    pub(super) fn fit_zoom(&self, layout: Option<&Layout>, (width, height): (f32, f32)) -> f32 {
        let Some(layout) = layout else {
            return self.fit;
        };
        let pages = layout.laid.pages();
        let widest = pages.iter().map(|page| page.width.0 as f32).fold(1.0, f32::max);
        let tallest = pages.iter().map(|page| page.height.0 as f32).fold(1.0, f32::max);
        let margins = 2.0 * f32::from(tokens::SPACE_4);
        let gutter = f32::from(tokens::SPREAD_GUTTER);
        ((width - margins - gutter) / (2.0 * widest))
            .min((height - margins) / tallest)
            .clamp(SMALLEST_FIT, ZOOMS[ZOOMS.len() - 1])
    }

    /// Page Down or Up. At fit it moves one row of sheets, so a spread lands whole in the middle. Larger, it moves most
    /// of a screen.
    pub(super) fn page_screen(&mut self, layout: Option<&Layout>, forward: bool) {
        let view = self.view.1;
        let row = (self.zoom == Zoom::Fit)
            .then(|| self.sheets(layout, self.current_zoom))
            .flatten()
            .and_then(|sheets| {
                let here = sheets.row_pages(sheets.page_at_middle(self.scroll.1, view)?);
                let next = if forward {
                    (here.end < sheets.len()).then_some(here.end)
                } else {
                    here.start.checked_sub(1)
                };
                Some(next.map_or(self.scroll.1, |page| sheets.scroll_to_centre(page, view)))
            });
        self.scroll.1 = row.unwrap_or(self.scroll.1 + if forward { view } else { -view } * 0.9);
    }

    /// Scrolls by a wheel's or trackpad's move.
    pub(super) fn scroll_by(&mut self, delta: Point<Pixels>) {
        self.scroll.0 -= f32::from(delta.x);
        self.scroll.1 -= f32::from(delta.y);
    }
}

/// The status line's zoom: smaller, the value, larger, and fit.
/// Minus, the value, plus, and Fit while `fit` has room.
pub(super) fn zoom_controls(
    place: &Place,
    fit: bool,
    tokens: &Tokens,
    cx: &mut Context<PreviewView>,
) -> impl IntoElement + use<> {
    let button = |id: &'static str, label: SharedString, aria: &'static str, on: bool| {
        div()
            .id(id)
            .button(tokens, aria)
            .flex_none()
            .min_w(tokens::ZOOM_BUTTON)
            .h(tokens::ZOOM_BUTTON)
            .px(tokens::SPACE_1)
            .flex()
            .items_center()
            .justify_center()
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .text_color(if on { tokens.text } else { tokens.text_quiet })
            .hover(|button| button.bg(tokens.raised))
            .child(label)
    };
    let percent = format!("{}%", (place.current_zoom * 100.0).round());
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(tokens::HAIR_GAP)
        .child(
            button("zoom-out", "\u{2212}".into(), crate::t!(Key::ZoomOut), false)
                .on_click(cx.listener(|view, _, _, cx| view.zoom_step(-1, cx))),
        )
        .child(
            div()
                .flex_none()
                .px(tokens::SPACE_1)
                .text_color(tokens.text)
                .child(percent),
        )
        .child(
            button("zoom-in", "+".into(), crate::t!(Key::ZoomIn), false)
                .on_click(cx.listener(|view, _, _, cx| view.zoom_step(1, cx))),
        )
        .when(fit, |controls| {
            controls.child(
                button(
                    "zoom-fit",
                    crate::t!(Key::ZoomFit).into(),
                    crate::t!(Key::ZoomFit),
                    place.zoom == Zoom::Fit,
                )
                .on_click(cx.listener(|view, _, _, cx| view.zoom_to_fit(cx))),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_zoom_scale_runs_from_contact_sheets_through_fit_to_larger() {
        let step = |now, current, direction| next_zoom(now, current, 1.53, direction);
        assert_eq!(step(Zoom::Fit, 1.53, -1), Some(Zoom::Fixed(0.45)));
        assert_eq!(step(Zoom::Fixed(0.45), 0.45, -1), Some(Zoom::Fixed(0.3)));
        assert_eq!(step(Zoom::Fixed(0.14), 0.14, -1), None);
        assert_eq!(step(Zoom::Fixed(0.45), 0.45, 1), Some(Zoom::Fit));
        assert_eq!(step(Zoom::Fit, 1.53, 1), Some(Zoom::Fixed(2.0)));
        assert_eq!(step(Zoom::Fixed(2.0), 2.0, -1), Some(Zoom::Fit));
        assert_eq!(step(Zoom::Fixed(3.0), 3.0, 1), None);
    }

    #[test]
    fn fit_goes_below_every_fixed_level() {
        assert_eq!(next_zoom(Zoom::Fit, 0.1, 0.1, -1), None);
        assert_eq!(next_zoom(Zoom::Fit, 0.1, 0.1, 1), Some(Zoom::Fixed(0.5)));
    }

    #[test]
    fn a_level_on_the_wrong_side_of_fit_is_skipped() {
        // In a small window a spread fits at 0.4, so the largest contact level would be larger than fit.
        assert_eq!(next_zoom(Zoom::Fit, 0.4, 0.4, -1), Some(Zoom::Fixed(0.3)));
        assert_eq!(next_zoom(Zoom::Fit, 0.4, 0.4, 1), Some(Zoom::Fixed(0.5)));
    }
}
