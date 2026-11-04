//! What the panel shows, as plain data. Events from the CLI go in, rows and pending approvals come out, and nothing
//! here knows about gpui or a child process, so a captured stream replays against it in a test.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::approval::{self, Approval};
use super::changes::{Hunk, TurnChanges};
use super::session::Event;
use super::strings;

/// How much of a tool's output a row keeps.
const OUTPUT_LIMIT: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Working,
    /// An interrupt was sent and the turn has not ended yet.
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    /// Asked the author and waiting.
    Waiting,
    Done,
    Failed,
    /// The author said no.
    Denied,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub target: String,
    pub state: ToolState,
    pub output: String,
}

/// What a prompt carried along, so the row can show what was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentContext {
    pub labels: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    User {
        text: String,
        context: Option<SentContext>,
    },
    Assistant {
        text: String,
    },
    Tool(ToolCall),
    /// Something that went wrong, as the CLI or the app said it.
    Error(String),
    /// Something the app says, such as a change it could not carry into the editor.
    Notice(String),
    Stopped,
    /// A turn is over. `changes` is its number when it changed files.
    TurnEnd {
        cost_usd: Option<f64>,
        changes: Option<usize>,
    },
}

/// What the owner of a conversation has to do after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// The session id to keep for the project.
    SessionId(String),
    /// A tool reported a patch for this file, now in the turn's changes.
    Patched(PathBuf),
    /// The turn is over: what it did to the files is on disk by now.
    TurnEnded,
    /// A tool created this file.
    Created(PathBuf),
    /// The process ended; a new prompt starts a new one.
    ProcessEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Allowed,
    Denied,
}

pub struct Conversation {
    base: PathBuf,
    entries: Vec<Entry>,
    status: Status,
    approvals: VecDeque<Approval>,
    session_id: Option<String>,
    /// What each turn changed, by turn number. Empty for a turn that changed nothing.
    turns: Vec<TurnChanges>,
    /// What the CLI process had spent when the last turn ended, since it counts from its start.
    process_total: f64,
    spent: f64,
    /// An event changed what the panel shows since the last [`Conversation::take_dirty`].
    dirty: bool,
}

impl Conversation {
    /// `base` is the project folder, which paths in rows are shown relative to.
    pub fn new(base: PathBuf) -> Self {
        Self {
            base,
            entries: Vec::new(),
            status: Status::Idle,
            approvals: VecDeque::new(),
            session_id: None,
            turns: Vec::new(),
            process_total: 0.,
            spent: 0.,
            dirty: false,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn is_idle(&self) -> bool {
        self.status == Status::Idle
    }

    /// The approvals waiting, oldest first.
    pub fn approvals(&self) -> impl Iterator<Item = &Approval> {
        self.approvals.iter()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn set_session_id(&mut self, id: Option<String>) {
        self.session_id = id;
    }

    /// What this conversation has spent since the app started it.
    pub fn spent(&self) -> f64 {
        self.spent
    }

    pub fn changes(&self, turn: usize) -> Option<&TurnChanges> {
        self.turns.get(turn).filter(|changes| !changes.is_empty())
    }

    pub fn changes_mut(&mut self, turn: usize) -> Option<&mut TurnChanges> {
        self.turns.get_mut(turn)
    }

    /// The newest turn that changed something.
    pub fn latest_changes(&self) -> Option<usize> {
        (0..self.turns.len()).rev().find(|turn| self.changes(*turn).is_some())
    }

    /// A new CLI process starts counting its cost from nothing.
    pub fn process_started(&mut self) {
        self.process_total = 0.;
    }

    /// The author sent a prompt.
    pub fn begin_turn(&mut self, text: String, context: Option<SentContext>) {
        self.entries.push(Entry::User { text, context });
        self.turns.push(TurnChanges::default());
        self.status = Status::Working;
    }

    /// Adds a line the app says.
    pub fn note(&mut self, message: String) {
        self.entries.push(Entry::Notice(message));
    }

    /// The number of the turn running or last run.
    pub fn current_turn(&self) -> Option<usize> {
        self.turns.len().checked_sub(1)
    }

    /// Something went wrong before the CLI could be asked: a missing program, a closed pipe.
    pub fn fail(&mut self, message: String) {
        self.entries.push(Entry::Error(message));
        self.status = Status::Idle;
        self.approvals.clear();
    }

    /// The author pressed stop. The approvals still waiting are returned to be denied, since the CLI waits on them.
    pub fn stop(&mut self) -> Vec<Approval> {
        if self.status == Status::Working {
            self.status = Status::Stopping;
        }
        let pending: Vec<_> = self.approvals.drain(..).collect();
        for approval in &pending {
            self.set_tool(approval.request.tool_use_id.as_deref(), ToolState::Denied);
        }
        pending
    }

    /// The author answered an approval.
    pub fn answer(&mut self, request_id: &str, answer: Answer) -> Option<Approval> {
        let at = self.approvals.iter().position(|a| a.request.request_id == request_id)?;
        let approval = self.approvals.remove(at)?;
        let state = match answer {
            Answer::Allowed => ToolState::Running,
            Answer::Denied => ToolState::Denied,
        };
        self.set_tool(approval.request.tool_use_id.as_deref(), state);
        Some(approval)
    }

    /// Whether an event changed anything to draw since this was last asked. Events a newer CLI adds change nothing.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Takes one event from the CLI. `read` gives a file's text, for the diff shown with an approval.
    pub fn apply(&mut self, event: Event, read: &dyn Fn(&Path) -> Option<String>) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.dirty |= !matches!(event, Event::Raw(_));
        match event {
            Event::Started { session_id, .. } => {
                self.session_id = Some(session_id.clone());
                effects.push(Effect::SessionId(session_id));
            }
            Event::TextDelta(delta) => match self.entries.last_mut() {
                Some(Entry::Assistant { text }) => text.push_str(&delta),
                _ => self.entries.push(Entry::Assistant { text: delta }),
            },
            Event::ToolUse { id, name, input } => {
                let target = approval::target(&name, &input, &self.base);
                self.entries.push(Entry::Tool(ToolCall {
                    id,
                    name,
                    target,
                    state: ToolState::Running,
                    output: String::new(),
                }));
            }
            Event::Permission(request) => {
                self.set_tool(request.tool_use_id.as_deref(), ToolState::Waiting);
                self.approvals.push_back(Approval::new(request, &self.base, read));
            }
            Event::ToolResult {
                tool_use_id,
                content,
                is_error,
                details,
            } => {
                let denied = self
                    .tool(&tool_use_id)
                    .is_some_and(|tool| tool.state == ToolState::Denied);
                let state = match (is_error, denied) {
                    (_, true) => ToolState::Denied,
                    (true, false) => ToolState::Failed,
                    (false, false) => ToolState::Done,
                };
                if let Some(tool) = self.tool_mut(&tool_use_id) {
                    tool.state = state;
                    tool.output = clip(&content);
                }
                if !is_error {
                    if let Some(details) = details {
                        effects.extend(self.record_changes(&details));
                    }
                }
            }
            Event::TurnFinished {
                session_id,
                total_cost_usd,
                is_error,
                stopped,
                result,
            } => {
                let cost = total_cost_usd
                    .map(|total| {
                        let turn = (total - self.process_total).max(0.);
                        self.process_total = total;
                        self.spent += turn;
                        turn
                    })
                    // A turn that was cut short reports nothing new.
                    .filter(|turn| *turn > 0.);
                let stopping = self.status == Status::Stopping;
                self.status = Status::Idle;
                self.approvals.clear();
                if stopped || (stopping && is_error) {
                    self.entries.push(Entry::Stopped);
                } else if is_error {
                    self.entries.push(Entry::Error(result));
                }
                self.entries.push(Entry::TurnEnd {
                    cost_usd: cost,
                    changes: self.turns.len().checked_sub(1),
                });
                effects.push(Effect::TurnEnded);
                if !session_id.is_empty() {
                    self.session_id = Some(session_id.clone());
                    effects.push(Effect::SessionId(session_id));
                }
            }
            Event::Error(message) => self.entries.push(Entry::Error(message)),
            Event::Exited(stderr) => {
                self.status = Status::Idle;
                self.approvals.clear();
                self.entries.push(Entry::Error(strings::exited(&stderr)));
                effects.push(Effect::ProcessEnded);
            }
            // Events a newer CLI adds. The panel shows nothing it does not know.
            Event::Raw(_) => {}
        }
        effects
    }

    /// Notes the files a finished tool changed, from the result the CLI attaches to it.
    fn record_changes(&mut self, details: &Value) -> Vec<Effect> {
        let Some(path) = details["filePath"].as_str().map(PathBuf::from) else {
            return Vec::new();
        };
        let Some(turn) = self.turns.last_mut() else {
            return Vec::new();
        };
        if details["type"] == "create" {
            turn.created.push(path.clone());
            return vec![Effect::Created(path)];
        }
        let hunks = Hunk::parse_all(&details["structuredPatch"]);
        if hunks.is_empty() {
            return Vec::new();
        }
        turn.add(path.clone(), hunks);
        vec![Effect::Patched(path)]
    }

    fn tool(&self, id: &str) -> Option<&ToolCall> {
        self.entries.iter().rev().find_map(|entry| match entry {
            Entry::Tool(tool) if tool.id == id => Some(tool),
            _ => None,
        })
    }

    fn tool_mut(&mut self, id: &str) -> Option<&mut ToolCall> {
        self.entries.iter_mut().rev().find_map(|entry| match entry {
            Entry::Tool(tool) if tool.id == id => Some(tool),
            _ => None,
        })
    }

    fn set_tool(&mut self, id: Option<&str>, state: ToolState) {
        if let Some(tool) = id.and_then(|id| self.tool_mut(id)) {
            tool.state = state;
        }
    }
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(OUTPUT_LIMIT) {
        Some((end, _)) => format!("{}\u{2026}", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::session::parse_line;
    use super::*;

    const CAPTURE: &str = include_str!("../../tests/fixtures/agent-turns.jsonl");
    const NOTES: &str = "# Notes\n\nHello world.\n\nA second line.\n";

    fn read(_: &Path) -> Option<String> {
        Some(NOTES.to_owned())
    }

    /// The lines of the capture that belong to turn `n` (0 to 3): everything from one `result` to the next.
    fn turn_events(n: usize) -> Vec<Event> {
        let mut turn = 0;
        let mut out = Vec::new();
        for line in CAPTURE.lines() {
            if turn == n {
                out.extend(parse_line(line));
            }
            if line.contains(r#""type":"result""#) {
                turn += 1;
            }
        }
        out
    }

    fn run(conversation: &mut Conversation, events: Vec<Event>) -> Vec<Effect> {
        events
            .into_iter()
            .flat_map(|event| conversation.apply(event, &read))
            .collect()
    }

    fn conversation() -> Conversation {
        Conversation::new(PathBuf::from("/work/project"))
    }

    #[test]
    fn a_denied_edit_waits_for_the_author_and_ends_denied() {
        let mut c = conversation();
        c.begin_turn("Change Hello to Howdy".into(), None);
        let events = turn_events(0);
        let permission = events
            .iter()
            .position(|e| matches!(e, Event::Permission(_)))
            .expect("request");
        run(&mut c, events[..=permission].to_vec());
        let waiting = c.approvals().next().expect("an approval is waiting");
        assert_eq!(waiting.target, "notes.md");
        let edit = c
            .entries()
            .iter()
            .rev()
            .find_map(|e| if let Entry::Tool(t) = e { Some(t) } else { None });
        assert_eq!(edit.map(|t| t.state), Some(ToolState::Waiting));

        let id = waiting.request.request_id.clone();
        c.answer(&id, Answer::Denied).expect("answered");
        assert_eq!(c.approvals().count(), 0);
        run(&mut c, events[permission + 1..].to_vec());
        let edit = c.entries().iter().find_map(|e| match e {
            Entry::Tool(t) if t.name == "Edit" => Some(t.state),
            _ => None,
        });
        assert_eq!(edit, Some(ToolState::Denied));
        assert!(c.is_idle());
        assert!(matches!(c.entries().last(), Some(Entry::TurnEnd { .. })));
    }

    #[test]
    fn streamed_text_joins_into_one_row_and_a_tool_splits_it() {
        let mut c = conversation();
        c.begin_turn("hi".into(), None);
        run(&mut c, turn_events(0));
        let kinds: Vec<_> = c
            .entries()
            .iter()
            .map(|e| match e {
                Entry::User { .. } => "user",
                Entry::Assistant { .. } => "assistant",
                Entry::Tool(_) => "tool",
                Entry::TurnEnd { .. } => "end",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["user", "tool", "tool", "assistant", "end"]);
        let Entry::Assistant { text } = &c.entries()[3] else {
            panic!("text")
        };
        assert!(text.starts_with("The edit to `notes.md` was declined"));
    }

    #[test]
    fn an_allowed_edit_records_its_hunks() {
        let mut c = conversation();
        c.begin_turn("again".into(), None);
        let effects = run(&mut c, turn_events(1));
        assert!(effects.contains(&Effect::Patched(PathBuf::from("/work/project/notes.md"))));
        let turn = c.latest_changes().expect("a turn with changes");
        let changes = c.changes(turn).expect("changes");
        assert_eq!((changes.files.len(), changes.hunk_count()), (1, 1));
        let Some(Entry::TurnEnd { changes: Some(n), .. }) = c.entries().last() else {
            panic!("turn end names its changes")
        };
        assert_eq!(*n, turn);
    }

    #[test]
    fn cost_is_per_turn_although_the_cli_counts_per_process() {
        let mut c = conversation();
        c.process_started();
        for n in [0, 1] {
            c.begin_turn("x".into(), None);
            run(&mut c, turn_events(n));
        }
        let costs: Vec<_> = c
            .entries()
            .iter()
            .filter_map(|e| {
                if let Entry::TurnEnd { cost_usd, .. } = e {
                    *cost_usd
                } else {
                    None
                }
            })
            .collect();
        assert!((costs[0] - 0.00126701).abs() < 1e-9);
        assert!((costs[1] - 0.00034193).abs() < 1e-9);
        assert!((c.spent() - 0.00160894).abs() < 1e-9);
    }

    #[test]
    fn a_resumed_process_starts_counting_again() {
        let mut c = conversation();
        c.process_started();
        for n in [0, 1] {
            c.begin_turn("x".into(), None);
            run(&mut c, turn_events(n));
        }
        c.process_started();
        c.begin_turn("again".into(), None);
        let effects = run(&mut c, turn_events(2));
        let Some(Entry::TurnEnd {
            cost_usd: Some(cost), ..
        }) = c.entries().last()
        else {
            panic!("turn end")
        };
        assert!((cost - 0.00008728).abs() < 1e-9);
        assert!(effects.iter().any(|e| matches!(e, Effect::SessionId(_))));
        assert_eq!(c.session_id(), Some("e1341d12-6f29-4505-acf2-d98202053623"));
    }

    #[test]
    fn a_stopped_turn_says_stopped_and_not_error() {
        let mut c = conversation();
        c.begin_turn("count".into(), None);
        let events = turn_events(3);
        let before_result = events.len() - 1;
        run(&mut c, events[..before_result].to_vec());
        c.stop();
        assert_eq!(c.status(), Status::Stopping);
        run(&mut c, events[before_result..].to_vec());
        assert!(c.is_idle());
        assert!(c.entries().contains(&Entry::Stopped));
        assert!(!c.entries().iter().any(|e| matches!(e, Entry::Error(_))));
    }

    #[test]
    fn stopping_hands_back_the_approvals_the_cli_waits_on() {
        let mut c = conversation();
        c.begin_turn("edit".into(), None);
        let events = turn_events(0);
        let permission = events
            .iter()
            .position(|e| matches!(e, Event::Permission(_)))
            .expect("request");
        run(&mut c, events[..=permission].to_vec());
        let pending = c.stop();
        assert_eq!(pending.len(), 1);
        assert_eq!(c.approvals().count(), 0);
    }

    #[test]
    fn unknown_events_change_nothing() {
        let mut c = conversation();
        c.begin_turn("x".into(), None);
        let before = c.entries().len();
        let effects = run(
            &mut c,
            parse_line(r#"{"type":"brand_new","payload":{"a":1}}"#)
                .into_iter()
                .collect(),
        );
        assert!(effects.is_empty());
        assert_eq!((c.entries().len(), c.status()), (before, Status::Working));
        // Nothing to draw, so nothing asks for a redraw. A known event does.
        assert!(!c.take_dirty());
        c.apply(Event::TextDelta("hi".into()), &read);
        assert!(c.take_dirty());
    }

    #[test]
    fn a_process_that_dies_ends_the_turn_with_its_error() {
        let mut c = conversation();
        c.begin_turn("x".into(), None);
        let effects = c.apply(Event::Exited("No conversation found".into()), &read);
        assert_eq!(effects, [Effect::ProcessEnded]);
        assert!(c.is_idle());
        assert_eq!(
            c.entries().last(),
            Some(&Entry::Error(strings::exited("No conversation found")))
        );
    }
}
