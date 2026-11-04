//! One `claude` child process speaking `stream-json` over stdio.
//!
//! The user's installed CLI does the work and owns authentication. This module
//! never reads, stores or sets credentials. Events arrive on a channel; a
//! permission request is answered with [`Session::allow`] or [`Session::deny`].

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use serde_json::{Value, json};

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
    /// The turn ended. `total_cost_usd` counts everything this CLI process has spent, not just this turn.
    TurnFinished {
        session_id: String,
        total_cost_usd: Option<f64>,
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

#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    /// The `claude` executable.
    pub program: PathBuf,
    /// Working directory, the project folder.
    pub cwd: PathBuf,
    pub model: Option<String>,
    /// Session id of an earlier run to continue.
    pub resume: Option<String>,
    /// Extra CLI flags, such as `--tools` or `--strict-mcp-config`.
    pub extra_args: Vec<OsString>,
    /// Append every raw stdout line here. For protocol captures only.
    pub transcript: Option<PathBuf>,
}

pub struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    closing: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    next_request: u32,
}

impl Session {
    /// Starts the CLI and returns the event stream. The child lives until the session drops.
    pub fn spawn(config: &SessionConfig) -> io::Result<(Self, Receiver<Event>)> {
        let mut child = Command::new(&config.program)
            .args(build_args(config))
            .current_dir(&config.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");

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
            let closing = closing.clone();
            thread::spawn(move || read_events(stdout, transcript, &tx, &closing, stderr_tail))
        };
        let session = Self {
            child,
            stdin,
            closing,
            reader: Some(reader),
            next_request: 0,
        };
        Ok((session, rx))
    }

    /// Sends a user message. The reply streams in as events.
    pub fn send(&mut self, text: &str) -> io::Result<()> {
        self.write(&json!({"type": "user", "message": {"role": "user", "content": text}}))
    }

    /// Lets the tool call run as the model asked.
    pub fn allow(&mut self, request: &PermissionRequest) -> io::Result<()> {
        self.respond(
            request,
            json!({"behavior": "allow", "updatedInput": request.input, "toolUseID": request.tool_use_id}),
        )
    }

    /// Lets the tool call run and applies one of the request's own suggestions, such as accepting edits for the
    /// rest of the CLI process. Only a suggestion the CLI sent for this request is passed back.
    pub fn allow_with(&mut self, request: &PermissionRequest, update: &Value) -> io::Result<()> {
        self.respond(
            request,
            json!({
                "behavior": "allow",
                "updatedInput": request.input,
                "updatedPermissions": [update],
                "toolUseID": request.tool_use_id,
            }),
        )
    }

    /// Refuses the tool call. The model sees the message.
    pub fn deny(&mut self, request: &PermissionRequest, message: &str) -> io::Result<()> {
        self.respond(
            request,
            json!({"behavior": "deny", "message": message, "toolUseID": request.tool_use_id}),
        )
    }

    /// Stops the current turn. The session stays usable.
    pub fn cancel(&mut self) -> io::Result<()> {
        self.next_request += 1;
        let request_id = format!("druck-{}", self.next_request);
        self.write(&json!({"type": "control_request", "request_id": request_id, "request": {"subtype": "interrupt"}}))
    }

    fn respond(&mut self, request: &PermissionRequest, response: Value) -> io::Result<()> {
        self.write(&json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": request.request_id, "response": response},
        }))
    }

    fn write(&mut self, message: &Value) -> io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
        writeln!(stdin, "{message}")?;
        stdin.flush()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Relaxed);
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub fn build_args(config: &SessionConfig) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
    ]
    .map(OsString::from)
    .into();
    if let Some(model) = &config.model {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(id) = &config.resume {
        args.extend(["--resume".into(), id.into()]);
    }
    args.extend(config.extra_args.iter().cloned());
    args
}

fn read_events(
    stdout: impl Read,
    mut transcript: Option<std::fs::File>,
    tx: &Sender<Event>,
    closing: &AtomicBool,
    stderr_tail: JoinHandle<String>,
) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if let Some(file) = transcript.as_mut() {
            let _ = writeln!(file, "{line}");
        }
        for event in parse_line(&line) {
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

/// Turns one stdout line into events. Anything not recognised becomes [`Event::Raw`].
pub fn parse_line(line: &str) -> Vec<Event> {
    if line.trim().is_empty() {
        return Vec::new();
    }
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => return vec![Event::Error(format!("unreadable CLI line: {error}"))],
    };
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let events = match (value["type"].as_str(), value["subtype"].as_str()) {
        (Some("system"), Some("init")) => vec![Event::Started {
            session_id: text(&value["session_id"]),
            model: text(&value["model"]),
            login: text(&value["apiKeySource"]),
        }],
        (Some("stream_event"), _) => match &value["event"]["delta"] {
            delta if delta["type"] == "text_delta" => vec![Event::TextDelta(text(&delta["text"]))],
            _ => Vec::new(),
        },
        (Some("assistant"), _) => content_blocks(&value, "tool_use", |block| Event::ToolUse {
            id: text(&block["id"]),
            name: text(&block["name"]),
            input: block["input"].clone(),
        }),
        (Some("user"), _) => content_blocks(&value, "tool_result", |block| Event::ToolResult {
            tool_use_id: text(&block["tool_use_id"]),
            content: flatten(&block["content"]),
            is_error: block["is_error"].as_bool().unwrap_or(false),
            details: value.get("tool_use_result").cloned(),
        }),
        (Some("control_request"), _) if value["request"]["subtype"] == "can_use_tool" => {
            let request = &value["request"];
            vec![Event::Permission(PermissionRequest {
                request_id: text(&value["request_id"]),
                tool_use_id: request["tool_use_id"].as_str().map(str::to_owned),
                tool_name: text(&request["tool_name"]),
                input: request["input"].clone(),
                suggestions: request["permission_suggestions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            })]
        }
        (Some("result"), _) => vec![Event::TurnFinished {
            session_id: text(&value["session_id"]),
            total_cost_usd: value["total_cost_usd"].as_f64(),
            is_error: value["is_error"].as_bool().unwrap_or(false),
            stopped: value["terminal_reason"]
                .as_str()
                .is_some_and(|reason| reason.starts_with("aborted")),
            result: text(&value["result"]),
        }],
        _ => Vec::new(),
    };
    if events.is_empty() {
        vec![Event::Raw(value)]
    } else {
        events
    }
}

/// Maps the content blocks of a message that have the given type.
fn content_blocks(message: &Value, kind: &str, map: impl Fn(&Value) -> Event) -> Vec<Event> {
    let blocks = message["message"]["content"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    blocks.iter().filter(|block| block["type"] == kind).map(map).collect()
}

/// Tool output is a string or a list of text blocks.
fn flatten(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::Duration;

    use super::*;

    /// Real output of four turns: a denied edit, an allowed edit, a resumed turn and a cancelled one.
    const CAPTURE: &str = include_str!("../../tests/fixtures/agent-turns.jsonl");

    fn captured_events() -> Vec<Event> {
        CAPTURE.lines().flat_map(parse_line).collect()
    }

    #[test]
    fn captured_turns_parse_into_typed_events() {
        let events = captured_events();
        let text: String = events
            .iter()
            .filter_map(|e| {
                if let Event::TextDelta(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(text.contains("declined") && text.contains("Howdy"));

        let edits = events
            .iter()
            .filter(|e| matches!(e, Event::ToolUse { name, .. } if name == "Edit"))
            .count();
        let requests: Vec<_> = events
            .iter()
            .filter_map(|e| if let Event::Permission(r) = e { Some(r) } else { None })
            .collect();
        assert_eq!((edits, requests.len()), (2, 2));
        assert_eq!(requests[0].tool_name, "Edit");
        assert_eq!(requests[0].input["new_string"], "Howdy world.");

        let results: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let Event::ToolResult { is_error, details, .. } = e {
                    Some((*is_error, details.clone()))
                } else {
                    None
                }
            })
            .collect();
        assert!(results.iter().any(|(is_error, _)| *is_error));
        assert!(
            results
                .iter()
                .any(|(_, d)| d.as_ref().is_some_and(|d| d["structuredPatch"].is_array()))
        );
    }

    #[test]
    fn captured_turns_finish_with_session_and_cost() {
        let finished: Vec<_> = captured_events()
            .into_iter()
            .filter_map(|e| {
                if let Event::TurnFinished {
                    session_id,
                    total_cost_usd,
                    is_error,
                    ..
                } = e
                {
                    Some((session_id, total_cost_usd, is_error))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(finished.len(), 4);
        assert!(finished[0].1.is_some_and(|cost| cost > 0.0));
        assert!(!finished[0].0.is_empty());
        assert!(!finished[2].2 && finished[3].2);
    }

    #[test]
    fn unknown_events_are_kept_raw() {
        let events = parse_line(r#"{"type":"brand_new","payload":{"a":1}}"#);
        assert_eq!(
            events,
            vec![Event::Raw(json!({"type": "brand_new", "payload": {"a": 1}}))]
        );
        assert!(matches!(parse_line("not json").as_slice(), [Event::Error(_)]));
    }

    #[test]
    fn missing_cli_fails_to_spawn() {
        let config = SessionConfig {
            program: "/nonexistent/claude".into(),
            cwd: ".".into(),
            ..SessionConfig::default()
        };
        let error = Session::spawn(&config).err().expect("spawn fails");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn resume_adds_the_session_id() {
        let config = SessionConfig {
            resume: Some("abc".into()),
            ..SessionConfig::default()
        };
        let args = build_args(&config);
        let at = args.iter().position(|a| a == "--resume").expect("flag");
        assert_eq!(args[at + 1], "abc");
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
        let (mut session, events) = Session::spawn(&config).expect("spawn");
        let Event::Permission(request) = events.recv_timeout(Duration::from_secs(5)).expect("event") else {
            panic!("not a permission")
        };
        session.deny(&request, "no").expect("answer");
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
        let (mut session, events) = Session::spawn(&config).expect("spawn");
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
            session.allow_with(request, &request.suggestions[0]).expect("answer");
        });
        let response = &line["response"]["response"];
        assert_eq!(response["behavior"], "allow");
        assert_eq!(response["updatedPermissions"][0]["destination"], "session");
    }

    #[test]
    fn the_cli_runs_with_its_own_settings_and_tools() {
        let args = build_args(&SessionConfig::default());
        for flag in ["--tools", "--setting-sources", "--strict-mcp-config", "--settings"] {
            assert!(
                !args.iter().any(|arg| arg == flag),
                "{flag} would change what a terminal does"
            );
        }
    }

    #[test]
    fn dropping_the_session_ends_the_child() {
        let config = fake_cli("shutdown", "sleep 60");
        let (session, _events) = Session::spawn(&config).expect("spawn");
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
        let (_session, events) = Session::spawn(&config).expect("spawn");
        let Event::Exited(message) = events.recv_timeout(Duration::from_secs(5)).expect("event") else {
            panic!("not an exit")
        };
        assert!(message.contains("boom"));
    }
}
