//! The box the author writes a prompt in: wrapped text, caret and selection, undo, and input methods, so dead keys and
//! umlauts type as they do in the galley. Enter sends, Shift+Enter starts a new line.

use std::ops::Range;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, HighlightStyle, Hsla, InspectorElementId, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, Pixels, ScrollHandle, SharedString, StyledText, TextLayout, UTF16Selection,
    UnderlineStyle, Window, actions, div, fill, point, px, size,
};

use crate::source::buffer::{Buffer, Edit, EditKind, Selection};
use crate::tokens::{self, Tokens};

const CONTEXT: &str = "AgentPrompt";
const MAX_HEIGHT: Pixels = px(160.);
const MIN_HEIGHT: Pixels = px(54.);

actions!(
    agent_prompt,
    [
        Submit,
        Dismiss,
        NewLine,
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        LineStart,
        LineEnd,
        SelectLeft,
        SelectRight,
        SelectAll,
        Backspace,
        Delete,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", Submit, context),
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("shift-enter", NewLine, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("alt-left", WordLeft, context),
        KeyBinding::new("alt-right", WordRight, context),
        KeyBinding::new("cmd-left", LineStart, context),
        KeyBinding::new("home", LineStart, context),
        KeyBinding::new("cmd-right", LineEnd, context),
        KeyBinding::new("end", LineEnd, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-z", Undo, context),
        KeyBinding::new("cmd-shift-z", Redo, context),
    ]);
}

/// The author pressed Enter.
pub struct Submitted;

impl EventEmitter<Submitted> for PromptInput {}

/// The author pressed Escape.
pub struct Dismissed;

impl EventEmitter<Dismissed> for PromptInput {}

pub struct PromptInput {
    buffer: Buffer,
    selection: Selection,
    marked: Option<Range<usize>>,
    focus: FocusHandle,
    placeholder: SharedString,
    /// One line with nothing around it, for a card that draws its own edge.
    bare: bool,
    /// Where the last paint put the text, for input methods and for clicks.
    layout: Option<TextLayout>,
    scroll: ScrollHandle,
}

impl Focusable for PromptInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PromptInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            buffer: Buffer::new(""),
            selection: Selection::default(),
            marked: None,
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            bare: false,
            layout: None,
            scroll: ScrollHandle::new(),
        }
    }

    /// A box of one line for a card, which still wraps what does not fit.
    pub fn one_line(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            bare: true,
            ..Self::new(placeholder, cx)
        }
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn set_placeholder(&mut self, placeholder: &'static str, cx: &mut Context<Self>) {
        if self.placeholder != placeholder {
            self.placeholder = placeholder.into();
            cx.notify();
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buffer = Buffer::new(text);
        self.selection = Selection::caret(text.len());
        self.marked = None;
        cx.notify();
    }

    fn edit(&mut self, kind: EditKind, range: Range<usize>, text: &str, marked: bool, cx: &mut Context<Self>) {
        let before = self.selection;
        let after = Selection::caret(range.start + text.len());
        let edit = Edit::new(range.clone(), text);
        if self
            .buffer
            .transact(kind, vec![edit], before, after, Instant::now())
            .is_ok()
        {
            self.selection = after;
            self.marked = marked.then(|| range.start..range.start + text.len());
            self.scroll.scroll_to_bottom();
            cx.notify();
        }
    }

    fn replace_selection(&mut self, text: &str, kind: EditKind, cx: &mut Context<Self>) {
        let range = self.selection.range();
        self.edit(kind, range, text, false, cx);
    }

    fn set_selection(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.selection = selection;
        self.marked = None;
        cx.notify();
    }

    fn previous(&self, offset: usize) -> usize {
        self.buffer.text()[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    fn next(&self, offset: usize) -> usize {
        self.buffer.text()[offset..]
            .chars()
            .next()
            .map_or(offset, |c| offset + c.len_utf8())
    }

    fn word_before(&self, offset: usize) -> usize {
        let text = &self.buffer.text()[..offset];
        let trimmed = text.trim_end_matches(|c: char| !c.is_alphanumeric());
        trimmed.trim_end_matches(char::is_alphanumeric).len()
    }

    fn word_after(&self, offset: usize) -> usize {
        let text = &self.buffer.text()[offset..];
        let skipped = text.len() - text.trim_start_matches(|c: char| !c.is_alphanumeric()).len();
        let rest = &text[skipped..];
        offset + skipped + (rest.len() - rest.trim_start_matches(char::is_alphanumeric).len())
    }

    fn line_start(&self, offset: usize) -> usize {
        self.buffer.text()[..offset].rfind('\n').map_or(0, |at| at + 1)
    }

    fn line_end(&self, offset: usize) -> usize {
        self.buffer.text()[offset..]
            .find('\n')
            .map_or(self.buffer.len(), |at| offset + at)
    }

    /// The offset one wrapped row above or below the caret, kept in the same column.
    fn vertical(&self, down: bool) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let at = layout.position_for_index(self.selection.head)?;
        let step = layout.line_height();
        let target = point(at.x, if down { at.y + step * 1.5 } else { at.y - step * 0.5 });
        Some(layout.index_for_position(target).unwrap_or_else(|nearest| nearest))
    }

    fn mv(&mut self, target: usize, extend: bool, cx: &mut Context<Self>) {
        let anchor = if extend { self.selection.anchor } else { target };
        self.set_selection(Selection { anchor, head: target }, cx);
    }

    fn on_left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.selection.range();
        let target = if self.selection.is_empty() {
            self.previous(range.start)
        } else {
            range.start
        };
        self.mv(target, false, cx);
    }

    fn on_right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.selection.range();
        let target = if self.selection.is_empty() {
            self.next(range.end)
        } else {
            range.end
        };
        self.mv(target, false, cx);
    }

    fn on_up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical(false).unwrap_or(0);
        self.mv(target, false, cx);
    }

    fn on_down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical(true).unwrap_or(self.buffer.len());
        self.mv(target, false, cx);
    }

    fn on_word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.word_before(self.selection.head);
        self.mv(target, false, cx);
    }

    fn on_word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.word_after(self.selection.head);
        self.mv(target, false, cx);
    }

    fn on_line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.line_start(self.selection.head);
        self.mv(target, false, cx);
    }

    fn on_line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.line_end(self.selection.head);
        self.mv(target, false, cx);
    }

    fn on_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.previous(self.selection.head);
        self.mv(target, true, cx);
    }

    fn on_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.next(self.selection.head);
        self.mv(target, true, cx);
    }

    fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let selection = Selection {
            anchor: 0,
            head: self.buffer.len(),
        };
        self.set_selection(selection, cx);
    }

    fn on_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selection.is_empty() {
            self.previous(self.selection.head)..self.selection.head
        } else {
            self.selection.range()
        };
        self.edit(EditKind::Deleting, range, "", false, cx);
    }

    fn on_delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selection.is_empty() {
            self.selection.head..self.next(self.selection.head)
        } else {
            self.selection.range()
        };
        self.edit(EditKind::Deleting, range, "", false, cx);
    }

    fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            let text = self.buffer.text()[self.selection.range()].to_owned();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
    }

    fn on_cut(&mut self, action: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        let _ = (action, window);
        if !self.selection.is_empty() {
            let text = self.buffer.text()[self.selection.range()].to_owned();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            self.replace_selection("", EditKind::Discrete, cx);
        }
    }

    fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_selection(&text.replace("\r\n", "\n"), EditKind::Discrete, cx);
        }
    }

    fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(restored) = self.buffer.undo() {
            self.set_selection(restored.selection, cx);
        }
    }

    fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(restored) = self.buffer.redo() {
            self.set_selection(restored.selection, cx);
        }
    }

    fn on_new_line(&mut self, _: &NewLine, _: &mut Window, cx: &mut Context<Self>) {
        self.replace_selection("\n", EditKind::Discrete, cx);
    }

    fn on_dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Dismissed);
    }

    fn on_submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        // While an input method composes, Enter commits the composition, which it handles before this runs.
        cx.emit(Submitted);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if let Some(layout) = &self.layout {
            let offset = layout
                .index_for_position(event.position)
                .unwrap_or_else(|nearest| nearest);
            self.mv(offset.min(self.buffer.len()), event.modifiers.shift, cx);
        }
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.buffer.range_from_utf16(range)
    }
}

impl Render for PromptInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .id("agent-prompt")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_submit))
            .on_action(cx.listener(Self::on_dismiss))
            .on_action(cx.listener(Self::on_new_line))
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_word_left))
            .on_action(cx.listener(Self::on_word_right))
            .on_action(cx.listener(Self::on_line_start))
            .on_action(cx.listener(Self::on_line_end))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .max_h(MAX_HEIGHT)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .when(!self.bare, |input| {
                input
                    .min_h(MIN_HEIGHT)
                    .px(tokens::SPACE_2)
                    .py(px(6.))
                    .rounded(tokens::RADIUS)
                    .border_1()
                    .border_color(tokens.divider)
                    .bg(tokens.field)
            })
            .child(PromptText {
                input: cx.entity(),
                placeholder: self.placeholder.clone(),
            })
    }
}

/// The text element: lays out the text, paints the selection and caret, and takes input for the box.
struct PromptText {
    input: Entity<PromptInput>,
    placeholder: SharedString,
}

struct Prepaint {
    layout: TextLayout,
    caret: Option<Bounds<Pixels>>,
}

impl IntoElement for PromptText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PromptText {
    type RequestLayoutState = StyledText;
    type PrepaintState = Prepaint;

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
    ) -> (LayoutId, StyledText) {
        let tokens = Tokens::get(cx);
        let (selection_color, marked_color) = (Hsla::from(tokens.accent).opacity(0.3), Hsla::from(tokens.text));
        let input = self.input.read(cx);
        let text = input.buffer.text();
        let empty = text.is_empty();
        let shown: SharedString = if empty {
            self.placeholder.clone()
        } else {
            text.to_owned().into()
        };
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        if empty {
            highlights.push((
                0..shown.len(),
                HighlightStyle {
                    color: Some(tokens.text_faint.into()),
                    ..Default::default()
                },
            ));
        } else {
            let selected = input.selection.range();
            let marked = input.marked.clone();
            let mut edges = vec![0, text.len()];
            edges.extend([selected.start, selected.end]);
            edges.extend(marked.iter().flat_map(|marked| [marked.start, marked.end]));
            edges.sort_unstable();
            edges.dedup();
            for pair in edges.windows(2) {
                let part = pair[0]..pair[1];
                let is_selected = !selected.is_empty() && selected.start <= part.start && part.end <= selected.end;
                let is_marked = marked
                    .as_ref()
                    .is_some_and(|m| m.start <= part.start && part.end <= m.end);
                highlights.push((
                    part,
                    HighlightStyle {
                        background_color: is_selected.then_some(selection_color),
                        underline: is_marked.then_some(UnderlineStyle {
                            thickness: px(1.),
                            color: Some(marked_color),
                            wavy: false,
                        }),
                        ..Default::default()
                    },
                ));
            }
        }
        let mut styled = StyledText::new(shown).with_highlights(highlights);
        let (layout_id, ()) = styled.request_layout(None, None, window, cx);
        (layout_id, styled)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        styled: &mut StyledText,
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        styled.prepaint(None, None, bounds, &mut (), window, cx);
        let layout = styled.layout().clone();
        let input = self.input.read(cx);
        let caret = input
            .selection
            .is_empty()
            .then(|| layout.position_for_index(input.selection.head))
            .flatten()
            .map(|origin| Bounds::new(origin, size(px(1.5), layout.line_height())));
        Prepaint { layout, caret }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        styled: &mut StyledText,
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);
        styled.paint(None, None, bounds, &mut (), &mut (), window, cx);
        if focus.is_focused(window)
            && let Some(caret) = prepaint.caret
        {
            window.paint_quad(fill(caret, Tokens::get(cx).accent));
        }
        let layout = prepaint.layout.clone();
        self.input.update(cx, |input, _| input.layout = Some(layout));
    }
}

impl EntityInputHandler for PromptInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.buffer.range_to_utf16(&range));
        Some(self.buffer.text()[range].to_owned())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.selection.range()),
            reversed: self.selection.reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.buffer.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.range());
        let kind = if self.marked.is_some() {
            EditKind::Composing
        } else {
            EditKind::Typing
        };
        self.edit(kind, range, new_text, false, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.range());
        self.edit(EditKind::Composing, range, new_text, !new_text.is_empty(), cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = layout.position_for_index(range.start)?;
        Some(Bounds::new(start, size(px(1.), layout.line_height())))
    }

    fn character_index_for_point(
        &mut self,
        at: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let offset = layout.index_for_position(at).unwrap_or_else(|nearest| nearest);
        Some(self.buffer.utf16_of(offset.min(self.buffer.len())))
    }
}
