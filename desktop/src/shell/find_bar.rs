//! The find bar of the galley: a small card at the top of the window, opened by Cmd+F, that finds in the open chapter.

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, Focusable, SharedString, Subscription, Window, div, px, svg};

use super::text::Key;
use crate::a11y::Control as _;
use crate::inspector::input::{Cancelled, Edited, Submitted, TextInput};
use crate::tokens::{self, Tokens};

pub enum FindEvent {
    /// The query changed.
    Query,
    Next,
    Previous,
    Closed,
}

impl EventEmitter<FindEvent> for FindBar {}

pub struct FindBar {
    open: bool,
    input: Entity<TextInput>,
    /// Which match the caret is at and how many there are, once the query has found any.
    position: Option<(usize, usize)>,
    _subscriptions: Vec<Subscription>,
}

impl FindBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(crate::t!(Key::FindPlaceholder))
        });
        let subscriptions = vec![
            cx.subscribe(&input, |bar, _, _: &Edited, cx| {
                bar.position = None;
                cx.emit(FindEvent::Query);
            }),
            cx.subscribe(&input, |_, _, _: &Submitted, cx| cx.emit(FindEvent::Next)),
            cx.subscribe(&input, |bar, _, _: &Cancelled, cx| bar.close(cx)),
        ];
        Self {
            open: false,
            input,
            position: None,
            _subscriptions: subscriptions,
        }
    }

    /// Puts the field's hint in the language now set.
    pub fn refresh_text(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_placeholder(crate::t!(Key::FindPlaceholder), cx)
        });
    }

    /// Opens the bar with its text selected, so a new search replaces the last one.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.input.update(cx, |input, cx| input.select_all(cx));
        let focus = self.input.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.emit(FindEvent::Closed);
            cx.notify();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self, cx: &gpui::App) -> String {
        self.input.read(cx).text().to_owned()
    }

    /// Shows which match the caret is at, counting from 1, out of how many; `None` when there are none.
    pub fn set_position(&mut self, position: Option<(usize, usize)>, cx: &mut Context<Self>) {
        self.position = position;
        cx.notify();
    }
}

impl Render for FindBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let searching = !self.input.read(cx).text().is_empty();
        let count: SharedString = match self.position {
            Some((current, total)) => crate::t!(Key::FindPosition, current = current, total = total).into(),
            None if searching => crate::t!(Key::NoMatches).into(),
            None => SharedString::default(),
        };
        let step = |id: &'static str,
                    label: &'static str,
                    icon: &'static str,
                    event: fn() -> FindEvent,
                    cx: &mut Context<Self>| {
            div()
                .id(id)
                .button(&tokens, label)
                .flex_none()
                .size(tokens::FIELD_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::RADIUS)
                .cursor_pointer()
                .hover(|button| button.bg(tokens.raised))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(event())))
                .child(svg().path(icon).size(tokens::ICON_SIZE).text_color(tokens.text_quiet))
        };
        div()
            .id("find-bar")
            .occlude()
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT + tokens::SECTION_HEIGHT)
            .left(tokens::SIDEBAR_WIDTH + tokens::SPACE_4)
            .w(px(380.))
            .p(tokens::SPACE_2)
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow_lg()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex_1().min_w_0().child(self.input.clone()))
            .child(
                div()
                    .flex_none()
                    .min_w(px(64.))
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(count),
            )
            .child(step(
                "find-previous",
                crate::t!(Key::FindPrevious),
                "icons/chevron-left.svg",
                || FindEvent::Previous,
                cx,
            ))
            .child(step(
                "find-next",
                crate::t!(Key::FindNext),
                "icons/chevron-right.svg",
                || FindEvent::Next,
                cx,
            ))
            .into_any_element()
    }
}
