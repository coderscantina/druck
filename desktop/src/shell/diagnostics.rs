//! The diagnostics panel: errors and warnings of the latest layout grouped by chapter, with the open `{todo:}`
//! placeholders. A click opens the chapter at the line.

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FontWeight, ScrollHandle, SharedString, Window, div, px};

use super::report::{Entry, Kind, Report};
use super::text::Key;
use super::ui;
use crate::session::ChapterId;
use crate::tokens::{self, Tokens};

pub enum DiagnosticsEvent {
    /// Open this chapter with the caret at the byte.
    Open(ChapterId, usize),
    /// Forget the note about a failed export.
    DismissNotice,
}

impl EventEmitter<DiagnosticsEvent> for DiagnosticsPanel {}

/// A line of the list: a chapter's heading, or one entry.
enum Item {
    Heading { title: SharedString, count: usize },
    Entry(Entry),
}

/// The height of a heading and of an entry.
const HEADING_HEIGHT: gpui::Pixels = px(28.);
const ENTRY_HEIGHT: gpui::Pixels = px(62.);

pub struct DiagnosticsPanel {
    items: Vec<Item>,
    counts: super::report::Counts,
    /// A failed export the entries belong to.
    notice: Option<SharedString>,
    scroll: ScrollHandle,
}

impl DiagnosticsPanel {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            counts: super::report::Counts::default(),
            notice: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn set(&mut self, report: &Report, notice: Option<String>, cx: &mut Context<Self>) {
        self.counts = report.counts();
        self.items = report
            .sections
            .iter()
            .flat_map(|section| {
                let heading = Item::Heading {
                    title: section.title.clone().into(),
                    count: section.entries.len(),
                };
                std::iter::once(heading).chain(section.entries.iter().cloned().map(Item::Entry))
            })
            .collect();
        self.notice = notice.map(SharedString::from);
        cx.notify();
    }

    fn row(&self, at: usize, tokens: &Tokens, cx: &mut Context<Self>) -> gpui::AnyElement {
        match &self.items[at] {
            Item::Heading { title, count } => div()
                .h(HEADING_HEIGHT)
                .px(tokens::SPACE_3)
                .flex()
                .items_center()
                .justify_between()
                .gap(tokens::SPACE_2)
                .border_b_1()
                .border_color(tokens.divider)
                .text_size(tokens::TEXT_SMALL)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.text_quiet)
                .child(div().min_w_0().truncate().child(title.clone()))
                .child(div().flex_none().text_color(tokens.text_faint).child(count.to_string()))
                .into_any_element(),
            Item::Entry(entry) => {
                let place = entry.place;
                let kind = match entry.kind {
                    Kind::Error => crate::t!(Key::KindError),
                    Kind::Warning => crate::t!(Key::KindWarning),
                    Kind::Todo => crate::t!(Key::KindTodo),
                };
                let label = match entry.line {
                    Some(line) => format!("{kind} \u{b7} {}", crate::t!(Key::LineNumber, line = line)),
                    None => kind.to_owned(),
                };
                div()
                    .id(("entry", at))
                    .h(ENTRY_HEIGHT)
                    .px(tokens::SPACE_3)
                    .py(tokens::SPACE_1)
                    .flex()
                    .gap(tokens::SPACE_2)
                    .when(place.is_some(), |row| {
                        row.cursor_pointer().hover(|row| row.bg(tokens.raised))
                    })
                    .when_some(place, |row, (id, offset)| {
                        row.on_click(cx.listener(move |_, _, _, cx| cx.emit(DiagnosticsEvent::Open(id, offset))))
                    })
                    .child(div().pt(px(5.)).child(ui::dot(tokens, entry.kind)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(tokens::TEXT_SMALL)
                                    .text_color(tokens.text_faint)
                                    .child(label),
                            )
                            .child(div().line_clamp(2).child(entry.message.clone())),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Render for DiagnosticsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let counts = self.counts;
        let summary = crate::t!(
            Key::Summary,
            errors = counts.errors,
            warnings = counts.warnings,
            todos = counts.todos
        );
        let rows: Vec<_> = (0..self.items.len()).map(|at| self.row(at, &tokens, cx)).collect();
        div()
            .id("diagnostics")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(ui::section_label(&tokens, crate::t!(Key::DiagnosticsTitle)))
            .child(
                div()
                    .flex_none()
                    .px(tokens::SPACE_3)
                    .py(tokens::SPACE_2)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(summary),
            )
            .when_some(self.notice.clone(), |panel, notice| {
                panel.child(
                    div()
                        .flex_none()
                        .px(tokens::SPACE_3)
                        .py(tokens::SPACE_2)
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(tokens::SPACE_2)
                        .border_b_1()
                        .border_color(tokens.divider)
                        .text_color(tokens.accent)
                        .child(div().min_w_0().child(notice))
                        .child(
                            div()
                                .id("dismiss")
                                .flex_none()
                                .text_color(tokens.text_quiet)
                                .cursor_pointer()
                                .hover(|link| link.text_color(tokens.text))
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(DiagnosticsEvent::DismissNotice)))
                                .child(crate::t!(Key::Dismiss)),
                        ),
                )
            })
            .child(if rows.is_empty() {
                div()
                    .p(tokens::SPACE_3)
                    .text_color(tokens.text_quiet)
                    .child(crate::t!(Key::NothingToReport))
                    .into_any_element()
            } else {
                div()
                    .id("diagnostics-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows)
                    .into_any_element()
            })
    }
}
