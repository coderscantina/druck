//! The card below a figure: its caption, label, and placement, written as one undo step when it closes.

use gpui::Focusable;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, MouseDownEvent, Pixels, Point, SharedString, Subscription, Window, div, point, px,
};

use super::{CONTEXT, NextField, PreviousField, RichFloat, card, field_label, small_button};
use crate::editor::EditorView;
use crate::editor::float::Float;
use crate::editor::place::Rect;
use crate::editor::rich::images::{Figure, Placement};
use crate::editor::rich::view::shapes::size_label;
use crate::editor::rich::{AtomKind, Key};
use crate::inspector::input::{Cancelled, Submitted, TextInput};
use crate::source::Edit;
use crate::tokens::{self, Tokens};

pub struct FigureFloat {
    /// Where the figure's block starts.
    start: usize,
    caption: Entity<TextInput>,
    label: Entity<TextInput>,
    placement: Placement,
    refused: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl FigureFloat {
    pub fn first_byte(&self) -> usize {
        self.start
    }
}

const WIDTH: Pixels = px(360.);

impl EditorView {
    /// The figure at this byte of the active chapter, as it stands now.
    fn figure_at(&self, start: usize) -> Option<(String, Figure)> {
        let index = self.blocks.index_at(start);
        let atom = self.rich_atom(index)?;
        let AtomKind::Figure(figure) = &atom.kind else {
            return None;
        };
        let block = &self.blocks.as_slice()[atom.first];
        Some((
            self.session.active_buffer().text()[block.range.clone()].to_owned(),
            figure.clone(),
        ))
    }

    pub(super) fn open_figure(&mut self, start: usize, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.blocks.as_slice()[self.blocks.index_at(start)].range.start;
        let Some((text, figure)) = self.figure_at(start) else {
            return;
        };
        let index = self.blocks.index_at(start);
        self.caret_out_of(index, index, window, cx);
        let make = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(window, cx)
                    .keep_focus_on_enter()
                    .with_placeholder(placeholder);
                input.set_value(value, true, cx);
                input
            })
        };
        let caption = make(
            figure.caption(&text).to_owned(),
            crate::t!(Key::CaptionPlaceholder),
            window,
            cx,
        );
        let label = make(
            figure.label_name(&text).to_owned(),
            crate::t!(Key::LabelPlaceholder),
            window,
            cx,
        );
        let subscriptions = [&caption, &label]
            .into_iter()
            .flat_map(|field| {
                [
                    cx.subscribe_in(field, window, |this, _, _: &Submitted, window, cx| {
                        this.commit_rich_float(window, cx);
                    }),
                    cx.subscribe_in(field, window, |this, _, _: &Cancelled, window, cx| {
                        this.commit_rich_float(window, cx);
                    }),
                ]
            })
            .collect();
        caption.focus_handle(cx).focus(window, cx);
        self.float = Float::Rich(RichFloat::Figure(FigureFloat {
            start,
            caption,
            label,
            placement: figure.placement,
            refused: None,
            _subscriptions: subscriptions,
        }));
        cx.notify();
    }

    pub(super) fn commit_figure(&mut self, mut float: FigureFloat, window: &mut Window, cx: &mut Context<Self>) {
        let Some((text, figure)) = self.figure_at(float.start) else {
            return;
        };
        let caption = float.caption.read(cx).text().to_owned();
        let label = float.label.read(cx).text().trim().to_owned();
        let Some(edit) = figure.edit(&text, float.start, &caption, &label, float.placement) else {
            return;
        };
        let active = self.session.active();
        if let Err(why) = self.write_rich(active, vec![edit], None, window, cx) {
            float.refused = Some(why);
            self.float = Float::Rich(RichFloat::Figure(float));
        }
    }

    fn delete_figure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Rich(RichFloat::Figure(float)) = std::mem::take(&mut self.float) else {
            return;
        };
        window.focus(&self.focus, cx);
        let index = self.blocks.index_at(float.start);
        let block = self.blocks.as_slice()[index].clone();
        let text = self.session.active_buffer().text();
        let end = block.range.end
            + text[block.range.end..]
                .bytes()
                .take_while(|byte| *byte == b'\n' || *byte == b'\r')
                .count();
        let active = self.session.active();
        if let Err(why) = self.write_rich(active, vec![Edit::new(block.range.start..end, "")], None, window, cx) {
            self.notify_rich(block.range.start, why, false, cx);
        }
        cx.notify();
    }

    fn figure_switch_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Rich(RichFloat::Figure(float)) = &self.float else {
            return;
        };
        let (caption, label) = (float.caption.clone(), float.label.clone());
        let target = if caption.focus_handle(cx).is_focused(window) {
            label
        } else {
            caption
        };
        target.focus_handle(cx).focus(window, cx);
    }

    pub(super) fn figure_element(
        &mut self,
        start: usize,
        area: Rect,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Point<Pixels>)> {
        let index = self.blocks.index_at(start);
        let top = self.block_top(index)?;
        let height = self.layout_block(index, window).height;
        let (text, figure) = self.figure_at(start)?;
        let written = figure.path(&text).to_owned();
        let file = self.session.base_dir().join(&written);
        let size = self.rich.image_size(&file);
        let tokens = Tokens::get(cx).clone();
        let Float::Rich(RichFloat::Figure(float)) = &self.float else {
            return None;
        };
        let (caption, label, placement) = (float.caption.clone(), float.label.clone(), float.placement);
        let refused = float.refused.clone();
        let info: SharedString = match &size {
            Ok(size) => format!("{written}  ·  {}", size_label(*size)).into(),
            Err(_) => crate::t!(Key::ImageMissing, path = written).into(),
        };
        let segment = |id: &'static str, choice: Placement, word: &'static str, cx: &mut Context<Self>| {
            small_button(&tokens, id, word)
                .when(placement == choice, |button| button.text_color(tokens.accent))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Float::Rich(RichFloat::Figure(float)) = &mut this.float {
                        float.placement = choice;
                    }
                    cx.notify();
                }))
        };
        let placements = div()
            .flex()
            .gap(tokens::SPACE_1)
            .child(segment(
                "place-here",
                Placement::Here,
                crate::t!(Key::PlacementHere),
                cx,
            ))
            .child(segment("place-top", Placement::Top, crate::t!(Key::PlacementTop), cx))
            .child(segment(
                "place-bottom",
                Placement::Bottom,
                crate::t!(Key::PlacementBottom),
                cx,
            ));
        let element = card(&tokens)
            .id("figure-float")
            .key_context(CONTEXT)
            .w(WIDTH)
            .p(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.commit_rich_float(window, cx)))
            .on_action(cx.listener(|this, _: &NextField, window, cx| this.figure_switch_field(window, cx)))
            .on_action(cx.listener(|this, _: &PreviousField, window, cx| this.figure_switch_field(window, cx)))
            .child(
                div()
                    .truncate()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(if size.is_ok() { tokens.text_faint } else { tokens.accent })
                    .child(info),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(field_label(&tokens, crate::t!(Key::CaptionLabel)))
                    .child(caption),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(field_label(&tokens, crate::t!(Key::LabelLabel)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(tokens::SPACE_1)
                            .child(div().text_color(tokens.text_faint).child("fig:"))
                            .child(div().flex_1().min_w_0().child(label)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(field_label(&tokens, crate::t!(Key::PlacementLabel)))
                    .child(placements),
            )
            .children(refused.map(|why| div().text_size(tokens::TEXT_SMALL).text_color(tokens.accent).child(why)))
            .child(
                div().flex().justify_end().child(
                    small_button(&tokens, "figure-delete", crate::t!(Key::DeleteFigure))
                        .on_click(cx.listener(|this, _, window, cx| this.delete_figure(window, cx))),
                ),
            )
            .into_any_element();
        let x = self.column.text_left + (self.column.text_width - WIDTH) * 0.5;
        // Below the image, or over it when that would leave the galley.
        let room = px(area.bottom) - (top + height);
        let y = if room >= px(280.) {
            top + height + tokens::FLOAT_GAP
        } else {
            (px(area.bottom) - px(290.)).max(px(area.top))
        };
        Some((element, point(x, y)))
    }
}
