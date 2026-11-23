//! "Fix the proof": one turn in which the agent tries typesetting fixes through the trial tool, and the queue of kept
//! fixes the author reviews.
//!
//! The agent's word keeps nothing. Each fix the tool says it kept is tried again here, on a copy of the book as the
//! session has it, and enters the queue only if that trial keeps it too. The trials run on a thread of their own with a
//! typesetter of their own, so neither typing nor Publish's pages wait for them. Nothing is written to a chapter or the
//! theme before the author accepts an entry, and an accepted entry goes in as any edit does. When an entry leaves the
//! queue, or the theme changes, the entries left are tried again on the book as it is then, for pages and counts that
//! say what accepting each would do now.

use std::path::{Path, PathBuf};
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

/// What a fix did to the warnings, as its entry says it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Counts {
    /// The warnings it clears, with those a page break excuses.
    pub cleared: usize,
    /// Of those, the ones on the page a page break ends, which may end short after a break the author set.
    pub excused: usize,
    /// The warnings it causes, which only a theme value may.
    pub caused: usize,
    /// The pages those are on, from 1, each once.
    pub caused_pages: Vec<usize>,
}

impl Counts {
    fn of(outcome: &trial::Outcome) -> Self {
        let mut caused_pages: Vec<usize> = outcome.caused.iter().filter_map(|warning| warning.page).collect();
        caused_pages.sort_unstable();
        caused_pages.dedup();
        Self {
            cleared: outcome.cleared.len() + outcome.excused.len(),
            excused: outcome.excused.len(),
            caused: outcome.caused.len(),
            caused_pages,
        }
    }
}

/// A kept fix waiting for the author.
pub(super) struct Entry {
    pub id: u64,
    pub fix: Fix,
    pub counts: Counts,
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
            Fix::TablePlacement(_, Placement::Top) => crate::t!(Key::FixTableTop).to_owned(),
            Fix::TablePlacement(_, Placement::Bottom) => crate::t!(Key::FixTableBottom).to_owned(),
            Fix::TablePlacement(_, Placement::Here) => crate::t!(Key::FixTableHere).to_owned(),
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
    counts: Counts,
    pages: Option<(Painted, Painted)>,
    anchor: Option<(String, usize)>,
}

/// The app's copy of the book's text for the trial tool in the CLI's process, removed when the run ends.
struct BookCopy(PathBuf);

/// A book copy's file name starts with this and the app's process id: `druck-book-{pid}-{nanos}.json`.
const COPY_PREFIX: &str = "druck-book-";

impl BookCopy {
    fn write(book: &Book) -> std::io::Result<Self> {
        let made = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let file = std::env::temp_dir().join(format!("{COPY_PREFIX}{}-{made}.json", std::process::id()));
        book.write_copy(&file)?;
        Ok(Self(file))
    }
}

/// Removes the book copies that apps which ended without their run's end, as in a crash, left in the temp folder. A
/// copy whose process still runs as Druck stays, since its run may be under way. For the start of the app.
pub fn sweep_book_copies() {
    sweep_copies_in(&std::env::temp_dir(), runs_as_druck);
}

fn sweep_copies_in(dir: &Path, running: impl Fn(u32) -> bool) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if let Some(pid) = entry.file_name().to_str().and_then(copy_pid)
            && !running(pid)
        {
            std::fs::remove_file(entry.path()).ok();
        }
    }
}

/// The process id in a book copy's name, for exactly the names [`BookCopy::write`] gives: a number for the process and
/// a number for the time, and nothing else.
fn copy_pid(name: &str) -> Option<u32> {
    let (pid, made) = name.strip_prefix(COPY_PREFIX)?.strip_suffix(".json")?.split_once('-')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    (digits(pid) && digits(made)).then(|| pid.parse().ok()).flatten()
}

/// Whether the process `pid` runs and is Druck, by its executable's name: `Druck` in the bundle, `druck-desktop` from
/// cargo.
#[cfg(target_os = "macos")]
fn runs_as_druck(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    let mut name = [0_u8; 256];
    // SAFETY: proc_name writes at most the buffer's length and returns how much it wrote, 0 for no such process.
    let written = unsafe { libc::proc_name(pid, name.as_mut_ptr().cast(), name.len() as u32) };
    let name = &name[..usize::try_from(written).unwrap_or(0)];
    String::from_utf8_lossy(name).to_lowercase().contains("druck")
}

/// Elsewhere no process is looked up, so every copy stays.
#[cfg(not(target_os = "macos"))]
fn runs_as_druck(_pid: u32) -> bool {
    true
}

impl Drop for BookCopy {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
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
    copy: BookCopy,
    _reports: Task<()>,
}

/// The entries tried again after one left the queue. Dropping it drops what it has not reported yet.
struct Repaint {
    cancel: Cancel,
    _reports: Task<()>,
}

impl Drop for Repaint {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// The typesetter's state: the run, and the queue.
#[derive(Default)]
pub(super) struct Fixing {
    run: Option<Run>,
    queue: Vec<Entry>,
    repaint: Option<Repaint>,
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

    /// The copy of the book the running turn's trial tool reads.
    pub fn book_copy(&self) -> Option<PathBuf> {
        self.run.as_ref().map(|run| run.copy.0.clone())
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

/// The app's own trial of a fix, which alone decides whether it enters the queue: it must pass its rule on top of the
/// fixes kept before it.
fn verified(bench: &mut Bench, fix: Fix, cancel: &Cancel) -> Option<Kept> {
    let tried = bench.try_fix(fix, cancel).ok().filter(Tried::kept)?;
    Some(kept_of(bench, tried))
}

fn kept_of(bench: &Bench, tried: Tried) -> Kept {
    let pages = tried
        .shown
        .and_then(|(before, after)| Some((paint(&tried.before, before)?, paint(&tried.after, after)?)));
    let anchor = tried.fix.place().and_then(|place| {
        let (_, text) = bench.book().file(&place.file)?;
        let block = fix::block_of(text, &tried.fix).ok()?;
        Some((text[block.clone()].lines().next()?.to_owned(), block.start))
    });
    Kept {
        counts: Counts::of(&tried.outcome),
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
        // The agent and the trials work on what is on screen, also where the agent reads a file.
        self.after_saving(window, cx, Self::start_fixing);
    }

    fn start_fixing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The save may have waited, and something else started meanwhile.
        if !self.conversation.is_idle() || self.pass_run.is_some() || self.fixing.is_running() {
            return;
        }
        let book = Book::of(self.editor.read(cx).session());
        let copy = match BookCopy::write(&book) {
            Ok(copy) => copy,
            Err(error) => return self.say(strings::fix_not_started(&error.to_string()), cx),
        };
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
            copy,
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
                        counts: kept.counts,
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
        let (Some(place), Some(_)) = (entry.fix.place(), &entry.anchor) else {
            return true;
        };
        let path = self.base.join(&place.file);
        self.editor.update(cx, |editor, cx| editor.commit_marked(window, cx));
        let session = self.editor.read(cx).session();
        let path = file_in_session(session, &path).unwrap_or(path);
        let edits = session.file_text(&path).and_then(|text| {
            let here = located(entry, &text)?;
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
        if self.accept_one(id, window, cx) {
            self.repaint_fixes(window, cx);
        }
    }

    /// Accepts one entry. Returns whether the book is changed now: a theme value is written later, and the theme's
    /// change brings the repaint.
    fn accept_one(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(at) = self.fixing.queue.iter().position(|entry| entry.id == id) else {
            return false;
        };
        let entry = self.fixing.queue.remove(at);
        cx.notify();
        if !self.apply_fix(&entry, window, cx) {
            self.fixing.queue.insert(at, entry);
            return false;
        }
        self.fixing
            .stale
            .extend(entry.before.iter().chain(&entry.after).map(|shot| shot.image.clone()));
        !matches!(entry.fix, Fix::Theme { .. })
    }

    pub(super) fn reject_fix(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.fixing.remove(id).is_some() {
            self.repaint_fixes(window, cx);
        }
        cx.notify();
    }

    pub(super) fn accept_all_fixes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self.fixing.queue.iter().map(|entry| entry.id).collect();
        let mut changed = false;
        for id in ids {
            changed |= self.accept_one(id, window, cx);
        }
        // What stays is a theme value that asks first, or one that could not be written.
        if changed {
            self.repaint_fixes(window, cx);
        }
    }

    pub(super) fn reject_all_fixes(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self.fixing.queue.iter().map(|entry| entry.id).collect();
        for id in ids {
            self.fixing.remove(id);
        }
        self.fixing.repaint = None;
        cx.notify();
    }

    /// The theme was written: the entries left are tried again on it.
    pub(super) fn theme_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.repaint_fixes(window, cx);
    }

    /// Tries every waiting entry again on the book as it is now, in the queue's order and each on top of those before
    /// it that still pass their rule, as the run did. Each entry's pages and counts are replaced as its trial ends.
    fn repaint_fixes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.fixing.repaint = None;
        if self.fixing.queue.is_empty() {
            return;
        }
        let book = Book::of(self.editor.read(cx).session());
        let jobs: Vec<(u64, Fix)> = (self.fixing.queue.iter())
            .filter_map(|entry| {
                let text = entry
                    .fix
                    .place()
                    .and_then(|place| book.file(&place.file))
                    .map(|(_, text)| text);
                let fix = match text {
                    Some(text) => located(entry, text)?,
                    None => entry.fix.clone(),
                };
                Some((entry.id, fix))
            })
            .collect();
        let cancel = Cancel::default();
        let stop = cancel.clone();
        let (reports, mut incoming) = futures::channel::mpsc::unbounded::<(u64, Kept)>();
        std::thread::Builder::new()
            .name("druck-repaint".into())
            .spawn(move || {
                let mut bench = Bench::new(book);
                for (id, fix) in jobs {
                    let Ok(tried) = bench.try_fix(fix, &stop) else {
                        continue;
                    };
                    if reports.unbounded_send((id, kept_of(&bench, tried))).is_err() {
                        return;
                    }
                }
            })
            .expect("the system starts a thread");
        let task = cx.spawn_in(window, async move |view, cx| {
            while let Some((id, kept)) = incoming.next().await {
                if view.update(cx, |view, cx| view.repainted(id, kept, cx)).is_err() {
                    return;
                }
            }
        });
        self.fixing.repaint = Some(Repaint { cancel, _reports: task });
    }

    fn repainted(&mut self, id: u64, kept: Kept, cx: &mut Context<Self>) {
        let Fixing { queue, stale, .. } = &mut self.fixing;
        let Some(entry) = queue.iter_mut().find(|entry| entry.id == id) else {
            return;
        };
        stale.extend(entry.before.iter().chain(&entry.after).map(|shot| shot.image.clone()));
        (entry.before, entry.after) = kept.pages.map(|(before, after)| (shot(before), shot(after))).unzip();
        entry.fix = kept.fix;
        entry.counts = kept.counts;
        entry.anchor = kept.anchor;
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

/// An entry's fix at the line its block starts on in `text`, its file as it is now. The block is found by its words,
/// since typing or another accepted fix may have moved it since the trial. A theme value stands nowhere.
fn located(entry: &Entry, text: &str) -> Option<Fix> {
    let Some((words, near)) = &entry.anchor else {
        return Some(entry.fix.clone());
    };
    let start = trial::nearest(text, words, *near)?;
    Some(entry.fix.at_line(fix::line_of(text, start)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::typesetter::trial::testing::{fix_at_block, short_ending, snapshot};

    #[test]
    fn a_copy_whose_app_is_gone_is_swept_and_a_running_ones_stays() {
        let dir = std::env::temp_dir().join(format!("druck-sweep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let names = [
            "druck-book-11-5.json",
            "druck-book-22-5.json",
            "notes.json",
            "druck-book-11-draft.json",
            "druck-book-11-5-6.json",
            "druck-book-+11-5.json",
            "druck-book-11-.json",
        ];
        for name in names {
            std::fs::write(dir.join(name), "{}").expect("file");
        }
        sweep_copies_in(&dir, |pid| pid == 22);
        let left: Vec<bool> = names.iter().map(|name| dir.join(name).exists()).collect();
        assert_eq!(left, [false, true, true, true, true, true, true]);
        assert!(runs_as_druck(std::process::id()), "the test binary is druck_desktop");
        std::fs::remove_dir_all(&dir).ok();
    }

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
        assert_eq!(queue[0].counts.cleared, 1);
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
    fn an_entry_counts_what_a_break_excuses_and_names_the_pages_of_what_it_causes() {
        let warning = |kind, page| trial::Warning {
            kind,
            at: None,
            page: Some(page),
            first_block: None,
            message: String::new(),
        };
        let outcome = trial::Outcome {
            cleared: vec![warning("heading-at-foot", 3)],
            excused: vec![warning("page-short", 3), warning("page-uneven", 4)],
            caused: vec![
                warning("loose-line", 9),
                warning("loose-line", 4),
                warning("short-last-line", 9),
            ],
            ..trial::Outcome::default()
        };
        let counts = Counts::of(&outcome);
        assert_eq!((counts.cleared, counts.excused, counts.caused), (3, 2, 3));
        assert_eq!(counts.caused_pages, [4, 9]);
        let lines = strings::fix_counts(&counts);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains('3') && lines[0].contains('2'));
        assert!(lines[1].contains("4, 9"));
        let clean = Counts {
            cleared: 1,
            ..Counts::default()
        };
        assert_eq!(
            strings::fix_counts(&clean).len(),
            1,
            "a directive causes nothing to name"
        );
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
