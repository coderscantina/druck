//! "Fix the proof": one turn in which the agent tries typesetting fixes through the trial tool, and the queue of kept
//! fixes the author reviews.
//!
//! The agent's word keeps nothing. Each fix the tool says it kept is tried again here, on a copy of the book as the
//! session has it, and enters the queue only if that trial clears a warning and causes none. The trials run on a thread
//! of their own with a typesetter of their own, so neither typing nor Publish's pages wait for them. Nothing is written
//! to a chapter or the theme before the author accepts an entry, and an accepted entry goes in as any edit does.

use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use druck::cancel::Cancel;
use futures::StreamExt as _;
use gpui::{App, Context, RenderImage, Task, Window};
use resvg::tiny_skia::Pixmap;
use serde_json::Value;

use super::conversation::Outcome;
use super::session::Access;
use super::strings::{self, Key};
use super::{AgentView, file_in_session};
use crate::editor::rich::images::Placement;
use crate::inspector::Tab;
use crate::preview::job::Layout;
use crate::source::EditKind;
use crate::theme_file::editor::Change;
use crate::typesetter::fix::{self, Fix};
use crate::typesetter::tool;
use crate::typesetter::trial::{self, Bench, Book, Shown, Tried, Warning};
use crate::workspace::Current;

/// The pixels a page of an entry is painted at: twice its width in the rail, for a dense display.
const PAGE_PIXELS: f32 = 264.;

/// A page of an entry: its picture, which page it is, and where on it the fix's block stands.
pub(super) struct Shot {
    pub image: Arc<RenderImage>,
    /// The page's place in the file, from 1.
    pub page: usize,
    /// The block as parts of the page's width and height: left, top, width, height.
    pub mark: Option<[f32; 4]>,
}

/// A kept fix waiting for the author.
pub(super) struct Entry {
    pub id: u64,
    pub fix: Fix,
    /// How many warnings the trial saw it clear.
    pub cleared: usize,
    pub before: Option<Shot>,
    pub after: Option<Shot>,
    /// The first line of the fix's block and where it started, to find the block again in text that has changed.
    anchor: Option<(String, usize)>,
}

impl Entry {
    /// What the fix does, in the app's language.
    pub fn title(&self) -> String {
        match &self.fix {
            Fix::PageBreak(_) => crate::t!(Key::FixPageBreak).to_owned(),
            Fix::Keep(_) => crate::t!(Key::FixKeep).to_owned(),
            Fix::Placement(_, Placement::Top) => crate::t!(Key::FixPlacementTop).to_owned(),
            Fix::Placement(_, Placement::Bottom) => crate::t!(Key::FixPlacementBottom).to_owned(),
            Fix::Placement(_, Placement::Here) => crate::t!(Key::FixPlacementHere).to_owned(),
            Fix::Theme { property, value } => crate::t!(Key::FixTheme, property = property, value = value),
        }
    }

    /// The words the fix stands at, or for a theme value that it holds for the whole book.
    pub fn place(&self) -> String {
        match &self.anchor {
            // A heading goes by its words, without the signs it is written with.
            Some((words, _)) => words.trim_start_matches('#').trim().to_owned(),
            None => crate::t!(Key::FixWholeBook).to_owned(),
        }
    }
}

/// A page of a kept fix as the trial's thread hands it over.
struct Painted {
    pixmap: Pixmap,
    shown: Shown,
    /// The page's size in points.
    size: (f32, f32),
}

/// What the trial's thread reports.
enum Report {
    /// The warnings the book has, with the book they were read from, or why it has no pages.
    Standing(Result<(Book, Vec<Warning>), String>),
    /// A fix was tried. `None` when the trial did not keep it.
    Tried(Option<Box<Kept>>),
}

struct Kept {
    fix: Fix,
    cleared: usize,
    pages: Option<(Painted, Painted)>,
    anchor: Option<(String, usize)>,
}

/// A run under way.
struct Run {
    /// Stops the layout a trial is in.
    cancel: Cancel,
    /// Fixes for the trial's thread, which ends when this is dropped.
    jobs: Sender<Fix>,
    /// Fixes handed to the thread that it has not reported yet.
    pending: usize,
    /// The prompt went to the CLI.
    asked: bool,
    /// How the agent's turn ended, once it has.
    ended: Option<Outcome>,
    kept: usize,
    /// The conversation the chat card had, which it gets back when the run ends.
    chat_session: Option<String>,
    _reports: Task<()>,
}

/// The typesetter's state: the run, and the queue.
#[derive(Default)]
pub(super) struct Fixing {
    run: Option<Run>,
    queue: Vec<Entry>,
    next_id: u64,
    /// A history label stands before the first accepted fix of this queue.
    labelled: bool,
    /// Pictures of entries that left the queue, to free at the next frame.
    stale: Vec<Arc<RenderImage>>,
}

impl Fixing {
    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    /// Drops the run without a word, when the agent it ran on is gone.
    pub fn abandon(&mut self) {
        if let Some(run) = self.run.take() {
            run.cancel.cancel();
        }
    }

    pub fn queue(&self) -> &[Entry] {
        &self.queue
    }

    fn remove(&mut self, id: u64) -> Option<Entry> {
        let at = self.queue.iter().position(|entry| entry.id == id)?;
        let entry = self.queue.remove(at);
        self.stale
            .extend(entry.before.iter().chain(&entry.after).map(|shot| shot.image.clone()));
        Some(entry)
    }
}

/// `DRUCK_FIX_LIMIT=20` hands the agent only the first warnings, for a run that should cost little.
fn limit() -> Option<usize> {
    #[cfg(any(debug_assertions, feature = "measure"))]
    return crate::gate::var("DRUCK_FIX_LIMIT").and_then(|limit| limit.parse().ok());
    #[cfg(not(any(debug_assertions, feature = "measure")))]
    None
}

/// The palette's and the Proof tab's command.
pub fn fix_proof(cx: &mut App) {
    Current::<AgentView>::defer(cx, AgentView::fix_proof);
}

/// Paints the page a trial says to look at.
fn paint(layout: &Layout, shown: Shown) -> Option<Painted> {
    let page = layout.laid.pages().get(shown.page)?;
    let size = (page.width.0 as f32, page.height.0 as f32);
    Some(Painted {
        pixmap: layout.painter.paint(page, PAGE_PIXELS / size.0),
        shown,
        size,
    })
}

/// The fixes of one call of the trial tool that are tried again here: those the tool says it kept, in the call's
/// order. What the tool did not keep cannot enter the queue, so it needs no second trial. An answer that cannot be read
/// sends every fix to the second trial.
fn to_verify(input: &Value, answer: &str) -> Vec<Fix> {
    let fixes = input["fixes"].as_array().map(Vec::as_slice).unwrap_or_default();
    let kept = tool::kept_in(answer).filter(|kept| kept.len() == fixes.len());
    (fixes.iter().enumerate())
        .filter(|(at, _)| kept.as_ref().is_none_or(|kept| kept[*at]))
        .filter_map(|(_, fix)| Fix::from_json(fix).ok())
        .collect()
}

/// The app's own trial of a fix, which alone decides whether it enters the queue: it must clear a warning and cause
/// none, on top of the fixes kept before it.
fn verified(bench: &mut Bench, fix: Fix, cancel: &Cancel) -> Option<Kept> {
    let tried = bench.try_fix(fix, cancel).ok().filter(|tried| tried.outcome.keeps())?;
    Some(kept_of(bench, tried))
}

fn kept_of(bench: &Bench, tried: Tried) -> Kept {
    let pages = tried
        .shown
        .and_then(|(before, after)| Some((paint(&tried.before, before)?, paint(&tried.after, after)?)));
    let anchor = tried.fix.place().and_then(|place| {
        let (_, text) = bench.book().file(&place.file)?;
        let block = fix::block_at(text, place.line).ok()?;
        Some((text[block.clone()].lines().next()?.to_owned(), block.start))
    });
    Kept {
        cleared: tried.outcome.cleared.len(),
        fix: tried.fix,
        pages,
        anchor,
    }
}

fn shot(painted: Painted) -> Shot {
    let (width, height) = painted.size;
    let mark = painted.shown.mark.map(|rect| {
        [
            rect.x.0 as f32 / width,
            rect.y.0 as f32 / height,
            rect.width.0 as f32 / width,
            rect.height.0 as f32 / height,
        ]
    });
    Shot {
        image: crate::preview::render_image(painted.pixmap).0,
        page: painted.shown.page + 1,
        mark,
    }
}

impl AgentView {
    /// The kept fixes that wait for the author.
    pub(super) fn fixes(&self) -> &[Entry] {
        self.fixing.queue()
    }

    /// Pictures of entries that left the queue, for the window to free.
    pub(super) fn take_stale_fix_pages(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.fixing.stale)
    }

    /// The typesetting warnings of the pages on screen, which is what there is to fix.
    pub(super) fn warnings_to_fix(&self, cx: &App) -> usize {
        (self.preview.read(cx).warnings().iter())
            .filter(|warning| trial::is_typesetting(warning))
            .count()
    }

    /// The quiet line at the window's foot while a run is under way.
    pub(super) fn fix_line(&self) -> Option<String> {
        let run = self.fixing.run.as_ref()?;
        Some(if run.asked {
            strings::fix_progress(run.kept)
        } else {
            crate::t!(Key::FixReading).to_owned()
        })
    }

    pub(super) fn fix_access(&self) -> Option<Access> {
        // The agent tries and never writes: the tool is its only way to a fix.
        self.fixing.is_running().then_some(Access::ReadOnly)
    }

    /// Starts a run: the book is laid out on the trial's thread first, and its warnings go to the agent.
    pub(super) fn fix_proof(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.inspector
            .update(cx, |inspector, cx| inspector.show(Tab::Proof, cx));
        if self.cli.is_none() {
            self.fix_asked = true;
            self.ensure_checked(window, cx);
            return;
        }
        if !self.cli_usable() {
            // The card says what is wrong and how to set it up.
            self.open_chat(window, cx);
            return;
        }
        if !self.conversation.is_idle() || self.pass_run.is_some() || self.fixing.is_running() {
            self.say(strings::turn_busy(self.backend().name()), cx);
            return;
        }
        // What waits in the queue is answered first: a new run would try its fixes on a book that may yet take these.
        if !self.fixing.queue().is_empty() {
            return;
        }
        let book = Book::of(self.editor.read(cx).session());
        let cancel = Cancel::default();
        let (jobs, todo) = channel::<Fix>();
        let (reports, mut incoming) = futures::channel::mpsc::unbounded::<Report>();
        let stop = cancel.clone();
        std::thread::Builder::new()
            .name("druck-trials".into())
            .spawn(move || {
                let mut bench = Bench::new(book);
                let standing =
                    (bench.warnings(&stop).map(<[Warning]>::to_vec)).map(|warnings| (bench.book().clone(), warnings));
                if reports.unbounded_send(Report::Standing(standing)).is_err() {
                    return;
                }
                for fix in todo {
                    let kept = verified(&mut bench, fix, &stop).map(Box::new);
                    if reports.unbounded_send(Report::Tried(kept)).is_err() {
                        return;
                    }
                }
            })
            .expect("the system starts a thread");
        let task = cx.spawn_in(window, async move |view, cx| {
            while let Some(report) = incoming.next().await {
                let alive = view.update_in(cx, |view, window, cx| view.fix_reported(report, window, cx));
                if alive.is_err() {
                    return;
                }
            }
        });
        self.pass_result = None;
        self.fixing.run = Some(Run {
            cancel,
            jobs,
            pending: 0,
            asked: false,
            ended: None,
            kept: 0,
            chat_session: self.conversation.session_id().map(str::to_owned),
            _reports: task,
        });
        cx.notify();
    }

    fn fix_reported(&mut self, report: Report, window: &mut Window, cx: &mut Context<Self>) {
        if self.fixing.run.is_none() {
            return;
        }
        match report {
            Report::Standing(Ok((book, warnings))) => {
                let warnings: Vec<Warning> = (warnings.into_iter())
                    .filter(|warning| warning.kind != trial::OTHER)
                    .collect();
                if warnings.is_empty() {
                    return self.end_fixing(crate::t!(Key::FixNothing).to_owned(), cx);
                }
                // A run's turn is not the conversation the chat card resumes, and starts in a process of its own.
                self.running = None;
                self.conversation.set_session_id(None);
                let message = tool::prompt(&book, &warnings, limit());
                let shown = crate::t!(Key::FixProof).to_owned();
                if self.start_turn(shown, message, None, window, cx) {
                    if let Some(run) = &mut self.fixing.run {
                        run.asked = true;
                    }
                } else {
                    // The conversation says why the CLI did not start.
                    self.end_fixing(strings::fix_ended(0, Outcome::Failed), cx);
                }
            }
            Report::Standing(Err(problem)) => self.end_fixing(strings::fix_no_pages(&problem), cx),
            Report::Tried(kept) => {
                if let Some(run) = &mut self.fixing.run {
                    run.pending = run.pending.saturating_sub(1);
                    run.kept += usize::from(kept.is_some());
                }
                if let Some(kept) = kept {
                    let (before, after) = kept.pages.map(|(before, after)| (shot(before), shot(after))).unzip();
                    self.fixing.next_id += 1;
                    self.fixing.queue.push(Entry {
                        id: self.fixing.next_id,
                        fix: kept.fix,
                        cleared: kept.cleared,
                        before,
                        after,
                        anchor: kept.anchor,
                    });
                }
                self.end_fixing_if_done(cx);
            }
        }
        cx.notify();
    }

    /// The trial tool answered a call of the agent's. What it says it kept is tried again here, in the same order.
    pub(super) fn fixes_tried(&mut self, input: &Value, answer: &str) {
        let Some(run) = &mut self.fixing.run else {
            return;
        };
        for fix in to_verify(input, answer) {
            if run.jobs.send(fix).is_ok() {
                run.pending += 1;
            }
        }
    }

    /// The run's turn ended, or its process did.
    pub(super) fn fix_turn_ended(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        if let Some(run) = &mut self.fixing.run {
            run.ended = Some(outcome);
            self.end_fixing_if_done(cx);
        }
    }

    /// Stop was pressed: the trial under way is cut short, and what is kept so far stays to review.
    pub(super) fn stop_fixing(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &mut self.fixing.run else {
            return;
        };
        run.cancel.cancel();
        if !run.asked {
            // No turn runs yet, so nothing else will end the run.
            let kept = run.kept;
            self.end_fixing(strings::fix_ended(kept, Outcome::Stopped), cx);
        }
    }

    fn end_fixing_if_done(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &self.fixing.run else {
            return;
        };
        let Some(outcome) = run.ended else {
            return;
        };
        // After Stop the trials still waiting are dropped with the run.
        if run.pending == 0 || outcome == Outcome::Stopped {
            self.end_fixing(strings::fix_ended(run.kept, outcome), cx);
        }
    }

    fn end_fixing(&mut self, said: String, cx: &mut Context<Self>) {
        let Some(run) = self.fixing.run.take() else {
            return;
        };
        run.cancel.cancel();
        if run.asked {
            // The chat card's conversation goes on where it was, in a process of its own.
            self.running = None;
            self.conversation.set_session_id(run.chat_session);
        }
        self.fixing.labelled = false;
        self.say(said, cx);
    }

    /// Writes an entry's fix into the book, as one undo step, or into the theme. Returns whether the entry is done with.
    fn apply_fix(&mut self, entry: &Entry, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Fix::Theme { property, value } = &entry.fix {
            let keys: Vec<&str> = property.split('.').collect();
            let change = Change::set(&keys, value.clone());
            let model = self.inspector.read(cx).model().clone();
            // A common theme asks first whether to copy it into the project, as any change to it does.
            return match model.update(cx, |model, cx| model.change(change, cx)) {
                Ok(_) => true,
                Err(reason) => {
                    self.say(reason, cx);
                    false
                }
            };
        }
        let (Some(place), Some((words, near))) = (entry.fix.place(), &entry.anchor) else {
            return true;
        };
        let path = self.base.join(&place.file);
        let session = self.editor.read(cx).session();
        let path = file_in_session(session, &path).unwrap_or(path);
        // The block is found by its words, since typing may have moved it since the trial.
        let edits = session.file_text(&path).and_then(|text| {
            let start = trial::nearest(&text, words, *near)?;
            let here = fix_at(&entry.fix, fix::line_of(&text, start));
            fix::edits(&text, &[&here]).ok()
        });
        let Some(edits) = edits else {
            self.say(crate::t!(Key::FixGone).to_owned(), cx);
            return true;
        };
        if !self.fixing.labelled {
            self.fixing.labelled = true;
            let label = crate::versions::Label::Words(crate::t!(Key::BeforeFixes).to_owned());
            crate::versions::label_history(session, label, cx);
        }
        let result = self.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| session.apply_to_file(&path, &edits, EditKind::Discrete),
                window,
                cx,
            )
        });
        if let Err(error) = result {
            self.conversation.note(error.to_string());
        }
        true
    }

    pub(super) fn accept_fix(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.fixing.queue.iter().position(|entry| entry.id == id) else {
            return;
        };
        let entry = self.fixing.queue.remove(at);
        if self.apply_fix(&entry, window, cx) {
            self.fixing
                .stale
                .extend(entry.before.iter().chain(&entry.after).map(|shot| shot.image.clone()));
        } else {
            self.fixing.queue.insert(at, entry);
        }
        cx.notify();
    }

    pub(super) fn reject_fix(&mut self, id: u64, cx: &mut Context<Self>) {
        self.fixing.remove(id);
        cx.notify();
    }

    pub(super) fn accept_all_fixes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self.fixing.queue.iter().map(|entry| entry.id).collect();
        for id in ids {
            self.accept_fix(id, window, cx);
        }
    }

    pub(super) fn reject_all_fixes(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self.fixing.queue.iter().map(|entry| entry.id).collect();
        for id in ids {
            self.fixing.remove(id);
        }
        cx.notify();
    }

    /// Shows an entry's page in Publish.
    pub(super) fn show_fix(&mut self, id: u64, cx: &mut Context<Self>) {
        let page = (self.fixing.queue.iter())
            .find(|entry| entry.id == id)
            .and_then(|entry| entry.before.as_ref())
            .map(|shot| shot.page - 1);
        if let Some(page) = page {
            self.preview.update(cx, |preview, cx| preview.show_page(page, cx));
        }
    }
}

/// The same fix at another line of its file.
fn fix_at(fix: &Fix, line: usize) -> Fix {
    let mut moved = fix.clone();
    match &mut moved {
        Fix::PageBreak(place) | Fix::Keep(place) | Fix::Placement(place, _) => place.line = line,
        Fix::Theme { .. } => {}
    }
    moved
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::typesetter::trial::testing::{fix_at_block, short_ending, snapshot};

    #[test]
    fn the_apps_own_trial_decides_whatever_the_tool_answered() {
        let (dir, mut bench, _) = short_ending("enforced");
        let before = snapshot(&dir);
        // The tool's answer claims both were kept. The second trades the warning for a short page.
        let input = json!({"fixes": [fix_at_block("page-break", 12), fix_at_block("page-break", 32)]});
        let answer = json!({"results": [{"kept": true}, {"kept": true}], "remaining": 0}).to_string();
        let fixes = to_verify(&input, &answer);
        assert_eq!(fixes.len(), 2);
        let cancel = Cancel::default();
        let queue: Vec<Kept> = (fixes.into_iter())
            .filter_map(|fix| verified(&mut bench, fix, &cancel))
            .collect();
        assert_eq!(queue.len(), 1, "only the fix that clears a warning and causes none");
        assert_eq!(queue[0].cleared, 1);
        assert_eq!(queue[0].fix.place().map(|place| place.line), Some(69));
        assert!(queue[0].pages.is_some(), "its page is painted before and after");
        assert!(
            queue[0]
                .anchor
                .as_ref()
                .is_some_and(|(words, _)| words.starts_with("The lamps"))
        );
        // Reject all drops the queue, and the queue is all there ever was: no file changed.
        drop(queue);
        assert_eq!(snapshot(&dir), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn what_the_tool_did_not_keep_gets_no_second_trial() {
        let input = json!({"fixes": [fix_at_block("keep", 1), fix_at_block("page-break", 33), {"kind": "tighten"}]});
        let answer = json!({"results": [{"kept": false}, {"kept": true}, {"kept": true}]}).to_string();
        let fixes = to_verify(&input, &answer);
        assert_eq!(fixes.len(), 1, "the one it kept that is a fix at all");
        assert!(matches!(fixes[0], Fix::PageBreak(_)));
        assert_eq!(
            to_verify(&input, "no answer of the tool's").len(),
            2,
            "unread, so every fix is tried"
        );
    }
}
