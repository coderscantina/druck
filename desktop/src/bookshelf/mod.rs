//! The bookshelf: the book as flat spines along the window's bottom edge, behind the Writing setting "Bookshelf".
//!
//! At rest only a trace of it stands there, one hairline per chapter as wide as its spine. It rises when the pointer
//! reaches the edge, or on `Cmd+Shift+\`, which also gives it the keys: Left and Right walk the spines, Return opens one,
//! Alt with an arrow moves it, and Escape puts it away and gives the keys back to the text. A click opens a chapter and
//! a drag moves it, dropping at a 2 px accent line.
//!
//! It holds no document. The workspace gives it the contents, and the shell gives it each chapter's figures. It hears
//! [`ShelfEvent`]s. [`layout`] has the rules for the groups and widths.

mod layout;
mod strings;

use std::collections::HashMap;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, KeyDownEvent, Role, ScrollHandle,
    SharedString, Task, Window, div, linear_color_stop, linear_gradient, px,
};

use self::layout::{Group, Spine};
use self::strings::Key;
use crate::navigator::book::Book;
use crate::session::{ChapterId, Contents};
use crate::sidebar::Mark;
use crate::tokens::{self, Tokens};

/// How long the pointer rests on the edge before the shelf rises, so passing it raises nothing, and how long it stays
/// after the pointer leaves.
const SETTLE: Duration = Duration::from_millis(140);

pub enum ShelfEvent {
    /// Open a chapter.
    Select(ChapterId),
    /// Move a chapter so it stands before the one now at this place in the book.
    Move(ChapterId, usize),
    /// The shelf had the keys and put itself away, so the text takes them back.
    Closed,
}

impl EventEmitter<ShelfEvent> for BookshelfView {}

/// What the session and the last layout know of a chapter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Figures {
    pub words: Option<usize>,
    /// Open placeholders.
    pub todos: usize,
    /// The pages that open with this chapter's text, when the last layout knows them.
    pub pages: Option<usize>,
}

/// The spine that is lit, and where it stands in the window as the row is laid out.
struct Lit {
    spine: Spine,
    width: f32,
    left: f32,
}

/// A spine being dragged.
#[derive(Clone)]
struct DraggedSpine {
    id: ChapterId,
    title: SharedString,
}

impl Render for DraggedSpine {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        div()
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_1)
            .rounded(tokens::RADIUS)
            .bg(tokens.raised)
            .border_1()
            .border_color(tokens.divider)
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .text_color(tokens.text)
            .child(self.title.clone())
    }
}

pub struct BookshelfView {
    contents: Contents,
    /// The book's matter and parts, as the path bar reads them.
    structure: Book,
    active: Option<ChapterId>,
    figures: HashMap<ChapterId, Figures>,
    marks: HashMap<ChapterId, Mark>,
    groups: Vec<Group>,
    raised: bool,
    /// The keys are in the shelf.
    keyed: bool,
    pointer_in: bool,
    hovered: Option<ChapterId>,
    /// The spine the arrow keys stand on.
    cursor: Option<ChapterId>,
    focus: FocusHandle,
    settle: Task<()>,
    /// Where the row is scrolled, when there are more spines than the window holds.
    scroll: ScrollHandle,
    _subscriptions: Vec<gpui::Subscription>,
    /// Stays raised whatever the pointer does, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pinned: bool,
}

impl Focusable for BookshelfView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BookshelfView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus_out(&focus, window, |shelf, _, _, cx| {
                shelf.keyed = false;
                if !shelf.pointer_in {
                    shelf.raised = false;
                }
                cx.notify();
            }),
            // Switched off while it is up: it stays down when switched on again.
            cx.observe_global::<crate::settings::Store>(|shelf, cx| {
                if !crate::settings::get(cx).bookshelf {
                    shelf.put_away();
                    cx.notify();
                }
            }),
        ];
        Self {
            contents: Contents::default(),
            structure: Book::default(),
            active: None,
            figures: HashMap::new(),
            marks: HashMap::new(),
            groups: Vec::new(),
            raised: false,
            keyed: false,
            pointer_in: false,
            hovered: None,
            cursor: None,
            focus,
            settle: Task::ready(()),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
            #[cfg(any(debug_assertions, feature = "measure"))]
            pinned: false,
        }
    }

    pub fn set_contents(
        &mut self,
        contents: Contents,
        structure: Book,
        active: Option<ChapterId>,
        cx: &mut Context<Self>,
    ) {
        if contents != self.contents || structure != self.structure || active != self.active {
            self.contents = contents;
            self.structure = structure;
            self.active = active;
            self.regroup();
            cx.notify();
        }
    }

    pub fn set_figures(&mut self, figures: HashMap<ChapterId, Figures>, cx: &mut Context<Self>) {
        if figures != self.figures {
            self.figures = figures;
            self.regroup();
            cx.notify();
        }
    }

    /// The dots of the chapters, as the Binder has them.
    pub fn set_marks(&mut self, marks: HashMap<ChapterId, Mark>, cx: &mut Context<Self>) {
        if marks != self.marks {
            self.marks = marks;
            cx.notify();
        }
    }

    fn regroup(&mut self) {
        let words = (self.figures.iter())
            .filter_map(|(id, figures)| Some((*id, figures.words?)))
            .collect();
        self.groups = layout::groups(&self.structure, &self.contents.not_in_book, &words);
    }

    /// Raises it for the moment, with the keys, or puts it away. For `Cmd+Shift+\`.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.raised {
            self.hide(cx);
            return;
        }
        self.raised = true;
        self.keyed = true;
        self.cursor = self.active.or_else(|| self.ids().first().copied());
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Puts it away at once, as when the window goes to another mode. The keys are not asked for.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.put_away();
        cx.notify();
    }

    /// Puts it away, and gives the keys back to the text if it had them.
    fn hide(&mut self, cx: &mut Context<Self>) {
        let had_keys = self.keyed;
        self.put_away();
        if had_keys {
            cx.emit(ShelfEvent::Closed);
        }
        cx.notify();
    }

    fn put_away(&mut self) {
        self.raised = false;
        self.keyed = false;
        self.hovered = None;
        self.cursor = None;
        #[cfg(any(debug_assertions, feature = "measure"))]
        {
            self.pinned = false;
        }
    }

    /// Raises it and keeps it up, with the spine at this place in the book lit as a hover would. For screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn pin_for_gate(&mut self, hover: Option<usize>, cx: &mut Context<Self>) {
        self.raised = true;
        self.pinned = true;
        self.hovered = hover.and_then(|at| self.contents.book.get(at)).map(|row| row.id);
        cx.notify();
    }

    /// The pointer came to the shelf's strip, or left what is raised. Decides after a short pause, so a pointer passing
    /// the edge raises nothing and one dragging a spine out of the strip does not put it away.
    fn hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.pointer_in = hovered;
        self.settle = cx.spawn(async move |shelf, cx| {
            loop {
                cx.background_executor().timer(SETTLE).await;
                let done = shelf.update(cx, |shelf, cx| shelf.settled(cx)).unwrap_or(true);
                if done {
                    break;
                }
            }
        });
    }

    /// Raises or puts away after the pause. False while a drag is going, which asks again.
    fn settled(&mut self, cx: &mut Context<Self>) -> bool {
        if self.pointer_in {
            if !self.raised {
                self.raised = true;
                cx.notify();
            }
            return true;
        }
        if cx.has_active_drag() {
            return false;
        }
        #[cfg(any(debug_assertions, feature = "measure"))]
        if self.pinned {
            return true;
        }
        if self.raised && !self.keyed {
            self.raised = false;
            self.hovered = None;
            cx.notify();
        }
        true
    }

    /// Every spine in the order the keys walk them.
    fn ids(&self) -> Vec<ChapterId> {
        (self.groups.iter())
            .flat_map(|group| group.spines.iter().map(|spine| spine.id))
            .collect()
    }

    fn book_ids(&self) -> Vec<ChapterId> {
        self.contents.book.iter().map(|row| row.id).collect()
    }

    /// The place in the book that moves `id` one spine left or right, if it can go.
    fn moved(&self, id: ChapterId, left: bool) -> Option<usize> {
        let book = self.book_ids();
        let at = book.iter().position(|other| *other == id)?;
        let place = if left { at.checked_sub(1)? } else { at + 2 };
        (place <= book.len()).then_some(place)
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let alt = event.keystroke.modifiers.alt;
        match event.keystroke.key.as_str() {
            key @ ("left" | "right") if alt => {
                if let Some(id) = self.cursor
                    && let Some(place) = self.moved(id, key == "left")
                {
                    cx.emit(ShelfEvent::Move(id, place));
                }
            }
            key @ ("left" | "right") => {
                let ids = self.ids();
                self.cursor = step(&ids, self.cursor, key == "right");
                cx.notify();
            }
            "enter" => {
                if let Some(id) = self.cursor {
                    self.hide(cx);
                    cx.emit(ShelfEvent::Select(id));
                }
            }
            "escape" => self.hide(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The spine that is lit: under the pointer, or under the arrow keys.
    fn lit(&self) -> Option<ChapterId> {
        self.hovered.or(self.cursor.filter(|_| self.keyed))
    }

    fn figures_of(&self, id: ChapterId) -> Figures {
        self.figures.get(&id).copied().unwrap_or_default()
    }

    /// The faint row at rest: one hairline per chapter, in the same places as the spines.
    fn trace(&self, fit: &layout::Fit, fits: bool, tokens: &Tokens) -> impl IntoElement + use<> {
        let mut widths = fit.widths.iter().copied();
        let groups: Vec<_> = (self.groups.iter().enumerate())
            .map(|(at, group)| {
                let segments: Vec<_> = (group.spines.iter())
                    .zip(widths.by_ref())
                    .map(|(spine, width)| {
                        let open = self.active == Some(spine.id);
                        let (color, opacity) = if open {
                            (tokens.text, tokens::TRACE_OPEN_OPACITY)
                        } else {
                            (tokens.text_faint, tokens::TRACE_OPACITY)
                        };
                        div()
                            .flex_none()
                            .w(px(width))
                            .h(tokens::TRACE_HEIGHT)
                            .rounded(tokens::HAIRLINE)
                            .bg(Hsla::from(color).opacity(if group.loose { opacity * 0.6 } else { opacity }))
                    })
                    .collect();
                div()
                    .flex_none()
                    .flex()
                    .gap(px(tokens::SPINE_GAP))
                    .when(at > 0, |group| group.ml(px(tokens::SHELF_GROUP_GAP)))
                    .children(segments)
            })
            .collect();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(tokens::TRACE_BOTTOM)
            .h(tokens::TRACE_HEIGHT)
            .flex()
            .when(fits, |row| row.justify_center())
            .when(!fits, |row| row.pl(tokens::SPACE_3))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .children(groups.into_iter().enumerate().flat_map(|(at, group)| {
                        // The room a dropped spine takes at the book's end stands after the last group of the book.
                        let end = (self.groups.get(at).is_some_and(|g| !g.loose)
                            && self.groups.get(at + 1).is_none_or(|next| next.loose))
                        .then(|| div().flex_none().w(px(tokens::SHELF_END_SLOT)).into_any_element());
                        [Some(group.into_any_element()), end].into_iter().flatten()
                    })),
            )
    }

    /// One spine, with the area it stands in that holds still while the spine lifts.
    fn spine(
        &self,
        spine: &Spine,
        width: f32,
        place: Option<usize>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = spine.id;
        let open = self.active == Some(id);
        let lit = self.lit() == Some(id);
        let lift = if open {
            tokens::SPINE_LIFT_OPEN
        } else if lit {
            tokens::SPINE_LIFT_HOVER
        } else {
            px(0.)
        };
        let loose = place.is_none();
        let (number_color, title_color) = if open {
            (tokens.text, tokens.text)
        } else if lit {
            (tokens.text, tokens.text_quiet)
        } else {
            (tokens.text_quiet, tokens.text_faint)
        };
        let dot = self.marks.get(&id).map(|mark| {
            let dot = div()
                .absolute()
                .top(px(9.))
                .right(px(5.))
                .size(tokens::DOT_SIZE)
                .rounded_full();
            match mark {
                Mark::Problem => dot.bg(tokens.accent),
                Mark::Placeholder => dot.border_1().border_color(tokens.accent),
            }
        });
        // A spine too narrow for its title shows the number alone, small and centred, so two digits fit the least width.
        let titled = width >= tokens::SPINE_TITLE_MIN_WIDTH;
        let body = if spine.matter {
            let room = (f32::from(tokens::SPINE_HEIGHT) / 11.) as usize - 1;
            let count = spine.title.chars().count();
            let shown: Vec<String> = if count <= room {
                spine.title.chars().map(String::from).collect()
            } else {
                (spine.title.chars().take(room - 1).map(String::from))
                    .chain(["\u{2026}".to_owned()])
                    .collect()
            };
            div()
                .size_full()
                .pt(tokens::SPACE_2)
                .flex()
                .flex_col()
                .items_center()
                .text_size(tokens::TEXT_SMALL)
                .line_height(px(11.))
                .text_color(title_color)
                .children(shown.into_iter().map(|letter| div().child(letter)))
        } else {
            div()
                .size_full()
                .overflow_hidden()
                .pt(px(10.))
                .when(titled, |body| body.px(px(6.)))
                .child(
                    div()
                        .flex()
                        .when(!titled, |number| number.justify_center())
                        .text_size(if titled { tokens::TEXT_BODY } else { tokens::TEXT_SMALL })
                        .line_height(px(16.))
                        .text_color(number_color)
                        .child(spine.number.map(|number| number.to_string()).unwrap_or_default()),
                )
                .when(titled, |body| {
                    body.child(
                        div()
                            .mt(tokens::SPACE_1)
                            .text_size(tokens::TEXT_SMALL)
                            .line_height(px(12.))
                            .line_clamp(3)
                            .text_color(title_color)
                            .child(spine.title.clone()),
                    )
                })
        };
        let face = div()
            .relative()
            .w_full()
            .h(tokens::SPINE_HEIGHT)
            .mb(lift)
            .rounded_t(px(2.))
            .border_1()
            .border_b_0()
            .when(loose, |face| {
                face.border_dashed()
                    .border_color(Hsla::from(tokens.text_faint).opacity(0.6))
            })
            .when(!loose, |face| face.border_color(tokens.divider))
            .when(open || lit, |face| face.bg(tokens.raised))
            .when(!open && !lit && !loose, |face| face.bg(tokens.field))
            .when(open, |face| face.shadow(tokens::float_shadow()))
            .child(body)
            .children(dot);
        let accent = tokens.accent;
        let title: SharedString = spine.title.clone().into();
        div()
            .id(gpui::ElementId::from(("spine", id.number())))
            .role(Role::ListItem)
            .aria_label(title.clone())
            .aria_selected(open)
            .relative()
            .flex_none()
            .w(px(width))
            .h(tokens::SPINE_HEIGHT + tokens::SPINE_LIFT_OPEN)
            .flex()
            .items_end()
            .cursor_pointer()
            .on_hover(cx.listener(move |shelf, hovered: &bool, _, cx| {
                if *hovered {
                    shelf.hovered = Some(id);
                } else if shelf.hovered == Some(id) {
                    shelf.hovered = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(ShelfEvent::Select(id))))
            .when_some(place, |wrapper, place| {
                wrapper
                    .on_drag(DraggedSpine { id, title }, |dragged, _, _, cx| {
                        cx.new(|_| dragged.clone())
                    })
                    .drag_over::<DraggedSpine>(move |style, _, _, _| style.border_l_2().border_color(accent))
                    .on_drop(cx.listener(move |_, dragged: &DraggedSpine, _, cx| {
                        cx.emit(ShelfEvent::Move(dragged.id, place));
                    }))
            })
            .child(face)
            .into_any_element()
    }

    /// The card over a lit spine: title, words, pages, and the open placeholders in the accent, its only use on the shelf.
    fn card(&self, lit: &Lit, tokens: &Tokens, window: &Window) -> impl IntoElement + use<> {
        let Lit { spine, width, left } = lit;
        let (width, left) = (*width, *left);
        let figures = self.figures_of(spine.id);
        let words = figures.words.map(|count| {
            let text = crate::shell::footer::grouped(count);
            if count == 1 {
                crate::t!(Key::WordsOne, count = text)
            } else {
                crate::t!(Key::WordsMany, count = text)
            }
        });
        let pages = figures.pages.map(|count| {
            if count == 1 {
                crate::t!(Key::PagesOne, count = count)
            } else {
                crate::t!(Key::PagesMany, count = count)
            }
        });
        let counted = [words, pages].into_iter().flatten().collect::<Vec<_>>().join(", ");
        let open = match figures.todos {
            0 => None,
            1 => Some(crate::t!(Key::OpenOne).to_owned()),
            count => Some(crate::t!(Key::OpenMany, count = count)),
        };
        let title = match spine.number {
            Some(number) => format!("{number} {}", spine.title),
            None => spine.title.clone(),
        };
        let hint = if self.book_ids().contains(&spine.id) {
            crate::t!(Key::HintBook)
        } else {
            crate::t!(Key::HintLoose)
        };
        // Centred over the spine, kept inside the window.
        let card = f32::from(tokens::SPINE_CARD_WIDTH);
        let room = f32::from(window.viewport_size().width);
        let at = (left + width / 2. - card / 2.).clamp(f32::from(tokens::SPACE_2), (room - card - 8.).max(8.));
        div()
            .absolute()
            .left(px(at))
            .bottom(
                tokens::SPACE_2
                    + tokens::SHELF_LABEL_HEIGHT
                    + tokens::SPINE_HEIGHT
                    + tokens::SPINE_LIFT_OPEN
                    + tokens::SPACE_2,
            )
            .w(tokens::SPINE_CARD_WIDTH)
            .px(px(10.))
            .py(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .shadow(tokens::float_shadow())
            .cursor_default()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(tokens::TEXT_BODY)
                    .text_color(tokens.text)
                    .truncate()
                    .child(title),
            )
            .when(!counted.is_empty(), |card| {
                card.child(
                    div()
                        .mt(px(2.))
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_quiet)
                        .child(counted),
                )
            })
            .children(open.map(|open| {
                div()
                    .mt(px(2.))
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.accent)
                    .child(open)
            }))
            .child(
                div()
                    .mt(tokens::SPACE_2)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(hint),
            )
    }

    /// The raised shelf: groups of spines on one baseline, each part's label below it.
    fn shelf(
        &self,
        fit: &layout::Fit,
        left: f32,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> (impl IntoElement + use<>, Option<Lit>) {
        let book_len = self.contents.book.len();
        let mut lit = None;
        let mut widths = fit.widths.iter().copied();
        let mut x = left;
        let mut at_book = 0;
        let mut columns: Vec<AnyElement> = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            if index > 0 {
                x += tokens::SHELF_GROUP_GAP;
            }
            let group_left = x;
            let mut spines: Vec<AnyElement> = Vec::new();
            for (position, spine) in group.spines.iter().enumerate() {
                let width = widths.next().unwrap_or(tokens::SPINE_MIN_WIDTH);
                if position > 0 {
                    x += tokens::SPINE_GAP;
                }
                let place = (!group.loose).then(|| {
                    at_book += 1;
                    at_book - 1
                });
                if self.lit() == Some(spine.id) {
                    lit = Some(Lit {
                        spine: spine.clone(),
                        width,
                        left: x,
                    });
                }
                spines.push(self.spine(spine, width, place, tokens, cx));
                x += width;
            }
            x += layout::loose_pad(group);
            let group_width = x - group_left;
            let label = group.label.clone().unwrap_or_default();
            columns.push(
                div()
                    .flex_none()
                    .w(px(group_width))
                    .flex()
                    .flex_col()
                    .when(index > 0, |column| column.ml(px(tokens::SHELF_GROUP_GAP)))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(tokens::SPINE_GAP))
                            .h(tokens::SPINE_HEIGHT + tokens::SPINE_LIFT_OPEN)
                            .children(spines),
                    )
                    .child(
                        div()
                            .h(tokens::SHELF_LABEL_HEIGHT)
                            .pt(px(6.))
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .whitespace_nowrap()
                            .truncate()
                            .child(label),
                    )
                    .into_any_element(),
            );
            let ends_book = !group.loose && self.groups.get(index + 1).is_none_or(|next| next.loose);
            if ends_book {
                let accent = tokens.accent;
                x += tokens::SHELF_END_SLOT;
                columns.push(
                    div()
                        .id("shelf-end")
                        .flex_none()
                        .w(px(tokens::SHELF_END_SLOT))
                        .h(tokens::SPINE_HEIGHT + tokens::SPINE_LIFT_OPEN)
                        .drag_over::<DraggedSpine>(move |style, _, _, _| style.border_l_2().border_color(accent))
                        .on_drop(cx.listener(move |_, dragged: &DraggedSpine, _, cx| {
                            cx.emit(ShelfEvent::Move(dragged.id, book_len));
                        }))
                        .into_any_element(),
                );
            }
        }
        let row = div()
            .relative()
            .flex_none()
            .flex()
            .items_start()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(tokens::SHELF_LABEL_HEIGHT)
                    .h(tokens::HAIRLINE)
                    .bg(Hsla::from(tokens.text_faint).opacity(tokens::SHELF_RULE_OPACITY)),
            )
            .children(columns);
        (row, lit)
    }
}

/// The spine one step left or right of `current`, staying on the end it reaches.
fn step(ids: &[ChapterId], current: Option<ChapterId>, right: bool) -> Option<ChapterId> {
    let at = current.and_then(|current| ids.iter().position(|id| *id == current));
    let target = match (at, right) {
        (None, _) => 0,
        (Some(at), true) => (at + 1).min(ids.len().checked_sub(1)?),
        (Some(at), false) => at.saturating_sub(1),
    };
    ids.get(target).copied()
}

impl Render for BookshelfView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let viewport = f32::from(window.viewport_size().width);
        let available = viewport - f32::from(tokens::SPACE_3) * 2.;
        let fit = layout::fit(&self.groups, available);
        let fits = fit.total <= available;
        let left = if fits {
            (viewport - fit.total) / 2.
        } else {
            f32::from(tokens::SPACE_3)
        };
        let root = div()
            .id("bookshelf")
            .absolute()
            .left_0()
            .bottom_0()
            .w_full()
            .key_context("Bookshelf")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_hover(cx.listener(|shelf, hovered: &bool, _, cx| shelf.hovered(*hovered, cx)));
        if !self.raised {
            return root
                .h(tokens::SHELF_EDGE_HEIGHT)
                .child(self.trace(&fit, fits, &tokens))
                .into_any_element();
        }
        let (clear, solid) = (
            Hsla::from(tokens.canvas).opacity(tokens::CLEAR),
            Hsla::from(tokens.canvas),
        );
        let (row, lit) = self.shelf(&fit, left, &tokens, cx);
        // The card stands apart from the row, which clips what pokes out of it when it scrolls.
        let card = lit.map(|lit| {
            let lit = Lit {
                left: lit.left + f32::from(self.scroll.offset().x),
                ..lit
            };
            self.card(&lit, &tokens, window)
        });
        root.role(Role::Navigation)
            .aria_label(crate::t!(Key::TheBookshelf))
            .h(tokens::SHELF_HEIGHT + tokens::SHELF_FADE_HEIGHT)
            .occlude()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(tokens::SHELF_FADE_HEIGHT)
                    .bg(linear_gradient(
                        tokens::FADE_DOWN,
                        linear_color_stop(clear, 0.),
                        linear_color_stop(solid, 1.),
                    )),
            )
            .child(
                div()
                    .id("shelf-row")
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(tokens::SHELF_HEIGHT)
                    .pb(tokens::SPACE_2)
                    .bg(tokens.canvas)
                    .flex()
                    .items_end()
                    .when(fits, |shelf| shelf.justify_center())
                    .when(!fits, |shelf| {
                        shelf.pl(tokens::SPACE_3).overflow_x_scroll().track_scroll(&self.scroll)
                    })
                    .child(row),
            )
            .children(card)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    fn ids() -> Vec<ChapterId> {
        let file = std::env::temp_dir().join(format!("druck-bookshelf-{}.md", std::process::id()));
        std::fs::write(&file, "# A\n\n# B\n\n# C\n").expect("file");
        let session = Session::open(&file).expect("opens");
        std::fs::remove_file(&file).ok();
        session.book().to_vec()
    }

    #[test]
    fn the_arrows_step_along_the_spines_and_stop_at_the_ends() {
        let ids = ids();
        assert_eq!(step(&ids, Some(ids[1]), true), Some(ids[2]));
        assert_eq!(step(&ids, Some(ids[2]), true), Some(ids[2]));
        assert_eq!(step(&ids, Some(ids[1]), false), Some(ids[0]));
        assert_eq!(step(&ids, Some(ids[0]), false), Some(ids[0]));
        assert_eq!(step(&ids, None, true), Some(ids[0]));
        assert_eq!(step(&[], None, true), None);
    }
}
