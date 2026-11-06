//! The writing partner: the user's own `claude` CLI, off until enabled in Settings, and on `Cmd+J` when it is on.
//!
//! With a selection `Cmd+J` opens a prompt at the selection, and without one a small chat card over the text. In Publish
//! the same conversation is a tab of the rail. The CLI writes files with its normal tools, and its changes show in the
//! text for review. Advice comes as margin notes through a small tool the app serves itself, see `note_tool`.
//!
//! Druck spawns the installed program over `stream-json`. It holds no credentials, reads no tokens, and sets no
//! authentication variable: the login is the CLI's, and the app only reports which login the CLI says it has. Project
//! `CLAUDE.md`, skills, and MCP work as in a terminal because the CLI runs with its defaults in the project folder. With
//! the setting off, nothing is spawned and nothing shows.
//!
//! **Before a public release documents a subscription login:** whether Anthropic allows spawning the user's own CLI with
//! one in a third party app must be confirmed with Anthropic in writing. Until then the setup text names an API key as
//! the way and leaves the login to the CLI. See `docs/DECISIONS.md`, "Desktop beta, agent panel".
//!
//! Logic without gpui, each with tests, many from the recorded stream in `tests/fixtures/agent-turns.jsonl`:
//!
//! - `session`, `cli`: the child process and the CLI lookup.
//! - `conversation`: events in, rows and approvals out.
//! - `approval`, `diff`: what is shown before a tool is allowed.
//! - `changes`: hunks of a turn from what Persistence saw on disk, found again in text that moved.
//! - `notes`, `note_tool`: margin notes and the tool that leaves them.
//! - `context`, `store`, `markdown`, `strings`: chips, the remembered session, replies, and the two languages.
//!
//! The gpui parts are `AgentView` here, `review` for what it does with changes and notes, `view` for drawing the
//! conversation, `card` for the layers around it, `prompt` for the box a prompt is written in, and `probe` for scripted
//! runs. The text draws review itself, in `editor/review.rs`.

mod approval;
mod card;
mod changes;
mod cli;
mod context;
mod conversation;
mod diff;
mod markdown;
pub mod note_tool;
mod notes;
#[cfg(any(debug_assertions, feature = "measure"))]
mod probe;
mod prompt;
mod review;
mod session;
mod store;
mod strings;
mod view;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Context, Entity, Focusable, Global, ScrollHandle, Subscription, Task, WeakEntity, Window,
};

use crate::editor::review::{NoteClicked, ReviewClicked};
use crate::editor::{EditorView, SessionChanged, TextChanged};
use crate::inspector::input::{Submitted as ReasonSubmitted, TextInput};
use crate::inspector::{InspectorView, Tab};
use crate::persist::PersistView;
use crate::preview::PreviewView;
use crate::session::{ChapterId, Session as Document};
use crate::settings;
use crate::workspace::{Mode, ModeChanged, Parts, Slots, Workspace};
use cli::{CliState, Login};
use context::{ChipKind, Context as PromptContext, Selected};
use conversation::{Answer, Conversation, Effect, SentContext};
use notes::Notes;
use session::{Event, Session as Child, SessionConfig};
use store::{Remembered, Store};
use strings::Key;

pub use note_tool::FLAG as NOTE_TOOL_FLAG;

/// How often the CLI's output is looked at while a turn runs, and while it waits for the author.
const BUSY_POLL: Duration = Duration::from_millis(16);
const IDLE_POLL: Duration = Duration::from_millis(250);
/// How long the text must rest before review catches up with it.
const SETTLE: Duration = Duration::from_millis(150);
/// How long after a turn a change on disk still counts as the turn's: a save running at its end holds the last look back.
const TURN_GRACE: Duration = Duration::from_millis(500);

/// The models the selector offers, as the CLI's own `--model` aliases. The first is the CLI's own choice.
const MODELS: [Option<&str>; 4] = [None, Some("haiku"), Some("sonnet"), Some("opus")];

/// The view `Cmd+J` and [`unreviewed_changes`] go to: the open document's.
struct Current {
    view: WeakEntity<AgentView>,
    window: AnyWindowHandle,
}

impl Global for Current {}

/// Mounts the conversation as a tab of the inspector rail, and what shows over the text.
pub fn mount(
    parts: &Parts,
    persist: Entity<PersistView>,
    slots: &mut Slots,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    prompt::bind_keys(cx);
    let workspace = cx.entity();
    let view = cx.new(|cx| AgentView::new(parts, persist, workspace, window, cx));
    cx.set_global(Current {
        view: view.downgrade(),
        window: window.window_handle(),
    });
    card::mount(&view, slots, window, cx);
    parts
        .inspector
        .update(cx, |inspector, _| inspector.mount(Tab::Agent, view.into()));
}

#[cfg(any(debug_assertions, feature = "measure"))]
pub fn apply_gate(window: &mut Window, cx: &mut Context<Workspace>) {
    if let Some(view) = cx.try_global::<Current>().and_then(|current| current.view.upgrade()) {
        probe::start(&view, window, cx);
    }
}

/// `Cmd+J`: a prompt at the selection, the chat card, or in Publish the rail's tab. Closes what is open.
pub fn ask(cx: &mut App) {
    let Some(current) = cx.try_global::<Current>() else {
        return;
    };
    let (view, window) = (current.view.clone(), current.window);
    // An action runs inside its window's update, which cannot be entered again.
    cx.defer(move |cx| {
        window
            .update(cx, |_, window, cx| {
                view.update(cx, |view, cx| view.ask(window, cx)).ok();
            })
            .ok();
    });
}

/// A chapter, or another file of the project, with agent changes the author has neither accepted nor rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreviewed {
    /// `None` for a file that is not a chapter, such as `druck.yaml`.
    pub chapter: Option<ChapterId>,
    pub path: PathBuf,
    /// The chapter's title or the file's name, for saying which.
    pub name: String,
    pub count: usize,
}

/// What has unreviewed agent changes and how many, as of the last time the text rested. Empty with the agent off. Export
/// and commit name these and do not block.
pub fn unreviewed_changes(cx: &App) -> Vec<Unreviewed> {
    (cx.try_global::<Current>())
        .and_then(|current| current.view.upgrade())
        .map(|view| view.read(cx).unreviewed.clone())
        .unwrap_or_default()
}

/// What looking for the CLI found.
struct Cli {
    program: PathBuf,
    /// The search path the CLI runs with.
    path: OsString,
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

/// What the project held when a turn began, to tell what the turn made.
struct TurnStart {
    files: HashSet<PathBuf>,
    manifest: Option<String>,
    at: SystemTime,
}

pub struct AgentView {
    workspace: WeakEntity<Workspace>,
    mode: Mode,
    /// The setting, as last heard. Off: nothing shows and nothing runs.
    enabled: bool,
    /// The chat card over the text.
    chat_open: bool,
    /// The prompt at the selection.
    selection_prompt: Entity<prompt::PromptInput>,
    /// The chat prompt holds words to send, which puts Send in the accent.
    can_send: bool,
    selection_card: Option<Entity<card::SelectionCard>>,
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    persist: Entity<PersistView>,
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
    /// Where each hunk stands in the text, by its id.
    standing: HashMap<u64, Standing>,
    settle: Task<()>,
    /// What has changes to review, as of the last look.
    unreviewed: Vec<Unreviewed>,
    notes: Notes,
    /// The note a reply in the prompt is about.
    replying: Option<u64>,
    /// Notes whose suggestion was applied, until an undo brings them back or the turn of events makes that moot.
    applied_notes: Vec<review::AppliedNote>,
    /// Changes the author took back since the last turn, for the next one to hear.
    rejections: Vec<String>,
    /// The setup lines were opened after they folded.
    setup_open: bool,
    turn_start: Option<TurnStart>,
    approvals_seen: usize,
    /// The turn that files changing on disk belong to, from its prompt until shortly after its end.
    collecting: Option<usize>,
    collecting_end: Task<()>,
    /// When the last prompt was sent, and how long the CLI took to start and to show its first text, for measurements.
    sent_at: Option<Instant>,
    first_event: Option<Duration>,
    first_text: Option<Duration>,
    _subscriptions: Vec<Subscription>,
}

impl AgentView {
    fn new(
        parts: &Parts,
        persist: Entity<PersistView>,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (project, base) = {
            let session = parts.editor.read(cx).session();
            (session.location().to_owned(), session.base_dir())
        };
        let store = Store::open();
        let remembered = match store.as_ref().map(|store| store.get(&project)) {
            Some(Err(error)) => {
                let kind = crate::persist::sync::Problem::AgentSessionUnread;
                crate::persist::report(&base, kind, Some(error.to_string()), cx);
                Remembered::default()
            }
            found => found.and_then(Result::ok).unwrap_or_default(),
        };
        let mut conversation = Conversation::new(base.clone());
        conversation.set_session_id(remembered.session.clone());
        let model = remembered
            .model
            .as_deref()
            .and_then(|model| MODELS.iter().copied().flatten().find(|known| *known == model));
        let prompt = cx.new(|cx| prompt::PromptInput::new(crate::t!(Key::Placeholder), cx));
        let selection_prompt = cx.new(|cx| prompt::PromptInput::one_line(crate::t!(Key::SelectionPlaceholder), cx));
        let reason = cx.new(|cx| TextInput::new(window, cx));
        let prompt_focus = prompt.read(cx).focus_handle(cx);
        let selection_focus = selection_prompt.read(cx).focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&prompt, window, |view, _, _: &prompt::Submitted, window, cx| {
                view.send(window, cx);
            }),
            cx.subscribe_in(&prompt, window, |view, _, _: &prompt::Dismissed, window, cx| {
                view.close_chat(window, cx);
            }),
            cx.observe(&prompt, |view, prompt, cx| {
                let can_send = !prompt.read(cx).text().trim().is_empty();
                if can_send != view.can_send {
                    view.can_send = can_send;
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &selection_prompt,
                window,
                |view, _, _: &prompt::Submitted, window, cx| {
                    view.send_selection(window, cx);
                },
            ),
            cx.subscribe_in(
                &selection_prompt,
                window,
                |view, _, _: &prompt::Dismissed, window, cx| {
                    view.close_selection_prompt(window, cx);
                },
            ),
            cx.on_focus_in(&prompt_focus, window, |view, _, cx| view.snapshot_context(cx)),
            // A click elsewhere closes the prompt at the selection, and leaves its words in it for next time. Another
            // app coming to the front counts as a blur too, and changes nothing here.
            cx.on_blur(&selection_focus, window, |view, window, cx| {
                if window.is_window_active() {
                    view.close_selection_prompt(window, cx);
                }
            }),
            cx.subscribe(&reason, |view, input, _: &ReasonSubmitted, cx| {
                if let Some(id) = view.denying.clone() {
                    let reason = input.read(cx).text().to_owned();
                    view.deny(&id, &reason, cx);
                }
            }),
            cx.subscribe_in(&parts.editor, window, |view, _, _: &SessionChanged, window, cx| {
                view.snapshot_context(cx);
                view.take_in_new_files(cx);
                view.refresh_changes(window, cx);
            }),
            cx.subscribe_in(
                &parts.editor,
                window,
                |view, _, ReviewClicked(id, action), window, cx| {
                    view.review_clicked(*id, *action, window, cx);
                },
            ),
            cx.subscribe_in(&parts.editor, window, |view, _, NoteClicked(id, action), window, cx| {
                view.note_clicked(*id, *action, window, cx);
            }),
            cx.subscribe_in(&workspace, window, |view, _, ModeChanged(mode), window, cx| {
                view.mode_changed(*mode, window, cx);
            }),
            cx.observe_global_in::<settings::Store>(window, |view, window, cx| view.settings_changed(window, cx)),
            cx.subscribe_in(&parts.editor, window, |view, _, _: &TextChanged, window, cx| {
                view.text_changed(window, cx);
            }),
            cx.subscribe_in(&persist, window, Self::file_changed),
            cx.on_app_quit(|view, _| {
                // The child must not outlive the app, even when the app exits without dropping its views.
                view.running = None;
                async {}
            }),
        ];
        let notes = Notes::open(&base);
        let mut view = Self {
            workspace: workspace.downgrade(),
            mode: Mode::Write,
            enabled: settings::get(cx).agent,
            chat_open: false,
            selection_prompt,
            can_send: false,
            selection_card: None,
            editor: parts.editor.clone(),
            preview: parts.preview.clone(),
            inspector: parts.inspector.clone(),
            persist,
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
            settle: Task::ready(()),
            unreviewed: Vec::new(),
            notes,
            replying: None,
            applied_notes: Vec::new(),
            rejections: Vec::new(),
            setup_open: false,
            turn_start: None,
            approvals_seen: 0,
            collecting: None,
            collecting_end: Task::ready(()),
            sent_at: None,
            first_event: None,
            first_text: None,
            _subscriptions: subscriptions,
        };
        view.snapshot_context(cx);
        view.refresh_changes(window, cx);
        view
    }

    // The CLI

    /// Looks for the CLI the first time it is asked for, so an app that never does never starts one.
    fn ensure_checked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled || self.cli.is_some() || self.checking {
            return;
        }
        self.checking = true;
        let lookup = cx.background_spawn(async {
            let path = cli::search_path();
            let program = cli::locate(&path)?;
            let state = cli::check(&program, &path);
            let login = if matches!(state, CliState::Missing) {
                Login::Unknown
            } else {
                cli::login(&program, &path)
            };
            Some(Cli {
                program,
                path,
                state,
                login,
            })
        });
        cx.spawn_in(window, async move |view, cx| {
            let found = lookup.await;
            view.update(cx, |view, cx| {
                view.checking = false;
                view.cli = Some(found.unwrap_or(Cli {
                    program: PathBuf::new(),
                    path: OsString::new(),
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

    fn remember(&self, cx: &mut App) {
        if let Some(store) = &self.store {
            let remembered = Remembered {
                session: self.conversation.session_id().map(str::to_owned),
                model: self.model.map(str::to_owned),
            };
            let error = store
                .set(&self.project, &remembered)
                .err()
                .map(|error| error.to_string());
            self.persist.update(cx, |persist, cx| {
                persist.report(&self.base, crate::persist::sync::Problem::AgentSession, error, cx);
            });
        }
    }

    /// Starts the CLI if it is not running, resuming the project's conversation.
    fn ensure_running(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        if self.running.is_some() {
            return Ok(());
        }
        let Some(cli) = &self.cli else {
            return Err(strings::cannot_start(crate::t!(strings::Key::CliNotFound)));
        };
        let config = SessionConfig {
            program: cli.program.clone(),
            path: Some(cli.path.clone()),
            cwd: self.base.clone(),
            model: self.model.map(str::to_owned),
            resume: self.conversation.session_id().map(str::to_owned),
            extra_args: note_tool::cli_args(),
            ..SessionConfig::default()
        };
        let (session, events) = Child::spawn(&config).map_err(|error| strings::cannot_start(&error.to_string()))?;
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
        let follow = max <= crate::tokens::HAIRLINE || -self.scroll.offset().y >= max - crate::tokens::FOLLOW_ROOM;
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
            Effect::SessionId(_) => self.remember(cx),
            Effect::Patched(path) => self.forget_patch_of_open_file(&path, cx),
            Effect::Created(_) => self.take_in_new_files(cx),
            Effect::NoteLeft(input) => self.note_left(&input, window, cx),
            Effect::TurnEnded => self.end_turn(window, cx),
            Effect::ProcessEnded => {
                let never_started = self.running.as_ref().is_some_and(|running| !running.started);
                self.running = None;
                self.persist.update(cx, |persist, _| persist.set_agent_working(false));
                // A resume the CLI did not understand would fail the same way again.
                if never_started && self.conversation.session_id().is_some() {
                    self.conversation.set_session_id(None);
                    self.resumable = false;
                    self.remember(cx);
                }
            }
        }
    }

    /// A waiting approval is seen from anywhere: the chat card opens over the text, or in Publish a dot marks the Write
    /// switch and the rail's tab. The keys stay where they are.
    fn update_attention(&mut self, cx: &mut Context<Self>) {
        let waiting = self.conversation.approvals().count();
        if waiting > self.approvals_seen && self.mode == Mode::Write {
            self.chat_open = true;
        }
        self.approvals_seen = waiting;
        self.inspector
            .update(cx, |inspector, cx| inspector.set_attention(Tab::Agent, waiting > 0, cx));
        let elsewhere = waiting > 0 && self.mode != Mode::Write;
        self.workspace
            .update(cx, |workspace, cx| workspace.set_write_attention(elsewhere, cx))
            .ok();
    }

    fn mode_changed(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        if mode == Mode::Publish {
            self.close_selection_prompt(window, cx);
        }
        if mode == Mode::Write && self.conversation.approvals().next().is_some() {
            self.chat_open = true;
        }
        self.update_attention(cx);
        cx.notify();
    }

    fn settings_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The language may be what changed.
        self.prompt
            .update(cx, |prompt, cx| prompt.set_placeholder(crate::t!(Key::Placeholder), cx));
        self.selection_prompt.update(cx, |prompt, cx| {
            prompt.set_placeholder(crate::t!(Key::SelectionPlaceholder), cx)
        });
        let enabled = settings::get(cx).agent;
        if enabled == self.enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.running = None;
            self.chat_open = false;
            self.close_selection_prompt(window, cx);
        }
        self.refresh_changes(window, cx);
        self.update_attention(cx);
        cx.notify();
    }

    /// `Cmd+J`.
    fn ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.ensure_checked(window, cx);
        self.snapshot_context(cx);
        if self.mode == Mode::Publish {
            self.inspector
                .update(cx, |inspector, cx| inspector.show(Tab::Agent, cx));
            window.focus(&self.prompt.read(cx).focus_handle(cx), cx);
            return;
        }
        if self.chat_open || self.selection_card.is_some() {
            self.close_chat(window, cx);
            self.close_selection_prompt(window, cx);
            return;
        }
        let selected = !self.editor.read(cx).selection_range().is_empty();
        let missing = self.cli.is_some() && !self.cli_usable();
        let waiting = self.conversation.approvals().next().is_some();
        if selected && !missing && !waiting {
            self.open_selection_prompt(window, cx);
        } else {
            self.open_chat(window, cx);
        }
    }

    fn open_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_open = true;
        window.focus(&self.prompt.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Closes the card. A running turn goes on, and its results stay.
    fn close_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_open {
            self.chat_open = false;
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
            cx.notify();
        }
    }

    fn open_selection_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let input = self.selection_prompt.clone();
        let card = cx.new(|cx| card::SelectionCard::new(this, input.clone(), cx));
        self.editor.update(cx, |editor, cx| {
            editor.set_selection_prompt(Some(card.clone().into()), cx)
        });
        self.selection_card = Some(card);
        window.focus(&input.read(cx).focus_handle(cx), cx);
    }

    fn close_selection_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_card.take().is_some() {
            self.editor
                .update(cx, |editor, cx| editor.set_selection_prompt(None, cx));
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
        }
    }

    // The author's actions

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.prompt.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let (message, sent) = self.compose(&text, cx);
        let context = sent.map(|text| SentContext {
            labels: self.chip_labels(),
            text,
        });
        if self.start_turn(text, message, context, window, cx) {
            self.prompt.update(cx, |prompt, cx| prompt.clear(cx));
            self.removed.clear();
            self.replying = None;
        }
    }

    /// Sends the prompt at the selection, which works on the selected text only.
    fn send_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.selection_prompt.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.snapshot_context(cx);
        let (message, sent) = self.context.compose(&text, &HashSet::from([ChipKind::Diagnostics]));
        let message = format!("{message}\n\n{}", strings::SELECTION_ONLY);
        let labels = self.chip_labels();
        let context = sent.map(|text| SentContext { labels, text });
        if self.start_turn(text, message, context, window, cx) {
            self.selection_prompt.update(cx, |prompt, cx| prompt.clear(cx));
            let end = self.editor.read(cx).selection_range().end;
            self.close_selection_prompt(window, cx);
            // The text under the selection is about to change, and a key must not type over it.
            self.editor.update(cx, |editor, cx| editor.jump_to(end, window, cx));
        }
    }

    /// The prompt with what its chips carry, and a note it replies to.
    fn compose(&self, text: &str, _: &Context<Self>) -> (String, Option<String>) {
        let (message, sent) = self.context.compose(text, &self.removed);
        let Some(note) = self.replying.and_then(|id| self.notes.get(id)) else {
            return (message, sent);
        };
        let about = format!(
            "{}\n\n{message}",
            strings::note_reply(&note.chapter, &note.quote, &note.title, &note.text)
        );
        (about, sent)
    }

    /// Starts a turn, if none is running and the CLI can. Returns whether it did. `shown` is the row, `message` what goes.
    fn start_turn(
        &mut self,
        shown: String,
        message: String,
        context: Option<SentContext>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Still looking for the CLI: the words stay in the prompt, which says so.
        if !self.enabled || !self.conversation.is_idle() || self.cli.is_none() {
            return false;
        }
        if !self.cli_usable() {
            // The card says what is wrong and how to set it up.
            self.close_selection_prompt(window, cx);
            self.open_chat(window, cx);
            return false;
        }
        if let Err(error) = self.ensure_running(window, cx) {
            self.conversation.fail(error);
            cx.notify();
            return false;
        }
        self.label_local_history_before_turn(cx);
        self.turn_start = Some(self.project_now(cx));
        // The conversation should know what was taken back, or it offers the same change again.
        let message = if self.rejections.is_empty() {
            message
        } else {
            format!(
                "<druck-rejected>\n{}\n</druck-rejected>\n\n{message}",
                self.rejections.join("\n\n")
            )
        };
        let running = self.running.as_mut().expect("started above");
        if let Err(error) = running.session.send(&message) {
            self.running = None;
            self.conversation.fail(strings::cannot_send(&error.to_string()));
            cx.notify();
            return false;
        }
        self.rejections.clear();
        self.conversation.begin_turn(shown, context);
        self.collecting = self.conversation.current_turn();
        self.collecting_end = Task::ready(());
        self.persist.update(cx, |persist, _| persist.set_agent_working(true));
        self.resumable = false;
        self.sent_at = Some(Instant::now());
        self.first_event = None;
        self.first_text = None;
        self.scroll.scroll_to_bottom();
        cx.notify();
        true
    }

    /// Marks local history, so a whole turn can be taken back from the timeline.
    fn label_local_history_before_turn(&mut self, cx: &mut Context<Self>) {
        crate::versions::label_history(self.editor.read(cx).session(), crate::versions::Moment::AgentTurn, cx);
    }

    fn project_now(&self, cx: &App) -> TurnStart {
        let session = self.editor.read(cx).session();
        let manifest = session.manifest_path();
        TurnStart {
            files: session.files().into_iter().collect(),
            manifest: manifest.and_then(|path| std::fs::read_to_string(path).ok()),
            at: SystemTime::now(),
        }
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
        // Changes under review stay under review: they are in the files, whichever conversation made them.
        self.conversation.restart();
        self.resumable = false;
        self.denying = None;
        self.expanded.clear();
        self.parsed.borrow_mut().clear();
        self.removed.clear();
        self.remember(cx);
        self.update_attention(cx);
        cx.notify();
    }

    fn choose_model(&mut self, model: Option<&'static str>, cx: &mut Context<Self>) {
        self.model_menu = false;
        if self.model != model && self.conversation.is_idle() {
            self.model = model;
            // The model is fixed when the process starts, so the next prompt starts one with the new choice.
            self.running = None;
            self.remember(cx);
        }
        cx.notify();
    }

    // Context

    /// Reads the open chapter, the selection, and the errors, to offer as chips. Typesetting warnings stay out: a book has
    /// hundreds, and they say nothing about the words.
    fn snapshot_context(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let session = editor.session();
        let chapter = session
            .chapter(session.active())
            .map(|chapter| relative(&self.base, chapter.path()));
        let chapter_title = session.title_of(session.active());
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
        let diagnostics = (self.preview.read(cx).diagnostics().iter())
            .map(|diagnostic| diagnostic.display_from(&self.base).to_string())
            .collect();
        let context = PromptContext {
            chapter,
            chapter_title,
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
            ChipKind::Chapter => strings::chip_chapter(&self.context.chapter_title),
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

    /// A file the session has shows what happened to it on disk, whichever tool wrote it. The CLI's patch is for the rest.
    fn forget_patch_of_open_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let session = self.editor.read(cx).session();
        let open = file_in_session(session, path).is_some_and(|known| session.file_text(&known).is_some());
        if open
            && let Some(changes) = self
                .conversation
                .current_turn()
                .and_then(|turn| self.conversation.changes_mut(turn))
        {
            changes.forget_patch(path);
        }
    }

    /// Everything the turn wrote is on disk now, so the last look needs no wait for the watcher.
    fn end_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.persist.update(cx, |persist, cx| persist.scan_now(window, cx));
        self.take_in_new_files(cx);
        self.refresh_changes(window, cx);
        self.collecting_end = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(TURN_GRACE).await;
            view.update(cx, |view, cx| {
                view.take_in_new_files(cx);
                view.collecting = None;
                view.persist.update(cx, |persist, _| persist.set_agent_working(false));
            })
            .ok();
        });
    }
}

/// The session's name for a file the CLI names: the same file may be reached through a link, as `/tmp` is on macOS.
fn file_in_session(session: &Document, path: &Path) -> Option<PathBuf> {
    let wanted = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    session
        .files()
        .into_iter()
        .find(|file| std::fs::canonicalize(file).as_deref().unwrap_or(file) == wanted)
}

/// `path` relative to the project folder where it lies inside it.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).display().to_string()
}
