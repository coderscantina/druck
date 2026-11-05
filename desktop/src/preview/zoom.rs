//! The one zoom scale and the view's place: contact sheets below fit, then fit, then larger, how the sheets stand at a
//! zoom, scrolling, and the zoom's controls at the status line's right.

use super::*;

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

impl PreviewView {
    /// The zoom to keep between runs. `None` fits the pages to the pane.
    pub fn zoom(&self) -> Option<f32> {
        match self.zoom {
            Zoom::Fit => None,
            Zoom::Fixed(zoom) => Some(zoom),
        }
    }

    /// Sets the zoom from a past run. `None` fits the pages to the pane.
    pub fn set_zoom(&mut self, zoom: Option<f32>, cx: &mut Context<Self>) {
        self.zoom = zoom.map_or(Zoom::Fit, Zoom::Fixed);
        self.prepare_ahead(cx);
        cx.notify();
    }

    pub(super) fn set_zoom_by_author(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        self.prepare_ahead(cx);
        cx.emit(ZoomChanged);
        cx.notify();
    }

    /// Fits a spread to the pane.
    pub fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        self.set_zoom_by_author(Zoom::Fit, cx);
    }

    /// One step along the zoom scale: larger for a positive `direction`, smaller for a negative one. Past fit it becomes a
    /// contact sheet.
    pub fn zoom_step(&mut self, direction: isize, cx: &mut Context<Self>) {
        if let Some(next) = next_zoom(self.zoom, self.current_zoom, self.fit, direction) {
            self.set_zoom_by_author(next, cx);
        }
    }

    /// Page Down or Up. At fit it moves one row of sheets, so a spread lands whole in the middle. Larger, it moves most
    /// of a screen.
    pub(super) fn page_screen(&mut self, forward: bool, cx: &mut Context<Self>) {
        let view = self.view.1;
        let row = (self.zoom == Zoom::Fit)
            .then(|| self.sheets(self.current_zoom))
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
        cx.notify();
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

    pub(super) fn sheets(&self, zoom: f32) -> Option<Sheets> {
        let layout = self.layout.as_ref()?;
        let pages = (layout.laid.pages().iter()).map(|page| (page.width.0 as f32, page.height.0 as f32));
        let arrangement = self.arrangement(zoom);
        let gap = match arrangement {
            Arrangement::Spreads { .. } => tokens::SPACE_4,
            Arrangement::Contact { .. } => tokens::CONTACT_GAP,
        };
        Some(Sheets::new(pages, zoom, f32::from(gap), arrangement))
    }

    /// The zoom at which a whole spread fills a view this large.
    pub(super) fn fit_zoom(&self, (width, height): (f32, f32)) -> f32 {
        let Some(layout) = self.layout.as_ref() else {
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

    pub(super) fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(tokens::SCROLL_LINE);
        self.scroll.0 -= f32::from(delta.x);
        self.scroll.1 -= f32::from(delta.y);
        cx.notify();
    }

    /// The status line's zoom: smaller, the value, larger, and fit.
    pub(super) fn zoom_controls(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
        let percent = format!("{}%", (self.current_zoom * 100.0).round());
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
            .child(
                button(
                    "zoom-fit",
                    crate::t!(Key::ZoomFit).into(),
                    crate::t!(Key::ZoomFit),
                    self.zoom == Zoom::Fit,
                )
                .on_click(cx.listener(|view, _, _, cx| view.zoom_to_fit(cx))),
            )
    }
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
