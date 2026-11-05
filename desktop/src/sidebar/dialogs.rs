//! The two dialogs of the contents list: a new section from a template, and renaming or removing a chapter.

use gpui::prelude::*;
use gpui::{
    Context, ElementId, Entity, EventEmitter, Focusable, FontWeight, KeyDownEvent, SharedString, Subscription, Window,
    div, svg,
};

use super::strings::Key;
use crate::a11y::Control as _;
use crate::inspector::input::{Submitted, TextInput};
use crate::session::templates::Group;
use crate::session::{ChapterId, TEMPLATES, Template};
use crate::tokens::{self, Tokens};

pub enum DialogEvent {
    Insert(&'static Template, String),
    Rename(ChapterId, String),
    Remove(ChapterId),
}

impl EventEmitter<DialogEvent> for SectionDialog {}

#[derive(Clone)]
enum State {
    Closed,
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
    _subscriptions: Vec<Subscription>,
}

impl SectionDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| TextInput::new(window, cx));
        let subscriptions = vec![cx.subscribe(&name, |dialog, _, Submitted, cx| dialog.submit(cx))];
        Self {
            state: State::Closed,
            name,
            _subscriptions: subscriptions,
        }
    }

    pub fn open_new_section(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = State::NewSection;
        self.open_name(String::new(), window, cx);
    }

    pub fn open_edit(
        &mut self,
        id: ChapterId,
        title: String,
        removable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    fn close(&mut self, cx: &mut Context<Self>) {
        self.state = State::Closed;
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if let State::Edit { id, .. } = self.state {
            let name = self.name.read(cx).text().to_owned();
            self.close(cx);
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
            .on_click(cx.listener(move |dialog, _, _, cx| {
                let name = dialog.name.read(cx).text().to_owned();
                dialog.close(cx);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let state = self.state.clone();
        if matches!(state, State::Closed) {
            return div().into_any_element();
        }
        let heading: SharedString = match &state {
            State::Edit { title, .. } => crate::t!(Key::EditSection, title = title).into(),
            _ => crate::t!(Key::NewSection).into(),
        };
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
        let body = match state {
            State::NewSection => {
                let groups = Group::ALL.iter().map(|&group| {
                    let rows: Vec<_> = TEMPLATES
                        .iter()
                        .filter(|template| template.group == group)
                        .map(|template| Self::template_row(template, &tokens, cx).into_any_element())
                        .collect();
                    div()
                        .pb(tokens::SPACE_2)
                        .child(
                            div()
                                .px(tokens::SPACE_3)
                                .py(tokens::SPACE_1)
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .child(group.label()),
                        )
                        .children(rows)
                });
                div()
                    .id("templates")
                    .max_h(tokens::DIALOG_LIST_HEIGHT)
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(tokens.divider)
                    .pt(tokens::SPACE_2)
                    .children(groups.collect::<Vec<_>>())
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
                            move |dialog, _, _, cx| {
                                dialog.close(cx);
                                cx.emit(DialogEvent::Remove(id));
                            },
                        )),
                    )
                }))
                .child(
                    Self::button(&tokens, "rename", crate::t!(Key::Save))
                        .on_click(cx.listener(|dialog, _, _, cx| dialog.submit(cx))),
                )
                .into_any_element(),
            State::Closed => div().into_any_element(),
        };
        let card = div()
            .id("dialog")
            .occlude()
            .absolute()
            .top(tokens::SPACE_4 * 3.)
            .left(tokens::SIDEBAR_WIDTH + tokens::SPACE_4)
            .w(tokens::DIALOG_WIDTH)
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
                    .child(heading)
                    .child(
                        div()
                            .id(ElementId::from("dialog-close"))
                            .button(&tokens, crate::t!(Key::CloseDialog))
                            .cursor_pointer()
                            .on_click(cx.listener(|dialog, _, _, cx| dialog.close(cx)))
                            .child(
                                svg()
                                    .path("icons/close.svg")
                                    .size(tokens::ICON_SIZE)
                                    .text_color(tokens.text_quiet),
                            ),
                    ),
            )
            .child(name_row)
            .child(body);
        div()
            .id("dialog-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_key_down(cx.listener(|dialog, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    dialog.close(cx);
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|dialog, _, _, cx| dialog.close(cx)),
            )
            .child(card)
            .into_any_element()
    }
}
