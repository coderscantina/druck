//! The proof slip: a card at a paragraph on the pages, with its written text in the theme's font, the directives that
//! exist, and at its foot a link to the paragraph's style. It holds a copy of the text only while it is on screen. The
//! chapter is the truth: [`SlipEvent::Edited`] asks [`super::slip_control`] to carry each change over.

use std::ops::Range;

use druck::output::BlockKind;
use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, EventEmitter, Focusable, Font, Role, ScrollHandle, SharedString, Stateful, Subscription,
    Window, div, font, point, px,
};

use super::directives::{self, State};
use super::job::Face;
use super::proof::{BlockAt, Parent};
use super::strings::Key;
use crate::a11y::Control as _;
use crate::editor::rich::styles;
use crate::inspector::input::{Cancelled, Committed, Edited, TextInput};
use crate::session::{ChapterId, Session};
use crate::source::buffer::Splice;
use crate::theme_file::blocks::styles_of;
use crate::tokens::{self, Tokens};

/// A directive the slip can add or take away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Directive {
    PageBreak,
    Keep,
}

pub enum SlipEvent {
    /// The text in the slip changed.
    Edited,
    Toggle(Directive),
    /// The name of the style at the slip's foot was pressed.
    StylesWanted,
    /// A style for the paragraph, or the theme's own.
    PickStyle(Option<String>),
    /// An alignment for the paragraph, as a named style.
    PickAlignment(&'static str),
    ShowStyle(String),
    Closed,
}

/// What the slip's list of styles offers a paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleChoice {
    /// The style it names now, if any.
    pub current: Option<String>,
    /// The theme's own style for its kind of block.
    pub base: &'static str,
    /// The custom styles its kind may use.
    pub styles: Vec<String>,
}

impl StyleChoice {
    /// The custom styles to list, without the ones that only align, which have a button of their own.
    fn listed(&self) -> impl Iterator<Item = &String> {
        self.styles.iter().filter(|name| styles::unaligned(name).is_none())
    }

    /// The alignment the paragraph's style gives it, when it is one of those.
    fn alignment(&self) -> Option<&str> {
        let current = self.current.as_deref()?;
        let base = styles::unaligned(current)?;
        current[base.len()..].strip_prefix('-')
    }
}

impl EventEmitter<SlipEvent> for ProofSlip {}

/// What a slip is open on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub chapter: ChapterId,
    /// The paragraph in the chapter's text.
    pub range: Range<usize>,
    /// The text the chapter held at `range` when the slip last matched it. The field can run ahead while it composes.
    pub synced: String,
    /// The chapter's revision then.
    pub revision: u64,
    /// Where the caret starts in it.
    pub caret: usize,
    /// The style's key, its own custom style if it has one.
    pub style: Option<String>,
    /// Whether the block may take directives: lists and quotations cannot hold a directive line, and a group of blocks
    /// kept together cannot hold a page break.
    pub takes_directives: bool,
}

/// What a click on a block opens, from the chapter as it stands.
pub fn opening(session: &Session, clicked: &BlockAt) -> Option<Opening> {
    let (chapter, start) = session.locate(&clicked.file, clicked.range.start)?;
    let text = session.chapter(chapter)?.text();
    let revision = session.buffer(chapter).revision();
    let end = text.floor_char_boundary((start + clicked.range.len()).min(text.len()));
    let written = text[start..end].trim_end();
    let range = start..start + written.len();
    let parents: Vec<BlockKind> = clicked.parents.iter().map(|parent| parent.kind).collect();
    let directive_name = |parent: &Parent| {
        let (_, at) = session.locate(&clicked.file, parent.range.start)?;
        let line = text[at..].lines().next()?.trim_start().strip_prefix(":::")?;
        line.trim_start_matches(':')
            .split_whitespace()
            .next()
            .map(str::to_owned)
    };
    Some(Opening {
        chapter,
        caret: clicked.offset.saturating_sub(clicked.range.start).min(written.len()),
        style: styles_of(clicked.kind, &parents, written).pop(),
        takes_directives: takes_directives(
            clicked.kind,
            &clicked.parents,
            directives::read(text, &range).keep,
            &directive_name,
        ),
        range,
        synced: written.to_owned(),
        revision,
    })
}

/// A directive line must stand outside lists, quotations, and notes. A keep or a bottom group holds no page break, except
/// a keep around this block alone, since the page break then goes before the keep.
fn takes_directives(
    kind: BlockKind,
    parents: &[Parent],
    own_keep: bool,
    directive_name: &dyn Fn(&Parent) -> Option<String>,
) -> bool {
    let block_fits = matches!(
        kind,
        BlockKind::Paragraph
            | BlockKind::Heading(_)
            | BlockKind::Code
            | BlockKind::Image
            | BlockKind::Quote
            | BlockKind::List
            | BlockKind::Table
    );
    let last = parents.len().wrapping_sub(1);
    block_fits
        && parents.iter().enumerate().all(|(at, parent)| {
            parent.kind == BlockKind::Directive
                && match directive_name(parent).as_deref() {
                    Some("columns" | "full-width") => true,
                    Some("keep") => own_keep && at == last,
                    _ => false,
                }
        })
}

/// Where a paragraph stands after an edit of its style: a list's `{.name}` line stands before it and moves it, and a
/// closing `{.name}` is part of it.
pub fn range_after_style(range: &Range<usize>, edit: &crate::source::Edit) -> Range<usize> {
    let change = edit.text.len() as isize - edit.range.len() as isize;
    if edit.range.start <= range.start {
        range.start.saturating_add_signed(change)..range.end.saturating_add_signed(change)
    } else {
        range.start..range.end.saturating_add_signed(change)
    }
}

/// Where a slip's text stands in a chapter whose text changed: `range` moved by the change, if `splice` says what that was
/// and the text is there, else the nearest place that holds it. A search alone could take the wrong one of two paragraphs
/// that read the same. `None` when it is gone.
pub fn relocated(text: &str, held: &str, range: &Range<usize>, splice: Option<Splice>) -> Option<Range<usize>> {
    let expected = match splice {
        Some(splice) if range.start >= splice.old_end => {
            let moved = |offset: usize| offset + splice.new_end - splice.old_end;
            moved(range.start)..moved(range.end)
        }
        _ => range.clone(),
    };
    if text.get(expected.clone()) == Some(held) {
        return Some(expected);
    }
    let nearest = text
        .match_indices(held)
        .map(|(at, _)| at)
        .min_by_key(|at| at.abs_diff(expected.start))?;
    Some(nearest..nearest + held.len())
}

/// The slip's type: the family, weight, and slant of the paragraph's style, at the slip's size.
pub fn type_of(face: &Face) -> (Font, gpui::Pixels) {
    let mut chosen = font(face.family.clone());
    chosen.weight = gpui::FontWeight(f32::from(face.weight));
    if face.italic {
        chosen.style = gpui::FontStyle::Italic;
    }
    (chosen, tokens::SLIP_TEXT)
}

pub struct ProofSlip {
    input: Entity<TextInput>,
    opened: Option<Opening>,
    /// What the block has now. `None` when it cannot take directives.
    state: Option<State>,
    /// The list of styles, while it is open.
    choice: Option<StyleChoice>,
    /// A style the question about a common theme holds back, to give the paragraph once it is there.
    waiting_style: Option<String>,
    /// Why a style could not be made.
    style_problem: Option<String>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ProofSlip {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::multiline(window, cx)
                .in_prose()
                .bare()
                .keep_focus_on_enter()
                .with_label(Key::SlipText)
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |slip, input, _: &Edited, window, cx| {
                if !input.read(cx).is_composing() && slip.opened.is_some() {
                    cx.emit(SlipEvent::Edited);
                }
                // The caret's place is known once the field has painted the text.
                cx.on_next_frame(window, |slip, _, cx| {
                    slip.follow_caret(cx);
                    cx.notify();
                });
            }),
            cx.subscribe(&input, |slip, _, Committed(_), cx| {
                if slip.opened.is_some() {
                    cx.emit(SlipEvent::Edited);
                }
            }),
            cx.subscribe(&input, |slip, _, _: &Cancelled, cx| {
                if slip.choice.is_some() {
                    slip.set_choice(None, cx);
                } else {
                    cx.emit(SlipEvent::Closed);
                }
            }),
        ];
        Self {
            input,
            opened: None,
            state: None,
            choice: None,
            waiting_style: None,
            style_problem: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Opens on a paragraph.
    pub fn open(&mut self, opening: Opening, state: State, face: &Face, window: &mut Window, cx: &mut Context<Self>) {
        self.state = opening.takes_directives.then_some(state);
        self.choice = None;
        self.waiting_style = None;
        self.style_problem = None;
        let (caret, text) = (opening.caret, opening.synced.clone());
        self.opened = Some(opening);
        self.input.update(cx, |input, cx| {
            input.set_book_type(Some(type_of(face)), cx);
            input.set_value(text, true, cx);
            input.set_caret(caret, cx);
        });
        self.scroll.set_offset(point(px(0.), px(0.)));
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.opened.take().is_some() {
            self.choice = None;
            cx.notify();
        }
    }

    /// Whether the list of styles is open.
    pub fn choice(&self) -> Option<&StyleChoice> {
        self.choice.as_ref()
    }

    /// Opens the list of styles with this choice, or closes it.
    pub fn set_choice(&mut self, choice: Option<StyleChoice>, cx: &mut Context<Self>) {
        self.choice = choice;
        self.style_problem = None;
        cx.notify();
    }

    /// Says why a style could not be made, or that it waits for the question about a common theme.
    pub fn set_style_problem(&mut self, problem: Option<String>, cx: &mut Context<Self>) {
        self.style_problem = problem;
        cx.notify();
    }

    /// The style waiting for the theme, which leaves the slip with this call.
    pub fn take_waiting_style(&mut self) -> Option<String> {
        self.waiting_style.take()
    }

    pub fn set_waiting_style(&mut self, name: String) {
        self.waiting_style = Some(name);
    }

    /// The paragraph has a new style: `written` is its text now, at `range`, which the field takes over.
    pub fn styled(
        &mut self,
        style: String,
        range: Range<usize>,
        written: String,
        revision: u64,
        state: State,
        cx: &mut Context<Self>,
    ) {
        self.input
            .update(cx, |input, cx| input.set_value(written.clone(), true, cx));
        if let Some(opened) = &mut self.opened {
            opened.style = Some(style);
        }
        self.choice = None;
        self.synced(range, written, revision, state, cx);
    }

    pub fn opened(&self) -> Option<&Opening> {
        self.opened.as_ref()
    }

    /// The text as it stands in the slip.
    pub fn text<'a>(&self, cx: &'a gpui::App) -> &'a str {
        self.input.read(cx).text()
    }

    /// The slip matches the chapter again: its paragraph holds `synced` at `range`, at `revision`.
    pub fn synced(&mut self, range: Range<usize>, synced: String, revision: u64, state: State, cx: &mut Context<Self>) {
        if let Some(opened) = &mut self.opened {
            opened.range = range;
            opened.synced = synced;
            opened.revision = revision;
            if opened.takes_directives {
                self.state = Some(state);
            }
            cx.notify();
        }
    }

    /// The field, for a scripted edit that goes the same way as keys.
    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    /// Keeps the caret in the part of the text the slip shows.
    fn follow_caret(&self, cx: &Context<Self>) {
        let Some((top, height)) = self.input.read(cx).caret_row() else {
            return;
        };
        let view = self.scroll.bounds().size.height;
        let offset = self.scroll.offset();
        if view <= px(0.) {
            return;
        }
        // The scroll offset is negative: the text moves up.
        if top + offset.y < px(0.) {
            self.scroll.set_offset(point(offset.x, -top));
        } else if top + height + offset.y > view {
            self.scroll.set_offset(point(offset.x, view - top - height));
        }
    }

    fn directive_button(
        &self,
        directive: Directive,
        label: Key,
        on: bool,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let label = crate::t!(label);
        div()
            .id(label)
            .toggle(tokens, label, on)
            .flex_none()
            .px(tokens::SPACE_2)
            .h(tokens::ZOOM_BUTTON + tokens::SPACE_1)
            .flex()
            .items_center()
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .cursor_pointer()
            .when(on, |button| button.bg(tokens.raised).text_color(tokens.text))
            .when(!on, |button| {
                button
                    .text_color(tokens.text_quiet)
                    .hover(|button| button.bg(tokens.raised))
            })
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(SlipEvent::Toggle(directive))))
            .child(label)
    }
}

impl ProofSlip {
    /// The short list at the slip's foot: the theme's own style, the custom ones, the alignments as named styles, and the
    /// way to the style's section.
    fn style_list(&self, choice: &StyleChoice, style: Option<&str>, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let row = |id: SharedString, label: String, chosen: bool| {
            div()
                .id(id)
                .button(tokens, label.clone())
                .flex_none()
                .h(tokens::MENU_ROW_HEIGHT)
                .px(tokens::SPACE_2)
                .flex()
                .items_center()
                .rounded(tokens::RADIUS)
                .cursor_pointer()
                .text_size(tokens::TEXT_SMALL)
                .text_color(if chosen { tokens.text } else { tokens.text_quiet })
                .when(chosen, |row| row.bg(tokens.raised))
                .hover(|row| row.bg(tokens.raised))
                .child(div().min_w_0().truncate().child(label))
        };
        let own = row(
            "slip-style-own".into(),
            crate::inspector::style_title(choice.base),
            choice.current.is_none(),
        )
        .on_click(cx.listener(|_, _, _, cx| cx.emit(SlipEvent::PickStyle(None))));
        let custom: Vec<_> = choice
            .listed()
            .map(|name| {
                let picked = name.clone();
                row(
                    format!("slip-style-{name}").into(),
                    format!(".{name}"),
                    choice.current.as_ref() == Some(name),
                )
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(SlipEvent::PickStyle(Some(picked.clone())))))
            })
            .collect();
        let aligned = choice.alignment().map(str::to_owned);
        let alignments = styles::ALIGNMENTS.map(|align| {
            let key = match align {
                "left" => Key::SlipAlignLeft,
                "center" => Key::SlipAlignCenter,
                "right" => Key::SlipAlignRight,
                _ => Key::SlipAlignJustify,
            };
            let label = crate::t!(key);
            let on = aligned.as_deref() == Some(align);
            div()
                .id(label)
                .toggle(tokens, label, on)
                .flex_1()
                .h(tokens::ZOOM_BUTTON + tokens::SPACE_1)
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::RADIUS)
                .border_1()
                .border_color(tokens.divider)
                .text_size(tokens::TEXT_SMALL)
                .cursor_pointer()
                .text_color(if on { tokens.text } else { tokens.text_quiet })
                .when(on, |button| button.bg(tokens.raised))
                .hover(|button| button.bg(tokens.raised))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(SlipEvent::PickAlignment(align))))
                .child(label)
        });
        let section = style.map(str::to_owned).map(|style| {
            row(
                "slip-style-section".into(),
                crate::t!(Key::SlipShowStyle).to_owned(),
                false,
            )
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(SlipEvent::ShowStyle(style.clone()))))
        });
        div()
            .flex()
            .flex_col()
            .gap(tokens::SPACE_1)
            .child(
                div()
                    .id("slip-styles")
                    .role(Role::List)
                    .aria_label(crate::t!(Key::SlipStyles))
                    .max_h(tokens::SLIP_STYLES_MAX_HEIGHT)
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(own)
                    .children(custom),
            )
            .child(div().flex().gap(tokens::SPACE_1).children(alignments))
            .children(section)
    }
}

impl Render for ProofSlip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        self.follow_caret(cx);
        let style = self.opened.as_ref().and_then(|opened| opened.style.clone());
        let list = self
            .choice
            .clone()
            .map(|choice| self.style_list(&choice, style.as_deref(), &tokens, cx));
        let buttons = self.state.filter(|_| list.is_none()).map(|state| {
            div()
                .flex()
                .gap(tokens::SPACE_2)
                .child(self.directive_button(Directive::PageBreak, Key::SlipPageBreak, state.page_break, &tokens, cx))
                .child(self.directive_button(Directive::Keep, Key::SlipKeep, state.keep, &tokens, cx))
        });
        let foot = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(tokens::SPACE_2)
            .pt(tokens::SPACE_2)
            .border_t_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
            .child(match style {
                Some(style) => {
                    let label = crate::inspector::style_title(&style);
                    div()
                        .id("slip-style")
                        .control(&tokens, Role::Link, SharedString::from(label.clone()))
                        .min_w_0()
                        .truncate()
                        .text_color(tokens.text_quiet)
                        .cursor_pointer()
                        .hover(|link| link.text_color(tokens.text))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(SlipEvent::StylesWanted)))
                        .child(label)
                        .into_any_element()
                }
                None => div().into_any_element(),
            })
            .child(div().flex_none().child(crate::t!(Key::SlipEscape)));
        div()
            .id("proof-slip")
            .role(Role::Dialog)
            .aria_label(crate::t!(Key::SlipLabel))
            .occlude()
            .w_full()
            .p(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .bg(tokens.panel)
            .shadow_lg()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("slip-text")
                    .max_h(if list.is_some() {
                        tokens::SLIP_TEXT_SHORT_HEIGHT
                    } else {
                        tokens::SLIP_TEXT_MAX_HEIGHT
                    })
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(self.input.clone()),
            )
            .children(buttons)
            .children(list)
            .when_some(self.style_problem.clone(), |slip, problem| {
                slip.child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.accent)
                        .child(problem),
                )
            })
            .child(foot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slip_opens_on_the_written_text_of_its_paragraph() {
        let dir = std::env::temp_dir().join(format!("druck-slip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("folder");
        let file = dir.join("doc.md");
        let text = "# One\n\nThe *first* paragraph. {.lead}\n\nThe next.\n";
        std::fs::write(&file, text).expect("file");
        let session = Session::open(&file).expect("opens");
        let start = text.find("The *first*").expect("in the text");
        let end = text.find("The next").expect("in the text");
        let clicked = BlockAt {
            file: file.clone(),
            offset: start + 4,
            // The block runs up to the next one.
            range: start..end,
            kind: BlockKind::Paragraph,
            parents: Vec::new(),
        };
        let opening = opening(&session, &clicked).expect("the chapter holds it");
        let chapter = session.chapter(opening.chapter).expect("a chapter");
        assert_eq!(&chapter.text()[opening.range.clone()], "The *first* paragraph. {.lead}");
        assert_eq!(opening.caret, 4);
        assert_eq!(opening.style.as_deref(), Some("lead"));
        assert!(opening.takes_directives);
        std::fs::remove_dir_all(dir).ok();
    }

    /// What the chapter holds after the slip writes `typed` to its paragraph, as a write looks for it first.
    fn slip_write(buffer: &mut crate::source::Buffer, synced: &str, range: &Range<usize>, revision: u64, typed: &str) {
        let splice = (buffer.revision() == revision + 1)
            .then(|| buffer.last_splice())
            .flatten();
        let found = relocated(buffer.text(), synced, range, splice).expect("the paragraph is there");
        let edit = directives::edit_between(found.start, synced, typed).expect("the text changed");
        let caret = crate::source::Selection::caret(0);
        buffer
            .transact(
                crate::source::EditKind::Typing,
                vec![edit],
                caret,
                caret,
                std::time::Instant::now(),
            )
            .expect("a valid edit");
    }

    #[test]
    fn a_composition_that_ends_after_an_outside_write_keeps_what_was_written() {
        let text = "First.\n\nThe slip's paragraph.\n\nLast.\n";
        let mut buffer = crate::source::Buffer::new(text);
        let range = 8..29;
        let revision = buffer.revision();
        // While the field composes, a line lands in front of the paragraph. The stale range now holds other words.
        let caret = crate::source::Selection::caret(0);
        buffer
            .transact(
                crate::source::EditKind::External,
                vec![crate::source::Edit::new(0..0, "Written from outside.\n\n")],
                caret,
                caret,
                std::time::Instant::now(),
            )
            .expect("a valid edit");
        slip_write(
            &mut buffer,
            &text[range.clone()],
            &range,
            revision,
            "The slip's own paragraph.",
        );
        assert_eq!(
            buffer.text(),
            "Written from outside.\n\nFirst.\n\nThe slip's own paragraph.\n\nLast.\n"
        );
    }

    #[test]
    fn a_paragraph_that_went_while_composing_is_not_written_back() {
        let held = "The slip's paragraph.";
        assert_eq!(relocated("First.\n\nSomething else.\n", held, &(8..29), None), None);
    }

    #[test]
    fn a_style_grows_a_paragraph_and_moves_a_list() {
        let closing = crate::source::Edit::new(10..10, " {.lead}");
        assert_eq!(range_after_style(&(4..10), &closing), 4..18);
        let removed = crate::source::Edit::new(10..18, "");
        assert_eq!(range_after_style(&(4..18), &removed), 4..10);
        let line_before = crate::source::Edit::new(4..4, "{.checks}\n");
        assert_eq!(range_after_style(&(4..10), &line_before), 14..20);
    }

    #[test]
    fn the_list_of_styles_keeps_alignments_to_their_buttons() {
        let choice = StyleChoice {
            current: Some("body-center".to_owned()),
            base: "body",
            styles: vec!["body-center".to_owned(), "lead".to_owned()],
        };
        assert_eq!(choice.listed().collect::<Vec<_>>(), ["lead"]);
        assert_eq!(choice.alignment(), Some("center"));
    }

    #[test]
    fn a_slip_keeps_its_paragraph_through_outside_changes_or_lets_it_go() {
        let held = "A paragraph.";
        let first = "One.\n\nA paragraph.\n";
        let range = 6..18;
        assert_eq!(relocated(first, held, &range, None), Some(6..18));
        let moved = "Something new.\n\nOne.\n\nA paragraph.\n";
        assert_eq!(relocated(moved, held, &range, None), Some(22..34));
        assert_eq!(relocated("One.\n\nChanged.\n", held, &range, None), None);
        let twice = "A paragraph.\n\nOne.\n\nx\n\nA paragraph.\n";
        assert_eq!(
            relocated(twice, held, &(20..32), None),
            Some(23..35),
            "the nearer of two"
        );
    }

    #[test]
    fn a_change_moves_the_slip_by_its_size_and_not_to_a_paragraph_that_reads_the_same() {
        let held = "Yes.";
        // The second of two, which is on 6..10. Six bytes in front of it put the first at the same offset.
        let text = "Added.Yes.\n\nYes.\n";
        let before = Splice {
            start: 0,
            old_end: 0,
            new_end: 6,
        };
        assert_eq!(relocated(text, held, &(6..10), Some(before)), Some(12..16));
        // A change after it leaves it alone, even when the same words stand as far along as the change is long.
        let after = Splice {
            start: 4,
            old_end: 4,
            new_end: 10,
        };
        let text = "Yes.\n\nYes.Added.\n";
        assert_eq!(relocated(text, held, &(0..4), Some(after)), Some(0..4));
        // A change over the paragraph falls back on the search.
        let over = Splice {
            start: 0,
            old_end: 8,
            new_end: 1,
        };
        assert_eq!(relocated("x\n\nYes.\n", held, &(2..6), Some(over)), Some(3..7));
    }

    fn parent(kind: BlockKind) -> Parent {
        Parent { kind, range: 0..1 }
    }

    fn named(name: &'static str) -> impl Fn(&Parent) -> Option<String> {
        move |_| Some(name.to_owned())
    }

    #[test]
    fn a_block_outside_lists_and_quotations_takes_directives() {
        let none = |_: &Parent| None;
        assert!(takes_directives(BlockKind::Paragraph, &[], false, &none));
        assert!(takes_directives(BlockKind::Heading(2), &[], false, &none));
        assert!(!takes_directives(BlockKind::Directive, &[], false, &none));
        assert!(!takes_directives(BlockKind::SceneBreak, &[], false, &none));
        let quoted = [parent(BlockKind::Quote)];
        assert!(!takes_directives(BlockKind::Paragraph, &quoted, false, &none));
        let listed = [parent(BlockKind::List), parent(BlockKind::ListItem)];
        assert!(!takes_directives(BlockKind::Paragraph, &listed, false, &none));
    }

    #[test]
    fn a_keep_of_several_blocks_holds_no_directive_but_a_keep_of_this_one_does() {
        let inside = [parent(BlockKind::Directive)];
        assert!(!takes_directives(BlockKind::Paragraph, &inside, false, &named("keep")));
        assert!(takes_directives(BlockKind::Paragraph, &inside, true, &named("keep")));
        assert!(!takes_directives(
            BlockKind::Paragraph,
            &inside,
            false,
            &named("bottom")
        ));
        assert!(takes_directives(
            BlockKind::Paragraph,
            &inside,
            false,
            &named("columns")
        ));
    }
}
