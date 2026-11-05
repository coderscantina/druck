//! The proof slip: a card at a paragraph on the pages, with its written text in the theme's font, the directives that
//! exist, and at its foot a link to the paragraph's style. It holds a copy of the text only while it is on screen. The
//! chapter is the truth: [`SlipEvent::Edited`] asks the workspace to carry each change over.

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
use crate::inspector::input::{Cancelled, Committed, Edited, TextInput};
use crate::session::{ChapterId, Session};
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
    ShowStyle(String),
    Closed,
}

impl EventEmitter<SlipEvent> for ProofSlip {}

/// What a slip is open on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub chapter: ChapterId,
    /// The paragraph in the chapter's text.
    pub range: Range<usize>,
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

/// Where a slip's text stands in a chapter whose text changed: at `range` if it is still there, else at the nearest place
/// that holds it. `None` when it is gone.
pub fn relocated(text: &str, held: &str, range: &Range<usize>) -> Option<Range<usize>> {
    if text.get(range.clone()) == Some(held) {
        return Some(range.clone());
    }
    let nearest = text
        .match_indices(held)
        .map(|(at, _)| at)
        .min_by_key(|at| at.abs_diff(range.start))?;
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
                .with_label(crate::t!(Key::SlipText))
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
            cx.subscribe(&input, |_, _, _: &Cancelled, cx| cx.emit(SlipEvent::Closed)),
        ];
        Self {
            input,
            opened: None,
            state: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Opens on a paragraph, whose written text is `text`.
    pub fn open(
        &mut self,
        opening: Opening,
        text: &str,
        state: State,
        face: &Face,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state = opening.takes_directives.then_some(state);
        let caret = opening.caret;
        self.opened = Some(opening);
        self.input.update(cx, |input, cx| {
            input.set_book_type(Some(type_of(face)), cx);
            input.set_value(text.to_owned(), true, cx);
            input.set_caret(caret, cx);
        });
        self.scroll.set_offset(point(px(0.), px(0.)));
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.opened.take().is_some() {
            cx.notify();
        }
    }

    pub fn opened(&self) -> Option<&Opening> {
        self.opened.as_ref()
    }

    /// The text as it stands in the slip.
    pub fn text<'a>(&self, cx: &'a gpui::App) -> &'a str {
        self.input.read(cx).text()
    }

    /// The paragraph moved in its chapter, by the slip's own change or another's.
    pub fn moved_to(&mut self, range: Range<usize>, state: State, cx: &mut Context<Self>) {
        if let Some(opened) = &mut self.opened {
            opened.range = range;
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

impl Render for ProofSlip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        self.follow_caret(cx);
        let style = self.opened.as_ref().and_then(|opened| opened.style.clone());
        let buttons = self.state.map(|state| {
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
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(SlipEvent::ShowStyle(style.clone()))))
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
            .w(tokens::SLIP_WIDTH)
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
                    .max_h(tokens::SLIP_TEXT_MAX_HEIGHT)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(self.input.clone()),
            )
            .children(buttons)
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

    #[test]
    fn a_slip_keeps_its_paragraph_through_outside_changes_or_lets_it_go() {
        let held = "A paragraph.";
        let first = "One.\n\nA paragraph.\n";
        let range = 6..18;
        assert_eq!(relocated(first, held, &range), Some(6..18));
        let moved = "Something new.\n\nOne.\n\nA paragraph.\n";
        assert_eq!(relocated(moved, held, &range), Some(22..34));
        assert_eq!(relocated("One.\n\nChanged.\n", held, &range), None);
        let twice = "A paragraph.\n\nOne.\n\nx\n\nA paragraph.\n";
        assert_eq!(relocated(twice, held, &(20..32)), Some(23..35), "the nearer of two");
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
