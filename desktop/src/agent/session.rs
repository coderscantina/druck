//! One agent CLI child process speaking JSON lines over stdio.
//!
//! The user's installed CLI does the work and owns authentication. This module
//! never reads, stores or sets credentials. Events arrive on a channel; a
//! permission request is answered with [`Session::answer`]. What goes over the
//! pipes is the [`Backend`]'s.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use serde_json::Value;

use super::backend::{Backend, Verdict};

/// How much of the CLI's stderr is kept for an error message.
const STDERR_LIMIT: u64 = 8 * 1024;

/// What the host sees from the CLI.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The CLI initialised the session. `login` is the CLI's own name for where its credentials come from, such as
    /// `none` or `ANTHROPIC_API_KEY`. Never a credential.
    Started {
        session_id: String,
        model: String,
        login: String,
    },
    /// A piece of the assistant's reply text.
    TextDelta(String),
    /// The assistant asked to run a tool. The input is complete.
    ToolUse { id: String, name: String, input: Value },
    /// A tool finished, with its output flattened to text.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        details: Option<Value>,
    },
    /// The CLI waits for the host to allow or deny a tool call.
    Permission(PermissionRequest),
    /// The turn ended.
    TurnFinished {
        session_id: String,
        is_error: bool,
        /// The turn was cut short by an interrupt.
        stopped: bool,
        result: String,
    },
    /// A line from the CLI that is not JSON. The stream goes on.
    Error(String),
    /// The CLI process ended without being asked to, with what it wrote to stderr.
    Exited(String),
    /// Any other event, kept as received so a CLI update does not break the stream.
    Raw(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_use_id: Option<String>,
    pub tool_name: String,
    pub input: Value,
    /// Updates the CLI offers to remember this answer, as sent. Each names where it would be kept.
    pub suggestions: Vec<Value>,
}

/// What a session may do to the files without asking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Access {
    /// As the author's own CLI settings say, as in a terminal.
    #[default]
    AsConfigured,
    /// Edits to the project's files run without a question, for a pass that returns changes: the author asked for
    /// them, and reviews each one in the text.
    Edits,
    /// No file is written, for a pass that returns margin notes only.
    ReadOnly,
}

#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    /// The CLI's executable.
    pub program: PathBuf,
    /// Working directory, the project folder.
    pub cwd: PathBuf,
    /// The search path for the CLI and what it starts. `None` keeps the app's own.
    pub path: Option<OsString>,
    pub model: Option<String>,
    /// Session id of an earlier run to continue.
    pub resume: Option<String>,
    /// The executable that serves Druck's tools over MCP, which is this app. `None` leaves the CLI without them.
    pub tool_server: Option<PathBuf>,
    pub access: Access,
    /// More flags for the command line, which only a protocol probe passes.
    pub extra_args: Vec<OsString>,
    /// Append every raw stdout line here. For protocol captures only.
    pub transcript: Option<PathBuf>,
}

/// The backend and the pipe it writes to, shared with the reader, which must answer some lines at once.
struct Wire {
    backend: Box<dyn Backend>,
    stdin: Option<ChildStdin>,
}

impl Wire {
    fn write(&mut self, messages: &[Value]) -> io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
        for message in messages {
            writeln!(stdin, "{message}")?;
        }
        stdin.flush()
    }
}

pub struct Session {
    child: Child,
    wire: Arc<Mutex<Wire>>,
    closing: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl Session {
    /// Starts the CLI and returns the event stream. The child lives until the session drops.
    pub fn spawn(config: &SessionConfig, mut backend: Box<dyn Backend>) -> io::Result<(Self, Receiver<Event>)> {
        let mut command = Command::new(&config.program);
        if let Some(path) = &config.path {
            command.env("PATH", path);
        }
        let mut child = command
            .args(backend.args(config))
            .current_dir(&config.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let opening = backend.open(config);
        let mut wire = Wire {
            backend,
            stdin: child.stdin.take(),
        };
        // A CLI that is gone already says so through the reader, as an exit.
        let _ = wire.write(&opening);
        let wire = Arc::new(Mutex::new(wire));

        let transcript = match &config.transcript {
            Some(path) => Some(std::fs::OpenOptions::new().create(true).append(true).open(path)?),
            None => None,
        };
        let closing = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let stderr_tail = thread::spawn(move || {
            let mut tail = String::new();
            let _ = stderr.take(STDERR_LIMIT).read_to_string(&mut tail);
            tail
        });
        let reader = {
            let (closing, wire) = (closing.clone(), wire.clone());
            thread::spawn(move || read_events(stdout, transcript, &wire, &tx, &closing, stderr_tail))
        };
        let session = Self {
            child,
            wire,
            closing,
            reader: Some(reader),
        };
        Ok((session, rx))
    }

    /// Sends a user message. The reply streams in as events.
    pub fn send(&mut self, text: &str) -> io::Result<()> {
        let mut wire = lock(&self.wire);
        let messages = wire.backend.prompt(text);
        wire.write(&messages)
    }

    /// Answers a waiting tool call.
    pub fn answer(&mut self, request: &PermissionRequest, verdict: Verdict) -> io::Result<()> {
        let mut wire = lock(&self.wire);
        let message = wire.backend.answer(request, verdict);
        wire.write(&[message])
    }

    /// Stops the current turn. The session stays usable.
    pub fn cancel(&mut self) -> io::Result<()> {
        let mut wire = lock(&self.wire);
        let messages = wire.backend.interrupt();
        wire.write(&messages)
    }
}

fn lock(wire: &Mutex<Wire>) -> std::sync::MutexGuard<'_, Wire> {
    wire.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Drop for Session {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Relaxed);
        lock(&self.wire).stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_events(
    stdout: impl Read,
    mut transcript: Option<std::fs::File>,
    wire: &Mutex<Wire>,
    tx: &Sender<Event>,
    closing: &AtomicBool,
    stderr_tail: JoinHandle<String>,
) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if let Some(file) = transcript.as_mut() {
            let _ = writeln!(file, "{line}");
        }
        let events = {
            let mut wire = lock(wire);
            let read = wire.backend.read(&line);
            if !read.replies.is_empty() {
                let _ = wire.write(&read.replies);
            }
            read.events
        };
        for event in events {
            if tx.send(event).is_err() {
                return;
            }
        }
    }
    if !closing.load(Ordering::Relaxed) {
        let tail = stderr_tail.join().unwrap_or_default();
        let _ = tx.send(Event::Exited(tail.trim().to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::Duration;

    use super::super::claude::Claude;
    use super::*;

    /// A real permission request of the Claude CLI.
    const CAPTURE: &str = include_str!("../../tests/fixtures/agent-turns.jsonl");

    fn spawn(config: &SessionConfig) -> io::Result<(Session, Receiver<Event>)> {
        Session::spawn(config, Box::new(Claude::default()))
    }

    #[test]
    fn missing_cli_fails_to_spawn() {
        let config = SessionConfig {
            program: "/nonexistent/claude".into(),
            cwd: ".".into(),
            ..SessionConfig::default()
        };
        let error = spawn(&config).err().expect("spawn fails");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    /// A stand-in CLI: a script that ignores its flags and runs `body`.
    fn fake_cli(name: &str, body: &str) -> SessionConfig {
        let path = std::env::temp_dir().join(format!("druck-fake-claude-{}-{name}", std::process::id()));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        SessionConfig {
            program: path,
            cwd: std::env::temp_dir(),
            ..SessionConfig::default()
        }
    }

    #[test]
    fn permission_answers_reach_the_cli() {
        let request = CAPTURE
            .lines()
            .find(|l| l.contains("can_use_tool"))
            .expect("captured request");
        let config = fake_cli(
            "permission",
            &format!("echo '{request}'\nread line\necho \"{{\\\"type\\\":\\\"echo\\\",\\\"line\\\":$line}}\""),
        );
        let (mut session, events) = spawn(&config).expect("spawn");
        let Event::Permission(request) = events.recv_timeout(Duration::from_secs(5)).expect("event") else {
            panic!("not a permission")
        };
        session.answer(&request, Verdict::Deny("no")).expect("answer");
        let Event::Raw(echo) = events.recv_timeout(Duration::from_secs(5)).expect("echo") else {
            panic!("not raw")
        };
        let response = &echo["line"]["response"];
        assert_eq!(response["request_id"], request.request_id.as_str());
        assert_eq!(response["response"]["behavior"], "deny");
        drop(session);
        std::fs::remove_file(config.program).expect("cleanup");
    }

    /// What the CLI got on stdin for one call, through a stand-in that echoes its first line.
    fn echoed(name: &str, call: impl FnOnce(&mut Session, &PermissionRequest)) -> Value {
        let request = CAPTURE
            .lines()
            .find(|l| l.contains("can_use_tool"))
            .expect("captured request");
        let config = fake_cli(
            name,
            &format!("echo '{request}'\nread line\necho \"{{\\\"type\\\":\\\"echo\\\",\\\"line\\\":$line}}\""),
        );
        let (mut session, events) = spawn(&config).expect("spawn");
        let Event::Permission(request) = events.recv_timeout(Duration::from_secs(5)).expect("event") else {
            panic!("not a permission")
        };
        call(&mut session, &request);
        let Event::Raw(echo) = events.recv_timeout(Duration::from_secs(5)).expect("echo") else {
            panic!("not raw")
        };
        drop(session);
        std::fs::remove_file(config.program).expect("cleanup");
        echo["line"].clone()
    }

    #[test]
    fn stop_sends_an_interrupt_request() {
        let line = echoed("interrupt", |session, _| session.cancel().expect("cancel"));
        assert_eq!(line["type"], "control_request");
        assert_eq!(line["request"]["subtype"], "interrupt");
    }

    #[test]
    fn a_session_choice_passes_back_the_clis_own_suggestion() {
        let line = echoed("session-choice", |session, request| {
            assert_eq!(request.suggestions[0]["mode"], "acceptEdits");
            session
                .answer(request, Verdict::AllowForSession(&request.suggestions[0]))
                .expect("answer");
        });
        let response = &line["response"]["response"];
        assert_eq!(response["behavior"], "allow");
        assert_eq!(response["updatedPermissions"][0]["destination"], "session");
    }

    #[test]
    fn dropping_the_session_ends_the_child() {
        let config = fake_cli("shutdown", "sleep 60");
        let (session, _events) = spawn(&config).expect("spawn");
        let pid = session.child.id().to_string();
        drop(session);
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .output()
            .expect("kill")
            .status
            .success();
        assert!(!alive, "child {pid} is still there");
        std::fs::remove_file(config.program).expect("cleanup");
    }

    #[test]
    fn unexpected_exit_reports_an_error() {
        let config = fake_cli("exit", "echo boom >&2\nexit 3");
        let (_session, events) = spawn(&config).expect("spawn");
        let Event::Exited(message) = events.recv_timeout(Duration::from_secs(5)).expect("event") else {
            panic!("not an exit")
        };
        assert!(message.contains("boom"));
    }
}
