//! What the text shows of an agent's work: changed passages in the book's type, with removed words struck through and new
//! words tinted, small buttons below each, and notes beside their passages. The agent module says what to show, and hears
//! what the buttons were pressed for through [`ReviewClicked`] and [`NoteClicked`]. Nothing here is in the buffer: the
//! removed words show as zero-length chips, so the caret and every offset keep to the Markdown.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, Context, Div, EventEmitter, FontWeight, MouseButton, Pixels, Point, Stateful, Window, div,
    point,
};

use super::EditorView;
use super::display::{Chip, ChipLook};
use super::strings::Key;
use crate::a11y::Control as _;
use crate::source::buffer::Splice;
use crate::tokens::{self, Tokens};

/// One changed passage of the open chapter. Offsets are into the chapter's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub id: u64,
    /// What came in, tinted.
    pub added: Vec<Range<usize>>,
    /// What went, struck through, standing before the offset.
    pub removed: Vec<(usize, String)>,
    /// An offset in the block the buttons stand under.
    pub end: usize,
}

/// A note for the margin of the open chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    pub id: u64,
    pub title: String,
    pub text: String,
    pub suggestion: Option<String>,
    /// Where its quote is. `None`: the text is gone.
    pub anchor: Option<Range<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewAction {
    Accept,
    Reject,
    TryAgain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteAction {
    Apply,
    Reply,
    Dismiss,
}

/// A passage's button was pressed.
pub struct ReviewClicked(pub u64, pub ReviewAction);

/// A note's button was pressed.
pub struct NoteClicked(pub u64, pub NoteAction);

impl EventEmitter<ReviewClicked> for EditorView {}
impl EventEmitter<NoteClicked> for EditorView {}

/// A small button: quiet, with a thin border, and nothing filled. The agent's panels use it too.
pub(crate) fn small_button(id: impl Into<gpui::ElementId>, label: String, tokens: &Tokens) -> Stateful<Div> {
    div()
        .id(id)
        .button(tokens, label.clone())
        .flex_none()
        .h(tokens::SMALL_BUTTON_HEIGHT)
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .rounded(tokens::RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .cursor_pointer()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .hover(|button| button.bg(tokens.raised).text_color(tokens.text))
        .child(label)
}

/// `offset` after a change of the text.
fn moved(offset: usize, splice: &Splice) -> usize {
    if offset <= splice.start {
        offset
    } else if offset >= splice.old_end {
        offset + splice.new_end - splice.old_end
    } else {
        splice.new_end
    }
}

/// Moves what is shown with a change of the text, so typing does not wait for the agent module to look again.
pub(super) fn shift(passages: &mut [Passage], advice: &mut [Advice], splice: &Splice) {
    let range = |range: &mut Range<usize>| {
        range.start = moved(range.start, splice);
        range.end = moved(range.end, splice);
    };
    for passage in passages {
        passage.added.iter_mut().for_each(range);
        for (at, _) in &mut passage.removed {
            *at = moved(*at, splice);
        }
        passage.end = moved(passage.end, splice);
    }
    for note in advice {
        note.anchor.iter_mut().for_each(range);
    }
}

impl EditorView {
    /// Shows these passages of the active chapter. They replace the earlier ones, and an empty list clears them.
    pub fn set_review(&mut self, passages: Vec<Passage>, cx: &mut Context<Self>) {
        if self.review != passages {
            self.review = passages;
            cx.notify();
        }
    }

    /// Shows these notes beside the active chapter's text.
    pub fn set_advice(&mut self, advice: Vec<Advice>, cx: &mut Context<Self>) {
        if self.advice != advice {
            self.advice = advice;
            cx.notify();
        }
    }

    /// The passages shown now, for scripted runs.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn review(&self) -> &[Passage] {
        &self.review
    }

    /// A card to float under the selection, or none. It holds the keys while the selection stays tinted behind it.
    pub fn set_selection_prompt(&mut self, prompt: Option<AnyView>, cx: &mut Context<Self>) {
        self.selection_prompt = prompt;
        cx.notify();
    }

    /// The removed words standing in a block, as chips. `taken` is what already shows as chips, and words inside `hidden`
    /// stand after it, since hidden text has nowhere to show them.
    pub(super) fn review_chips(
        &self,
        block: &Range<usize>,
        block_text: &str,
        hidden: &[Range<usize>],
        taken: &[Chip],
    ) -> Vec<Chip> {
        let last = block.end == self.text().len();
        let mut chips = Vec::new();
        for passage in &self.review {
            for (at, text) in &passage.removed {
                if *at < block.start || *at > block.end || (*at == block.end && !last) {
                    continue;
                }
                let at = hidden
                    .iter()
                    .find(|range| range.start < *at - block.start && *at - block.start < range.end)
                    .map_or(*at - block.start, |range| range.end);
                let inside = taken.iter().any(|chip| chip.range.start < at && at < chip.range.end);
                if !inside && block_text.is_char_boundary(at) {
                    chips.push(Chip {
                        range: at..at,
                        text: text.clone(),
                        look: ChipLook::Removed,
                        atomic: false,
                    });
                }
            }
        }
        chips
    }

    /// The room a block keeps below its text for its passages' buttons.
    pub(super) fn review_room(&self, index: usize) -> Pixels {
        let count = self
            .review
            .iter()
            .filter(|passage| self.blocks.index_at(passage.end) == index)
            .count();
        tokens::REVIEW_ROW_HEIGHT * count as f32
    }

    /// The buttons under the passages on screen.
    pub(super) fn review_rows(&self, cx: &mut Context<Self>) -> Vec<(AnyElement, Point<Pixels>)> {
        let tokens = Tokens::get(cx).clone();
        let mut rows: Vec<(AnyElement, Point<Pixels>)> = Vec::new();
        for (index, top) in &self.visible {
            let here: Vec<&Passage> = (self.review.iter())
                .filter(|passage| self.blocks.index_at(passage.end) == *index)
                .collect();
            if here.is_empty() {
                continue;
            }
            let Some(layout) = self.slots[*index].layout.as_ref() else {
                continue;
            };
            // Under the last line with text, not a blank line closing the chapter.
            let text_bottom = (layout.whole.is_none())
                .then(|| {
                    (layout.lines.iter().rev())
                        .find(|line| !line.skipped && !line.display.text.trim().is_empty())
                        .map(|line| line.y + line.height)
                })
                .flatten()
                .unwrap_or(layout.height - tokens::REVIEW_ROW_HEIGHT * here.len() as f32);
            let first = *top + text_bottom;
            for (row, passage) in here.iter().enumerate() {
                let y = first + tokens::REVIEW_ROW_HEIGHT * row as f32 + tokens::REVIEW_ROW_GAP;
                let element = self.review_row(passage.id, &tokens, cx).into_any_element();
                rows.push((element, point(self.column.text_left, y)));
            }
        }
        rows
    }

    fn review_row(&self, id: u64, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let button = |name: &'static str, label: Key, action: ReviewAction, cx: &mut Context<Self>| {
            small_button((name, id as usize), crate::t!(label).to_owned(), tokens)
                .when(action == ReviewAction::Accept, |button| {
                    button.text_color(tokens.accent)
                })
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(ReviewClicked(id, action))))
        };
        div()
            .flex()
            .gap(tokens::SPACE_1)
            .font_family(tokens::FONT_UI)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(button("review-accept", Key::Accept, ReviewAction::Accept, cx))
            .child(button("review-reject", Key::Reject, ReviewAction::Reject, cx))
            .child(button("review-again", Key::TryAgain, ReviewAction::TryAgain, cx))
    }

    /// The notes for this frame, each with the height it wants to stand at. Notes on passages off screen wait, and notes
    /// whose text is gone stand at the top.
    pub(super) fn advice_elements(&self, window: &Window, cx: &mut Context<Self>) -> Vec<(AnyElement, Pixels)> {
        if self.column.note_left.is_none() {
            return Vec::new();
        }
        let tokens = Tokens::get(cx).clone();
        let top = self.column.viewport.top() + tokens::SPACE_3;
        let body = self.style.body.clone();
        let mut notes = Vec::new();
        for note in &self.advice {
            let y = match &note.anchor {
                None => top,
                Some(anchor) => match self.first_line_top(anchor.start) {
                    Some(y) => y,
                    None => continue,
                },
            };
            notes.push((self.advice_note(note, &body, window, &tokens, cx).into_any_element(), y));
        }
        notes
    }

    /// The window height a visible offset's block starts at, with its line's own height added for a block of many lines.
    pub(super) fn first_line_top(&self, offset: usize) -> Option<Pixels> {
        let index = self.blocks.index_at(offset);
        let (_, top) = self.visible.iter().find(|(visible, _)| *visible == index)?;
        let layout = self.slots[index].layout.as_ref()?;
        let block_start = self.blocks.as_slice()[index].range.start;
        let line = layout
            .lines
            .iter()
            .find(|line| offset - block_start <= line.display.source.end)?;
        Some(*top + line.y)
    }

    fn advice_note(
        &self,
        note: &Advice,
        body: &gpui::Font,
        window: &Window,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = note.id;
        let button = |name: &'static str, label: Key, action: NoteAction, cx: &mut Context<Self>| {
            small_button((name, id as usize), crate::t!(label).to_owned(), tokens)
                .px(tokens::SPACE_3)
                .text_size(tokens::TEXT_BODY)
                .when(action == NoteAction::Apply, |button| button.text_color(tokens.accent))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(NoteClicked(id, action))))
        };
        let body_size = self.style.body_size;
        let gone = note.anchor.is_none();
        let text = if gone {
            crate::t!(Key::NoteTextGone).to_owned()
        } else {
            note.text.clone()
        };
        let apply =
            (note.suggestion.is_some() && !gone).then(|| button("note-apply", Key::Apply, NoteAction::Apply, cx));
        let reply = (!gone).then(|| button("note-reply", Key::Reply, NoteAction::Reply, cx));
        let dismiss = button("note-dismiss", Key::Dismiss, NoteAction::Dismiss, cx);
        let text_size = body_size * tokens::ADVICE_TEXT;
        let text_width = tokens::NOTE_WIDTH - tokens::SPACE_3 - tokens::NOTE_RULE;
        let open = self.open_advice.contains(&id);
        let label = crate::t!(if open { Key::NoteLess } else { Key::NoteMore });
        let measure = |text: &str| f32::from(super::margin::width_of(text, text_size, window, tokens));
        let room = measure(label) + f32::from(tokens::SPACE_1);
        let clipped = clipped(&text, tokens::ADVICE_QUIET_LINES, f32::from(text_width), room, measure);
        let long = clipped.is_some();
        let more = long.then(|| {
            div()
                .id(("note-more", id as usize))
                .button(tokens, label)
                .flex_none()
                .cursor_pointer()
                .text_color(tokens.text_quiet)
                .hover(|link| link.text_color(tokens.text))
                .on_click(cx.listener(move |editor, _, _, cx| {
                    if !editor.open_advice.remove(&id) {
                        editor.open_advice.insert(id);
                    }
                    cx.notify();
                }))
                .child(label)
        });
        div()
            .w(tokens::NOTE_WIDTH)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .pl(tokens::SPACE_3)
            .py(tokens::SPACE_1)
            .bg(tokens.canvas)
            .border_l_1()
            .border_color(tokens.accent)
            .font_family(tokens::FONT_UI)
            .text_size(text_size)
            .line_height(text_size * tokens::ADVICE_LEADING)
            .text_color(tokens.text_quiet)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(body_size * tokens::ADVICE_TITLE)
                    .line_height(body_size * tokens::ADVICE_TITLE * tokens::ADVICE_LEADING)
                    .text_color(tokens.text)
                    .child(note.title.clone()),
            )
            .map(|column| match clipped.filter(|_| !open) {
                // Its last line ends in an ellipsis with More after it.
                Some(mut lines) => {
                    let last = lines.pop().unwrap_or_default();
                    column.child(
                        div()
                            .flex()
                            .flex_col()
                            .children(lines.into_iter().map(|line| div().whitespace_nowrap().child(line)))
                            .child(
                                div()
                                    .flex()
                                    .gap(tokens::SPACE_1)
                                    .child(div().whitespace_nowrap().child(last))
                                    .children(more),
                            ),
                    )
                }
                None => column.child(text).children(more.map(|more| more.mt(-tokens::SPACE_1))),
            })
            .when_some(note.suggestion.clone().filter(|_| !gone), |column, suggestion| {
                column.child(
                    div()
                        .font(body.clone())
                        .text_size(body_size * tokens::ADVICE_SUGGESTION)
                        .line_height(body_size * tokens::ADVICE_SUGGESTION * tokens::ADVICE_LEADING)
                        .text_color(tokens.writing)
                        .child(suggestion),
                )
            })
            .child(
                div()
                    .mt(tokens::SPACE_1)
                    .flex()
                    .flex_wrap()
                    .gap(tokens::SPACE_2)
                    .children(apply)
                    .children(reply)
                    .child(dismiss),
            )
    }

    /// Takes a note's anchor as a thin line under its passage, for this frame's blocks.
    pub(super) fn advice_anchors(&self) -> Vec<Range<usize>> {
        self.advice.iter().filter_map(|note| note.anchor.clone()).collect()
    }

    /// Closes the review of a chapter being left: the agent module gives the next chapter's when it hears of the change.
    pub(super) fn forget_review(&mut self) {
        self.review.clear();
        self.advice.clear();
        self.pins.clear();
        self.open_comments.clear();
        self.open_advice.clear();
        self.comment_focus.clear();
    }
}

/// The first `lines` lines of `text` broken at words to fit `width`, the last ending in an ellipsis with `room` left
/// after it, or `None` when the whole text fits in that many lines.
pub(super) fn clipped(
    text: &str,
    lines: usize,
    width: f32,
    room: f32,
    measure: impl Fn(&str) -> f32,
) -> Option<Vec<String>> {
    let mut words = text.split_whitespace().peekable();
    let mut shown: Vec<String> = (1..lines).map(|_| take_line(&mut words, width, &measure)).collect();
    let mut rest = words.clone();
    take_line(&mut rest, width, &measure);
    rest.peek()?;
    let ellipsis = "\u{2026}";
    let mut last = String::new();
    while let Some(word) = words.peek() {
        let longer = if last.is_empty() {
            (*word).to_owned()
        } else {
            format!("{last} {word}")
        };
        if measure(&format!("{longer}{ellipsis}")) + room > width {
            break;
        }
        last = longer;
        words.next();
    }
    let mut last = last.trim_end_matches([',', ';', ':']).to_owned();
    last.push_str(ellipsis);
    shown.push(last);
    Some(shown)
}

/// The words of one line up to `width`, at least one so a long word still moves on.
fn take_line<'a>(
    words: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
    width: f32,
    measure: &impl Fn(&str) -> f32,
) -> String {
    let mut line = String::new();
    while let Some(word) = words.peek() {
        let longer = if line.is_empty() {
            (*word).to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && measure(&longer) > width {
            break;
        }
        line = longer;
        words.next();
    }
    line
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init, reason = "a list of byte ranges that holds one")]
mod tests {
    use super::*;

    fn passage() -> Passage {
        Passage {
            id: 1,
            added: vec![10..20],
            removed: vec![(10, "old".to_owned())],
            end: 10,
        }
    }

    #[test]
    fn a_clipped_note_ends_at_a_word_with_room_for_more() {
        let measure = |text: &str| text.chars().count() as f32;
        assert_eq!(clipped("one two three", 2, 10., 4., measure), None);
        let lines = clipped("one two three four five six", 2, 10., 4., measure);
        assert_eq!(lines, Some(vec!["one two".to_owned(), "three\u{2026}".to_owned()]));
        let lines = clipped("one two three, four five six", 2, 10., 3., measure);
        assert_eq!(lines, Some(vec!["one two".to_owned(), "three\u{2026}".to_owned()]));
    }

    #[test]
    fn what_is_shown_moves_with_typing() {
        let mut passages = vec![passage()];
        let mut notes = vec![Advice {
            id: 1,
            title: String::new(),
            text: String::new(),
            suggestion: None,
            anchor: Some(30..40),
        }];
        // Three letters typed before all of it.
        let splice = Splice {
            start: 2,
            old_end: 2,
            new_end: 5,
        };
        shift(&mut passages, &mut notes, &splice);
        assert_eq!(passages[0].added, [13..23]);
        assert_eq!(passages[0].removed[0].0, 13);
        assert_eq!(notes[0].anchor, Some(33..43));
        // Text typed inside the passage grows it, and text after it leaves it alone.
        shift(
            &mut passages,
            &mut notes,
            &Splice {
                start: 15,
                old_end: 15,
                new_end: 16,
            },
        );
        assert_eq!(passages[0].added, [13..24]);
        shift(
            &mut passages,
            &mut notes,
            &Splice {
                start: 26,
                old_end: 26,
                new_end: 27,
            },
        );
        assert_eq!(passages[0].added, [13..24]);
    }
}
