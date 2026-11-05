//! `Cmd+Y`: the chapter at any point of its past, over the whole window until closed. A strip at the foot holds commits as
//! tall marks and local history as small ticks, oldest at the left and now at the right, with the current branch and the
//! sync state at its ends. Above it the chapter shows at that point, word differences against now on tints. A passage or
//! the whole chapter can be restored, which is an ordinary edit that can be undone.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, DefiniteLength, Entity, FocusHandle, FontStyle, FontWeight, HighlightStyle, Hsla,
    InteractiveText, KeyBinding, MouseButton, Pixels, ScrollHandle, SharedString, StrikethroughStyle, StyledText,
    Subscription, Task, UnderlineStyle, WeakEntity, Window, actions, canvas, div, fill, point, px, size,
};

use super::history::{History, now_ms};
use super::repo::Repo;
use super::shown::{self, Shown, ShownBlock, Tint};
use super::strings::Key;
use super::{RepoState, Versions, dates, sync_words, ui};
use crate::editor::EditorView;
use crate::persist::merge::edits_between;
use crate::preview::Typeset;
use crate::preview::job::{BookType, Face};
use crate::source::{BlockKind, Edit, EditKind};
use crate::tokens::{self, Tokens};
use crate::workspace::Parts;

actions!(
    timeline,
    [
        Close,
        Earlier,
        Later,
        EarlierCommit,
        LaterCommit,
        Oldest,
        Newest,
        PreviousPassage,
        NextPassage,
        RestorePassage,
        RestoreChapter,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let context = Some("Timeline");
    cx.bind_keys([
        KeyBinding::new("escape", Close, context),
        KeyBinding::new("left", Earlier, context),
        KeyBinding::new("right", Later, context),
        KeyBinding::new("alt-left", EarlierCommit, context),
        KeyBinding::new("alt-right", LaterCommit, context),
        KeyBinding::new("home", Oldest, context),
        KeyBinding::new("end", Newest, context),
        KeyBinding::new("up", PreviousPassage, context),
        KeyBinding::new("down", NextPassage, context),
        KeyBinding::new("enter", RestorePassage, context),
        KeyBinding::new("cmd-enter", RestoreChapter, context),
    ]);
}

/// A point on the strip.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mark {
    Commit {
        id: String,
        summary: String,
        blob: String,
        time_ms: i64,
    },
    Local {
        blob: String,
        label: Option<String>,
        time_ms: i64,
    },
}

impl Mark {
    fn time_ms(&self) -> i64 {
        match self {
            Self::Commit { time_ms, .. } | Self::Local { time_ms, .. } => *time_ms,
        }
    }

    fn blob(&self) -> &str {
        match self {
            Self::Commit { blob, .. } | Self::Local { blob, .. } => blob,
        }
    }

    fn is_commit(&self) -> bool {
        matches!(self, Self::Commit { .. })
    }
}

/// What the text area shows at a position.
struct AtPoint {
    /// The version's exact text.
    version: Arc<str>,
    /// The text it was compared with.
    current: Arc<str>,
    shown: Shown,
}

pub struct Timeline {
    view: WeakEntity<Versions>,
    editor: Entity<EditorView>,
    store: Option<Arc<History>>,
    /// The book's folder, or nothing for a single file.
    book_folder: Option<PathBuf>,
    open: bool,
    focus: FocusHandle,
    before: Option<FocusHandle>,
    path: PathBuf,
    title: String,
    /// In order of time, oldest first.
    marks: Vec<Mark>,
    /// A position in `marks`, or its length for now.
    at: usize,
    /// Each blob loaded so far, so scrubbing back over it is immediate.
    texts: HashMap<String, Arc<str>>,
    point: Option<AtPoint>,
    selected: usize,
    /// The newest load: an older one that finishes later is dropped.
    generation: u64,
    notice: Option<SharedString>,
    book: BookType,
    scroll: ScrollHandle,
    strip: Rc<Cell<Bounds<Pixels>>>,
    dragging: bool,
    loading: Task<()>,
    showing: Task<()>,
    _subscription: Subscription,
}

impl Timeline {
    pub fn new(
        view: WeakEntity<Versions>,
        parts: &Parts,
        store: Option<Arc<History>>,
        book_folder: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&parts.preview, |timeline, _, Typeset(book): &Typeset, cx| {
            timeline.book = book.clone();
            cx.notify();
        });
        Self {
            view,
            editor: parts.editor.clone(),
            store,
            book_folder,
            open: false,
            focus: cx.focus_handle(),
            before: None,
            path: PathBuf::new(),
            title: String::new(),
            marks: Vec::new(),
            at: 0,
            texts: HashMap::new(),
            point: None,
            selected: 0,
            generation: 0,
            notice: None,
            book: tokens::stand_in_type(),
            scroll: ScrollHandle::new(),
            strip: Rc::new(Cell::new(Bounds::default())),
            dragging: false,
            loading: Task::ready(()),
            showing: Task::ready(()),
            _subscription: subscription,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens on the chapter the text is on.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.before = window.focused(cx);
        self.open = true;
        let session = self.editor.read(cx).session();
        self.path = session.active_chapter().path().to_owned();
        self.title = session.title_of(session.active());
        self.notice = None;
        self.focus.focus(window, cx);
        self.reload(cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.point = None;
        self.texts.clear();
        self.marks.clear();
        if let Some(before) = self.before.take() {
            window.focus(&before, cx);
        }
        cx.notify();
    }

    /// The active chapter changed while the timeline is open: it follows.
    pub fn follow_active(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let session = self.editor.read(cx).session();
        let path = session.active_chapter().path().to_owned();
        if path != self.path {
            self.title = session.title_of(session.active());
            self.path = path;
            self.reload(cx);
        }
    }

    /// Reads the file's marks again, from the store and the repo, off the main thread.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let (store, book) = (self.store.clone(), self.book_folder.clone());
        let path = self.path.clone();
        let work = cx.background_spawn(async move {
            let mut marks: Vec<Mark> = Vec::new();
            if let Some(store) = store {
                marks.extend(store.versions(&path).into_iter().map(|version| Mark::Local {
                    blob: version.hash,
                    label: version.label,
                    time_ms: version.time,
                }));
            }
            let repo = book.as_deref().and_then(|book| Repo::open(book).ok().flatten());
            if let Some(repo) = repo {
                marks.extend(
                    repo.file_history(&path)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(commit, blob)| Mark::Commit {
                            id: commit.id,
                            summary: commit.summary,
                            blob,
                            time_ms: commit.time * 1000,
                        }),
                );
            }
            marks.sort_by_key(Mark::time_ms);
            marks
        });
        self.loading = cx.spawn(async move |timeline, cx| {
            let marks = work.await;
            timeline
                .update(cx, |timeline, cx| {
                    let current = timeline.current_text(cx);
                    let current_blob = current.as_deref().map(|text| super::history::blob_id(text.as_bytes()));
                    // Start at the newest point that says anything: the last one that differs from now.
                    let start = (marks
                        .iter()
                        .rposition(|mark| Some(mark.blob()) != current_blob.as_deref()))
                    .unwrap_or(marks.len());
                    timeline.marks = marks;
                    timeline.at = crate::gate::var("DRUCK_TIMELINE_AT")
                        .and_then(|back| back.parse::<usize>().ok())
                        .map_or(start, |back| timeline.marks.len().saturating_sub(back));
                    timeline.show_position(cx);
                })
                .ok();
        });
    }

    fn current_text(&self, cx: &App) -> Option<String> {
        self.editor.read(cx).session().file_text(&self.path)
    }

    /// Shows the version at the position: from what was loaded, or else off the main thread.
    fn show_position(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.selected = 0;
        self.notice = None;
        cx.notify();
        let Some(current) = self.current_text(cx).map(Arc::<str>::from) else {
            return;
        };
        let mark = self.marks.get(self.at).cloned();
        let known = match &mark {
            Some(mark) => self.texts.get(mark.blob()).cloned(),
            None => Some(current.clone()),
        };
        let (store, book) = (self.store.clone(), self.book_folder.clone());
        let path = self.path.clone();
        let work = cx.background_spawn(async move {
            let version: Option<Arc<str>> = known.or_else(|| match mark.as_ref()? {
                Mark::Local { blob, .. } => store?.text(blob).ok().map(Arc::from),
                Mark::Commit { id, .. } => {
                    let repo = Repo::open(&book?).ok()??;
                    repo.text_at(id, &path).ok()?.map(Arc::from)
                }
            });
            let shown = version.as_ref().map(|version| shown::build(version, &current));
            (mark, version, current, shown)
        });
        self.showing = cx.spawn(async move |timeline, cx| {
            let (mark, version, current, shown) = work.await;
            timeline
                .update(cx, |timeline, cx| {
                    if timeline.generation != generation {
                        return;
                    }
                    timeline.point = match (version, shown) {
                        (Some(version), Some(shown)) => {
                            if let Some(mark) = &mark {
                                timeline.texts.insert(mark.blob().to_owned(), version.clone());
                            }
                            Some(AtPoint {
                                version,
                                current,
                                shown,
                            })
                        }
                        _ => None,
                    };
                    timeline.scroll_to_selected();
                    cx.notify();
                })
                .ok();
        });
    }

    fn go(&mut self, at: usize, cx: &mut Context<Self>) {
        let at = at.min(self.marks.len());
        if at != self.at {
            self.at = at;
            self.show_position(cx);
        }
    }

    fn earlier_commit(&mut self, cx: &mut Context<Self>) {
        if let Some(at) = self.marks[..self.at.min(self.marks.len())]
            .iter()
            .rposition(Mark::is_commit)
        {
            self.go(at, cx);
        }
    }

    fn later_commit(&mut self, cx: &mut Context<Self>) {
        let from = self.at + 1;
        let found = (self.marks.iter().enumerate().skip(from))
            .find(|(_, mark)| mark.is_commit())
            .map_or(self.marks.len(), |(at, _)| at);
        self.go(found, cx);
    }

    fn step_passage(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(point) = &self.point else {
            return;
        };
        let count = point.shown.passage_count();
        if count == 0 {
            return;
        }
        self.selected = if forward {
            (self.selected + 1).min(count - 1)
        } else {
            self.selected.saturating_sub(1)
        };
        self.scroll_to_selected();
        cx.notify();
    }

    fn scroll_to_selected(&self) {
        let Some(point) = &self.point else {
            return;
        };
        let block = (point.shown.blocks.iter())
            .position(|block| (block.runs.iter()).any(|run| run.passage == Some(self.selected)));
        if let Some(block) = block {
            self.scroll.scroll_to_top_of_item(block);
        }
    }

    /// Replaces a passage with what this version holds.
    fn restore_passage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edits = self
            .point
            .as_ref()
            .map(|point| point.shown.restore_passage(self.selected, &point.version));
        self.restore(edits, window, cx);
    }

    /// Makes the chapter exactly what this version holds.
    fn restore_chapter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edits = self
            .point
            .as_ref()
            .map(|point| edits_between(&point.current, &point.version));
        self.restore(edits, window, cx);
    }

    /// One undoable step, labelled in local history first, on the text the differences were shown against.
    fn restore(&mut self, edits: Option<Vec<Edit>>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(edits), Some(point)) = (edits, &self.point) else {
            return;
        };
        if edits.is_empty() {
            return;
        }
        // The text moved on under the timeline, as an agent's write would.
        if self.current_text(cx).as_deref() != Some(&*point.current) {
            self.show_position(cx);
            return;
        }
        let path = self.path.clone();
        let editor = self.editor.clone();
        super::label_history(editor.read(cx).session(), crate::t!(Key::BeforeRestore), cx);
        let result = editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| session.apply_to_file(&path, &edits, EditKind::Discrete),
                window,
                cx,
            )
        });
        match result {
            Ok(()) => {
                self.show_position(cx);
                self.notice = Some(crate::t!(Key::Restored).into());
            }
            Err(error) => self.notice = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn position_from(&self, x: Pixels) -> usize {
        let bounds = self.strip.get();
        let slots = self.marks.len() + 1;
        let share = (f32::from(x - bounds.left()) / f32::from(bounds.size.width).max(1.)).clamp(0., 0.9999);
        (share * slots as f32) as usize
    }

    /// The line above the text: this chapter, when, and what this point is.
    fn heading(&self) -> String {
        let now = now_ms() / 1000;
        let mut parts = vec![self.title.clone()];
        match self.marks.get(self.at) {
            None => parts.push(crate::t!(Key::Now).to_owned()),
            Some(mark) => {
                parts.push(dates::long(mark.time_ms() / 1000, now));
                match mark {
                    Mark::Commit { summary, .. } => parts.push(summary.clone()),
                    Mark::Local { label: Some(label), .. } => parts.push(label.clone()),
                    Mark::Local { label: None, .. } => parts.push(crate::t!(Key::LocalHistory).to_owned()),
                }
            }
        }
        parts.join(" · ")
    }

    #[allow(clippy::too_many_arguments, reason = "a private helper of render, which holds these")]
    fn block(
        &self,
        at: usize,
        block: &ShownBlock,
        follows_paragraph: bool,
        measure: Pixels,
        tokens: &Tokens,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let face = face_of(block.kind, &self.book);
        // gpui's text has no first-line indent. Em spaces stand in for it, about as wide.
        let indent = if block.kind == BlockKind::Paragraph && follows_paragraph {
            "\u{2003}".repeat((face.first_line_indent / face.size).round() as usize)
        } else {
            String::new()
        };
        let shift = indent.len();
        let scale = tokens::POINT * zoom;
        let text_size = face.size * scale;
        let highlights: Vec<_> = (block.runs.iter())
            .filter(|run| run.tint.is_some() || run.bold || run.italic)
            .map(|run| {
                let selected = run.passage == Some(self.selected);
                let tint = run.tint.map(|tint| match tint {
                    Tint::Removed => Hsla::from(tokens.removed),
                    Tint::Added => Hsla::from(tokens.added),
                });
                (
                    run.range.start + shift..run.range.end + shift,
                    HighlightStyle {
                        background_color: tint.map(|tint| tint.opacity(tokens::DIFF_TINT)),
                        strikethrough: (run.tint == Some(Tint::Removed)).then(|| StrikethroughStyle {
                            thickness: px(1.),
                            color: None,
                        }),
                        underline: selected.then(|| UnderlineStyle {
                            thickness: px(1.),
                            color: Some(tokens.accent.into()),
                            wavy: false,
                        }),
                        font_weight: run.bold.then_some(FontWeight::BOLD),
                        font_style: run.italic.then_some(FontStyle::Italic),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let clickable: Vec<_> = (block.runs.iter())
            .filter(|run| run.passage.is_some())
            .map(|run| run.range.start + shift..run.range.end + shift)
            .collect();
        let passages: Vec<usize> = (block.runs.iter()).filter_map(|run| run.passage).collect();
        let text: SharedString = if block.text.is_empty() {
            " ".into()
        } else {
            format!("{indent}{}", block.text).into()
        };
        let styled = StyledText::new(text).with_highlights(highlights);
        let timeline = cx.entity().downgrade();
        let shown = InteractiveText::new(("timeline-block", at), styled).on_click(clickable, move |index, _, cx| {
            let Some(passage) = passages.get(index).copied() else {
                return;
            };
            timeline
                .update(cx, |timeline, cx| {
                    timeline.selected = passage;
                    cx.notify();
                })
                .ok();
        });
        let gap = face
            .space_after
            .max(if face.first_line_indent == 0. && face.space_after == 0. {
                face.size * tokens::PARAGRAPH_GAP
            } else {
                0.
            });
        div()
            .flex_none()
            .w(measure)
            .max_w_full()
            .mt(px(face.space_before * scale))
            .mb(px(gap * scale))
            .font_family(SharedString::from(face.family.clone()))
            .text_size(px(text_size))
            .line_height(DefiniteLength::from(px(text_size * face.line_height)))
            .font_weight(FontWeight(f32::from(face.weight)))
            .when(face.italic, |text| text.italic())
            .text_color(tokens.text)
            .when(
                block.kind == BlockKind::Comment || block.kind == BlockKind::FrontMatter,
                |text| text.text_color(tokens.text_quiet),
            )
            .child(shown)
    }

    fn strip(&self, tokens: &Tokens) -> impl IntoElement + use<> {
        let marks: Vec<bool> = self.marks.iter().map(Mark::is_commit).collect();
        let at = self.at;
        let holder = self.strip.clone();
        let (ink, faint, accent) = (tokens.text, tokens.text_faint, tokens.accent);
        canvas(
            move |bounds, _, _| holder.set(bounds),
            move |bounds, _, window, _| {
                let slots = marks.len() + 1;
                let x_of = |slot: usize| bounds.left() + bounds.size.width * ((slot as f32 + 0.5) / slots as f32);
                let mark = |slot: usize, width: Pixels, height: Pixels| {
                    let top = bounds.top() + (bounds.size.height - height) * 0.5;
                    Bounds::new(point(x_of(slot) - width * 0.5, top), size(width, height))
                };
                for (slot, is_commit) in marks.iter().enumerate() {
                    if slot == at {
                        continue;
                    }
                    if *is_commit {
                        window.paint_quad(fill(
                            mark(slot, tokens::STRIP_COMMIT_WIDTH, tokens::STRIP_COMMIT_HEIGHT),
                            ink,
                        ));
                    } else {
                        window.paint_quad(fill(
                            mark(slot, tokens::STRIP_TICK_WIDTH, tokens::STRIP_TICK_HEIGHT),
                            faint,
                        ));
                    }
                }
                window.paint_quad(fill(
                    mark(at, tokens::STRIP_COMMIT_WIDTH, tokens::STRIP_POSITION_HEIGHT),
                    accent,
                ));
            },
        )
        .size_full()
    }
}

/// The face a block is set in, as in Write.
fn face_of(kind: BlockKind, book: &BookType) -> &Face {
    match kind {
        BlockKind::Heading { level } => &book.headings[usize::from(level).clamp(1, 6) - 1],
        BlockKind::Quote => &book.quote,
        BlockKind::List => &book.list,
        BlockKind::Code | BlockKind::FrontMatter | BlockKind::Table | BlockKind::Directive | BlockKind::Raw => {
            &book.code
        }
        BlockKind::FootnoteDefinition => &book.footnote,
        _ => &book.body,
    }
}

impl Render for Timeline {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let view = self.view.upgrade();
        let (branch, sync) = match view.as_ref().map(|view| view.read(cx).state()) {
            Some(RepoState::Ready(overview)) => (overview.branch.clone(), sync_words(overview.sync)),
            _ => (String::new(), String::new()),
        };
        let unplaced = view.as_ref().and_then(|view| {
            view.read(cx)
                .unplaced_history(&self.path)
                .map(|orphan| orphan.path.display().to_string())
        });
        let zoom = self.editor.read(cx).zoom();
        let measure = px(self.book.measure * tokens::POINT * zoom);
        let (passage_count, blocks) = match &self.point {
            Some(point) => (point.shown.passage_count(), point.shown.blocks.clone()),
            None => (0, Vec::new()),
        };
        let at_now = self.at >= self.marks.len();
        let elements: Vec<_> = (blocks.iter().enumerate())
            .map(|(at, block)| {
                let follows_paragraph = at > 0 && blocks[at - 1].kind == BlockKind::Paragraph;
                self.block(at, block, follows_paragraph, measure, &tokens, zoom, cx)
                    .into_any_element()
            })
            .collect();
        let says = if self.marks.is_empty() {
            crate::t!(Key::NoHistory).to_owned()
        } else if at_now {
            String::new()
        } else {
            match (self.point.is_some(), passage_count) {
                (false, _) => crate::t!(Key::NoPlaceInFile).to_owned(),
                (true, 0) => crate::t!(Key::SameAsNow).to_owned(),
                (true, 1) => crate::t!(Key::OneChange).to_owned(),
                (true, count) => crate::t!(Key::ChangeCount, count = count),
            }
        };
        let heading = self.heading();
        let can_restore = self.point.is_some() && !at_now;
        div()
            .id("timeline")
            .occlude()
            .absolute()
            .top(tokens::TITLEBAR_HEIGHT)
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .flex_col()
            .bg(tokens.canvas)
            .track_focus(&self.focus)
            .key_context("Timeline")
            .on_action(cx.listener(|timeline, _: &Close, window, cx| timeline.close(window, cx)))
            .on_action(cx.listener(|timeline, _: &Earlier, _, cx| timeline.go(timeline.at.saturating_sub(1), cx)))
            .on_action(cx.listener(|timeline, _: &Later, _, cx| timeline.go(timeline.at + 1, cx)))
            .on_action(cx.listener(|timeline, _: &EarlierCommit, _, cx| timeline.earlier_commit(cx)))
            .on_action(cx.listener(|timeline, _: &LaterCommit, _, cx| timeline.later_commit(cx)))
            .on_action(cx.listener(|timeline, _: &Oldest, _, cx| timeline.go(0, cx)))
            .on_action(cx.listener(|timeline, _: &Newest, _, cx| timeline.go(timeline.marks.len(), cx)))
            .on_action(cx.listener(|timeline, _: &PreviousPassage, _, cx| timeline.step_passage(false, cx)))
            .on_action(cx.listener(|timeline, _: &NextPassage, _, cx| timeline.step_passage(true, cx)))
            .on_action(cx.listener(|timeline, _: &RestorePassage, window, cx| timeline.restore_passage(window, cx)))
            .on_action(cx.listener(|timeline, _: &RestoreChapter, window, cx| timeline.restore_chapter(window, cx)))
            .child(
                div()
                    .flex_none()
                    .min_h(tokens::SECTION_HEIGHT)
                    .px(tokens::SPACE_4)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_3)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(div().min_w_0().truncate().child(heading))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(tokens.text_faint)
                            .child(says),
                    )
                    .children(
                        self.notice
                            .clone()
                            .map(|notice| div().flex_none().text_color(tokens.accent).child(notice)),
                    )
                    .when(can_restore && passage_count > 0, |row| {
                        row.child(
                            ui::button(&tokens, "restore-passage", crate::t!(Key::RestorePassage))
                                .on_click(cx.listener(|timeline, _, window, cx| timeline.restore_passage(window, cx))),
                        )
                        .child(
                            ui::button(&tokens, "restore-chapter", crate::t!(Key::RestoreChapter))
                                .text_color(tokens.accent)
                                .on_click(cx.listener(|timeline, _, window, cx| timeline.restore_chapter(window, cx))),
                        )
                    }),
            )
            .children(unplaced.map(|old_path| {
                let path = self.path.clone();
                let timeline = cx.entity().downgrade();
                div()
                    .flex_none()
                    .px(tokens::SPACE_4)
                    .pb(tokens::SPACE_1)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.accent)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(crate::t!(Key::UnplacedHistory, path = old_path)),
                    )
                    .child(
                        ui::button(&tokens, "use-history", crate::t!(Key::UseHistory)).on_click(move |_, _, cx| {
                            timeline
                                .update(cx, |timeline, cx| {
                                    if let Some(view) = timeline.view.upgrade() {
                                        view.update(cx, |view, cx| view.claim_history(&path, cx));
                                    }
                                })
                                .ok();
                        }),
                    )
            }))
            .child(
                div()
                    .id("timeline-text")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .items_center()
                    .py(tokens::TEXT_MARGIN_Y)
                    .children(elements),
            )
            .child(
                div()
                    .flex_none()
                    .h(tokens::STRIP_HEIGHT)
                    .px(tokens::SPACE_4)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_4)
                    .border_t_1()
                    .border_color(tokens.divider)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(div().flex_none().child(branch))
                    .child(
                        div()
                            .id("timeline-strip")
                            .role(gpui::Role::Slider)
                            .aria_label(crate::t!(Key::TimelineStrip))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|timeline, event: &gpui::MouseDownEvent, _, cx| {
                                    timeline.dragging = true;
                                    timeline.go(timeline.position_from(event.position.x), cx);
                                }),
                            )
                            .on_mouse_move(cx.listener(|timeline, event: &gpui::MouseMoveEvent, _, cx| {
                                if timeline.dragging && event.pressed_button == Some(MouseButton::Left) {
                                    timeline.go(timeline.position_from(event.position.x), cx);
                                }
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|timeline, _: &gpui::MouseUpEvent, _, _| timeline.dragging = false),
                            )
                            .child(self.strip(&tokens)),
                    )
                    .child(div().flex_none().child(sync)),
            )
            .into_any_element()
    }
}
