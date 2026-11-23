//! The dialogs of the contents list: a page from the gallery, a new section from a template, and renaming or removing
//! a chapter.

use gpui::prelude::*;
use gpui::{
    Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, ScrollHandle,
    SharedString, Subscription, Window, div, svg,
};

use super::strings::Key;
use crate::a11y::Control as _;
use crate::inspector::input::{Cancelled, Submitted, TextInput};
use crate::session::templates::Group;
use crate::session::{ChapterId, Page, Template};
use crate::tokens::{self, Tokens};
use crate::workspace::{FocusNext, FocusPrevious};

pub enum DialogEvent {
    /// Add a page from the gallery where books have it.
    AddPage(&'static Page),
    Insert(&'static Template, String),
    Rename(ChapterId, String),
    Remove(ChapterId),
}

impl EventEmitter<DialogEvent> for SectionDialog {}

/// The gallery of pages: a row of sheets, and a sheet's size and the step between the lines drawn on it.
const GALLERY_COLUMNS: usize = 4;
const GALLERY_WIDTH: gpui::Pixels = gpui::px(500.);
const SHEET_WIDTH: gpui::Pixels = gpui::px(62.);
const SHEET_HEIGHT: gpui::Pixels = gpui::px(86.);
const SHEET_LINE_STEP: gpui::Pixels = gpui::px(5.);

#[derive(Clone)]
enum State {
    Closed,
    /// The gallery of pages, with the sheet that has the keys.
    AddPage(usize),
    NewSection,
    Edit {
        id: ChapterId,
        title: SharedString,
        removable: bool,
    },
}

pub struct SectionDialog {
    state: State,
    name: Entity<TextInput>,
    /// The card, which keeps Tab inside it.
    card: FocusHandle,
    /// What had the keys before the dialog opened, to give them back.
    before: Option<FocusHandle>,
    templates: ScrollHandle,
    /// The templates the open dialog lists, which follow the book's theme.
    offered: Vec<&'static Template>,
    /// The pages the gallery offers: those the book may have again.
    pages: Vec<&'static Page>,
    /// Whether the Binder's rows can be dragged, which the gallery's foot then says.
    moves: bool,
    _subscriptions: Vec<Subscription>,
}

impl SectionDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A field that keeps its focus reports Escape, which closes the dialog. Otherwise Escape would only blur the
        // field and the dialog would stay.
        let name = cx.new(|cx| TextInput::new(window, cx).keep_focus_on_enter().with_label(Key::Name));
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |dialog, _, Submitted, window, cx| {
                dialog.submit(window, cx)
            }),
            cx.subscribe_in(&name, window, |dialog, _, Cancelled, window, cx| {
                dialog.close(window, cx)
            }),
        ];
        Self {
            state: State::Closed,
            name,
            card: cx.focus_handle(),
            before: None,
            templates: ScrollHandle::new(),
            offered: Vec::new(),
            pages: Vec::new(),
            moves: false,
            _subscriptions: subscriptions,
        }
    }

    /// Opens the gallery of pages. `offered` is for its way on to a new section from a template.
    pub fn open_add_page(
        &mut self,
        pages: Vec<&'static Page>,
        offered: Vec<&'static Template>,
        moves: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pages = pages;
        self.offered = offered;
        self.moves = moves;
        self.remember_keys(window, cx);
        self.state = State::AddPage(0);
        self.card.focus(window, cx);
        cx.notify();
    }

    fn open_new_section(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = State::NewSection;
        self.open_name(String::new(), window, cx);
    }

    /// Adds the page of the sheet at `at`.
    fn add_page(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.pages.get(at).copied() {
            self.close(window, cx);
            cx.emit(DialogEvent::AddPage(page));
        }
    }

    /// Arrows move between the gallery's sheets, a row of them at a time up and down, and Return adds the page.
    fn gallery_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let State::AddPage(at) = self.state else {
            return;
        };
        let last = self.pages.len().saturating_sub(1);
        let to = match event.keystroke.key.as_str() {
            "left" => at.saturating_sub(1),
            "right" => (at + 1).min(last),
            "up" => at.checked_sub(GALLERY_COLUMNS).unwrap_or(at),
            "down" => Some(at + GALLERY_COLUMNS).filter(|to| *to <= last).unwrap_or(at),
            "enter" => {
                cx.stop_propagation();
                return self.add_page(at, window, cx);
            }
            _ => return,
        };
        cx.stop_propagation();
        self.state = State::AddPage(to);
        cx.notify();
    }

    /// A page of the gallery: a small sheet showing its shape, and its name.
    fn sheet(
        page: &'static Page,
        at: usize,
        chosen: bool,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let line = |width: f32, dark: bool, centred: bool| {
            div()
                .flex_none()
                .h(tokens::HAIR_GAP)
                .w(gpui::relative(width))
                .rounded(tokens::HAIRLINE)
                .when(centred, |line| line.mx_auto())
                .map(|line| {
                    if dark {
                        line.bg(tokens.text_quiet)
                    } else {
                        line.bg(tokens.text_faint).opacity(tokens::SKETCH_OPACITY)
                    }
                })
        };
        let gap = |lines: f32| div().flex_none().h(SHEET_LINE_STEP * lines);
        let marks: Vec<gpui::AnyElement> = match page.slug {
            "title-page" => vec![
                gap(4.).into_any_element(),
                line(0.8, true, true).into_any_element(),
                line(0.5, false, true).into_any_element(),
                gap(3.).into_any_element(),
                line(0.4, false, true).into_any_element(),
            ],
            "imprint" => vec![
                div().flex_1().into_any_element(),
                line(1., false, false).into_any_element(),
                line(0.9, false, false).into_any_element(),
                line(0.7, false, false).into_any_element(),
            ],
            "dedication" => vec![
                gap(6.).into_any_element(),
                line(0.7, false, true).into_any_element(),
                line(0.45, false, true).into_any_element(),
            ],
            "epigraph" => vec![
                gap(4.).into_any_element(),
                line(0.7, false, false).into_any_element(),
                line(0.8, false, false).into_any_element(),
                line(0.4, true, false).into_any_element(),
            ],
            "contents" => [line(0.55, true, false), gap(1.)]
                .into_iter()
                .map(IntoElement::into_any_element)
                .chain((0..5).map(|_| line(1., false, false).into_any_element()))
                .collect(),
            "index" => [line(0.4, true, false), gap(1.)]
                .into_iter()
                .map(IntoElement::into_any_element)
                .chain([0.45, 0.38, 0.48, 0.35, 0.44, 0.4].map(|width| line(width, false, false).into_any_element()))
                .collect(),
            _ => [line(0.72, true, false), gap(1.)]
                .into_iter()
                .map(IntoElement::into_any_element)
                .chain([1., 0.92, 0.6].map(|width| line(width, false, false).into_any_element()))
                .collect(),
        };
        let name = crate::t!(page.name);
        div()
            .id(page.slug)
            .button(tokens, name)
            .aria_selected(chosen)
            .flex()
            .flex_col()
            .items_center()
            .gap(tokens::SPACE_2)
            .px(tokens::SPACE_1)
            .pt(tokens::SPACE_2)
            .pb(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .text_size(tokens::TEXT_SMALL)
            .text_color(if chosen { tokens.text } else { tokens.text_quiet })
            .when(chosen, |cell| cell.bg(tokens.raised))
            .hover(|cell| cell.bg(tokens.raised))
            .on_click(cx.listener(move |dialog, _, window, cx| dialog.add_page(at, window, cx)))
            .child(
                div()
                    .flex_none()
                    .w(SHEET_WIDTH)
                    .h(SHEET_HEIGHT)
                    .px(tokens::SPACE_2)
                    .py(tokens::SPACE_2 + tokens::HAIRLINE)
                    .flex()
                    .flex_col()
                    .gap(SHEET_LINE_STEP - tokens::HAIR_GAP)
                    .bg(tokens.sheet)
                    .border_1()
                    .border_color(tokens.divider)
                    .rounded(tokens::HAIR_GAP)
                    .children(marks),
            )
            .child(div().max_w_full().truncate().child(name))
    }

    pub fn open_edit(
        &mut self,
        id: ChapterId,
        title: String,
        removable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember_keys(window, cx);
        self.state = State::Edit {
            id,
            title: title.clone().into(),
            removable,
        };
        self.open_name(title, window, cx);
    }

    fn open_name(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.name.update(cx, |name, cx| name.set_value(text, true, cx));
        self.name.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn remember_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.state, State::Closed) {
            self.before = window.focused(cx);
        }
    }

    /// Closes the dialog and gives the keys back to what had them.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = State::Closed;
        if let Some(before) = self.before.take() {
            window.focus(&before, cx);
        }
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let State::Edit { id, .. } = self.state {
            let name = self.name.read(cx).text().to_owned();
            self.close(window, cx);
            cx.emit(DialogEvent::Rename(id, name));
        }
    }

    fn template_row(template: &'static Template, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id(template.slug)
            .button(tokens, crate::t!(template.name))
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_1)
            .flex()
            .gap(tokens::SPACE_2)
            .cursor_pointer()
            .rounded(tokens::RADIUS)
            .hover(|row| row.bg(tokens.raised))
            .on_click(cx.listener(move |dialog, _, window, cx| {
                let name = dialog.name.read(cx).text().to_owned();
                dialog.close(window, cx);
                cx.emit(DialogEvent::Insert(template, name));
            }))
            .child(
                svg()
                    .flex_none()
                    .mt(tokens::SPACE_1)
                    .path(template.icon)
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.text_quiet),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().child(crate::t!(template.name)))
                    .child(
                        div()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_quiet)
                            .child(crate::t!(template.description)),
                    ),
            )
    }

    fn button(tokens: &Tokens, id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .button(tokens, label)
            .h(tokens::FIELD_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .cursor_pointer()
            .hover(|button| button.bg(tokens.raised))
            .child(label)
    }
}

impl Render for SectionDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let state = self.state.clone();
        if matches!(state, State::Closed) {
            return div().into_any_element();
        }
        let heading: SharedString = match &state {
            State::Edit { title, .. } => crate::t!(Key::EditSection, title = title).into(),
            State::AddPage(_) => crate::t!(Key::AddPage).into(),
            _ => crate::t!(Key::NewSection).into(),
        };
        let gallery = matches!(state, State::AddPage(_));
        let name_row = div()
            .px(tokens::SPACE_3)
            .pb(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_3)
            .child(
                div()
                    .flex_none()
                    .text_color(tokens.text_quiet)
                    .child(crate::t!(Key::Name)),
            )
            .child(div().flex_1().min_w_0().child(self.name.clone()));
        let more_below = crate::shell::ui::more_below(&self.templates);
        // The list's height is known after its first layout, which the fade needs.
        if matches!(state, State::NewSection) && self.templates.bounds().size.height == gpui::Pixels::ZERO {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
        let body = match state {
            State::AddPage(chosen) => {
                let sheets: Vec<_> = (self.pages.iter().enumerate())
                    .map(|(at, page)| Self::sheet(page, at, at == chosen, &tokens, cx).into_any_element())
                    .collect();
                let foot = if self.moves {
                    format!(
                        "{}{}",
                        crate::t!(Key::PlacedWhereBooksHaveIt),
                        crate::t!(Key::DragToMove)
                    )
                } else {
                    crate::t!(Key::PlacedWhereBooksHaveIt).to_owned()
                };
                div()
                    .px(tokens::SPACE_3)
                    .pb(tokens::SPACE_3)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_3)
                    .child(
                        div()
                            .grid()
                            .grid_cols(GALLERY_COLUMNS as u16)
                            .gap(tokens::SPACE_3)
                            .children(sheets),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(tokens::SPACE_3)
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(div().flex_1().min_w_0().child(foot))
                            .child(
                                Self::button(&tokens, "other-sections", crate::t!(Key::OtherSections))
                                    .flex_none()
                                    .h(tokens::SMALL_BUTTON_HEIGHT)
                                    .text_color(tokens.text_quiet)
                                    .on_click(cx.listener(|dialog, _, window, cx| dialog.open_new_section(window, cx))),
                            ),
                    )
                    .into_any_element()
            }
            State::NewSection => {
                let offered = self.offered.clone();
                let groups = Group::ALL.iter().filter_map(|&group| {
                    let rows: Vec<_> = offered
                        .iter()
                        .filter(|template| template.group == group)
                        .map(|template| Self::template_row(template, &tokens, cx).into_any_element())
                        .collect();
                    if rows.is_empty() {
                        return None;
                    }
                    let labelled = div()
                        .pb(tokens::SPACE_2)
                        .child(
                            div()
                                .px(tokens::SPACE_3)
                                .py(tokens::SPACE_1)
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .child(group.label()),
                        )
                        .children(rows);
                    Some(labelled)
                });
                div()
                    .relative()
                    .border_t_1()
                    .border_color(tokens.divider)
                    .child(
                        div()
                            .id("templates")
                            .max_h(tokens::DIALOG_LIST_HEIGHT)
                            .overflow_y_scroll()
                            .track_scroll(&self.templates)
                            .pt(tokens::SPACE_2)
                            .children(groups.collect::<Vec<_>>()),
                    )
                    .when(more_below, |list| list.child(crate::shell::ui::foot_fade(&tokens)))
                    .into_any_element()
            }
            State::Edit { id, removable, .. } => div()
                .p(tokens::SPACE_3)
                .border_t_1()
                .border_color(tokens.divider)
                .flex()
                .justify_between()
                .gap(tokens::SPACE_2)
                .child(div().when(removable, |left| {
                    left.child(
                        Self::button(&tokens, "remove", crate::t!(Key::RemoveFromBook)).on_click(cx.listener(
                            move |dialog, _, window, cx| {
                                dialog.close(window, cx);
                                cx.emit(DialogEvent::Remove(id));
                            },
                        )),
                    )
                }))
                .child(
                    Self::button(&tokens, "rename", crate::t!(Key::Save))
                        .on_click(cx.listener(|dialog, _, window, cx| dialog.submit(window, cx))),
                )
                .into_any_element(),
            State::Closed => div().into_any_element(),
        };
        let width = if gallery {
            let left = crate::workspace::TextColumn::asking(cx, GALLERY_WIDTH);
            GALLERY_WIDTH.min(window.viewport_size().width - left - tokens::SPACE_4)
        } else {
            tokens::DIALOG_WIDTH
        };
        let card = div()
            .id("dialog")
            .role(gpui::Role::Dialog)
            .aria_label(heading.clone())
            .track_focus(&self.card)
            .on_key_down(cx.listener(Self::gallery_key))
            .occlude()
            .absolute()
            .top(tokens::SPACE_4 * 3.)
            .left(crate::workspace::TextColumn::asking(cx, width))
            .w(width)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow(tokens::card_shadow())
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(tokens::SPACE_3)
                    .h(tokens::SECTION_HEIGHT + tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .justify_between()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(heading)
                    .child(
                        div()
                            .id(ElementId::from("dialog-close"))
                            .button(&tokens, crate::t!(Key::CloseDialog))
                            .cursor_pointer()
                            .on_click(cx.listener(|dialog, _, window, cx| dialog.close(window, cx)))
                            .child(
                                svg()
                                    .path("icons/close.svg")
                                    .size(tokens::ICON_SIZE)
                                    .text_color(tokens.text_quiet),
                            ),
                    ),
            )
            .when(!gallery, |card| card.child(name_row))
            .child(body);
        div()
            .id("dialog-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_key_down(cx.listener(|dialog, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    dialog.close(window, cx);
                }
            }))
            .on_action(cx.listener(|dialog, _: &FocusNext, window, cx| {
                crate::a11y::cycle_within(&dialog.card, true, window, cx)
            }))
            .on_action(cx.listener(|dialog, _: &FocusPrevious, window, cx| {
                crate::a11y::cycle_within(&dialog.card, false, window, cx)
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|dialog, _, window, cx| dialog.close(window, cx)),
            )
            .child(card)
            .into_any_element()
    }
}
