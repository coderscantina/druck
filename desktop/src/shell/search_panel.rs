//! The project search panel: a card over the window with the query and replacement fields, the matches per file, a
//! preview of what a replacement would change, and a jump to a match. It searches nothing itself: it emits what the
//! author asked for and shows what the shell hands back.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, Focusable, FontWeight, ScrollHandle, SharedString, Subscription, Window, div, svg,
};

use super::search::{FileHits, Options};
use super::text::Key;
use super::ui;
use crate::a11y::Control as _;
use crate::inspector::input::{Cancelled, Edited, Submitted, TextInput};
use crate::session::ChapterId;
use crate::tokens::{self, Tokens};

pub enum SearchEvent {
    /// The query, the replacement, or an option changed: search again.
    Changed,
    Open(ChapterId, Range<usize>),
    ReplaceAll,
    ReplaceFile(ChapterId),
}

impl EventEmitter<SearchEvent> for SearchPanel {}

/// What the status line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Idle,
    Searching,
    /// The query is not a valid expression.
    Invalid(String),
    /// Something was replaced.
    Replaced(usize),
}

/// The most rows drawn. A search for a common word finds more than any list shows, and a replacement still covers all.
const MAX_ROWS: usize = 300;

pub struct SearchPanel {
    open: bool,
    query: Entity<TextInput>,
    replacement: Entity<TextInput>,
    options: Options,
    results: Vec<FileHits>,
    status: Status,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl SearchPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(crate::t!(Key::SearchPlaceholder))
        });
        let replacement = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(crate::t!(Key::ReplacePlaceholder))
        });
        let mut subscriptions = Vec::new();
        for input in [&query, &replacement] {
            subscriptions.push(cx.subscribe(input, |_, _, _: &Edited, cx| cx.emit(SearchEvent::Changed)));
            subscriptions.push(cx.subscribe(input, |panel, _, _: &Cancelled, cx| panel.close(cx)));
        }
        subscriptions.push(cx.subscribe(&query, |panel, _, _: &Submitted, cx| {
            if let Some((file, hit)) = panel.results.first().and_then(|file| Some((file, file.hits.first()?))) {
                cx.emit(SearchEvent::Open(file.id, hit.range.clone()));
            }
        }));
        Self {
            open: false,
            query,
            replacement,
            options: Options::default(),
            results: Vec::new(),
            status: Status::Idle,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Puts the fields' hints in the language now set.
    pub fn refresh_text(&mut self, cx: &mut Context<Self>) {
        self.query.update(cx, |input, cx| {
            input.set_placeholder(crate::t!(Key::SearchPlaceholder), cx)
        });
        self.replacement.update(cx, |input, cx| {
            input.set_placeholder(crate::t!(Key::ReplacePlaceholder), cx)
        });
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.query.update(cx, |input, cx| input.select_all(cx));
        let focus = self.query.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    /// The text of the query and the replacement, and the options.
    pub fn request(&self, cx: &gpui::App) -> (String, String, Options) {
        (
            self.query.read(cx).text().to_owned(),
            self.replacement.read(cx).text().to_owned(),
            self.options,
        )
    }

    pub fn set_results(&mut self, results: Vec<FileHits>, status: Status, cx: &mut Context<Self>) {
        self.results = results;
        self.status = status;
        cx.notify();
    }

    pub fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.status = status;
        cx.notify();
    }

    fn option(
        &self,
        id: &'static str,
        label: &'static str,
        on: bool,
        tokens: &Tokens,
        cx: &mut Context<Self>,
        flip: fn(&mut Options),
    ) -> gpui::AnyElement {
        ui::chip(tokens, id, label, on)
            .on_click(cx.listener(move |panel, _, _, cx| {
                flip(&mut panel.options);
                cx.emit(SearchEvent::Changed);
                cx.notify();
            }))
            .into_any_element()
    }

    fn rows(&self, replacing: bool, tokens: &Tokens, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let mut rows = Vec::new();
        let mut drawn = 0;
        'files: for file in &self.results {
            let id = file.id;
            rows.push(
                div()
                    .h(tokens::FIELD_HEIGHT)
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
                    .child(div().min_w_0().truncate().child(file.title.clone()))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(tokens::SPACE_2)
                            .child(div().text_color(tokens.text_faint).child(file.hits.len().to_string()))
                            .when(replacing, |end| {
                                end.child(
                                    div()
                                        .id(("replace-file", id.number()))
                                        .button(tokens, crate::t!(Key::ReplaceInFile))
                                        .cursor_pointer()
                                        .text_color(tokens.accent)
                                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(SearchEvent::ReplaceFile(id))))
                                        .child(crate::t!(Key::ReplaceInFile)),
                                )
                            }),
                    )
                    .into_any_element(),
            );
            for (at, hit) in file.hits.iter().enumerate() {
                if drawn == MAX_ROWS {
                    break 'files;
                }
                drawn += 1;
                let range = hit.range.clone();
                rows.push(
                    div()
                        .id(("hit", id.number() * 100_000 + at))
                        .button(tokens, format!("{}{}", hit.before, hit.after))
                        .px(tokens::SPACE_3)
                        .py(tokens::SEARCH_HIT_Y)
                        .flex()
                        .gap(tokens::SPACE_2)
                        .cursor_pointer()
                        .hover(|row| row.bg(tokens.raised))
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(SearchEvent::Open(id, range.clone()))))
                        .child(
                            div()
                                .flex_none()
                                .w(tokens::SEARCH_LINE_WIDTH)
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .child(hit.line.to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().truncate().child(SharedString::from(hit.before.clone())))
                                .when(replacing, |column| {
                                    column.child(
                                        div()
                                            .truncate()
                                            .text_color(tokens.accent)
                                            .child(SharedString::from(hit.after.clone())),
                                    )
                                }),
                        )
                        .into_any_element(),
                );
            }
        }
        rows
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let replacing = !self.replacement.read(cx).text().is_empty();
        let hits: usize = self.results.iter().map(|file| file.hits.len()).sum();
        let status = match &self.status {
            Status::Searching => Some((crate::t!(Key::Searching).to_owned(), false)),
            Status::Invalid(message) => Some((message.clone(), true)),
            Status::Replaced(count) => Some((crate::t!(Key::Replaced, count = count), false)),
            Status::Idle if self.query.read(cx).text().is_empty() => None,
            Status::Idle if hits == 0 => Some((crate::t!(Key::NoMatches).to_owned(), false)),
            Status::Idle => Some((
                crate::t!(Key::SearchCount, count = hits, files = self.results.len()),
                false,
            )),
        };
        let rows = self.rows(replacing, &tokens, cx);
        let options = div()
            .flex()
            .gap(tokens::SPACE_1)
            .child(self.option("opt-regex", ".*", self.options.regex, &tokens, cx, |o| {
                o.regex = !o.regex
            }))
            .child(
                self.option("opt-case", "Aa", self.options.case_sensitive, &tokens, cx, |o| {
                    o.case_sensitive = !o.case_sensitive;
                }),
            )
            .child(self.option(
                "opt-word",
                "\u{201c}\u{201d}",
                self.options.whole_word,
                &tokens,
                cx,
                |o| {
                    o.whole_word = !o.whole_word;
                },
            ));
        let legend = [
            (".*", crate::t!(Key::OptionRegex)),
            ("Aa", crate::t!(Key::OptionCase)),
            ("\u{201c}\u{201d}", crate::t!(Key::OptionWord)),
        ]
        .map(|(mark, name)| div().flex_none().child(format!("{mark} {name}")));
        div()
            .id("search-card")
            .occlude()
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT + tokens::SECTION_HEIGHT)
            .left(tokens::SIDEBAR_WIDTH + tokens::SPACE_4)
            .w(tokens::SEARCH_CARD_WIDTH)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow_lg()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(tokens::SPACE_3)
                    .h(tokens::SECTION_HEIGHT + tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .justify_between()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crate::t!(Key::SearchTitle))
                    .child(
                        div()
                            .id("search-close")
                            .button(&tokens, crate::t!(Key::Dismiss))
                            .cursor_pointer()
                            .on_click(cx.listener(|panel, _, _, cx| panel.close(cx)))
                            .child(
                                svg()
                                    .path("icons/close.svg")
                                    .size(tokens::ICON_SIZE)
                                    .text_color(tokens.text_quiet),
                            ),
                    ),
            )
            .child(
                div()
                    .px(tokens::SPACE_3)
                    .pb(tokens::SPACE_2)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_2)
                    .child(
                        div()
                            .flex()
                            .gap(tokens::SPACE_2)
                            .child(div().flex_1().min_w_0().child(self.query.clone()))
                            .child(options),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(tokens::SPACE_2)
                            .child(div().flex_1().min_w_0().child(self.replacement.clone()))
                            .child(
                                ui::button(&tokens, "replace-all", crate::t!(Key::ReplaceAll))
                                    .when(!replacing || hits == 0, |button| {
                                        button.opacity(tokens::DISABLED_OPACITY).cursor_default()
                                    })
                                    .when(replacing && hits > 0, |button| {
                                        button.on_click(cx.listener(|_, _, _, cx| cx.emit(SearchEvent::ReplaceAll)))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_x(tokens::SPACE_3)
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .children(legend),
                    )
                    .when_some(status, |column, (text, problem)| {
                        column.child(
                            div()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(if problem { tokens.accent } else { tokens.text_quiet })
                                .child(SharedString::from(text)),
                        )
                    }),
            )
            .when(!rows.is_empty(), |card| {
                card.child(
                    div()
                        .id("search-results")
                        .max_h(tokens::SEARCH_RESULTS_HEIGHT)
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .border_t_1()
                        .border_color(tokens.divider)
                        .children(rows),
                )
            })
            .into_any_element()
    }
}
