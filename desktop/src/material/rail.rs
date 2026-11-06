//! The rail in Write: the inspector with Write's tabs at the right of the text while it is open, and a faint tick at the
//! window's right edge while it is not, which opens it when the pointer rests there. Opened so, it leaves again when the
//! pointer does, unless a click in it or the keys keep it. It slides in and out on the shared spring.

use gpui::prelude::*;
use gpui::{AnimationExt, Context, Entity, Hsla, SpringAnimation, Task, Window, div};

use super::{Material, Rail, panel_width};
use crate::inspector::InspectorView;
use crate::tokens::{self, Tokens};

/// How long the rail's content stays drawn after it closes, while the rail slides shut.
const SLIDE_OUT: std::time::Duration = std::time::Duration::from_millis(320);

/// The rail's place beside the text in Write. Draws nothing while closed or in Publish, where the workspace draws it.
pub struct WriteRail {
    inspector: Entity<InspectorView>,
    material: Entity<Material>,
    /// Whether the content is drawn: while open, and while sliding shut.
    drawn: bool,
    /// The width it last opened to, which it slides shut from.
    width: gpui::Pixels,
    closing: Task<()>,
    leaving: Task<()>,
}

impl WriteRail {
    pub fn new(inspector: Entity<InspectorView>, material: Entity<Material>) -> Self {
        Self {
            inspector,
            material,
            drawn: false,
            width: gpui::Pixels::ZERO,
            closing: Task::ready(()),
            leaving: Task::ready(()),
        }
    }

    fn hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered || !self.material.read(cx).peeking() {
            self.leaving = Task::ready(());
            return;
        }
        let material = self.material.clone();
        self.leaving = cx.spawn(async move |_, cx| {
            cx.background_executor().timer(tokens::LEAVE_GRACE).await;
            material.update(cx, |material, cx| {
                if material.peeking() {
                    material.set_open(false, cx);
                }
            });
        });
    }
}

impl Render for WriteRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rail = Rail::get(cx);
        let fits = panel_width(window.viewport_size().width);
        let open = rail.open && rail.write && fits.is_some();
        if open {
            self.width = fits.unwrap_or_default() + tokens::RAIL_WIDTH;
            self.drawn = true;
            self.closing = Task::ready(());
        } else if self.drawn && rail.write && fits.is_some() {
            self.closing = cx.spawn(async move |rail, cx| {
                cx.background_executor().timer(SLIDE_OUT).await;
                rail.update(cx, |rail, cx| {
                    rail.drawn = false;
                    cx.notify();
                })
                .ok();
            });
        } else {
            self.drawn = false;
        }
        if !self.drawn {
            return div().into_any_element();
        }
        let width = self.width;
        div()
            .id("write-rail")
            .tab_group()
            .tab_index(tokens::TAB_RIGHT)
            .tab_stop(false)
            .flex_none()
            .h_full()
            .overflow_hidden()
            .on_hover(cx.listener(|rail, hovered: &bool, _, cx| rail.hovered(*hovered, cx)))
            .on_any_mouse_down(cx.listener(|rail, _, _, cx| {
                rail.leaving = Task::ready(());
                rail.material.update(cx, |material, _| material.keep());
            }))
            // The content keeps its full width and the rail opens over it from the left, so nothing in it reflows.
            .child(div().h_full().w(width).child(self.inspector.clone()))
            .with_spring(
                "write-rail-width",
                SpringAnimation::new(tokens::SPRING)
                    .to(if open { width } else { gpui::Pixels::ZERO })
                    .from(gpui::Pixels::ZERO),
                |rail, width| rail.w(width),
            )
            .into_any_element()
    }
}

/// The tick at Write's right edge and the band around it the pointer rests on.
pub struct RailEdge {
    material: Entity<Material>,
    resting: Task<()>,
    lit: bool,
}

impl RailEdge {
    pub fn new(material: Entity<Material>) -> Self {
        Self {
            material,
            resting: Task::ready(()),
            lit: false,
        }
    }

    fn hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.lit = hovered;
        cx.notify();
        if !hovered {
            self.resting = Task::ready(());
            return;
        }
        let material = self.material.clone();
        self.resting = cx.spawn(async move |edge, cx| {
            cx.background_executor().timer(tokens::MATERIAL_REST).await;
            edge.update(cx, |edge, _| edge.lit = false).ok();
            material.update(cx, |material, cx| material.peek(cx));
        });
    }
}

impl Render for RailEdge {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rail = Rail::get(cx);
        let fits = panel_width(window.viewport_size().width).is_some();
        if rail.open || !rail.write || !fits {
            self.lit = false;
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx);
        let (tick_width, tick_height) = tokens::MATERIAL_TICK;
        let (lit_height, lit_opacity) = tokens::MATERIAL_TICK_LIT;
        let (height, opacity) = if self.lit {
            (lit_height, lit_opacity)
        } else {
            (tick_height, tokens::MATERIAL_TICK_OPACITY)
        };
        let colour = Hsla::from(tokens.text_faint);
        // Only a band around the tick opens the rail, so a pointer passing along the edge does not.
        div()
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT)
            .bottom(tokens::FOOTER_HEIGHT)
            .right(tokens::SCROLLBAR_WIDTH)
            .w(tokens::MATERIAL_EDGE)
            .flex()
            .items_center()
            .child(
                div()
                    .id("material-edge")
                    .w_full()
                    .h(tokens::MATERIAL_EDGE_BAND)
                    .flex()
                    .items_center()
                    .justify_end()
                    .pr(tokens::MATERIAL_TICK_INSET)
                    .cursor_pointer()
                    .on_hover(cx.listener(|edge, hovered: &bool, _, cx| edge.hovered(*hovered, cx)))
                    // A click opens it at once, and keeps it.
                    .on_click(cx.listener(|edge, _, _, cx| {
                        edge.resting = Task::ready(());
                        edge.material.update(cx, |material, cx| material.set_open(true, cx));
                    }))
                    .child(
                        div()
                            .w(tick_width)
                            .rounded(tick_width)
                            .with_spring(
                                "material-tick",
                                SpringAnimation::new(tokens::SPRING).to(height),
                                |tick, height| tick.h(height),
                            )
                            .map_element(move |tick| tick.bg(colour.opacity(opacity))),
                    ),
            )
            .into_any_element()
    }
}
