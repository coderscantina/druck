//! The bare page's contents: one short faint line per chapter at the left edge, the current one longer and in full ink.
//! Hovering opens the contents as a card over the edge. A tick never marks a page problem.

use std::collections::HashMap;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, ElementId, EventEmitter, FontWeight, Role, ScrollHandle, Task, Window, div};

use super::{Mark, SidebarData, SidebarEvent, mark_dot, row_lead};
use crate::session::ChapterId;
use crate::tokens::{self, Tokens};

/// How long the pointer rests on the ticks before the card opens, so passing the edge opens nothing.
const OPEN_DELAY: Duration = Duration::from_millis(140);

impl EventEmitter<SidebarEvent> for TicksView {}

pub struct TicksView {
    data: SidebarData,
    marks: HashMap<ChapterId, Mark>,
    open: bool,
    opening: Task<()>,
    scroll: ScrollHandle,
    /// Stays open whatever the pointer does, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pinned: bool,
}

impl TicksView {
    pub fn new() -> Self {
        Self {
            data: SidebarData::default(),
            marks: HashMap::new(),
            open: false,
            #[cfg(any(debug_assertions, feature = "measure"))]
            pinned: false,
            opening: Task::ready(()),
            scroll: ScrollHandle::new(),
        }
    }

    fn close(&mut self) {
        #[cfg(any(debug_assertions, feature = "measure"))]
        if self.pinned {
            return;
        }
        self.open = false;
    }

    /// Opens the card and keeps it open, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn pin_open(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        self.pinned = true;
        cx.notify();
    }

    pub fn set_data(&mut self, data: SidebarData, cx: &mut Context<Self>) {
        if data != self.data {
            self.data = data;
            cx.notify();
        }
    }

    pub fn set_marks(&mut self, marks: HashMap<ChapterId, Mark>, cx: &mut Context<Self>) {
        self.marks = marks;
        cx.notify();
    }

    fn hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if !hovered {
            self.opening = Task::ready(());
            self.close();
            cx.notify();
            return;
        }
        self.opening = cx.spawn(async move |ticks, cx| {
            cx.background_executor().timer(OPEN_DELAY).await;
            ticks
                .update(cx, |ticks, cx| {
                    ticks.open = true;
                    cx.notify();
                })
                .ok();
        });
    }

    fn ticks(&self, height: gpui::Pixels, tokens: &Tokens) -> impl IntoElement + use<> {
        let count = self.data.contents.book.len().max(1) as f32;
        let row = tokens::TICK_ROW.min(height / count);
        let rows = self.data.contents.book.iter().map(|chapter| {
            let current = self.data.active == Some(chapter.id);
            let tick = div()
                .h(tokens::TICK_HEIGHT)
                .w(if current {
                    tokens::TICK_WIDTH_CURRENT
                } else {
                    tokens::TICK_WIDTH
                })
                .when(current, |tick| tick.bg(tokens.text))
                .when(!current, |tick| {
                    tick.bg(tokens.text_faint).opacity(tokens::TICK_OPACITY)
                });
            div().flex_none().h(row).flex().items_center().child(tick)
        });
        div()
            .flex()
            .flex_col()
            .pt(tokens::SPACE_4)
            .pl(tokens::SPACE_3)
            .children(rows)
    }

    /// As wide as the longest title needs, between the card's narrowest and widest.
    fn card_width(&self, window: &Window) -> gpui::Pixels {
        let longest = (self.data.contents.book.iter())
            .map(|chapter| crate::shell::ui::text_width(&chapter.title, tokens::TEXT_BODY, window))
            .fold(gpui::px(0.), gpui::Pixels::max);
        let around = tokens::SPACE_3 + tokens::LIST_LEAD_WIDTH + tokens::SPACE_1 + tokens::DOT_SIZE + tokens::SPACE_2;
        (longest + around).clamp(tokens::CONTENTS_CARD_WIDTH, tokens::CONTENTS_CARD_MAX_WIDTH)
    }

    fn card(
        &self,
        width: gpui::Pixels,
        height: gpui::Pixels,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let rows: Vec<_> = self
            .data
            .contents
            .book
            .iter()
            .map(|chapter| {
                let id = chapter.id;
                let selected = self.data.active == Some(id);
                div()
                    .id(ElementId::from(("contents-card", id.number())))
                    .role(Role::ListItem)
                    .aria_label(chapter.title.clone())
                    .aria_selected(selected)
                    .flex_none()
                    .h(tokens::MENU_ROW_HEIGHT)
                    .pl(tokens::SPACE_3)
                    .pr(tokens::SPACE_1)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_color(if selected { tokens.text } else { tokens.text_quiet })
                    .when(selected, |row| row.bg(tokens.raised))
                    .hover(|row| row.bg(tokens.raised))
                    .on_click(cx.listener(move |ticks, _, _, cx| {
                        ticks.close();
                        cx.emit(SidebarEvent::Select(id));
                    }))
                    .child(row_lead(tokens, chapter, false, selected))
                    .child(div().flex_1().min_w_0().truncate().child(chapter.title.clone()))
                    .children(mark_dot(tokens, self.marks.get(&id).copied()))
            })
            .collect();
        div()
            .absolute()
            .top(tokens::SPACE_2)
            .left(tokens::SPACE_2)
            .w(width)
            .max_h(height)
            .flex()
            .flex_col()
            .py(tokens::SPACE_1)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow_lg()
            .child(
                div()
                    .flex_none()
                    .px(tokens::SPACE_3)
                    .py(tokens::SPACE_1)
                    .truncate()
                    .text_size(tokens::TEXT_SMALL)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.text_quiet)
                    .child(self.data.title.clone()),
            )
            .child(
                div()
                    .id("contents-card-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .children(rows),
            )
    }
}

impl Render for TicksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let room = window.viewport_size().height - tokens::TITLEBAR_HEIGHT;
        let height = room - tokens::SPACE_4 * 2.;
        let open = self.open;
        let width = if open {
            self.card_width(window)
        } else {
            tokens::CONTENTS_CARD_WIDTH
        };
        div()
            .id("chapter-ticks")
            .absolute()
            .top_0()
            .left_0()
            .h(if open {
                room
            } else {
                (tokens::TICK_ROW * self.data.contents.book.len() as f32 + tokens::SPACE_4 * 2.).min(room)
            })
            .w(if open {
                width + tokens::SPACE_4
            } else {
                tokens::TICK_ZONE_WIDTH
            })
            .when(open, |zone| zone.occlude())
            .on_hover(cx.listener(|ticks, hovered: &bool, _, cx| ticks.hovered(*hovered, cx)))
            .child(self.ticks(height, &tokens))
            .when(open, |zone| zone.child(self.card(width, height, &tokens, cx)))
    }
}
