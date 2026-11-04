//! The optional agent panel: the user's own `claude` CLI in a tab of the right rail.
//!
//! Druck spawns the installed program over `stream-json` and shows the conversation. It holds no credentials, reads
//! no tokens, and sets no authentication variable: the login is the CLI's. The panel only reports which login the CLI
//! says it has. Project `CLAUDE.md`, skills, and MCP work as in a terminal because the CLI runs with its defaults in
//! the project folder. Rendering never needs the panel, and without the CLI the panel explains how to set it up.
//!
//! **Before a public release:** whether Anthropic allows spawning the user's own CLI with a subscription login in a
//! third party app must be confirmed with Anthropic in writing. Until then the panel's setup text names API key setup
//! as the documented way and leaves the login to the CLI. See `docs/DECISIONS.md`, "Desktop beta, agent panel".
//!
//! Logic without gpui, each with tests from the recorded stream in `tests/fixtures/agent-turns.jsonl`:
//!
//! - `session`, `cli`: the child process and the CLI lookup.
//! - `conversation`: events in, rows and approvals out.
//! - `approval`, `diff`: what is shown before a tool is allowed.
//! - `changes`: hunks of a turn, found again in text that moved. This is the seam to the Persistence slice.
//! - `context`, `store`, `markdown`, `strings`: chips, the remembered session, replies, and the two languages.
//!
//! The gpui parts are `AgentView` here, `view` for drawing it, and `prompt` for the box the prompt is written in.

mod approval;
mod carry;
mod changes;
mod cli;
mod context;
mod conversation;
mod diff;
mod markdown;
mod prompt;
mod session;
mod store;
mod strings;
mod view;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{Context, Entity, Focusable, ScrollHandle, Subscription, Task, Window, px};

use crate::editor::{EditorView, SessionChanged, TextChanged};
use crate::inspector::input::{Submitted as ReasonSubmitted, TextInput};
use crate::inspector::{InspectorView, Tab};
use crate::preview::PreviewView;
use crate::source::{Edit, EditKind};
use crate::workspace::{Parts, Workspace};
use changes::Presence;
use cli::{CliState, Login};
use context::{ChipKind, Context as PromptContext, Selected};
use conversation::{Answer, Conversation, Effect, SentContext};
use session::{Event, Session as Child, SessionConfig};
use store::{Remembered, Store};

/// How often the CLI's output is looked at while a turn runs, and while it waits for the author.
const BUSY_POLL: Duration = Duration::from_millis(16);
const IDLE_POLL: Duration = Duration::from_millis(250);
/// How long the text must rest before marks and revert buttons catch up with it.
const SETTLE: Duration = Duration::from_millis(150);

/// The models the selector offers, as the CLI's own `--model` aliases. The first is the CLI's own choice.
const MODELS: [Option<&str>; 4] = [None, Some("haiku"), Some("sonnet"), Some("opus")];

/// Mounts the panel as a tab of the inspector rail.
pub fn mount(parts: &Parts, window: &mut Window, cx: &mut Context<Workspace>) {
    prompt::bind_keys(cx);
    let view = cx.new(|cx| AgentView::new(parts, window, cx));
    parts
        .inspector
        .update(cx, |inspector, _| inspector.mount(Tab::Agent, view.into()));
}

/// What looking for the CLI found.
struct Cli {
    program: PathBuf,
    state: CliState,
    login: Login,
}

/// A running CLI process and what it says.
struct Running {
    session: Child,
    events: Receiver<Event>,
    /// The CLI has said it started, so a resume was understood.
    started: bool,
}

/// Where a hunk is in the author's text now, for its row in a turn's summary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Standing {
    Applied,
    Reverted,
    Moved,
    NotOpen,
}

/// Parsed replies by row, each with the length of the text it was parsed from.
type Parsed = HashMap<usize, (usize, Rc<Vec<markdown::Block>>)>;

/// A hunk of a turn: the turn, the file's place in it, and the hunk's place in the file.
type HunkKey = (usize, usize, usize);

pub struct AgentView {
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    /// The folder or file the document was opened from. Remembered sessions are keyed by it.
    project: PathBuf,
    /// The folder the CLI works in.
    base: PathBuf,
    store: Option<Store>,
    cli: Option<Cli>,
    checking: bool,
    conversation: Conversation,
    running: Option<Running>,
    pump: Task<()>,
    model: Option<&'static str>,
    model_menu: bool,
    /// An earlier conversation of this project is kept, and nothing has been sent in this run.
    resumable: bool,
    prompt: Entity<prompt::PromptInput>,
    reason: Entity<TextInput>,
    /// The approval the reason field is open for.
    denying: Option<String>,
    context: PromptContext,
    removed: HashSet<ChipKind>,
    scroll: ScrollHandle,
    expanded: HashSet<usize>,
    /// Each reply's parsed Markdown, by row, with the length of the text it was parsed from.
    parsed: RefCell<Parsed>,
    standing: HashMap<HunkKey, Standing>,
    marks_shown: bool,
    settle: Task<()>,
    /// When the last prompt was sent, and how long the CLI took to start and to show its first text, for measurements.
    sent_at: Option<Instant>,
    first_event: Option<Duration>,
    first_text: Option<Duration>,
    _subscriptions: Vec<Subscription>,
}

impl AgentView {
    fn new(parts: &Parts, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (project, base) = {
            let session = parts.editor.read(cx).session();
            (session.location().to_owned(), session.base_dir())
        };
        let store = Store::open();
        let remembered = store.as_ref().map(|store| store.get(&project)).unwrap_or_default();
        let mut conversation = Conversation::new(base.clone());
        conversation.set_session_id(remembered.session.clone());
        let model = remembered
            .model
            .as_deref()
            .and_then(|model| MODELS.iter().copied().flatten().find(|known| *known == model));
        let prompt = cx.new(|cx| prompt::PromptInput::new(strings::PLACEHOLDER.get(), cx));
        let reason = cx.new(|cx| TextInput::new(window, cx));
        let prompt_focus = prompt.read(cx).focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&prompt, window, |view, _, _: &prompt::Submitted, window, cx| {
                view.send(window, cx);
            }),
            cx.on_focus_in(&prompt_focus, window, |view, _, cx| view.snapshot_context(cx)),
            cx.subscribe(&reason, |view, input, _: &ReasonSubmitted, cx| {
                if let Some(id) = view.denying.clone() {
                    let reason = input.read(cx).text().to_owned();
                    view.deny(&id, &reason, cx);
                }
            }),
            cx.subscribe_in(&parts.editor, window, |view, _, _: &SessionChanged, window, cx| {
                view.snapshot_context(cx);
                view.refresh_changes(window, cx);
            }),
            cx.subscribe_in(&parts.editor, window, |view, _, _: &TextChanged, window, cx| {
                view.text_changed(window, cx);
            }),
            cx.on_app_quit(|view, _| {
                // The child must not outlive the app, even when the app exits without dropping its views.
                view.running = None;
                async {}
            }),
        ];
        let mut view = Self {
            editor: parts.editor.clone(),
            preview: parts.preview.clone(),
            inspector: parts.inspector.clone(),
            project,
            base,
            store,
            cli: None,
            checking: false,
            resumable: remembered.session.is_some(),
            conversation,
            running: None,
            pump: Task::ready(()),
            model,
            model_menu: false,
            prompt,
            reason,
            denying: None,
            context: PromptContext::default(),
            removed: HashSet::new(),
            scroll: ScrollHandle::new(),
            expanded: HashSet::new(),
            parsed: RefCell::new(HashMap::new()),
            standing: HashMap::new(),
            marks_shown: false,
            settle: Task::ready(()),
            sent_at: None,
            first_event: None,
            first_text: None,
            _subscriptions: subscriptions,
        };
        view.snapshot_context(cx);
        view
    }

    // The CLI

    /// Looks for the CLI the first time the panel is on screen, so an app that never opens it never starts one.
    fn ensure_checked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cli.is_some() || self.checking {
            return;
        }
        self.checking = true;
        let lookup = cx.background_spawn(async {
            let program = cli::locate()?;
            let state = cli::check(&program);
            let login = if matches!(state, CliState::Missing) {
                Login::Unknown
            } else {
                cli::login(&program)
            };
            Some(Cli { program, state, login })
        });
        cx.spawn_in(window, async move |view, cx| {
            let found = lookup.await;
            view.update(cx, |view, cx| {
                view.checking = false;
                view.cli = Some(found.unwrap_or(Cli {
                    program: PathBuf::new(),
                    state: CliState::Missing,
                    login: Login::Unknown,
                }));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn cli_usable(&self) -> bool {
        self.cli
            .as_ref()
            .is_some_and(|cli| matches!(cli.state, CliState::Ready(_) | CliState::Untested(_)))
    }

    fn remember(&self) {
        if let Some(store) = &self.store {
            let remembered = Remembered {
                session: self.conversation.session_id().map(str::to_owned),
                model: self.model.map(str::to_owned),
            };
            if let Err(error) = store.set(&self.project, &remembered) {
                eprintln!("cannot remember the agent session: {error}");
            }
        }
    }

    /// Starts the CLI if it is not running, resuming the project's conversation.
    fn ensure_running(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        if self.running.is_some() {
            return Ok(());
        }
        let Some(cli) = &self.cli else {
            return Err(strings::cannot_start("not found"));
        };
        let config = SessionConfig {
            program: cli.program.clone(),
            cwd: self.base.clone(),
            model: self.model.map(str::to_owned),
            resume: self.conversation.session_id().map(str::to_owned),
            ..SessionConfig::default()
        };
        let (session, events) = Child::spawn(&config).map_err(|error| strings::cannot_start(&error.to_string()))?;
        self.conversation.process_started();
        self.running = Some(Running {
            session,
            events,
            started: false,
        });
        self.start_pump(window, cx);
        Ok(())
    }

    fn start_pump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pump = cx.spawn_in(window, async move |view, cx| {
            loop {
                let wait = view.update_in(cx, |view, window, cx| {
                    view.drain(window, cx);
                    view.running.is_some().then(|| {
                        if view.conversation.is_idle() {
                            IDLE_POLL
                        } else {
                            BUSY_POLL
                        }
                    })
                });
                match wait {
                    Ok(Some(wait)) => cx.background_executor().timer(wait).await,
                    _ => return,
                }
            }
        });
    }

    /// Takes what the CLI has written since the last look.
    fn drain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = &mut self.running else {
            return;
        };
        let events: Vec<Event> = std::iter::from_fn(|| running.events.try_recv().ok())
            .take(512)
            .collect();
        if events.is_empty() {
            return;
        }
        // New text follows the reader to the bottom, unless they scrolled up to read something.
        let max = self.scroll.max_offset().y;
        let follow = max <= px(1.) || -self.scroll.offset().y >= max - px(32.);
        for event in events {
            match &event {
                Event::Started { .. } => {
                    if let Some(running) = &mut self.running {
                        running.started = true;
                    }
                    if self.first_event.is_none() {
                        self.first_event = self.sent_at.map(|sent| sent.elapsed());
                    }
                }
                Event::TextDelta(_) if self.first_text.is_none() => {
                    self.first_text = self.sent_at.map(|sent| sent.elapsed());
                }
                _ => {}
            }
            let effects = self
                .conversation
                .apply(event, &|path| std::fs::read_to_string(path).ok());
            for effect in effects {
                self.take(effect, window, cx);
            }
        }
        // Thinking and other events the panel does not draw arrive often and change nothing.
        if !self.conversation.take_dirty() {
            return;
        }
        self.update_attention(cx);
        if self.conversation.is_idle() {
            self.snapshot_context(cx);
        }
        if follow {
            self.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    fn take(&mut self, effect: Effect, window: &mut Window, cx: &mut Context<Self>) {
        match effect {
            Effect::SessionId(_) => self.remember(),
            Effect::Patched(path) => self.carry_into_editor(&path, window, cx),
            Effect::Created(_) => {}
            Effect::ProcessEnded => {
                let never_started = self.running.as_ref().is_some_and(|running| !running.started);
                self.running = None;
                // A resume the CLI did not understand would fail the same way again.
                if never_started && self.conversation.session_id().is_some() {
                    self.conversation.set_session_id(None);
                    self.resumable = false;
                    self.remember();
                }
            }
        }
    }

    /// The dot on the rail while an approval waits and another tab is open.
    fn update_attention(&self, cx: &mut Context<Self>) {
        let waiting = self.conversation.approvals().next().is_some();
        self.inspector
            .update(cx, |inspector, cx| inspector.set_attention(Tab::Agent, waiting, cx));
    }

    // The author's actions

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.prompt.read(cx).text().trim().to_owned();
        if text.is_empty() || !self.conversation.is_idle() || !self.cli_usable() {
            return;
        }
        if let Err(error) = self.ensure_running(window, cx) {
            self.conversation.fail(error);
            cx.notify();
            return;
        }
        let (message, sent) = self.context.compose(&text, &self.removed);
        let context = sent.map(|text| SentContext {
            labels: self.chip_labels(),
            text,
        });
        let running = self.running.as_mut().expect("started above");
        if let Err(error) = running.session.send(&message) {
            self.running = None;
            self.conversation.fail(strings::cannot_send(&error.to_string()));
            cx.notify();
            return;
        }
        self.conversation.begin_turn(text, context);
        self.prompt.update(cx, |prompt, cx| prompt.clear(cx));
        self.removed.clear();
        self.resumable = false;
        self.sent_at = Some(Instant::now());
        self.first_event = None;
        self.first_text = None;
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(running) = &mut self.running else {
            return;
        };
        for approval in self.conversation.stop() {
            running.session.deny(&approval.request, strings::STOPPED_DENIAL).ok();
        }
        running.session.cancel().ok();
        self.update_attention(cx);
        cx.notify();
    }

    fn allow(&mut self, request_id: &str, for_session: bool, cx: &mut Context<Self>) {
        let Some(approval) = self.conversation.answer(request_id, Answer::Allowed) else {
            return;
        };
        if let Some(running) = &mut self.running {
            let result = match (&approval.session_rule, for_session) {
                (Some(rule), true) => running.session.allow_with(&approval.request, &rule.update),
                _ => running.session.allow(&approval.request),
            };
            if let Err(error) = result {
                self.conversation.fail(strings::cannot_send(&error.to_string()));
            }
        }
        self.update_attention(cx);
        cx.notify();
    }

    fn deny(&mut self, request_id: &str, reason: &str, cx: &mut Context<Self>) {
        let Some(approval) = self.conversation.answer(request_id, Answer::Denied) else {
            return;
        };
        let reason = match reason.trim() {
            "" => strings::DENIED_DEFAULT,
            reason => reason,
        };
        if let Some(running) = &mut self.running
            && let Err(error) = running.session.deny(&approval.request, reason)
        {
            self.conversation.fail(strings::cannot_send(&error.to_string()));
        }
        self.denying = None;
        self.reason
            .update(cx, |input, cx| input.set_value(String::new(), false, cx));
        self.update_attention(cx);
        cx.notify();
    }

    /// Starts over: the CLI process ends, the remembered session goes, and the rows clear.
    fn new_conversation(&mut self, cx: &mut Context<Self>) {
        self.running = None;
        self.conversation = Conversation::new(self.base.clone());
        self.resumable = false;
        self.denying = None;
        self.expanded.clear();
        self.parsed.borrow_mut().clear();
        self.standing.clear();
        self.removed.clear();
        self.remember();
        self.update_attention(cx);
        self.editor.update(cx, |editor, cx| editor.set_marks(Vec::new(), cx));
        self.marks_shown = false;
        cx.notify();
    }

    fn choose_model(&mut self, model: Option<&'static str>, cx: &mut Context<Self>) {
        self.model_menu = false;
        if self.model != model && self.conversation.is_idle() {
            self.model = model;
            // The model is fixed when the process starts, so the next prompt starts one with the new choice.
            self.running = None;
            self.remember();
        }
        cx.notify();
    }

    // Context

    /// Reads the open chapter, the selection, and the diagnostics, to offer as chips.
    fn snapshot_context(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let session = editor.session();
        let chapter = session
            .chapter(session.active())
            .map(|chapter| relative(&self.base, chapter.path()));
        let range = editor.selection_range();
        let selection = (!range.is_empty()).then(|| {
            let buffer = session.active_buffer();
            let text = editor.text();
            let last = range.end - usize::from(text[..range.end].ends_with('\n'));
            Selected {
                lines: (buffer.line_of(range.start), buffer.line_of(last.max(range.start))),
                text: text[range].to_owned(),
            }
        });
        let preview = self.preview.read(cx);
        let diagnostics = preview
            .diagnostics()
            .iter()
            .chain(preview.warnings())
            .map(|diagnostic| diagnostic.display_from(&self.base).to_string())
            .collect();
        let context = PromptContext {
            chapter,
            selection,
            diagnostics,
        };
        if context != self.context {
            self.context = context;
            cx.notify();
        }
    }

    fn chip_labels(&self) -> Vec<String> {
        self.context
            .chips()
            .into_iter()
            .filter(|kind| !self.removed.contains(kind))
            .map(|kind| self.chip_label(kind))
            .collect()
    }

    fn chip_label(&self, kind: ChipKind) -> String {
        match kind {
            ChipKind::Chapter => strings::chip_chapter(self.context.chapter.as_deref().unwrap_or_default()),
            ChipKind::Selection => {
                let (first, last) = self
                    .context
                    .selection
                    .as_ref()
                    .map_or((0, 0), |selected| selected.lines);
                strings::chip_selection(last - first + 1)
            }
            ChipKind::Diagnostics => strings::chip_diagnostics(self.context.diagnostics.len()),
        }
    }

    // The turn's changes in the editor

    /// TEMPORARY, until the Persistence slice merges files that change on disk into their buffers. See [`carry`].
    fn carry_into_editor(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(turn) = self.conversation.current_turn() else {
            return;
        };
        let Some(file) = self
            .conversation
            .changes_mut(turn)
            .and_then(|changes| changes.file_mut(path))
        else {
            return;
        };
        let carried = self.editor.update(cx, |editor, cx| {
            editor.update_session(|session| carry::carry(session, path, &mut file.hunks), window, cx)
        });
        for error in carried.errors {
            self.conversation.note(error);
        }
        if carried.conflicts > 0 {
            self.conversation
                .note(strings::merge_conflict(carried.conflicts, &relative(&self.base, path)));
        }
        self.refresh_changes(window, cx);
    }

    /// Works out where every hunk is in the text now, for the summary rows and the marks in the galley.
    fn refresh_changes(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let latest = self.conversation.latest_changes();
        if latest.is_none() && self.standing.is_empty() && !self.marks_shown {
            return;
        }
        let editor = self.editor.read(cx);
        let session = editor.session();
        let active = session.active();
        let mut standing = HashMap::new();
        let mut marks = Vec::new();
        for turn in 0..=self.conversation.current_turn().unwrap_or(0) {
            let Some(changes) = self.conversation.changes_mut(turn) else {
                continue;
            };
            for (file_at, file) in changes.files.iter_mut().enumerate() {
                for (hunk_at, tracked) in file.hunks.iter_mut().enumerate() {
                    let state = match tracked.chapter.filter(|id| session.chapter(*id).is_some()) {
                        None => Standing::NotOpen,
                        Some(id) => match tracked.presence(session.buffer(id).text()) {
                            Presence::Applied(place) => {
                                if Some(turn) == latest && id == active {
                                    marks.push(place.range);
                                }
                                Standing::Applied
                            }
                            Presence::Absent(_) => Standing::Reverted,
                            Presence::Moved => Standing::Moved,
                        },
                    };
                    standing.insert((turn, file_at, hunk_at), state);
                }
            }
        }
        self.standing = standing;
        self.marks_shown = !marks.is_empty();
        self.editor.update(cx, |editor, cx| editor.set_marks(marks, cx));
        cx.notify();
    }

    /// Typing moves text under the marks. They hide at once and come back when the text rests.
    fn text_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.standing.is_empty() && !self.marks_shown {
            return;
        }
        if self.marks_shown {
            self.marks_shown = false;
            self.editor.update(cx, |editor, cx| editor.set_marks(Vec::new(), cx));
        }
        self.settle = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(SETTLE).await;
            view.update_in(cx, |view, window, cx| view.refresh_changes(window, cx))
                .ok();
        });
    }

    /// Puts the lines from before a hunk back, as one undo step in the editor.
    fn revert(&mut self, key: HunkKey, window: &mut Window, cx: &mut Context<Self>) {
        let (turn, file_at, hunk_at) = key;
        let Some(tracked) = self
            .conversation
            .changes_mut(turn)
            .and_then(|changes| changes.files.get_mut(file_at))
            .and_then(|file| file.hunks.get_mut(hunk_at))
        else {
            return;
        };
        let Some(id) = tracked.chapter else {
            return;
        };
        let text = self.editor.read(cx).session().buffer(id).text().to_owned();
        let Some(replacement) = tracked.revert(&text) else {
            return;
        };
        let edit = Edit::new(replacement.range, replacement.text);
        let result = self.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| session.edit_chapter(id, vec![edit], EditKind::Discrete),
                window,
                cx,
            )
        });
        if let Err(error) = result {
            self.conversation.note(error.to_string());
        }
        self.refresh_changes(window, cx);
    }

    /// Opens the chapter a hunk is in and puts the caret on it.
    fn reveal(&mut self, key: HunkKey, window: &mut Window, cx: &mut Context<Self>) {
        let (turn, file_at, hunk_at) = key;
        let Some(tracked) = self
            .conversation
            .changes_mut(turn)
            .and_then(|changes| changes.files.get_mut(file_at))
            .and_then(|file| file.hunks.get_mut(hunk_at))
        else {
            return;
        };
        let Some(id) = tracked.chapter else {
            return;
        };
        let text = self.editor.read(cx).session().buffer(id).text().to_owned();
        let at = match tracked.presence(&text) {
            Presence::Applied(place) | Presence::Absent(place) => place.range.start,
            Presence::Moved => return,
        };
        self.editor
            .update(cx, |editor, cx| editor.show_place(id, at, window, cx));
        self.refresh_changes(window, cx);
    }
}

/// `path` relative to the project folder where it lies inside it.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).display().to_string()
}
