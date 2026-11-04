//! The views of the rich blocks: what the galley floats, and the Selection and Images tabs. Each float is drawn by the
//! galley's own frame, as the link field is, and has its own focus while it is open.

mod figure;
mod grid;
mod images_panel;
mod note;
mod picker;
mod selection;
pub mod shapes;

use std::ops::Range;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, ExternalPaths, KeyBinding, MouseButton, Pixels, Point, SharedString, Window, actions,
    div, point, px, size,
};

use self::figure::FigureFloat;
use self::grid::GridFloat;
use self::note::NoteFloat;
use self::picker::PickerFloat;
use crate::editor::commands::{Command, normalized, selection_after};
use crate::editor::float::{Float, placed_at_caret};
use crate::editor::place::Rect;
use crate::editor::rich::{Atom, AtomKind, Key, check, facts, footnotes, images, styles};
use crate::editor::{EditorView, ToggleBold, ToggleItalic, ToggleStrike, ToggleUnderline};
use crate::session::ChapterId;
use crate::source::inline::SpanKind;
use crate::source::{BlockKind, Edit, EditKind, Selection};
use crate::tokens::{self, Tokens};
use crate::workspace::{Parts, Workspace};

actions!(rich_float, [NextField, PreviousField]);

/// How long typing must pause before the book is read again.
const PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

/// The context of a float that holds fields.
pub(super) const CONTEXT: &str = "RichFloat";

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("tab", NextField, context),
        KeyBinding::new("shift-tab", PreviousField, context),
        KeyBinding::new("cmd-b", ToggleBold, context),
        KeyBinding::new("cmd-i", ToggleItalic, context),
        KeyBinding::new("cmd-u", ToggleUnderline, context),
        KeyBinding::new("cmd-shift-x", ToggleStrike, context),
    ]);
}

/// The book's facts were read again, for panels that show them.
pub struct RichChanged;

impl gpui::EventEmitter<RichChanged> for EditorView {}

/// Mounts the Selection and Images tabs.
pub fn mount(parts: &Parts, cx: &mut Context<Workspace>) {
    use crate::inspector::Tab;
    let editor = parts.editor.clone();
    let inspector = parts.inspector.clone();
    let model = inspector.read(cx).model().clone();
    let panel_inspector = inspector.clone();
    let selection = cx.new(|cx| selection::SelectionPanel::new(editor.clone(), model, panel_inspector, cx));
    let images = cx.new(|cx| images_panel::ImagesPanel::new(editor.clone(), cx));
    inspector.update(cx, |inspector, _| {
        inspector.mount(Tab::Selection, selection.into());
        inspector.mount(Tab::Images, images.into());
    });
}

/// A message about a block, with the way to its source when that is what it takes.
pub struct NoticeFloat {
    at: usize,
    message: SharedString,
    offers_source: bool,
}

/// The offer after a reference went: its definition has no reference left.
pub struct OfferFloat {
    labels: Vec<String>,
    /// The buffer's revision when it was offered. If it still stands, removing joins the deletion's undo step.
    revision: u64,
}

/// What the rich blocks float over the galley.
pub enum RichFloat {
    Note(NoteFloat),
    Picker(PickerFloat),
    Grid(GridFloat),
    Figure(FigureFloat),
    Notice(NoticeFloat),
    Offer(OfferFloat),
}

impl RichFloat {
    pub fn takes_keys(&self) -> bool {
        !matches!(self, Self::Notice(_) | Self::Offer(_))
    }
}

/// A small button in a float.
pub(super) fn small_button(
    tokens: &Tokens,
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(22.))
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(tokens.divider)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .cursor_pointer()
        .hover(|button| button.bg(tokens.raised))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label.into())
}

/// The card behind a float.
pub(super) fn card(tokens: &Tokens) -> gpui::Div {
    div()
        .occlude()
        .flex()
        .flex_col()
        .rounded(tokens::RADIUS)
        .bg(tokens.panel)
        .border_1()
        .border_color(tokens.divider)
        .shadow_md()
        .text_size(tokens::TEXT_BODY)
        .text_color(tokens.text)
}

/// A field's label inside a float.
pub(super) fn field_label(tokens: &Tokens, label: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_faint)
        .child(label.into())
}

impl EditorView {
    // The book's facts

    /// Reads the book again once typing pauses. A key does nothing more than start this timer.
    pub(in crate::editor) fn schedule_rich(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rich_pause = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(PAUSE).await;
            this.update_in(cx, |this, window, cx| this.refresh_rich(window, cx))
                .ok();
        });
    }

    /// Reads what changed in the book. If a definition lost its last reference, offers to remove it. If what chips show
    /// may have changed, asks the engine on another thread.
    pub(in crate::editor) fn refresh_rich(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let orphaned = self.rich.refresh(&self.session);
        if !orphaned.is_empty() && matches!(self.float, Float::None) && self.marked.is_none() {
            self.float = Float::Rich(RichFloat::Offer(OfferFloat {
                labels: orphaned,
                revision: self.session.active_buffer().revision(),
            }));
        }
        // An undo brought the reference back.
        if let Float::Rich(RichFloat::Offer(offer)) = &self.float
            && offer
                .labels
                .iter()
                .any(|label| self.rich.book.referenced.contains(label))
        {
            self.float = Float::None;
        }
        cx.emit(RichChanged);
        cx.notify();
        if self.rich.labels_wanted().is_none() {
            return;
        }
        let joined = self.session.joined();
        let kind = self.session.kind().clone();
        let theme = self.session.theme_file();
        let found =
            cx.background_spawn(async move { crate::preview::job::Typesetter::new(kind, theme).labels(joined) });
        self.rich_labels = cx.spawn_in(window, async move |this, cx| {
            let labels = found.await;
            this.update(cx, |this, cx| {
                if this.rich.set_labels(labels) {
                    cx.notify();
                }
            })
            .ok();
        });
    }

    /// The table or figure block `index` belongs to.
    pub(in crate::editor) fn rich_atom(&self, index: usize) -> Option<&Atom> {
        let atoms = &self.rich.atoms;
        let at = atoms.partition_point(|atom| atom.last < index);
        atoms.get(at).filter(|atom| atom.first <= index)
    }

    /// Whether an atom shows whole now: unless it cannot, or the caret was brought inside it to show its source.
    pub(in crate::editor) fn atom_is_whole(&self, atom: &Atom) -> bool {
        if self.source || !atom.is_shown_whole() {
            return false;
        }
        let blocks = self.blocks.as_slice();
        let range = blocks[atom.first].range.start..blocks[atom.last].range.end;
        let at_end = |at: usize| at == self.session.active_buffer().len();
        !self.selection_is_inside(&range) && self.reveal_offset().is_none_or(|at| !range.contains(&at) || at_end(at))
    }

    /// Whether a selection stands wholly inside this range, as the find bar's does. It shows what it selected.
    fn selection_is_inside(&self, range: &Range<usize>) -> bool {
        let selected = self.selection.range();
        !selected.is_empty() && range.start <= selected.start && selected.end <= range.end
    }

    /// Moves a caret standing inside this block run to the text after it, so the run shows whole.
    pub(super) fn caret_out_of(&mut self, first: usize, last: usize, window: &mut Window, cx: &mut Context<Self>) {
        let blocks = self.blocks.as_slice();
        let range = blocks[first].range.start..blocks[last].range.end;
        let head = self.selection.head;
        if range.contains(&head) || self.selection_is_inside(&range) {
            let after = range.end.min(self.session.active_buffer().len());
            self.selection = Selection::caret(after);
            self.selection = Selection::caret(self.skip_hidden(after, true));
            self.reveal_at = self.selection.head;
            self.restart_blink(window, cx);
        }
    }

    /// Whether a note's text is left out because its reference shows it. A note without a reference stays a raw block.
    pub(in crate::editor) fn definition_is_hidden(&self, index: usize) -> bool {
        let block = &self.blocks.as_slice()[index];
        self.rich
            .is_referenced(&self.session.active_buffer().text()[block.range.clone()])
            && self.reveal_offset().is_none_or(|at| self.blocks.index_at(at) != index)
            && !self.selection_is_inside(&block.range)
    }

    /// Whether the caret passes over a block.
    pub(in crate::editor) fn takes_no_caret(&self, index: usize) -> bool {
        self.is_hidden_block(index)
            || self
                .rich_atom(index)
                .is_some_and(|atom| !self.source && self.atom_is_whole(atom))
    }

    /// Whether the text's last block is a table or figure, whose last line is still somewhere to stand.
    pub(in crate::editor) fn ends_in_atom(&self) -> bool {
        self.session.active_buffer().text().ends_with('\n')
            && self
                .rich_atom(self.blocks.as_slice().len() - 1)
                .is_some_and(|atom| atom.is_shown_whole())
    }

    /// `target`, moved to the edge of an atomic chip it landed inside, on the side the caret is heading.
    pub(in crate::editor) fn past_chip(&self, target: usize, forward: bool) -> usize {
        if self.source {
            return target;
        }
        let index = self.blocks.index_at(target);
        let start = self.blocks.as_slice()[index].range.start;
        let Some(layout) = &self.slots[index].layout else {
            return target;
        };
        let relative = target - start;
        layout
            .chips
            .iter()
            .find(|chip| chip.atomic && chip.range.start < relative && relative < chip.range.end)
            .map_or(target, |chip| {
                start + if forward { chip.range.end } else { chip.range.start }
            })
    }

    // Writing

    /// Writes edits to a chapter as one undo step, if the book stays valid. Otherwise says why and writes nothing.
    /// `after` is the selection in the active chapter, by default on the same text as before.
    pub(super) fn write_rich(
        &mut self,
        chapter: ChapterId,
        edits: Vec<Edit>,
        after: Option<Selection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let edits = normalized(edits, self.session.buffer(chapter).text());
        if edits.is_empty() {
            return Ok(());
        }
        let before = check::problems_now(&self.session);
        if let Some(why) = check::refusal(&self.session, chapter, &edits, &before) {
            return Err(why);
        }
        if chapter == self.session.active() {
            let after = after.unwrap_or_else(|| selection_after(self.selection, &edits));
            let scroll = self.scroll;
            self.edit(EditKind::Discrete, edits, after, None, window, cx);
            // The caret may stand far from what a float changed. The galley stays where the author is looking.
            self.scroll = scroll;
            self.autoscroll = false;
        } else {
            self.update_session(
                |session| session.edit_chapter(chapter, edits, EditKind::Discrete),
                window,
                cx,
            )
            .map_err(|error| error.to_string())?;
            self.schedule_rich(window, cx);
        }
        Ok(())
    }

    /// Shows a message over a block.
    pub(super) fn notify_rich(&mut self, at: usize, message: String, offers_source: bool, cx: &mut Context<Self>) {
        self.float = Float::Rich(RichFloat::Notice(NoticeFloat {
            at,
            message: message.into(),
            offers_source,
        }));
        cx.notify();
    }

    /// Closes what floats, writing what is unsaved in it. Something that cannot be written keeps it open.
    pub(in crate::editor) fn commit_rich_float(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.float, Float::Rich(_)) {
            return;
        }
        let Float::Rich(float) = std::mem::take(&mut self.float) else {
            return;
        };
        match float {
            RichFloat::Note(note) => self.commit_note(note, window, cx),
            RichFloat::Grid(grid) => self.commit_grid(grid, window, cx),
            RichFloat::Figure(figure) => self.commit_figure(figure, window, cx),
            RichFloat::Picker(_) | RichFloat::Notice(_) | RichFloat::Offer(_) => {}
        }
        if !matches!(self.float, Float::Rich(_)) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    // Opening by mouse and key

    /// What a press at `position` opens: a chip's editor, a table's grid, a figure's card, or for a table the grid cannot
    /// read, why not. Returns whether the press is taken.
    pub(in crate::editor) fn rich_mouse_down(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.source || !self.column.viewport.contains(&position) {
            return false;
        }
        if position.x < self.column.text_left || position.x > self.column.text_left + self.column.text_width {
            return false;
        }
        let Some(index) = self.block_at_y(position.y) else {
            return false;
        };
        let top = self
            .visible
            .iter()
            .find(|(block, _)| *block == index)
            .map_or(px(0.), |(_, top)| *top);
        if let Some(atom) = self.rich_atom(index).cloned() {
            if let AtomKind::Table(Err(why)) = &atom.kind {
                let start = self.blocks.as_slice()[atom.first].range.start;
                self.notify_rich(start, crate::t!(*why).to_owned(), true, cx);
                return false;
            }
            if self.atom_is_whole(&atom) && index == atom.first {
                let layout = self.layout_block(index, window);
                let height = layout.whole.as_ref().map_or(px(0.), |whole| whole.height());
                if position.y < top + height {
                    // After the press, whose focus would otherwise land on the galley again.
                    cx.defer_in(window, move |this, window, cx| this.open_atom(&atom, window, cx));
                    return true;
                }
            }
            return false;
        }
        let layout = self.layout_block(index, window);
        let relative_y = position.y - top;
        let x = position.x - self.column.text_left;
        let chip = layout.lines.iter().flat_map(|line| &line.chips).find(|(bounds, _)| {
            bounds.left() <= x && x <= bounds.right() && bounds.top() <= relative_y && relative_y <= bounds.bottom()
        });
        if chip.is_none() {
            return false;
        }
        let Some(line) = layout.lines.iter().find(|line| {
            line.chips
                .iter()
                .any(|(bounds, _)| bounds.contains(&point(x, relative_y)))
        }) else {
            return false;
        };
        let row = (((relative_y - line.y) / line.line_height) as usize).min(line.rows.len() - 1);
        let shown = line.display_at_x(row, (x - line.x).max(px(0.)));
        let Some((range, _)) = line
            .display
            .chips
            .iter()
            .find(|(range, _)| range.start <= shown && shown <= range.end)
        else {
            return false;
        };
        let source = line.display.to_source(range.start);
        let start = self.blocks.as_slice()[index].range.start;
        if let Some(chip) = layout.chips.iter().find(|chip| chip.range.start == source).cloned() {
            let range = start + chip.range.start..start + chip.range.end;
            cx.defer_in(window, move |this, window, cx| this.open_chip(range, window, cx));
            return true;
        }
        false
    }

    fn open_atom(&mut self, atom: &Atom, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.blocks.as_slice()[atom.first].range.start;
        match &atom.kind {
            AtomKind::Table(Ok(_)) => self.open_grid_at(start, window, cx),
            AtomKind::Table(Err(_)) => {}
            AtomKind::Figure(_) => self.open_figure(start, window, cx),
        }
    }

    /// Opens the editor of the chip at `range` of the active chapter.
    fn open_chip(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let written = self.session.active_buffer().text()[range.clone()].to_owned();
        if let Some(label) = facts::reference_label(&written) {
            let label = label.to_owned();
            self.open_note(&label, range, window, cx);
        } else if crate::editor::rich::citations::read_reference(&written)
            .is_some_and(|(label, _)| ["sec:", "fig:", "tbl:"].iter().any(|prefix| label.starts_with(prefix)))
        {
            self.open_reference_picker_on(range, window, cx);
        } else {
            self.open_citation_picker_on(range, window, cx);
        }
    }

    /// Cmd+Enter: answers the offer, or opens the editor of the chip or block the caret stands at.
    pub(in crate::editor) fn edit_rich_at_caret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Float::Rich(RichFloat::Offer(_)) = &self.float {
            self.accept_offer(window, cx);
            return;
        }
        let head = self.selection.head;
        let index = self.blocks.index_at(head);
        let block = self.blocks.as_slice()[index].clone();
        let text = self.session.active_buffer().text();
        for offset in [index, index.wrapping_sub(1), index + 1] {
            if let Some(atom) = self.rich_atom(offset).cloned() {
                let range = self.blocks.as_slice()[atom.first].range.start..self.blocks.as_slice()[atom.last].range.end;
                if range.contains(&head) || range.end == head || range.start == head {
                    self.open_atom(&atom, window, cx);
                    return;
                }
            }
        }
        let relative = head - block.range.start;
        let span = crate::source::inline::inline_spans(block.kind, &text[block.range.clone()])
            .into_iter()
            .find(|span| {
                matches!(
                    span.kind,
                    SpanKind::FootnoteReference | SpanKind::Citation | SpanKind::CrossReference
                ) && span.range.start <= relative
                    && relative <= span.range.end
            });
        if let Some(span) = span {
            self.open_chip(
                block.range.start + span.range.start..block.range.start + span.range.end,
                window,
                cx,
            );
        }
    }

    // The offer to remove a note's text

    fn accept_offer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Float::Rich(RichFloat::Offer(offer)) = std::mem::take(&mut self.float) else {
            return;
        };
        window.focus(&self.focus, cx);
        for label in offer.labels {
            // The reference may have come back since, by an undo.
            let Some((chapter, _)) = self
                .rich
                .book
                .definitions
                .get(&label)
                .filter(|_| !self.rich.book.referenced.contains(&label))
                .cloned()
            else {
                continue;
            };
            let same_step =
                chapter == self.session.active() && self.session.active_buffer().revision() == offer.revision;
            let text = self.session.buffer(chapter).text();
            let blocks = crate::source::Blocks::parse(text);
            let facts = facts::scan(text);
            let Some(definition) = facts
                .definitions
                .iter()
                .find(|definition| footnotes::folded(&definition.label) == label)
            else {
                continue;
            };
            let edit = footnotes::remove_definition(text, &blocks, definition);
            if same_step {
                let edits = vec![edit];
                let after = selection_after(self.selection, &edits);
                let started = std::time::Instant::now();
                match self.session.active_buffer_mut().amend(edits, after, started) {
                    Ok(splice) => {
                        self.resegment(splice);
                        self.selection = after;
                        self.reveal_at = after.head;
                    }
                    Err(error) => eprintln!("edit rejected: {error}"),
                }
                cx.emit(crate::editor::TextChanged);
                self.schedule_rich(window, cx);
            } else if let Err(why) = self.write_rich(chapter, vec![edit], None, window, cx) {
                self.notify_rich(self.selection.head, why, false, cx);
            }
        }
        cx.notify();
    }

    fn offer_element(&self, offer: &OfferFloat, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx).clone();
        let labels = offer.labels.join(", ");
        card(&tokens)
            .w(tokens::LINK_POPUP_WIDTH)
            .p(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .child(crate::t!(Key::OrphanOffer, label = labels))
            .child(
                div()
                    .flex()
                    .gap(tokens::SPACE_1)
                    .child(
                        small_button(&tokens, "offer-remove", crate::t!(Key::OrphanRemove))
                            .text_color(tokens.accent)
                            .on_click(cx.listener(|this, _, window, cx| this.accept_offer(window, cx))),
                    )
                    .child(
                        small_button(&tokens, "offer-keep", crate::t!(Key::OrphanKeep))
                            .on_click(cx.listener(|this, _, _, cx| this.close_float(cx))),
                    ),
            )
            .into_any_element()
    }

    fn notice_element(&self, notice: &NoticeFloat, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx).clone();
        let offers = notice.offers_source;
        card(&tokens)
            .w(tokens::LINK_POPUP_WIDTH)
            .p(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .child(notice.message.clone())
            .when(offers, |card| {
                card.child(div().flex().child(
                    small_button(&tokens, "notice-source", crate::t!(Key::ShowSource)).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.close_float(cx);
                            this.toggle_source(window, cx);
                        },
                    )),
                ))
            })
            .into_any_element()
    }

    // The frame

    /// The window rectangle of a byte range's start, if it is on screen.
    pub(super) fn caret_rect(&mut self, offset: usize, window: &Window) -> Option<gpui::Bounds<Pixels>> {
        self.offset_bounds(offset, false, window)
    }

    /// The top of a block in this frame.
    pub(super) fn block_top(&self, index: usize) -> Option<Pixels> {
        self.visible
            .iter()
            .find(|(block, _)| *block == index)
            .map(|(_, top)| *top)
    }

    /// Builds the open float for this frame, with where it goes.
    pub(in crate::editor) fn rich_float(
        &mut self,
        area: Rect,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Point<Pixels>)> {
        let target = match &self.float {
            Float::Rich(RichFloat::Note(note)) => note.anchor(),
            Float::Rich(RichFloat::Picker(picker)) => picker.anchor(),
            Float::Rich(RichFloat::Offer(_)) => self.selection.head,
            Float::Rich(RichFloat::Notice(notice)) => notice.at,
            Float::Rich(RichFloat::Grid(grid)) => {
                return self.grid_element(grid.first_byte(), cx);
            }
            Float::Rich(RichFloat::Figure(figure)) => {
                return self.figure_element(figure.first_byte(), area, window, cx);
            }
            _ => return None,
        };
        let bounds = self.caret_rect(target, window)?;
        let wanted = match &self.float {
            Float::Rich(RichFloat::Note(note)) => size(note::WIDTH, note.height(cx)),
            Float::Rich(RichFloat::Picker(_)) => size(picker::WIDTH, px(460.)),
            _ => size(tokens::LINK_POPUP_WIDTH, px(90.)),
        };
        let float = std::mem::take(&mut self.float);
        let built = match &float {
            Float::Rich(RichFloat::Note(note)) => self.note_element(note, cx),
            Float::Rich(RichFloat::Picker(picker)) => self.picker_element(picker, cx),
            Float::Rich(RichFloat::Offer(offer)) => self.offer_element(offer, cx),
            Float::Rich(RichFloat::Notice(notice)) => self.notice_element(notice, cx),
            _ => unreachable!("taken above"),
        };
        self.float = float;
        Some((built, placed_at_caret(bounds, wanted, area)))
    }

    // Images, from the dialog and by drop

    /// Asks for an image, then takes it in at the caret, or in place of the typed slash.
    pub(in crate::editor) fn choose_image(
        &mut self,
        slash: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::t!(Key::ImagePrompt).into()),
        });
        let at = slash.clone().unwrap_or_else(|| self.selection.range());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.take_image(path, at, window, cx))
                .ok();
        })
        .detach();
    }

    /// Copies an image into the document and writes its figure after the block at `at`, replacing what `at` covers.
    pub(in crate::editor) fn take_image(
        &mut self,
        path: PathBuf,
        at: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let base = self.session.base_dir();
        let is_project = matches!(self.session.kind(), crate::session::Kind::Project { .. });
        let written = match images::bring_in(&base, is_project, &path) {
            Ok(written) => written,
            Err(why) => {
                self.notify_rich(at.start, why, false, cx);
                return;
            }
        };
        let text = self.session.active_buffer().text();
        let new = images::write(&written, "", "", images::Placement::Here);
        let Command { edits, selection } = crate::editor::rich::image_command(text, &self.blocks, at.clone(), &new);
        let chapter = self.session.active();
        self.rich.forget_sizes();
        self.forget_shapes();
        match self.write_rich(chapter, edits, Some(selection), window, cx) {
            Ok(()) => {
                self.autoscroll = true;
                self.open_figure(selection.head, window, cx);
            }
            Err(why) => self.notify_rich(at.start, why, false, cx),
        }
    }

    /// Takes image files dropped on the galley, at the block under the pointer.
    pub(in crate::editor) fn drop_paths(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let position = window.mouse_position();
        for path in paths.paths().to_vec() {
            let (offset, _) = self.offset_at(position, window);
            self.take_image(path, offset..offset, window, cx);
        }
    }
}

/// The block under the caret, for the Selection tab.
pub(in crate::editor) struct BlockInfo {
    pub kind: BlockKind,
    pub is_figure: bool,
    pub class: Option<styles::Class>,
    pub style: Option<String>,
}

impl EditorView {
    /// The block under the caret, if it is one that can show.
    pub(in crate::editor) fn block_info(&self) -> Option<BlockInfo> {
        let index = self.blocks.index_at(self.selection.head);
        let block = &self.blocks.as_slice()[index];
        if matches!(block.kind, BlockKind::Blank | BlockKind::FrontMatter) {
            return None;
        }
        let text = self.session.active_buffer().text();
        let written = &text[block.range.clone()];
        let in_table = matches!(self.rich_atom(index).map(|atom| &atom.kind), Some(AtomKind::Table(_)));
        let is_figure = matches!(self.rich_atom(index).map(|atom| &atom.kind), Some(AtomKind::Figure(_)));
        Some(BlockInfo {
            kind: if in_table { BlockKind::Table } else { block.kind },
            is_figure,
            class: if in_table || is_figure {
                None
            } else {
                styles::class_of(block.kind, written)
            },
            style: styles::style_of(text, &self.blocks, index),
        })
    }

    /// Gives the block under the caret a named style, or none, or says why it cannot have it.
    pub(in crate::editor) fn set_block_style(
        &mut self,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let index = self.blocks.index_at(self.selection.head);
        let text = self.session.active_buffer().text();
        let Some(edit) = styles::set_style(text, &self.blocks, index, name) else {
            return Ok(());
        };
        let active = self.session.active();
        self.write_rich(active, vec![edit], None, window, cx)
    }

    /// Every use of an image in the book.
    pub(in crate::editor) fn image_uses(&self) -> Vec<(ChapterId, String, usize)> {
        self.rich.image_uses(&self.session)
    }
}
