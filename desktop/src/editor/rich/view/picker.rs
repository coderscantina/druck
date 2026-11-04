//! The pickers for citations and cross-references. A chip's picker opens on what it says, and what it writes changes only
//! the item being edited.

use std::ops::Range;

use druck::editor::syntax::Form;
use gpui::Focusable;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, MouseDownEvent, ScrollHandle, SharedString, Subscription, Window, div, px};

use super::{CONTEXT, NextField, PreviousField, RichFloat, card, field_label, small_button};
use crate::editor::EditorView;
use crate::editor::float::Float;
use crate::editor::rich::{Key, citations};
use crate::inspector::input::{self, Cancelled, Edited, Submitted, TextInput};
use crate::source::{Edit, Selection};
use crate::tokens::{self, Tokens};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Citation,
    Reference,
}

/// What a pick writes.
#[derive(Clone)]
enum Target {
    /// A new citation or reference in place of this range.
    New(Range<usize>),
    /// The chip at this range. `item` is the work being changed, and `adding` adds another instead.
    Chip {
        range: Range<usize>,
        item: usize,
        adding: bool,
    },
}

pub struct PickerFloat {
    mode: Mode,
    target: Target,
    query: Entity<TextInput>,
    locator: Entity<TextInput>,
    selected: usize,
    scroll: ScrollHandle,
    /// A citation in the sentence, as `@key`, or a reference showing its page.
    option: bool,
    /// What is wrong with the chip, or why the last pick was refused.
    message: Option<String>,
    _subscriptions: Vec<Subscription>,
}

pub const WIDTH: gpui::Pixels = px(400.);
const ROWS: usize = 8;
const ROW_HEIGHT: gpui::Pixels = px(40.);

impl PickerFloat {
    pub fn anchor(&self) -> usize {
        match &self.target {
            Target::New(range) | Target::Chip { range, .. } => range.start,
        }
    }
}

/// What a picker opens on.
struct Opening {
    mode: Mode,
    target: Target,
    option: bool,
    locator: String,
    message: Option<String>,
}

impl EditorView {
    fn open_picker(&mut self, opening: Opening, window: &mut Window, cx: &mut Context<Self>) {
        let Opening {
            mode,
            target,
            option,
            locator: locator_text,
            message,
        } = opening;
        let placeholder = crate::t!(match mode {
            Mode::Citation => Key::SearchWorks,
            Mode::Reference => Key::SearchLabels,
        });
        let query = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(placeholder)
        });
        let locator = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(crate::t!(Key::LocatorPlaceholder));
            input.set_value(locator_text, true, cx);
            input
        });
        let subscriptions = vec![
            cx.subscribe(&query, |this, _, _: &Edited, cx| this.picker_query_changed(cx)),
            cx.subscribe_in(&query, window, |this, _, _: &Submitted, window, cx| {
                this.picker_accept(None, window, cx);
            }),
            cx.subscribe_in(&locator, window, |this, _, _: &Submitted, window, cx| {
                this.picker_accept(None, window, cx);
            }),
            cx.subscribe_in(&query, window, |this, _, _: &Cancelled, window, cx| {
                this.picker_close(window, cx)
            }),
            cx.subscribe_in(&locator, window, |this, _, _: &Cancelled, window, cx| {
                this.picker_close(window, cx)
            }),
        ];
        query.focus_handle(cx).focus(window, cx);
        self.rich.refresh(&self.session);
        self.float = Float::Rich(RichFloat::Picker(PickerFloat {
            mode,
            target,
            query,
            locator,
            selected: 0,
            scroll: ScrollHandle::new(),
            option,
            message,
            _subscriptions: subscriptions,
        }));
        self.picker_select_current(cx);
        cx.notify();
    }

    /// The citation picker for a new citation at the caret, in place of the typed slash if there is one.
    pub(in crate::editor) fn open_citation_picker(
        &mut self,
        slash: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = slash.unwrap_or_else(|| self.selection.range());
        let message = (!self.rich.has_bibliography).then(|| crate::t!(Key::NoBibliography).to_owned());
        let opening = Opening {
            mode: Mode::Citation,
            target: Target::New(range),
            option: false,
            locator: String::new(),
            message,
        };
        self.open_picker(opening, window, cx);
    }

    /// The picker on the citation chip at `range`.
    pub(in crate::editor) fn open_citation_picker_on(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let written = self.session.active_buffer().text()[range.clone()].to_owned();
        let message = self.rich.problem_of(&written);
        let (locator, narrative) = match citations::read(&written) {
            Ok(citation) => {
                let tail = citations::tail(&written, &citation, 0);
                let tail = tail.trim_start_matches('[').trim_end_matches(']').to_owned();
                (tail, citation.form == Form::Narrative)
            }
            Err(_) => (String::new(), false),
        };
        let target = Target::Chip {
            range,
            item: 0,
            adding: false,
        };
        let opening = Opening {
            mode: Mode::Citation,
            target,
            option: narrative,
            locator,
            message,
        };
        self.open_picker(opening, window, cx);
    }

    /// The cross-reference picker for a new reference at the caret, in place of the typed slash if there is one.
    pub(in crate::editor) fn open_reference_picker(
        &mut self,
        slash: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = slash.unwrap_or_else(|| self.selection.range());
        let opening = Opening {
            mode: Mode::Reference,
            target: Target::New(range),
            option: false,
            locator: String::new(),
            message: None,
        };
        self.open_picker(opening, window, cx);
    }

    /// The picker on the cross-reference chip at `range`.
    pub(in crate::editor) fn open_reference_picker_on(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let written = self.session.active_buffer().text()[range.clone()].to_owned();
        let message = self.rich.problem_of(&written);
        let page = citations::read_reference(&written).is_some_and(|(_, page)| page);
        let target = Target::Chip {
            range,
            item: 0,
            adding: false,
        };
        let opening = Opening {
            mode: Mode::Reference,
            target,
            option: page,
            locator: String::new(),
            message,
        };
        self.open_picker(opening, window, cx);
    }

    fn picker(&self) -> Option<&PickerFloat> {
        match &self.float {
            Float::Rich(RichFloat::Picker(picker)) => Some(picker),
            _ => None,
        }
    }

    fn picker_mut(&mut self) -> Option<&mut PickerFloat> {
        match &mut self.float {
            Float::Rich(RichFloat::Picker(picker)) => Some(picker),
            _ => None,
        }
    }

    /// What the query leaves, as a label, a line under it, and what a pick writes.
    fn picker_rows(&self, picker: &PickerFloat, cx: &gpui::App) -> Vec<(SharedString, SharedString, String)> {
        let query = picker.query.read(cx).text().to_owned();
        match picker.mode {
            Mode::Citation => citations::search_works(&self.rich.works, &query)
                .into_iter()
                .map(|work| {
                    let who = format!("{} {}", work.authors, work.year).trim().to_owned();
                    (
                        SharedString::from(if who.is_empty() { work.key.clone() } else { who }),
                        SharedString::from(format!("{} · {}", work.title, work.key)),
                        work.key.clone(),
                    )
                })
                .collect(),
            Mode::Reference => citations::search_labels(&self.rich.book.labels, &query)
                .into_iter()
                .map(|label| {
                    let shown = self
                        .rich
                        .labels
                        .references
                        .get(&label.label)
                        .cloned()
                        .unwrap_or_else(|| kind_name(&label.label).to_owned());
                    (
                        SharedString::from(shown),
                        SharedString::from(format!("{} · {}", label.title, label.label)),
                        label.label.clone(),
                    )
                })
                .collect(),
        }
    }

    fn picker_query_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker_mut() {
            picker.selected = 0;
        }
        cx.notify();
    }

    /// Selects the row of what the chip names.
    fn picker_select_current(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.picker() else {
            return;
        };
        let Target::Chip { range, item, .. } = &picker.target else {
            return;
        };
        let written = &self.session.active_buffer().text()[range.clone()];
        let current = match picker.mode {
            Mode::Citation => citations::read(written)
                .ok()
                .and_then(|citation| citation.items.get(*item).map(|item| item.key.clone())),
            Mode::Reference => citations::read_reference(written).map(|(label, _)| label.to_owned()),
        };
        let at = current.and_then(|current| {
            self.picker_rows(picker, cx)
                .iter()
                .position(|(_, _, value)| *value == current)
        });
        if let (Some(at), Some(picker)) = (at, self.picker_mut()) {
            picker.selected = at;
            picker.scroll.scroll_to_item(at);
        }
    }

    fn picker_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.float = Float::None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn picker_move(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.picker().map_or(0, |picker| self.picker_rows(picker, cx).len());
        if count == 0 {
            return;
        }
        if let Some(picker) = self.picker_mut() {
            picker.selected = if down {
                (picker.selected + 1) % count
            } else {
                (picker.selected + count - 1) % count
            };
            picker.scroll.scroll_to_item(picker.selected);
        }
        cx.notify();
    }

    /// Writes the pick: the row `index`, or the selected one.
    fn picker_accept(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker() else {
            return;
        };
        let rows = self.picker_rows(picker, cx);
        let chosen = index.unwrap_or(picker.selected);
        let Some((_, _, value)) = rows.get(chosen).cloned() else {
            return;
        };
        let locator = picker.locator.read(cx).text().trim().to_owned();
        let (mode, option, target) = (picker.mode, picker.option, picker.target.clone());
        let text = self.session.active_buffer().text();
        let range = match &target {
            Target::New(range) | Target::Chip { range, .. } => range.clone(),
        };
        // A short form needs a word start: right after a letter it would be plain text.
        let after_word = text[..range.start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric);
        let written = match (mode, &target) {
            (Mode::Citation, Target::New(_)) => Ok(citations::new_citation(&value, &locator, option && !after_word)),
            (Mode::Citation, Target::Chip { item, adding, .. }) => {
                let old = &text[range.clone()];
                citations::read(old).map(|citation| {
                    if *adding {
                        citations::with_added_item(old, &value).unwrap_or_else(|| old.to_owned())
                    } else if citation.items.len() == 1 && (citation.form == Form::Narrative) != option {
                        citations::new_citation(&value, &locator, option && !after_word)
                    } else {
                        citations::with_item(old, &citation, *item, &value, &locator)
                    }
                })
            }
            (Mode::Reference, _) => {
                let was_bracketed = matches!(&target, Target::Chip { .. }) && text[range.clone()].starts_with('[');
                Ok(citations::reference_text(&value, option, was_bracketed || after_word))
            }
        };
        let checked = written.and_then(|written| match mode {
            Mode::Citation => citations::read(&written).map(|_| written),
            Mode::Reference => Ok(written),
        });
        let written = match checked {
            Ok(written) => written,
            Err(why) => {
                if let Some(picker) = self.picker_mut() {
                    picker.message = Some(why);
                }
                cx.notify();
                return;
            }
        };
        // Brackets that touch would read as a link.
        let written = if matches!(&target, Target::New(_)) && text[..range.start].ends_with(']') {
            format!(" {written}")
        } else {
            written
        };
        let caret = range.start + written.len();
        let active = self.session.active();
        match self.write_rich(
            active,
            vec![Edit::new(range, written)],
            Some(Selection::caret(caret)),
            window,
            cx,
        ) {
            Ok(()) => {
                self.float = Float::None;
                self.autoscroll = true;
                self.toolbar_dismissed = true;
                window.focus(&self.focus, cx);
            }
            Err(why) => {
                if let Some(picker) = self.picker_mut() {
                    picker.message = Some(why);
                }
            }
        }
        cx.notify();
    }

    fn picker_switch_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker() else {
            return;
        };
        let (query, locator) = (picker.query.clone(), picker.locator.clone());
        let target = if query.focus_handle(cx).is_focused(window) {
            locator
        } else {
            query
        };
        target.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Goes to another work of a group, or to adding one.
    fn picker_set_item(&mut self, item: usize, adding: bool, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text().to_owned();
        let Some(picker) = self.picker_mut() else {
            return;
        };
        if let Target::Chip {
            item: current,
            adding: was_adding,
            range,
        } = &mut picker.target
        {
            *current = item;
            *was_adding = adding;
            let written = &text[range.clone()];
            let tail = citations::read(written)
                .ok()
                .filter(|citation| item < citation.items.len() && !adding)
                .map_or(String::new(), |citation| {
                    citations::tail(written, &citation, item).to_owned()
                });
            picker
                .locator
                .update(cx, |locator, cx| locator.set_value(tail, true, cx));
        }
        self.picker_select_current(cx);
        cx.notify();
    }

    fn picker_remove_item(&mut self, item: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Target::Chip { range, .. }) = self.picker().map(|picker| picker.target.clone()) else {
            return;
        };
        let written = self.session.active_buffer().text()[range.clone()].to_owned();
        let Some(new) = citations::read(&written)
            .ok()
            .and_then(|citation| citations::without_item(&written, &citation, item))
        else {
            return;
        };
        let new_end = range.start + new.len();
        let active = self.session.active();
        if let Err(why) = self.write_rich(active, vec![Edit::new(range.clone(), new)], None, window, cx) {
            if let Some(picker) = self.picker_mut() {
                picker.message = Some(why);
            }
        } else if let Some(picker) = self.picker_mut() {
            picker.target = Target::Chip {
                range: range.start..new_end,
                item: 0,
                adding: false,
            };
        }
        self.picker_set_item(0, false, cx);
    }

    pub(super) fn picker_element(&self, picker: &PickerFloat, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx).clone();
        let rows = self.picker_rows(picker, cx);
        let selected = picker.selected;
        let mode = picker.mode;
        let list: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(index, (label, line, _))| {
                let chosen = index == selected;
                div()
                    .id(("picker-row", index))
                    .flex_none()
                    .h(ROW_HEIGHT)
                    .px(tokens::SPACE_2)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .rounded(tokens::RADIUS)
                    .cursor_pointer()
                    .when(chosen, |row| row.bg(tokens.raised))
                    .hover(|row| row.bg(tokens.raised))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| this.picker_accept(Some(index), window, cx)))
                    .child(
                        div()
                            .truncate()
                            .text_color(if chosen { tokens.text } else { tokens.text_quiet })
                            .child(label.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(line.clone()),
                    )
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            div()
                .p(tokens::SPACE_2)
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_faint)
                .child(crate::t!(match mode {
                    Mode::Citation => Key::NoWorks,
                    Mode::Reference => Key::NoLabels,
                }))
        });
        let shown_rows = rows.len().clamp(1, ROWS) as f32;
        let group = self.picker_group(picker, &tokens, cx);
        let option_label = crate::t!(match mode {
            Mode::Citation => Key::NarrativeOption,
            Mode::Reference => Key::PageOption,
        });
        let option = picker.option;
        let toggle = div()
            .id("picker-option")
            .flex_none()
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .cursor_pointer()
            .text_size(tokens::TEXT_SMALL)
            .text_color(if option { tokens.text } else { tokens.text_quiet })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(picker) = this.picker_mut() {
                    picker.option = !picker.option;
                }
                cx.notify();
            }))
            .child(
                div()
                    .size(px(12.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(tokens.divider)
                    .when(option, |check| check.bg(tokens.accent)),
            )
            .child(option_label);
        card(&tokens)
            .id("picker-float")
            .key_context(CONTEXT)
            .w(WIDTH)
            .p(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.picker_close(window, cx)))
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.picker_move(false, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.picker_move(true, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &NextField, window, cx| this.picker_switch_field(window, cx)))
            .on_action(cx.listener(|this, _: &PreviousField, window, cx| this.picker_switch_field(window, cx)))
            .child(
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(tokens.text_quiet)
                    .child(crate::t!(match mode {
                        Mode::Citation => Key::CitationTitle,
                        Mode::Reference => Key::ReferenceTitle,
                    })),
            )
            .children(group)
            .child(picker.query.clone())
            .children(picker.message.as_ref().map(|message| {
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.accent)
                    .child(message.clone())
            }))
            .child(
                div()
                    .id("picker-rows")
                    .flex()
                    .flex_col()
                    .h(ROW_HEIGHT * shown_rows)
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    .children(list)
                    .children(empty),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .when(mode == Mode::Citation, |row| {
                        row.child(field_label(&tokens, crate::t!(Key::LocatorLabel)))
                            .child(div().flex_1().min_w_0().child(picker.locator.clone()))
                    })
                    .when(mode == Mode::Reference, |row| row.child(div().flex_1()))
                    .child(toggle),
            )
            .into_any_element()
    }

    /// The works of a group, to go between them, add one, or take one out.
    fn picker_group(&self, picker: &PickerFloat, tokens: &Tokens, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let Target::Chip { range, item, adding } = &picker.target else {
            return None;
        };
        if picker.mode != Mode::Citation {
            return None;
        }
        let written = &self.session.active_buffer().text()[range.clone()];
        let citation = citations::read(written).ok()?;
        if citation.form != Form::Parenthetical {
            return None;
        }
        let several = citation.items.len() > 1;
        let (item, adding) = (*item, *adding);
        let mut row = div().flex().flex_wrap().items_center().gap(tokens::SPACE_1);
        for (index, work) in citation.items.iter().enumerate() {
            let chosen = index == item && !adding;
            row = row.child(
                small_button(tokens, ("picker-work", index), work.key.clone())
                    .when(chosen, |button| button.text_color(tokens.accent))
                    .on_click(cx.listener(move |this, _, _, cx| this.picker_set_item(index, false, cx))),
            );
        }
        if several && !adding {
            row = row.child(
                small_button(tokens, "picker-remove-work", crate::t!(Key::RemoveWork))
                    .on_click(cx.listener(move |this, _, window, cx| this.picker_remove_item(item, window, cx))),
            );
        }
        row = row.child(
            small_button(tokens, "picker-add-work", crate::t!(Key::AddWork))
                .when(adding, |button| button.text_color(tokens.accent))
                .on_click(cx.listener(|this, _, _, cx| this.picker_set_item(0, true, cx))),
        );
        Some(row)
    }
}

fn kind_name(label: &str) -> &'static str {
    crate::t!(match label.split(':').next() {
        Some("fig") => Key::KindFigure,
        Some("tbl") => Key::KindTable,
        _ => Key::KindHeading,
    })
}
