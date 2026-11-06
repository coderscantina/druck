//! The rail in Write: the inspector with Write's tabs at the right of the text while it is open, and a faint tick at the
//! window's right edge while it is not, which opens it when the pointer rests there.

use gpui::prelude::*;
use gpui::{Context, Entity, Task, Window, div};

use super::{Material, Rail, panel_width};
use crate::inspector::InspectorView;
use crate::tokens::{self, Tokens};

/// The rail's place beside the text in Write. Draws nothing while closed or in Publish, where the workspace draws it.
pub struct WriteRail {
    inspector: Entity<InspectorView>,
}

impl WriteRail {
    pub fn new(inspector: Entity<InspectorView>) -> Self {
        Self { inspector }
    }
}

impl Render for WriteRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rail = Rail::get(cx);
        let width = panel_width(window.viewport_size().width).filter(|_| rail.open && rail.write);
        let Some(width) = width else {
            return div().into_any_element();
        };
        div()
            .tab_group()
            .tab_index(tokens::TAB_RIGHT)
            .tab_stop(false)
            .flex_none()
            .h_full()
            .w(width + tokens::RAIL_WIDTH)
            .child(self.inspector.clone())
            .into_any_element()
    }
}

/// The tick at Write's right edge and the strip the pointer rests on.
pub struct RailEdge {
    material: Entity<Material>,
    resting: Task<()>,
}

impl RailEdge {
    pub fn new(material: Entity<Material>) -> Self {
        Self {
            material,
            resting: Task::ready(()),
        }
    }

    fn hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if !hovered {
            self.resting = Task::ready(());
            return;
        }
        let material = self.material.clone();
        self.resting = cx.spawn(async move |_, cx| {
            cx.background_executor().timer(tokens::MATERIAL_REST).await;
            material.update(cx, |material, cx| material.set_open(true, cx));
        });
    }
}

impl Render for RailEdge {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rail = Rail::get(cx);
        let fits = panel_width(window.viewport_size().width).is_some();
        if rail.open || !rail.write || !fits {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx);
        let (tick_width, tick_height) = tokens::MATERIAL_TICK;
        div()
            .id("material-edge")
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT)
            .bottom(tokens::FOOTER_HEIGHT)
            .right_0()
            .w(tokens::MATERIAL_EDGE)
            .flex()
            .items_center()
            .justify_end()
            .pr(tokens::MATERIAL_TICK_INSET)
            .on_hover(cx.listener(|edge, hovered: &bool, _, cx| edge.hovered(*hovered, cx)))
            .child(
                div()
                    .w(tick_width)
                    .h(tick_height)
                    .rounded(tick_width)
                    .bg(gpui::Hsla::from(tokens.text_faint).opacity(tokens::MATERIAL_TICK_OPACITY)),
            )
            .into_any_element()
    }
}
