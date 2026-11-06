//! The status line under the pages: which pages show, what is left to look at, the warnings with their Marks switch,
//! the window's foot line, and the zoom.

use gpui::prelude::*;
use gpui::{AnyView, Context, Role, SharedString, Toggled, Window, div};

use super::strings::Key;
use super::{PreviewView, ProblemsWanted, in_words};
use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

impl PreviewView {
    /// Whether the status line holds `parts` in full beside the zoom, with the Marks `switch` and the zoom's `fit` when
    /// they show.
    pub(super) fn status_fits(&self, parts: &[&str], switch: bool, fit: bool, window: &Window) -> bool {
        let width = |text: &str| f32::from(crate::shell::ui::small_text_width(text, window));
        let words: f32 = parts
            .iter()
            .filter(|part| !part.is_empty())
            .map(|part| width(part))
            .sum();
        let percent = format!("{}%", (self.place.current_zoom * 100.0).round());
        let mut zoom =
            width(&percent) + f32::from(tokens::ZOOM_BUTTON * 2. + tokens::SPACE_1 * 4. + tokens::HAIR_GAP * 2.);
        if fit {
            zoom += width(crate::t!(Key::ZoomFit)) + f32::from(tokens::SPACE_1 * 2. + tokens::HAIR_GAP);
        }
        if switch {
            zoom += f32::from(tokens::SWITCH_WIDTH + tokens::SPACE_1 * 3.);
        }
        let gaps = f32::from(tokens::SPACE_3) * (parts.len() + 3) as f32;
        words + zoom + gaps <= self.place.view.0
    }

    /// The window's foot line, to show in the status line.
    pub fn set_foot(&mut self, foot: Vec<AnyView>) {
        self.foot = foot;
    }

    /// Which pages show, and the numbers printed on them when those differ from their places in the file.
    pub(super) fn status(&self) -> (String, Option<String>) {
        let Some(layout) = &self.layout else {
            let text = if self.diagnostics.is_empty() {
                crate::t!(Key::NoPagesYet).to_owned()
            } else {
                crate::t!(Key::NoPages).to_owned()
            };
            return (text, None);
        };
        let pages = layout.laid.pages().len();
        if self.is_contact_sheet() {
            let text = match pages {
                1 => crate::t!(Key::OnePage).to_owned(),
                pages => crate::t!(Key::PageCount, pages = pages),
            };
            return (text, None);
        }
        let shown = self.sheets(self.place.current_zoom).and_then(|sheets| {
            let page = sheets.page_at_middle(self.place.scroll.1, self.place.view.1)?;
            Some(sheets.row_pages(page))
        });
        let shown = shown.unwrap_or(self.place.page..self.place.page + 1);
        let shown = shown.start.min(pages - 1)..shown.end.min(pages);
        let range = if shown.len() > 1 {
            crate::t!(Key::PagesOf, first = shown.start + 1, last = shown.end, pages = pages)
        } else {
            crate::t!(Key::PageOf, page = shown.start + 1, pages = pages)
        };
        (range, printed_labels(&layout.laid, shown))
    }

    /// The line itself. A narrow one drops the printed page numbers, then the word "warnings", then the switch's label,
    /// then the switch, then the zoom's Fit, and never cuts a number.
    pub(super) fn status_line(
        &self,
        tokens: &Tokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // With no pages the whole message stands where they would, see `empty_state`.
        let problem = self.layout.as_ref().and(self.diagnostics.first()).map(|first| {
            let more = self.diagnostics.len() - 1;
            let suffix = if more > 0 { format!(" (+{more})") } else { String::new() };
            format!("{}{suffix}", in_words(first, &self.base))
        });
        let warnings = self.problems.iter().filter(|problem| problem.warning).count();
        let count = self.problems.len() - warnings;
        let look_label = (count > 0).then(|| crate::t!(Key::ToLookAt, count = count));
        // The printed numbers go first when the line is short, so the zoom is never cut.
        let status = match self.status() {
            (range, Some(labels)) => {
                let full = format!("{range} ({labels})");
                let fewest = [&full, look_label.as_deref().unwrap_or_default(), &warnings.to_string()];
                if self.status_fits(&fewest, warnings > 0, true, window) {
                    full
                } else {
                    range
                }
            }
            (range, None) => range,
        };
        let to_look_at = look_label.clone().map(|label| {
            div()
                .id("to-look-at")
                .control(tokens, Role::Link, SharedString::from(label.clone()))
                .flex_none()
                .text_color(tokens.accent)
                .cursor_pointer()
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ProblemsWanted)))
                .child(label)
        });
        let look = look_label.as_deref().unwrap_or_default();
        let toggle = crate::t!(Key::WarningMarks);
        let warning_label = match warnings {
            0 => String::new(),
            1 => crate::t!(Key::OneWarning).to_owned(),
            warnings => crate::t!(Key::Warnings, count = warnings),
        };
        let number = if warnings > 0 {
            warnings.to_string()
        } else {
            String::new()
        };
        // A narrow line drops the word "warnings", then the switch's label, then the switch, then Fit.
        let fits = |parts: &[&str], switch| self.status_fits(parts, switch && warnings > 0, true, window);
        let (shown, toggle_label, switch) = if fits(&[&status, look, &warning_label, toggle], true) {
            (warning_label.clone(), Some(toggle), true)
        } else if fits(&[&status, look, &number, toggle], true) {
            (number, Some(toggle), true)
        } else if fits(&[&status, look, &number], true) {
            (number, None, true)
        } else {
            (number, None, false)
        };
        let fit = self.status_fits(&[&status, look, &shown], switch && warnings > 0, true, window);
        let warning_count = (warnings > 0).then(|| {
            let marks_on = crate::settings::get(cx).warning_marks;
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(tokens::SPACE_1)
                .child(
                    div()
                        .id("warnings")
                        .control(tokens, Role::Link, SharedString::from(warning_label.clone()))
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ProblemsWanted)))
                        .child(shown),
                )
                .when(switch, |line| {
                    line.child(
                        div()
                            .id("warning-marks")
                            .control(tokens, Role::Switch, toggle)
                            .aria_toggled(if marks_on { Toggled::True } else { Toggled::False })
                            .flex_none()
                            .h(tokens::ZOOM_BUTTON)
                            .px(tokens::SPACE_1)
                            .flex()
                            .items_center()
                            .gap(tokens::SPACE_1)
                            .rounded(tokens::RADIUS)
                            .cursor_pointer()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| {
                                crate::settings::change(cx, |settings| {
                                    settings.warning_marks = !settings.warning_marks
                                });
                            })
                            .child(crate::shell::ui::switch_track(tokens, marks_on))
                            .children(toggle_label),
                    )
                })
        });
        div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_3)
            .border_t_1()
            .border_color(tokens.divider)
            .bg(tokens.canvas)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .child(div().flex_none().child(status))
            .children(to_look_at)
            .children(warning_count)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .when_some(problem, |slot, problem| {
                        slot.child(div().min_w_0().truncate().text_color(tokens.accent).child(problem))
                    })
                    .children(self.foot.clone()),
            )
            .child(super::zoom::zoom_controls(&self.place, fit, tokens, cx))
    }
}

/// The number printed on a page, if it has one.
pub(super) fn printed_label(laid: &druck::output::Layout, page: usize) -> Option<String> {
    let number = laid.page_numbers().get(page).copied().flatten()?;
    Some(number.format.format(number.value))
}

/// The numbers printed on the pages in view, as `iii–iv`, when they say something the pages' places in the file do not.
pub(super) fn printed_labels(laid: &druck::output::Layout, pages: std::ops::Range<usize>) -> Option<String> {
    let labels: Vec<(usize, String)> = pages
        .filter_map(|page| Some((page, printed_label(laid, page)?)))
        .collect();
    if labels.iter().all(|(page, label)| *label == (page + 1).to_string()) {
        return None;
    }
    match labels.as_slice() {
        [(_, only)] => Some(only.clone()),
        [(_, first), .., (_, last)] => Some(format!("{first}\u{2013}{last}")),
        [] => None,
    }
}
