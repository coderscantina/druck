//! The Proof tab: errors and warnings of the latest layout grouped by chapter, with the open `{todo:}` placeholders. A
//! click or the walking keys go to a problem's place on the pages, and Write opens there next.

use gpui::prelude::*;
use gpui::{Context, EventEmitter, FontWeight, ScrollHandle, SharedString, Window, div};

use super::report::{Entry, Kind, Report};
use super::text::Key;
use super::ui;
use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

pub enum DiagnosticsEvent {
    /// Go to the entry with this place in the list.
    Select(usize),
    /// Forget the note about a failed export.
    DismissNotice,
}

impl EventEmitter<DiagnosticsEvent> for DiagnosticsPanel {}

/// A line of the list: a chapter's heading, or one entry.
enum Item {
    Heading {
        title: SharedString,
        count: usize,
    },
    /// An entry, with its place among the entries.
    Entry(usize, Entry),
}

pub struct DiagnosticsPanel {
    items: Vec<Item>,
    counts: super::report::Counts,
    /// A failed export the entries belong to.
    notice: Option<SharedString>,
    /// The entry being looked at.
    current: Option<usize>,
    scroll: ScrollHandle,
}

impl DiagnosticsPanel {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            counts: super::report::Counts::default(),
            notice: None,
            current: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn set(&mut self, report: &Report, notice: Option<String>, cx: &mut Context<Self>) {
        self.counts = report.counts();
        self.items.clear();
        let mut number = 0;
        for section in &report.sections {
            self.items.push(Item::Heading {
                title: section.title.clone().into(),
                count: section.entries.len(),
            });
            for entry in &section.entries {
                self.items.push(Item::Entry(number, entry.clone()));
                number += 1;
            }
        }
        self.notice = notice.map(SharedString::from);
        cx.notify();
    }

    /// Marks the entry being looked at, and brings its row into view.
    pub fn set_current(&mut self, current: Option<usize>, cx: &mut Context<Self>) {
        self.current = current;
        let row = self
            .items
            .iter()
            .position(|item| matches!(item, Item::Entry(number, _) if Some(*number) == current));
        if let Some(row) = row {
            self.scroll.scroll_to_item(row);
        }
        cx.notify();
    }

    fn row(&self, at: usize, tokens: &Tokens, cx: &mut Context<Self>) -> gpui::AnyElement {
        match &self.items[at] {
            Item::Heading { title, count } => div()
                .h(tokens::PROOF_HEADING_HEIGHT)
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
            Item::Entry(number, entry) => {
                let number = *number;
                let goes_somewhere = entry.place.is_some() || entry.page.is_some();
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
                    .button(tokens, format!("{label}: {}", entry.message))
                    .h(tokens::PROOF_ENTRY_HEIGHT)
                    .px(tokens::SPACE_3)
                    .py(tokens::SPACE_1)
                    .flex()
                    .gap(tokens::SPACE_2)
                    .when(self.current == Some(number), |row| row.bg(tokens.raised))
                    .when(goes_somewhere, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(tokens.raised))
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(DiagnosticsEvent::Select(number))))
                    })
                    .child(div().pt(tokens::PROOF_DOT_TOP).child(ui::dot(tokens, entry.kind)))
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
                                .button(&tokens, crate::t!(Key::Dismiss))
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
