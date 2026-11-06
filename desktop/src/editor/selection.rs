//! Where the caret and the selection go: by keys, by row and page, and by the pointer, with a drag that scrolls the text
//! while it is outside.

use super::*;

impl EditorView {
    pub(super) fn set_selection(
        &mut self,
        selection: Selection,
        upstream: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if selection.head != self.selection.head {
            cx.emit(CaretMoved(selection.head));
        }
        self.selection = selection;
        self.upstream = upstream;
        self.goal_x = None;
        self.toolbar_dismissed = false;
        if self.drag.is_none() {
            self.reveal_at = selection.head;
        }
        self.autoscroll = true;
        self.follow_slash();
        self.restart_blink(window, cx);
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    pub(super) fn move_caret(
        &mut self,
        motion_kind: Motion,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = self.trace.input();
        self.caret_by_keys = true;
        let text = self.session.active_buffer().text();
        let head = self.selection.head;
        let range = self.selection.range();
        let collapsing = !extend && !self.selection.is_empty();
        let goal_x = self.goal_x;
        let mut new_goal = None;
        let (target, upstream) = match motion_kind {
            Motion::Left if collapsing => (range.start, false),
            Motion::Right if collapsing => (range.end, false),
            Motion::Left if self.source => (motion::prev_grapheme(text, head), false),
            Motion::Right if self.source => (motion::next_grapheme(text, head), false),
            Motion::Left => (motion::left(text, head), false),
            Motion::Right => (motion::right(text, head), false),
            Motion::WordLeft => (motion::prev_word(text, head), false),
            Motion::WordRight => (motion::next_word(text, head), false),
            Motion::DocStart => (self.chapter_edge(false), false),
            Motion::DocEnd => (self.chapter_edge(true), false),
            Motion::LineStart => self.row_edge(false, window),
            Motion::LineEnd => self.row_edge(true, window),
            Motion::Up | Motion::Down => {
                let (target, upstream, goal) =
                    self.vertical_target(matches!(motion_kind, Motion::Down), goal_x, window);
                new_goal = Some(goal);
                (target, upstream)
            }
            Motion::PageUp | Motion::PageDown => {
                let (target, upstream, goal) =
                    self.page_target(matches!(motion_kind, Motion::PageDown), goal_x, window);
                new_goal = Some(goal);
                (target, upstream)
            }
        };
        let target = self.skip_hidden(target, target > head);
        let target = self.past_chip(target, target > head);
        let selection = Selection {
            anchor: if extend { self.selection.anchor } else { target },
            head: target,
        };
        self.set_selection(selection, upstream, window, cx);
        self.goal_x = new_goal;
        self.trace.handled(started);
    }

    /// Where Command+Up and Down go. The source view shows everything, so there it is the text's ends.
    fn chapter_edge(&self, end: bool) -> usize {
        let text = self.session.active_buffer().text();
        if self.source {
            return if end { text.len() } else { 0 };
        }
        chapter_edge(text, &self.blocks, end, |index| self.slots[index].expanded)
    }

    /// The source offset of the start or end of the caret's row.
    pub(super) fn row_edge(&mut self, end: bool, window: &Window) -> (usize, bool) {
        let here = self.locate(self.selection.head, self.upstream, window);
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let row = &line.rows[here.row];
        let display = if end { row.end } else { row.start };
        let upstream = end && !line.is_last_row(here.row);
        let start = self.blocks.as_slice()[here.block].range.start;
        (start + line.display.to_source(display), upstream)
    }

    /// The target of up or down, and the column it aims for.
    pub(super) fn vertical_target(
        &mut self,
        down: bool,
        goal: Option<Pixels>,
        window: &Window,
    ) -> (usize, bool, Pixels) {
        let here = self.locate(self.selection.head, self.upstream, window);
        let layout = self.layout_block(here.block, window);
        let line = &layout.lines[here.line];
        let goal = goal.unwrap_or(line.x + line.x_in_row(here.display, here.row));
        let last_block = self.blocks.as_slice().len() - 1;
        let (mut block, mut line_index, mut row) = (here.block, here.line, here.row);
        let mut current = layout;
        loop {
            if down {
                if row + 1 < current.lines[line_index].rows.len() {
                    row += 1;
                } else if line_index + 1 < current.lines.len() {
                    line_index += 1;
                    row = 0;
                } else if block < last_block {
                    block += 1;
                    current = self.layout_block(block, window);
                    line_index = 0;
                    row = 0;
                } else {
                    return (self.session.active_buffer().len(), false, goal);
                }
            } else if row > 0 {
                row -= 1;
            } else if line_index > 0 {
                line_index -= 1;
                row = current.lines[line_index].rows.len() - 1;
            } else if block > 0 {
                block -= 1;
                current = self.layout_block(block, window);
                line_index = current.lines.len() - 1;
                row = current.lines[line_index].rows.len() - 1;
            } else {
                return (0, false, goal);
            }
            if !current.lines[line_index].skipped {
                break;
            }
        }
        let line = &current.lines[line_index];
        let display = line.display_at_x(row, goal - line.x);
        let upstream = display == line.rows[row].end && !line.is_last_row(row);
        let start = self.blocks.as_slice()[block].range.start;
        (start + line.display.to_source(display), upstream, goal)
    }

    /// Scrolls a page and returns the place at the caret's old height on screen.
    pub(super) fn page_target(&mut self, down: bool, goal: Option<Pixels>, window: &Window) -> (usize, bool, Pixels) {
        let height = self.column.viewport.size.height;
        let distance = (height - tokens::PAGE_KEY_OVERLAP).max(tokens::PAGE_KEY_OVERLAP);
        let caret = self.caret_bounds(window);
        let here = caret.map_or(self.column.viewport.origin, |caret| caret.origin);
        let goal = goal.unwrap_or(here.x);
        self.scroll_by(if down { distance } else { px(0.) - distance });
        self.lay_out_visible(window);
        let target = point(goal, here.y.max(self.column.viewport.top()));
        let (offset, upstream) = self.offset_at(target, window);
        (offset, upstream, goal)
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.active_buffer().text();
        let anchor = notes::leading_region(text, &self.blocks)
            .filter(|region| !self.source && region.start == 0)
            .map_or(0, |region| region.end);
        let selection = Selection {
            anchor,
            head: self.session.active_buffer().len(),
        };
        self.set_selection(selection, false, window, cx);
    }

    pub(super) fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.pointer = event.position;
        self.caret_by_keys = false;
        self.commit_rich_float(window, cx);
        self.close_float(cx);
        if self.scrollbar_mouse_down(event.position, cx) || self.gutter_mouse_down(event.position, cx) {
            return;
        }
        if event.click_count == 1 && !event.modifiers.shift && self.rich_mouse_down(event.position, window, cx) {
            return;
        }
        let (offset, upstream) = self.offset_at(event.position, window);
        let offset = self.skip_hidden(offset, true);
        let text = self.session.active_buffer().text();
        let (origin, unit) = match event.click_count {
            1 => (offset..offset, DragUnit::Character),
            2 => (motion::word_range(text, offset), DragUnit::Word),
            _ => (motion::line_range(text, offset), DragUnit::Line),
        };
        let selection = if event.modifiers.shift {
            Selection {
                anchor: self.selection.anchor,
                head: offset,
            }
        } else {
            Selection {
                anchor: origin.start,
                head: origin.end,
            }
        };
        self.drag = Some(Drag { origin, unit });
        self.reveal_at = selection.head;
        self.set_selection(selection, upstream, window, cx);
    }

    pub(super) fn mouse_moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.pointer = event.position;
        let viewport = self.column.viewport;
        let inside = viewport.contains(&event.position);
        let hovered = if inside {
            self.block_at_y(event.position.y)
        } else {
            None
        };
        if hovered != self.hovered {
            self.hovered = hovered;
            cx.notify();
        }
        let in_margin = inside && event.position.x < self.column.text_left;
        #[cfg(any(debug_assertions, feature = "measure"))]
        let in_margin = in_margin || self.gutter_pinned;
        if in_margin != self.gutter_shown {
            self.gutter_shown = in_margin;
            cx.notify();
        }
        let over_thumb = self.scrollbar.is_some_and(|bar| bar.track.contains(&event.position));
        if over_thumb != self.thumb_hovered {
            self.thumb_hovered = over_thumb;
            cx.notify();
        }
        let pressed = event.pressed_button == Some(MouseButton::Left);
        if !pressed && (self.drag.is_some() || self.grab.is_some() || self.thumb_grab.is_some()) {
            self.end_mouse(window, cx);
            return;
        }
        if self.thumb_grab.is_some() {
            self.scrollbar_mouse_moved(event.position, cx);
            return;
        }
        if self.grab.is_some() {
            self.gutter_mouse_moved(event.position, cx);
            self.keep_scrolling_while_outside(window, cx);
            return;
        }
        if self.drag.is_some() {
            self.extend_drag(window, cx);
            self.keep_scrolling_while_outside(window, cx);
        }
    }

    /// Extends the selection in progress to the pointer.
    pub(super) fn extend_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.clone() else {
            return;
        };
        let (offset, upstream) = self.offset_at(self.pointer, window);
        let text = self.session.active_buffer().text();
        let unit = match drag.unit {
            DragUnit::Character => offset..offset,
            DragUnit::Word => motion::word_range(text, offset),
            DragUnit::Line => motion::line_range(text, offset),
        };
        let selection = if self.selection.anchor != drag.origin.start && drag.unit == DragUnit::Character {
            // A shift-click drag keeps the anchor it started from.
            Selection {
                anchor: self.selection.anchor,
                head: offset,
            }
        } else if unit.start < drag.origin.start {
            Selection {
                anchor: drag.origin.end,
                head: unit.start,
            }
        } else {
            Selection {
                anchor: drag.origin.start,
                head: unit.end.max(drag.origin.end),
            }
        };
        if selection != self.selection {
            self.set_selection(selection, upstream, window, cx);
            self.autoscroll = false;
        }
    }

    /// How far to scroll for a pointer standing outside the galley, up or down.
    pub(super) fn drag_scroll_step(&self) -> Option<Pixels> {
        let viewport = self.column.viewport;
        let above = viewport.top() - self.pointer.y;
        let below = self.pointer.y - viewport.bottom();
        let step = |distance: Pixels| {
            (distance * DRAG_SCROLL_SPEED)
                .max(tokens::HAIRLINE)
                .min(DRAG_SCROLL_MAX)
        };
        if above > px(0.) {
            Some(px(0.) - step(above))
        } else if below > px(0.) {
            Some(step(below))
        } else {
            None
        }
    }

    /// Scrolls on a timer while a drag's pointer stands outside the galley, whether or not it moves.
    pub(super) fn keep_scrolling_while_outside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag_scroll_step().is_none() {
            self.drag_scroll = None;
            return;
        }
        if self.drag_scroll.is_some() {
            return;
        }
        self.drag_scroll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(DRAG_SCROLL_TICK).await;
                let going = this.update_in(cx, |this, window, cx| {
                    let Some(step) = this
                        .drag_scroll_step()
                        .filter(|_| this.drag.is_some() || this.grab.is_some())
                    else {
                        return false;
                    };
                    this.scroll_by(step);
                    this.lay_out_visible(window);
                    this.extend_drag(window, cx);
                    this.gutter_mouse_moved(this.pointer, cx);
                    window.invalidate_character_coordinates();
                    cx.notify();
                    true
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
            this.update(cx, |this, _| this.drag_scroll = None).ok();
        }));
    }

    pub(super) fn mouse_up(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.end_mouse(window, cx);
    }

    /// The button came up: a selection settles, a held icon drops or opens its menu, and the thumb lets go.
    pub(super) fn end_mouse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drag_scroll = None;
        self.thumb_grab = None;
        self.gutter_mouse_up(window, cx);
        if self.drag.take().is_some() {
            self.reveal_at = self.selection.head;
            cx.notify();
        }
    }
}

/// The start of a chapter's first text that shows, its title, or with `end` the end of its last, never inside a note
/// folded away above or below it: the author opens a note by going to it on purpose. `expanded` says which comments
/// their gutter marker shows in the flow.
pub(super) fn chapter_edge(text: &str, blocks: &Blocks, end: bool, expanded: impl Fn(usize) -> bool) -> usize {
    let blocks = blocks.as_slice();
    let folded = |index: usize| {
        let block = &blocks[index];
        let written = &text[block.range.clone()];
        block.kind == BlockKind::Comment
            && notes::is_closed(written)
            && !crate::persist::merge::is_marker(written)
            && !expanded(index)
    };
    let shows = |index: &usize| blocks[*index].kind != BlockKind::Blank && !folded(*index);
    if !end {
        return (0..blocks.len())
            .find(shows)
            .map_or(0, |index| blocks[index].range.start);
    }
    match (0..blocks.len()).rev().find(shows) {
        Some(last) if (last + 1..blocks.len()).any(folded) => {
            let range = blocks[last].range.clone();
            range.start + text[range].trim_end_matches(['\n', '\r']).len()
        }
        _ => text.len(),
    }
}
