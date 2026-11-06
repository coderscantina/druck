//! The text element: paints the visible blocks on the app's surface, the selection, the caret, the block gutter, and the
//! scrollbar, and hands what floats over them to [`super::float`].

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, LayoutId, MouseMoveEvent, MouseUpEvent, Pixels,
    SharedString, Style, TextAlign, TextRun, Window, div, fill, point, px, relative, size,
};

use super::directives::{Marks, paint_marks};
use super::display::ChipLook;
use super::float::prepaint_float;
use super::gutter::{CommentMarker, block_icon, gutter_cursor, paint_gutter};
use super::layout::{BlockLayout, Column};
use super::margin::{Wanted, flow};
use super::placeholders::{self, Display};
use super::{EditorView, Scrollbar};
use crate::source::BlockKind;
use crate::tokens::{self, Tokens};

pub struct Galley {
    view: Entity<EditorView>,
}

/// How far up a line a misspelled word's underline stands, as a share of the line's height.
const MISSPELLED_RISE: f32 = 0.14;

impl Galley {
    pub fn new(view: Entity<EditorView>) -> Self {
        Self { view }
    }
}

pub(super) struct FrameBlock {
    pub kind: BlockKind,
    pub top: Pixels,
    /// Its window x and width: the text's, or its column's inside two columns.
    pub left: Pixels,
    pub width: Pixels,
    /// Where its gutter icon stands: in the left margin, or in the gap before a second column.
    pub icon_x: Pixels,
    pub layout: Rc<BlockLayout>,
    pub hovered: bool,
    pub icon: Option<&'static str>,
}

pub struct Frame {
    pub(super) column: Column,
    pub(super) blocks: Vec<FrameBlock>,
    selection: Vec<Bounds<Pixels>>,
    /// What an agent's changes brought in, tinted.
    marks: Vec<Bounds<Pixels>>,
    /// The thin lines under the passages notes are about.
    note_lines: Vec<Bounds<Pixels>>,
    /// The line under the passage a conflict is about.
    conflict_lines: Vec<Bounds<Pixels>>,
    /// The find bar's matches on screen.
    found: Vec<Bounds<Pixels>>,
    /// Misspelled words, from the rectangles a selection of them would have.
    misspelled: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
    hitbox: Hitbox,
    gutter_hitbox: Hitbox,
    pub(super) markers: Vec<CommentMarker>,
    pub(super) drop_line: Option<Bounds<Pixels>>,
    /// The pointer is in the left margin, so the gutter shows.
    pub(super) gutter_shown: bool,
    scrollbar: Option<Scrollbar>,
    scrollbar_strong: bool,
    /// The images of visible figures, drawn inside the text's bounds.
    images: Vec<AnyElement>,
    /// The directives' brackets, sheet gaps, and ghost.
    directives: Marks,
    /// The chapter's notes, drawn inside the same bounds, so they scroll under the title bar with the text.
    margin: Option<AnyElement>,
    /// A passage's buttons and the agent's notes, drawn inside the same bounds for the same reason.
    review: Vec<AnyElement>,
    source: bool,
    pub(super) ink: Hsla,
    pub(super) quiet: Hsla,
    pub(super) rule: Hsla,
    link: Hsla,
    error: Hsla,
    code_background: Hsla,
    removed: Hsla,
    body_size: Pixels,
    body_font: gpui::Font,
    accent: Hsla,
    faint: Hsla,
    /// How placeholders' values show.
    placeholders: Display,
    /// The value of the placeholder the caret is in, over where it stands.
    hint: Option<(Bounds<Pixels>, String)>,
    /// The style of the block the caret is in, far out in the left margin: its first line's top and height, and the name.
    /// The caret's block's style name: its row's top and height, the name, and the left edge of a bracket name on that
    /// row, which it ends before.
    style_name: Option<(Pixels, Pixels, String, Option<Pixels>)>,
}

impl IntoElement for Galley {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Galley {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.flex_grow = 1.;
        style.min_size.height = px(0.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Frame {
        let (mut frame, floats, margin, rows, advice) = self.view.update(cx, |view, cx| {
            view.trace.prepaint_started();
            view.set_viewport(bounds, cx);
            view.reveal_float(window, cx);
            if view.autoscroll {
                view.autoscroll = false;
                view.scroll_to_caret(window);
            }
            view.lay_out_visible(window);
            view.spelling_wanted(window, cx);
            view.update_scrollbar();
            let gutter_left = view.column.text_left - tokens::GUTTER_WIDTH;
            let gutter_hitbox = window.insert_hitbox(
                Bounds::new(
                    point(gutter_left, bounds.top()),
                    size(tokens::GUTTER_WIDTH, bounds.size.height),
                ),
                HitboxBehavior::Normal,
            );
            let hitbox = window.insert_hitbox(
                Bounds::new(
                    point(view.column.text_left, bounds.top()),
                    size(bounds.right() - view.column.text_left, bounds.size.height),
                ),
                HitboxBehavior::Normal,
            );
            let frame = view.frame(hitbox, gutter_hitbox, window, cx);
            let selection = frame.selection.clone();
            let caret = frame.caret.or_else(|| view.caret_bounds(window));
            let floats = view.floats(&selection, caret, window, cx);
            let margin = view.margin_element();
            let mut rows = view.review_rows(cx);
            rows.extend(view.pin_markers(cx));
            let mut advice = view.advice_elements(cx);
            advice.extend(view.comment_notes(window, cx));
            advice.extend(view.pin_notes(window, cx));
            advice.sort_by_key(|(_, y)| *y);
            (frame, floats, margin, rows, advice)
        });
        frame.images = self.prepaint_images(&frame, bounds, window, cx);
        let note_left = frame.column.note_left.unwrap_or_default();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (mut element, origin) in rows {
                element.prepaint_as_root(origin, AvailableSpace::min_size(), window, cx);
                frame.review.push(element);
            }
            let measured: Vec<_> = advice
                .into_iter()
                .map(|(mut element, y)| {
                    let height = element.layout_as_root(AvailableSpace::min_size(), window, cx).height;
                    (element, Wanted { y, height })
                })
                .collect();
            let wanted: Vec<Wanted> = measured.iter().map(|(_, wanted)| *wanted).collect();
            for ((mut element, _), y) in measured.into_iter().zip(flow(&wanted, bounds.top())) {
                element.prepaint_at(point(note_left, y), window, cx);
                frame.review.push(element);
            }
        });
        // The chapter's opening notes stand in the flow, above its first text.
        frame.margin = margin.map(|(mut element, origin)| {
            let height = window.with_content_mask(Some(ContentMask { bounds }), |window| {
                let height = element.layout_as_root(AvailableSpace::min_size(), window, cx).height;
                element.prepaint_at(origin, window, cx);
                height
            });
            // Without asking for the next frame, notes unfolded by keyboard focus drew over the text until something else
            // drew, as the caret's blink does in the text but not in a note's field.
            if self.view.update(cx, |view, _| view.set_note_room(height)) {
                let view = self.view.clone();
                window.on_next_frame(move |_, cx| view.update(cx, |_, cx| cx.notify()));
            }
            element
        });
        for (element, spot) in floats {
            prepaint_float(element, spot, window, cx);
        }
        let colors = Tokens::get(cx).clone();
        if let Some((target, text)) = frame.hint.take() {
            prepaint_hint(target, text, bounds, &colors, window, cx);
        }
        if let Some((top, height, name, before)) = frame.style_name.take() {
            prepaint_style_name(&frame.column, top, height, name, before, &colors, window, cx);
        }
        frame
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut Frame,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus, caret_on) = {
            let view = self.view.read(cx);
            (view.focus.clone(), view.caret_on)
        };
        self.view.update(cx, |view, _| view.trace.paint_started());
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        window.set_cursor_style(CursorStyle::IBeam, &frame.hitbox);
        window.set_cursor_style(gutter_cursor(frame), &frame.gutter_hitbox);
        if frame.drop_line.is_some() {
            window.set_window_cursor_style(CursorStyle::ClosedHand);
        }

        let colors = Tokens::get(cx);
        let accent = colors.accent;
        let selection_color = Hsla::from(accent).opacity(tokens::SELECTION_TINT);
        let found_color = Hsla::from(accent).opacity(tokens::MATCH_TINT);
        let thumb_color = Hsla::from(colors.text_faint).opacity(if frame.scrollbar_strong {
            tokens::THUMB_STRONG
        } else {
            tokens::THUMB_FAINT
        });
        let mark_color = Hsla::from(colors.added).opacity(tokens::REVIEW_TINT);
        let misspelled_color = Hsla::from(accent).opacity(tokens::MISSPELLED_TINT);
        let note_line_color = colors.text_faint;
        let focused = focus.is_focused(window);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for block in &frame.blocks {
                paint_block_background(block, frame, window);
            }
            for rectangle in &frame.marks {
                window.paint_quad(fill(*rectangle, mark_color));
            }
            for line in &frame.note_lines {
                window.paint_quad(fill(*line, note_line_color));
            }
            for line in &frame.conflict_lines {
                window.paint_quad(fill(*line, accent));
            }
            for rectangle in &frame.found {
                window.paint_quad(fill(*rectangle, found_color));
            }
            for rectangle in &frame.selection {
                window.paint_quad(fill(*rectangle, selection_color));
            }
            for block in &frame.blocks {
                paint_block_text(block, frame, window, cx);
            }
            paint_marks(&frame.directives, frame, window, cx);
            for word in &frame.misspelled {
                let line = Bounds::new(
                    point(word.left(), word.bottom() - word.size.height * MISSPELLED_RISE),
                    size(word.size.width, tokens::HAIRLINE),
                );
                window.paint_quad(fill(line, misspelled_color));
            }
            for image in &mut frame.images {
                image.paint(window, cx);
            }
            if let Some(margin) = &mut frame.margin {
                margin.paint(window, cx);
            }
            for element in &mut frame.review {
                element.paint(window, cx);
            }
            paint_gutter(frame, window, cx);
            if let Some(bar) = frame.scrollbar {
                window.paint_quad(fill(bar.thumb, thumb_color).corner_radii(tokens::BOX_RADIUS));
            }
            if focused
                && caret_on
                && let Some(caret) = frame.caret
            {
                window.paint_quad(fill(caret, accent));
            }
        });

        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase.bubble() {
                view.update(cx, |view, cx| view.mouse_moved(event, window, cx));
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase.bubble() {
                view.update(cx, |view, cx| view.mouse_up(event, window, cx));
            }
        });
        self.view.update(cx, |view, _| {
            view.trace.painted();
            #[cfg(any(debug_assertions, feature = "measure"))]
            view.note_painted();
        });
    }
}

impl Galley {
    fn prepaint_images(
        &self,
        frame: &Frame,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        let mut images = Vec::new();
        for block in &frame.blocks {
            if let Some(element) = block
                .layout
                .whole
                .as_ref()
                .and_then(|whole| whole.image_element(point(block.left, block.top), bounds, window, cx))
            {
                images.push(element);
            }
        }
        images
    }
}

fn paint_block_background(block: &FrameBlock, frame: &Frame, window: &mut Window) {
    let dotted = frame.placeholders == Display::Underline;
    for line in &block.layout.lines {
        for (chip, look) in &line.chips {
            let bounds = Bounds::new(
                point(block.left + chip.origin.x, block.top + chip.origin.y),
                chip.size,
            );
            let color = match look {
                ChipLook::Mark | ChipLook::NoteLabel | ChipLook::Ghost => continue,
                ChipLook::Value | ChipLook::Note if dotted => {
                    paint_dots(bounds, frame.faint, window);
                    continue;
                }
                ChipLook::Open if dotted => {
                    paint_dots(bounds, frame.accent, window);
                    continue;
                }
                ChipLook::Label => frame.link.opacity(tokens::CHIP_TINT),
                ChipLook::Error => frame.error.opacity(tokens::CHIP_ERROR_TINT),
                ChipLook::Removed => frame.removed.opacity(tokens::REVIEW_TINT),
                ChipLook::Value => frame.ink.opacity(tokens::PLACEHOLDER_TINT),
                ChipLook::Open => frame.accent.opacity(tokens::CHIP_ERROR_TINT),
                ChipLook::Note => frame.ink.opacity(tokens::NOTE_TINT),
            };
            window.paint_quad(fill(bounds, color).corner_radii(tokens::BOX_RADIUS));
        }
    }
    if frame.source {
        return;
    }
    let height = block.layout.height;
    match block.kind {
        BlockKind::Code => {
            let bounds = Bounds::new(
                point(block.left - tokens::CODE_OUTDENT, block.top),
                size(block.width + tokens::CODE_OUTDENT * 2., height),
            );
            window.paint_quad(fill(bounds, frame.code_background));
        }
        BlockKind::Quote => {
            let bounds = Bounds::new(
                point(block.left + tokens::QUOTE_RULE_X, block.top),
                size(tokens::QUOTE_RULE_WIDTH, height),
            );
            window.paint_quad(fill(bounds, frame.rule));
        }
        _ => {}
    }
}

/// A faint dotted line along the foot of a chip's box, under the words.
fn paint_dots(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let y = bounds.bottom() - tokens::HAIRLINE;
    let mut x = bounds.left() + tokens::HAIR_GAP;
    while x + tokens::HAIRLINE <= bounds.right() - tokens::HAIR_GAP {
        window.paint_quad(fill(
            Bounds::new(point(x, y), size(tokens::HAIRLINE, tokens::HAIRLINE)),
            color,
        ));
        x += tokens::DOT_STEP;
    }
}

/// The small hint over a placeholder the caret is in, above its line, or below it at the top of the text.
fn prepaint_hint(
    target: Bounds<Pixels>,
    text: String,
    area: Bounds<Pixels>,
    colors: &Tokens,
    window: &mut Window,
    cx: &mut App,
) {
    let mut element = div()
        .px(tokens::SPACE_2)
        .py(tokens::SPACE_1)
        .rounded(tokens::RADIUS)
        .bg(colors.panel)
        .border_1()
        .border_color(colors.divider)
        .shadow(tokens::float_shadow())
        .font_family(tokens::FONT_UI)
        .text_size(tokens::TEXT_SMALL)
        .text_color(colors.text_quiet)
        .whitespace_nowrap()
        .child(SharedString::from(text))
        .into_any_element();
    let wanted = element.layout_as_root(AvailableSpace::min_size(), window, cx);
    let above = target.top() - wanted.height - tokens::FLOAT_GAP;
    let y = if above >= area.top() {
        above
    } else {
        target.bottom() + tokens::FLOAT_GAP
    };
    let x = target.left().min(area.right() - wanted.width).max(area.left());
    window.defer_draw(element, point(x, y), 1, None);
}

/// A block's style name, one faint word that ends clear of the block gutter's icons and starts clear of the heading
/// ticks. Where the margin has no room for it, it does not show.
fn prepaint_style_name(
    column: &Column,
    top: Pixels,
    height: Pixels,
    name: String,
    before: Option<Pixels>,
    colors: &Tokens,
    window: &mut Window,
    cx: &mut App,
) {
    let mut element = div()
        .h(height)
        .flex()
        .items_center()
        .font_family(tokens::FONT_UI)
        .text_size(tokens::TEXT_SMALL)
        .text_color(colors.text_faint)
        .whitespace_nowrap()
        .child(SharedString::from(name))
        .into_any_element();
    let wanted = element.layout_as_root(AvailableSpace::min_size(), window, cx);
    let clear_of_icons = column.text_left - tokens::GUTTER_WIDTH + tokens::GUTTER_ICON_INSET;
    let right = before.map_or(clear_of_icons, |left| left.min(clear_of_icons)) - tokens::STYLE_NAME_GAP;
    let left = right - wanted.width;
    // Clear of the heading ticks at the pane's left edge too.
    if left < column.viewport.left() + tokens::TICK_ZONE_WIDTH {
        return;
    }
    window.defer_draw(element, point(left, top), 1, None);
}

fn paint_block_text(block: &FrameBlock, frame: &Frame, window: &mut Window, cx: &mut App) {
    if let Some(whole) = &block.layout.whole {
        whole.paint(point(block.left, block.top), frame, window, cx);
    }
    paint_lines(&block.layout, block.left, block.top, frame, window, cx);
}

/// Paints a shaped block's lines with its top left corner at (`left`, `top`).
pub(super) fn paint_lines(
    layout: &BlockLayout,
    left: Pixels,
    top: Pixels,
    frame: &Frame,
    window: &mut Window,
    cx: &mut App,
) {
    for line in &layout.lines {
        if line.height == px(0.) {
            continue;
        }
        let top = top + line.y;
        if let Some((bullet, x)) = &line.bullet {
            let run = TextRun {
                len: bullet.len(),
                font: frame.body_font.clone(),
                color: frame.quiet,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(bullet.clone(), frame.body_size, &[run], None);
            let origin = point(left + *x, top);
            shaped
                .paint(origin, line.line_height, TextAlign::Left, None, window, cx)
                .ok();
        }
        let origin = point(left + line.x, top);
        line.shaped
            .paint(origin, line.line_height, line.align, None, window, cx)
            .ok();
    }
}

impl EditorView {
    /// Everything paint needs, gathered while the view is mutably at hand.
    fn frame(&mut self, hitbox: Hitbox, gutter_hitbox: Hitbox, window: &Window, cx: &App) -> Frame {
        let column = self.column;
        let gutter_icon = column.text_left - tokens::GUTTER_WIDTH + tokens::GUTTER_ICON_INSET;
        let gap = self.directives.column_gap();
        let mut blocks = Vec::with_capacity(self.visible.len());
        for (index, top) in self.visible.clone() {
            let block = self.blocks.as_slice()[index].clone();
            let icon = match self.slots[index].icon {
                Some(icon) => Some(icon),
                None => {
                    let icon = block_icon(block.kind, &self.session.active_buffer().text()[block.range.clone()]);
                    self.slots[index].icon = icon;
                    icon
                }
            };
            // A list table starts with its directive line, and it is a table all the same.
            let icon = if self
                .rich_atom(index)
                .is_some_and(|atom| atom.first == index && atom.is_shown_whole() && block.kind == BlockKind::Directive)
            {
                Some("icons/block-table.svg")
            } else {
                icon
            };
            let left = self.block_left(index);
            blocks.push(FrameBlock {
                kind: block.kind,
                top,
                left,
                width: self.block_width(index),
                icon_x: if left > column.text_left {
                    left - (gap + tokens::GUTTER_ICON) * 0.5
                } else {
                    gutter_icon
                },
                layout: self.layout_block(index, window),
                hovered: self.hovered == Some(index),
                icon,
            });
        }
        let selection = self.range_rectangles(self.selection.range(), &blocks, false);
        let marks = (self.review.iter())
            .flat_map(|passage| &passage.added)
            .flat_map(|added| self.range_rectangles(added.clone(), &blocks, true))
            .collect();
        let rule_under = |passage: Bounds<Pixels>| {
            Bounds::new(
                point(passage.left(), passage.bottom() - tokens::NOTE_RULE),
                size(passage.size.width, tokens::NOTE_RULE),
            )
        };
        let note_lines = (self.advice_anchors().into_iter())
            .flat_map(|anchor| self.range_rectangles(anchor, &blocks, true))
            .map(rule_under)
            .collect();
        let conflict_rows: Vec<_> = (self.conflict_mark.clone().into_iter())
            .flat_map(|mark| self.range_rectangles(mark, &blocks, true))
            .collect();
        self.conflict_rows = (conflict_rows.iter().copied()).reduce(|all, row| all.union(&row));
        let conflict_lines = conflict_rows.into_iter().map(rule_under).collect();
        let on_screen = (self.visible.first().zip(self.visible.last())).map(|((first, _), (last, _))| {
            let list = self.blocks.as_slice();
            list[*first].range.start..list[*last].range.end
        });
        let found = (self.find_matches.iter())
            .filter(|found| {
                on_screen
                    .as_ref()
                    .is_some_and(|shown| found.start < shown.end && shown.start < found.end)
            })
            .flat_map(|found| self.range_rectangles(found.clone(), &blocks, false))
            .collect();
        let misspelled = self
            .spelling_ranges()
            .into_iter()
            .flat_map(|word| self.range_rectangles(word, &blocks, false))
            .collect();
        let caret = if self.selection.is_empty() {
            self.caret_bounds(window)
        } else {
            None
        };
        self.markers = self.comment_markers(&blocks);
        let hint = self.placeholder_hint(&blocks);
        let indices: Vec<usize> = self.visible.iter().map(|(index, _)| *index).collect();
        let directives = self.directive_marks(&blocks, &indices, window, cx);
        // After the brackets, whose names on the same row the style's name ends before.
        let style_name = self.style_name_at_caret(&blocks).map(|(top, height, name)| {
            let before = self.directives.name_left_beside(top, top + height);
            (top, height, name, before)
        });
        Frame {
            column,
            blocks,
            selection,
            marks,
            note_lines,
            conflict_lines,
            found,
            misspelled,
            caret,
            hitbox,
            gutter_hitbox,
            markers: self.markers.clone(),
            drop_line: self.drop_line(),
            gutter_shown: self.gutter_shown || self.grab.is_some(),
            scrollbar: self.scrollbar,
            scrollbar_strong: self.thumb_hovered || self.thumb_grab.is_some(),
            images: Vec::new(),
            directives,
            margin: None,
            review: Vec::new(),
            source: self.source,
            ink: self.style.ink,
            quiet: self.style.quiet,
            rule: self.style.rule,
            link: self.style.link,
            error: self.style.error,
            code_background: self.style.code_background,
            removed: self.style.removed,
            body_size: self.style.body_size,
            body_font: self.style.body.clone(),
            accent: self.style.accent,
            faint: self.style.faint,
            placeholders: self.writing.placeholders,
            hint,
            style_name,
        }
    }

    /// The hint over the placeholder the caret stands inside, with where its source shows.
    fn placeholder_hint(&self, blocks: &[FrameBlock]) -> Option<(Bounds<Pixels>, String)> {
        if self.source || !self.selection.is_empty() {
            return None;
        }
        let text = self.session.active_buffer().text();
        let (range, name) = placeholders::placeholder_at(text, &self.blocks, self.selection.head)?;
        let kind = self.blocks.as_slice()[self.blocks.index_at(range.start)].kind;
        let target = *self.range_rectangles(range, blocks, false).first()?;
        Some((target, placeholders::hint(kind, &name, &self.placeholder_values)))
    }

    /// The style name of the block the caret is in, beside its first line, while that block is on screen.
    fn style_name_at_caret(&self, blocks: &[FrameBlock]) -> Option<(Pixels, Pixels, String)> {
        if self.source {
            return None;
        }
        let index = self.blocks.index_at(self.selection.head);
        let (block, _) = blocks
            .iter()
            .zip(&self.visible)
            .find(|(_, (visible, _))| *visible == index)?;
        let line = block.layout.lines.iter().find(|line| line.height > px(0.))?;
        let name = placeholders::style_name(self.session.active_buffer().text(), &self.blocks, index)?;
        Some((block.top + line.y, line.line_height, name))
    }

    /// The rectangles that cover `selection` in the visible blocks. `past_chip` starts them after removed words standing
    /// at the range's start.
    fn range_rectangles(
        &self,
        selection: std::ops::Range<usize>,
        blocks: &[FrameBlock],
        past_chip: bool,
    ) -> Vec<Bounds<Pixels>> {
        if selection.is_empty() {
            return Vec::new();
        }
        let newline_width = self.style.body_size * 0.4;
        let mut rectangles = Vec::new();
        for (block, (index, _)) in blocks.iter().zip(&self.visible) {
            let block_start = self.blocks.as_slice()[*index].range.start;
            // A table or figure the selection takes in whole shows as one rectangle.
            if let Some(whole) = &block.layout.whole
                && selection.start <= block_start
                && selection.end > block_start
            {
                rectangles.push(Bounds::new(
                    point(block.left, block.top),
                    size(block.width, whole.height()),
                ));
            }
            for line in &block.layout.lines {
                if line.height == px(0.) {
                    continue;
                }
                let line_start = block_start + line.display.source.start;
                let line_end = block_start + line.display.source.end;
                if selection.end < line_start || selection.start > line_end {
                    continue;
                }
                let start = selection
                    .start
                    .saturating_sub(block_start)
                    .max(line.display.source.start);
                let from = if past_chip {
                    line.display.to_display_past_chip(start)
                } else {
                    line.display.to_display(start)
                };
                let to = line
                    .display
                    .to_display(selection.end.saturating_sub(block_start).min(line.display.source.end));
                let crosses_line_end = selection.end > line_end;
                // A blank gap between blocks is how Markdown spells the boundary: nothing to show as selected.
                if line.skipped {
                    continue;
                }
                for (row, range) in line.rows.iter().enumerate() {
                    let start = from.max(range.start);
                    let end = to.min(range.end);
                    let last = line.is_last_row(row);
                    if start > end || (start == end && !(last && crosses_line_end)) {
                        continue;
                    }
                    let x0 = line.x_in_row(start, row);
                    let mut x1 = line.x_in_row(end, row);
                    if last && crosses_line_end {
                        x1 += newline_width;
                    }
                    rectangles.push(Bounds::new(
                        point(
                            block.left + line.x + x0,
                            block.top + line.y + line.line_height * row as f32,
                        ),
                        size(x1 - x0, line.line_height),
                    ));
                }
            }
        }
        rectangles
    }
}
