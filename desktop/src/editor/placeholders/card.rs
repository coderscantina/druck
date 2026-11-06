//! Completing a placeholder: the ghost after the caret and the two-pane card under it. The card lists every name with
//! its live value, grouped, the ones that do not match dimmed, and shows the chosen one on the right. Arrows choose,
//! Tab and Return insert, Escape closes until another `{` is typed. The keys stay in the text throughout.

use gpui::prelude::*;
use gpui::{Context, FontWeight, MouseButton, MouseDownEvent, ScrollHandle, SharedString, Window, div, svg};

use super::{Group, Key, Query, Values, best, describe, ghost, insert, matches, query_at, shown};
use crate::a11y::Control as _;
use crate::editor::EditorView;
use crate::editor::float::Float;
use crate::tokens::{self, Tokens};

/// An open completion.
pub(in crate::editor) struct Completion {
    pub query: Query,
    names: Vec<(String, Group)>,
    /// An index into the names.
    chosen: usize,
    scroll: ScrollHandle,
}

impl Completion {
    fn matching(&self) -> Vec<usize> {
        (0..self.names.len())
            .filter(|index| matches(&self.names[*index].0, &self.query.typed))
            .collect()
    }

    fn chosen_name(&self) -> &str {
        &self.names[self.chosen].0
    }

    /// What the ghost after the caret says.
    pub fn ghost(&self) -> Option<String> {
        ghost(self.chosen_name(), &self.query.typed)
    }
}

fn icon(group: Group) -> &'static str {
    match group {
        Group::Statistics => "icons/diagnostics.svg",
        Group::Book => "icons/book.svg",
        Group::Date => "icons/details.svg",
    }
}

impl EditorView {
    fn completion(&self) -> Option<&Completion> {
        match &self.float {
            Float::Complete(completion) => Some(completion),
            _ => None,
        }
    }

    pub(in crate::editor) fn is_completing(&self) -> bool {
        self.completion().is_some()
    }

    /// The ghost a block shows at the caret, as an offset in the chapter and its text.
    pub(in crate::editor) fn completion_ghost(&self) -> Option<(usize, String)> {
        let completion = self.completion()?;
        Some((self.selection.head, completion.ghost()?))
    }

    /// Opens the card if what was just typed is a name after `{`, unless Escape closed it at that brace.
    pub(in crate::editor) fn open_completion(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.float, Float::None) || self.source || !self.selection.is_empty() {
            return;
        }
        let text = self.session.active_buffer().text();
        let Some(query) = query_at(text, &self.blocks, self.selection.head) else {
            return;
        };
        if self.completion_dismissed == Some(query.start) {
            return;
        }
        let names = self.placeholder_values.names();
        let Some(chosen) = best(&names, &query.typed) else {
            return;
        };
        self.float = Float::Complete(Completion {
            query,
            names,
            chosen,
            scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    /// Follows the text and caret while the card is open. A caret that leaves the name, or a name nothing matches,
    /// closes it, and the typed text stays.
    pub(in crate::editor) fn follow_completion(&mut self) {
        let text = self.session.active_buffer().text();
        let query = (self.selection.is_empty())
            .then(|| query_at(text, &self.blocks, self.selection.head))
            .flatten();
        if query.as_ref().map(|query| query.start) != self.completion_dismissed {
            self.completion_dismissed = None;
        }
        let Float::Complete(completion) = &mut self.float else {
            return;
        };
        match query.filter(|query| query.start == completion.query.start) {
            Some(query) => match best(&completion.names, &query.typed) {
                Some(chosen) => {
                    completion.chosen = chosen;
                    completion.query = query;
                }
                None => self.float = Float::None,
            },
            None => self.float = Float::None,
        }
    }

    /// Escape: closes the card, and it stays closed while the same name is typed.
    pub(in crate::editor) fn dismiss_completion(&mut self) -> bool {
        let Float::Complete(completion) = &self.float else {
            return false;
        };
        self.completion_dismissed = Some(completion.query.start);
        self.float = Float::None;
        true
    }

    /// Moves the choice to the next or previous matching name.
    pub(in crate::editor) fn completion_move(&mut self, down: bool, cx: &mut Context<Self>) -> bool {
        let Float::Complete(completion) = &mut self.float else {
            return false;
        };
        let matching = completion.matching();
        if let Some(at) = matching.iter().position(|index| *index == completion.chosen) {
            let count = matching.len();
            let next = if down {
                (at + 1) % count
            } else {
                (at + count - 1) % count
            };
            completion.chosen = matching[next];
            completion.scroll.scroll_to_item(completion.chosen);
            cx.notify();
        }
        true
    }

    /// Writes the chosen name, `{name}`, over what is typed, as one undo step.
    pub(in crate::editor) fn accept_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Float::Complete(completion) = std::mem::take(&mut self.float) else {
            return false;
        };
        let text = self.session.active_buffer().text();
        let command = insert(text, &completion.query, self.selection.head, completion.chosen_name());
        self.run_command(command, window, cx);
        self.toolbar_dismissed = true;
        cx.notify();
        true
    }

    fn choose_completion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Float::Complete(completion) = &mut self.float {
            completion.chosen = index;
        }
        self.accept_completion(window, cx);
    }

    /// The card under the caret.
    pub(in crate::editor) fn completion_card(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let completion = self.completion()?;
        let tokens = Tokens::get(cx).clone();
        let values: &Values = &self.placeholder_values;
        let mut list = Vec::new();
        let mut group = None;
        for (index, (name, in_group)) in completion.names.iter().enumerate() {
            if group != Some(*in_group) {
                group = Some(*in_group);
                list.push(
                    div()
                        .flex_none()
                        .h(tokens::COMPLETION_GROUP_HEIGHT)
                        .px(tokens::SPACE_2)
                        .flex()
                        .items_end()
                        .pb(tokens::SPACE_1)
                        .text_size(tokens::TEXT_SMALL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.text_faint)
                        .child(in_group.label())
                        .into_any_element(),
                );
            }
            let chosen = index == completion.chosen;
            let matching = matches(name, &completion.query.typed);
            let ink = if chosen {
                tokens.text
            } else if matching {
                tokens.text_quiet
            } else {
                tokens.text_faint
            };
            let value = shown(&values.lookup(name));
            list.push(
                div()
                    .id(("placeholder-name", index))
                    .role(gpui::Role::MenuItem)
                    .aria_label(SharedString::from(format!("{name}, {value}")))
                    .aria_selected(chosen)
                    .flex_none()
                    .h(tokens::MENU_ROW_HEIGHT)
                    .px(tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .rounded(tokens::RADIUS)
                    .cursor_pointer()
                    .text_size(tokens::TEXT_BODY)
                    .text_color(ink)
                    .when(chosen, |row| row.bg(tokens.raised))
                    .hover(|row| row.bg(tokens.raised))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| this.choose_completion(index, window, cx)))
                    .child(
                        svg()
                            .flex_none()
                            .path(icon(*in_group))
                            .size(tokens::ICON_SIZE)
                            .text_color(if chosen { tokens.text } else { tokens.text_faint }),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                    .child(
                        div()
                            .flex_none()
                            .max_w(tokens::COMPLETION_VALUE_WIDTH)
                            .truncate()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(if matching { tokens.text_quiet } else { tokens.text_faint })
                            .child(value),
                    )
                    .into_any_element(),
            );
        }
        let name = completion.chosen_name().to_owned();
        let (title, counts) = describe(&name, values);
        let value = shown(&values.lookup(&name));
        let detail = div()
            .flex_1()
            .min_w_0()
            .p(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .border_l_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.text_faint)
                    .child(completion.names[completion.chosen].1.label()),
            )
            .child(div().text_size(tokens::TEXT_BODY).text_color(tokens.text).child(title))
            .child(
                div()
                    .font_family(self.style.body.family.clone())
                    .text_size(tokens::COMPLETION_VALUE_SIZE)
                    .text_color(tokens.text)
                    .line_clamp(2)
                    .child(value),
            )
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(counts),
            )
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(format!("{} {{{name}}}", crate::t!(Key::Writes))),
            )
            .child(div().flex_1())
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("placeholder-insert")
                        .button(&tokens, crate::t!(Key::Insert))
                        .flex_none()
                        .h(tokens::SMALL_BUTTON_HEIGHT)
                        .px(tokens::SPACE_2)
                        .flex()
                        .items_center()
                        .rounded(tokens::CHIP_RADIUS)
                        .border_1()
                        .border_color(tokens.divider)
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.accent)
                        .cursor_pointer()
                        .hover(|button| button.bg(tokens.raised))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.accept_completion(window, cx);
                        }))
                        .child(format!("{} \u{21b5}", crate::t!(Key::Insert))),
                ),
            );
        Some(
            div()
                .occlude()
                .w(tokens::COMPLETION_LIST_WIDTH + tokens::COMPLETION_DETAIL_WIDTH)
                .h(tokens::COMPLETION_HEIGHT)
                .flex()
                .rounded(tokens::RADIUS)
                .bg(tokens.panel)
                .border_1()
                .border_color(tokens.divider)
                .shadow(tokens::float_shadow())
                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_float(cx)))
                .child(
                    div()
                        .id("placeholder-names")
                        .role(gpui::Role::Menu)
                        .flex_none()
                        .w(tokens::COMPLETION_LIST_WIDTH)
                        .h_full()
                        .p(tokens::SPACE_1)
                        .flex()
                        .flex_col()
                        .overflow_y_scroll()
                        .track_scroll(&completion.scroll)
                        .children(list),
                )
                .child(detail)
                .into_any_element(),
        )
    }
}
