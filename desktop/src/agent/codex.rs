//! Codex: `codex app-server`, JSON-RPC as one object per line on stdio.
//!
//! Checked against the CLI's own schema at 0.166.0 (`codex app-server generate-json-schema`) and against recorded turns
//! in `tests/fixtures/codex-turns.jsonl`. `codex exec --json` lost: it streams, but it cannot ask the host before a
//! change, and the app server can. The CLI calls the app server experimental, so a version outside the tested line says
//! so on the card.
//!
//! A conversation is a thread, and a prompt starts a turn in it. The thread's id only comes with the reply to
//! `thread/start`, so a prompt sent before that waits here and goes out with that reply. Druck's tools come in as an
//! MCP server through `-c mcp_servers.druck...` for one run, which touches no `config.toml`.

use std::collections::HashMap;
use std::ffi::OsString;

use serde_json::{Value, json};

use super::backend::{Backend, Read, Verdict, parse_version};
use super::cli::{CliState, Login};
use super::session::{Access, Event, PermissionRequest, SessionConfig};
use super::tools;

/// The CLI line this protocol code was captured and tested against.
const TESTED: (u32, u32) = (0, 166);

/// The ids of the two requests this side sends once. Prompts and interrupts count on from there.
const INITIALIZE: u64 = 1;
const THREAD: u64 = 2;

/// The tool names the rest of the module knows a shell command and a file change by.
const COMMAND: &str = "Bash";
const PATCH: &str = "Patch";

pub struct Codex {
    next_id: u64,
    thread: Option<String>,
    /// The running turn, which an interrupt names.
    turn: Option<String>,
    /// Prompts sent before the thread had its id.
    waiting: Vec<String>,
    /// What each running item was started with, for the approval that names it only by id.
    inputs: HashMap<String, Value>,
    /// The assistant has said something this turn, so its next message starts a new paragraph.
    spoke: bool,
}

impl Default for Codex {
    fn default() -> Self {
        Self {
            next_id: THREAD,
            thread: None,
            turn: None,
            waiting: Vec::new(),
            inputs: HashMap::new(),
            spoke: false,
        }
    }
}

impl Codex {
    fn turn_start(&mut self, thread: &str, text: &str) -> Value {
        self.next_id += 1;
        json!({
            "id": self.next_id,
            "method": "turn/start",
            "params": {"threadId": thread, "input": [{"type": "text", "text": text}]},
        })
    }

    fn started(&mut self, item: &Value) -> Vec<Event> {
        let id = text(&item["id"]);
        let (name, input) = match item["type"].as_str() {
            Some("agentMessage") => {
                let gap = std::mem::replace(&mut self.spoke, true);
                return if gap {
                    vec![Event::TextDelta("\n\n".to_owned())]
                } else {
                    Vec::new()
                };
            }
            Some("commandExecution") => (COMMAND.to_owned(), json!({"command": item["command"]})),
            Some("fileChange") => (
                PATCH.to_owned(),
                json!({"file_path": item["changes"][0]["path"], "changes": item["changes"]}),
            ),
            Some("mcpToolCall") => (
                format!("mcp__{}__{}", text(&item["server"]), text(&item["tool"])),
                item["arguments"].clone(),
            ),
            Some("webSearch") => ("WebSearch".to_owned(), json!({"query": item["query"]})),
            _ => return Vec::new(),
        };
        self.inputs.insert(id.clone(), input.clone());
        vec![Event::ToolUse { id, name, input }]
    }

    fn completed(&mut self, item: &Value) -> Vec<Event> {
        let id = text(&item["id"]);
        if self.inputs.remove(&id).is_none() {
            return Vec::new();
        }
        let failed = matches!(item["status"].as_str(), Some("failed" | "declined")) || !item["error"].is_null();
        let content = match item["type"].as_str() {
            Some("commandExecution") => text(&item["aggregatedOutput"]),
            Some("mcpToolCall") => (item["result"]["content"].as_array())
                .map(|blocks| {
                    let texts: Vec<_> = blocks.iter().filter_map(|block| block["text"].as_str()).collect();
                    texts.join("\n")
                })
                .unwrap_or_else(|| text(&item["error"]["message"])),
            _ => String::new(),
        };
        vec![Event::ToolResult {
            tool_use_id: id,
            content,
            is_error: failed,
            details: None,
        }]
    }

    /// A request the CLI waits on. Some are answered here at once, and the rest go to the author.
    fn asked(&mut self, id: &Value, method: &str, params: &Value) -> Read {
        let item = params["itemId"].as_str().map(str::to_owned);
        let known = item.as_ref().and_then(|item| self.inputs.get(item)).cloned();
        let ask = |tool_name: &str, input: Value| {
            Read::events(vec![Event::Permission(PermissionRequest {
                request_id: id.to_string(),
                tool_use_id: item.clone(),
                tool_name: tool_name.to_owned(),
                input,
                suggestions: vec![json!({"type": "codex", "destination": "session"})],
            })])
        };
        match method {
            "item/commandExecution/requestApproval" => {
                ask(COMMAND, known.unwrap_or_else(|| json!({"command": params["command"]})))
            }
            "item/fileChange/requestApproval" => ask(PATCH, known.unwrap_or(Value::Null)),
            "mcpServer/elicitation/request" if params["_meta"]["codex_approval_kind"] == "mcp_tool_call" => {
                // Druck's own tools change nothing, and the app started their server itself.
                if params["serverName"] == tools::SERVER {
                    return Read {
                        events: Vec::new(),
                        replies: vec![json!({"id": id, "result": {"action": "accept"}})],
                    };
                }
                let server = format!("mcp__{}", text(&params["serverName"]));
                ask(&server, params["_meta"]["tool_params"].clone())
            }
            // A form or a question the card cannot show. Without an answer the CLI would wait for good.
            _ => Read {
                events: vec![Event::Raw(json!({"method": method, "params": params}))],
                replies: vec![
                    json!({"id": id, "error": {"code": -32601, "message": "Druck cannot answer this request."}}),
                ],
            },
        }
    }

    fn answered(&mut self, id: u64, message: &Value) -> Read {
        let error = message["error"]["message"].as_str();
        match (id, error) {
            (INITIALIZE, _) => Read::default(),
            (THREAD, None) => {
                let thread = text(&message["result"]["thread"]["id"]);
                let waiting = std::mem::take(&mut self.waiting);
                let replies = waiting.iter().map(|prompt| self.turn_start(&thread, prompt)).collect();
                let started = Event::Started {
                    session_id: thread.clone(),
                    model: text(&message["result"]["model"]),
                    login: String::new(),
                };
                self.thread = Some(thread);
                Read {
                    events: vec![started],
                    replies,
                }
            }
            // Without a thread nothing can follow, which is what an exit means to the conversation.
            (THREAD, Some(error)) => Read::events(vec![Event::Exited(error.to_owned())]),
            (_, Some(error)) => Read::events(vec![Event::TurnFinished {
                session_id: self.thread.clone().unwrap_or_default(),
                is_error: true,
                stopped: false,
                result: error.to_owned(),
            }]),
            (_, None) => Read::default(),
        }
    }
}

impl Backend for Codex {
    fn name(&self) -> &'static str {
        "Codex"
    }

    fn vendor(&self) -> &'static str {
        "OpenAI"
    }

    fn memory_file(&self) -> &'static str {
        "AGENTS.md"
    }

    fn subscription(&self) -> &'static str {
        "ChatGPT"
    }

    fn program(&self) -> &'static str {
        "codex"
    }

    /// The model is the one `~/.codex/config.toml` names: the CLI's list changes too often to keep one here.
    fn models(&self) -> &'static [&'static str] {
        &[]
    }

    fn classify_version(&self, output: &str) -> CliState {
        let version = output.split_whitespace().nth(1).unwrap_or_default();
        match parse_version(version) {
            None => CliState::Unreadable(output.trim().to_owned()),
            Some((major, minor, _)) if (major, minor) == TESTED => CliState::Ready(version.to_owned()),
            Some(_) => CliState::Untested(version.to_owned()),
        }
    }

    fn login_args(&self) -> &'static [&'static str] {
        &["login", "status"]
    }

    fn classify_login(&self, output: &str) -> Login {
        let said = output.to_lowercase();
        let logged_in = |method: &str, subscription| Login::LoggedIn {
            method: method.to_owned(),
            plan: None,
            subscription,
        };
        if said.contains("not logged in") {
            Login::LoggedOut
        } else if said.contains("chatgpt") {
            logged_in("ChatGPT", true)
        } else if said.contains("api key") {
            logged_in("API key", false)
        } else {
            Login::Unknown
        }
    }

    fn args(&self, config: &SessionConfig) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["app-server".into()];
        if let Some(executable) = &config.tool_server {
            // The values are TOML, and a JSON string or list of strings is valid TOML.
            let server = format!("mcp_servers.{}", tools::SERVER);
            args.extend([
                "-c".into(),
                format!("{server}.command={}", json!(executable)).into(),
                "-c".into(),
                format!("{server}.args={}", json!([tools::FLAG])).into(),
            ]);
        }
        args.extend(config.extra_args.iter().cloned());
        args
    }

    fn open(&mut self, config: &SessionConfig) -> Vec<Value> {
        let mut params = json!({"cwd": config.cwd});
        if let Some(model) = &config.model {
            params["model"] = json!(model);
        }
        if config.tool_server.is_some() {
            params["developerInstructions"] = json!(tools::SYSTEM_PROMPT);
        }
        let sandbox = match config.access {
            Access::AsConfigured => None,
            Access::Edits => Some("workspace-write"),
            Access::ReadOnly => Some("read-only"),
        };
        if let Some(sandbox) = sandbox {
            params["sandbox"] = json!(sandbox);
            params["approvalPolicy"] = json!("on-request");
        }
        let method = match &config.resume {
            Some(thread) => {
                params["threadId"] = json!(thread);
                "thread/resume"
            }
            None => "thread/start",
        };
        let client = json!({"name": "druck", "title": "Druck", "version": env!("CARGO_PKG_VERSION")});
        vec![
            json!({"id": INITIALIZE, "method": "initialize", "params": {"clientInfo": client}}),
            json!({"method": "initialized"}),
            json!({"id": THREAD, "method": method, "params": params}),
        ]
    }

    fn prompt(&mut self, text: &str) -> Vec<Value> {
        match self.thread.clone() {
            Some(thread) => vec![self.turn_start(&thread, text)],
            None => {
                self.waiting.push(text.to_owned());
                Vec::new()
            }
        }
    }

    fn answer(&mut self, request: &PermissionRequest, verdict: Verdict) -> Value {
        // The id goes back as it came, a number or a string.
        let id: Value = serde_json::from_str(&request.request_id).unwrap_or_else(|_| json!(request.request_id));
        let result = if request.tool_name.starts_with("mcp__") {
            // An MCP call's own answer has no place for a reason.
            let action = match verdict {
                Verdict::Allow | Verdict::AllowForSession(_) => "accept",
                Verdict::Deny(_) => "decline",
            };
            json!({"action": action})
        } else {
            let decision = match verdict {
                Verdict::Allow => "accept",
                Verdict::AllowForSession(_) => "acceptForSession",
                Verdict::Deny(_) => "decline",
            };
            json!({"decision": decision})
        };
        json!({"id": id, "result": result})
    }

    fn interrupt(&mut self) -> Vec<Value> {
        let (Some(thread), Some(turn)) = (&self.thread, &self.turn) else {
            return Vec::new();
        };
        self.next_id += 1;
        vec![json!({
            "id": self.next_id,
            "method": "turn/interrupt",
            "params": {"threadId": thread, "turnId": turn},
        })]
    }

    fn read(&mut self, line: &str) -> Read {
        if line.trim().is_empty() {
            return Read::default();
        }
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => return Read::events(vec![Event::Error(format!("unreadable CLI line: {error}"))]),
        };
        let params = &message["params"];
        let read = match (message["method"].as_str(), message.get("id")) {
            (Some(method), Some(id)) => self.asked(id, method, params),
            (None, Some(id)) => self.answered(id.as_u64().unwrap_or_default(), &message),
            (Some("item/agentMessage/delta"), None) => Read::events(vec![Event::TextDelta(text(&params["delta"]))]),
            (Some("item/started"), None) => Read::events(self.started(&params["item"])),
            (Some("item/completed"), None) => Read::events(self.completed(&params["item"])),
            (Some("turn/started"), None) => {
                self.turn = params["turn"]["id"].as_str().map(str::to_owned);
                self.spoke = false;
                Read::default()
            }
            (Some("turn/completed"), None) => {
                self.turn = None;
                self.inputs.clear();
                let turn = &params["turn"];
                Read::events(vec![Event::TurnFinished {
                    session_id: text(&params["threadId"]),
                    is_error: turn["status"] == "failed",
                    stopped: turn["status"] == "interrupted",
                    result: text(&turn["error"]["message"]),
                }])
            }
            _ => Read::default(),
        };
        if read.events.is_empty() && read.replies.is_empty() {
            Read::events(vec![Event::Raw(message)])
        } else {
            read
        }
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real output of three turns: a margin note through Druck's tool, a declined patch, and an accepted one.
    const CAPTURE: &str = include_str!("../../tests/fixtures/codex-turns.jsonl");

    /// Each turn's events and replies, with a fresh backend per turn as each was its own process.
    fn captured() -> Vec<Read> {
        let mut turns: Vec<Read> = Vec::new();
        let mut codex = Codex::default();
        for line in CAPTURE.lines() {
            if line.starts_with(r#"{"id":1,"#) {
                codex = Codex::default();
                turns.push(Read::default());
            }
            let read = codex.read(line);
            let turn = turns.last_mut().expect("a turn starts with the handshake");
            turn.events.extend(read.events);
            turn.replies.extend(read.replies);
        }
        turns
    }

    fn tool_uses(read: &Read) -> Vec<(&str, &Value)> {
        (read.events.iter())
            .filter_map(|event| match event {
                Event::ToolUse { name, input, .. } => Some((name.as_str(), input)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_captured_note_reaches_druck_as_its_own_tool_and_is_allowed_without_asking() {
        let turns = captured();
        let note = &turns[0];
        assert!(
            note.events
                .iter()
                .any(|event| matches!(event, Event::Started { session_id, model, .. }
            if session_id.starts_with("01a1") && model == "gpt-5.6-luna"))
        );
        let (_, input) = (tool_uses(note).into_iter())
            .find(|(name, _)| *name == "mcp__druck__leave_note")
            .expect("the note tool");
        assert_eq!(input["quote"], "Hello");
        assert_eq!(note.replies, vec![json!({"id": 0, "result": {"action": "accept"}})]);
        assert!(!note.events.iter().any(|event| matches!(event, Event::Permission(_))));
        let said: String = (note.events.iter())
            .filter_map(|event| match event {
                Event::TextDelta(delta) => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert!(said.contains("using the document tool, without changing the file contents.\n\nAdded one margin note"));
        assert!(matches!(
            note.events.last(),
            Some(Event::TurnFinished {
                is_error: false,
                stopped: false,
                ..
            })
        ));
    }

    #[test]
    fn a_captured_patch_asks_with_what_it_would_change() {
        let turns = captured();
        for (turn, declined) in [(&turns[1], true), (&turns[2], false)] {
            let request = (turn.events.iter())
                .find_map(|event| match event {
                    Event::Permission(request) => Some(request),
                    _ => None,
                })
                .expect("an approval");
            assert_eq!(request.tool_name, PATCH);
            assert_eq!(request.input["file_path"], "/book/a.md");
            assert!(text(&request.input["changes"][0]["diff"]).contains("+Howdy world."));
            let result = (turn.events.iter())
                .find_map(|event| match event {
                    Event::ToolResult {
                        tool_use_id, is_error, ..
                    } if Some(tool_use_id) == request.tool_use_id.as_ref() => Some(*is_error),
                    _ => None,
                })
                .expect("the patch ends");
            assert_eq!(result, declined);
        }
    }

    #[test]
    fn answers_go_back_under_the_requests_own_id() {
        let mut codex = Codex::default();
        let request = |tool: &str| PermissionRequest {
            request_id: "7".into(),
            tool_use_id: None,
            tool_name: tool.into(),
            input: Value::Null,
            suggestions: Vec::new(),
        };
        let session = json!({});
        assert_eq!(
            codex.answer(&request(PATCH), Verdict::Allow),
            json!({"id": 7, "result": {"decision": "accept"}})
        );
        assert_eq!(
            codex.answer(&request(COMMAND), Verdict::AllowForSession(&session))["result"]["decision"],
            "acceptForSession"
        );
        assert_eq!(
            codex.answer(&request(PATCH), Verdict::Deny("no"))["result"]["decision"],
            "decline"
        );
        assert_eq!(
            codex.answer(&request("mcp__other"), Verdict::Deny("no"))["result"]["action"],
            "decline"
        );
    }

    #[test]
    fn a_prompt_waits_for_the_thread_and_an_interrupt_names_the_turn() {
        let mut codex = Codex::default();
        assert!(codex.prompt("Hello").is_empty());
        assert!(codex.interrupt().is_empty());
        let read = codex.read(r#"{"id":2,"result":{"thread":{"id":"t1"},"model":"m"}}"#);
        assert_eq!(read.replies[0]["method"], "turn/start");
        assert_eq!(read.replies[0]["params"]["threadId"], "t1");
        assert_eq!(read.replies[0]["params"]["input"][0]["text"], "Hello");
        assert_eq!(codex.prompt("Again")[0]["params"]["input"][0]["text"], "Again");
        codex.read(r#"{"method":"turn/started","params":{"threadId":"t1","turn":{"id":"u1"}}}"#);
        let interrupt = &codex.interrupt()[0];
        assert_eq!(interrupt["method"], "turn/interrupt");
        assert_eq!(interrupt["params"], json!({"threadId": "t1", "turnId": "u1"}));
    }

    #[test]
    fn the_command_line_brings_the_tools_and_the_thread_says_what_may_be_written() {
        let config = SessionConfig {
            cwd: "/book".into(),
            model: Some("gpt-5.6-luna".into()),
            tool_server: Some("/Apps/Druck App/druck".into()),
            access: Access::ReadOnly,
            ..SessionConfig::default()
        };
        let mut codex = Codex::default();
        let args = codex.args(&config);
        assert_eq!(args[0], "app-server");
        assert_eq!(args[2], r#"mcp_servers.druck.command="/Apps/Druck App/druck""#);
        assert_eq!(args[4], r#"mcp_servers.druck.args=["--agent-note-tool"]"#);
        let opening = codex.open(&config);
        assert_eq!(opening[0]["method"], "initialize");
        assert_eq!(opening[1], json!({"method": "initialized"}));
        let thread = &opening[2];
        assert_eq!(thread["method"], "thread/start");
        assert_eq!(thread["params"]["cwd"], "/book");
        assert_eq!(thread["params"]["model"], "gpt-5.6-luna");
        assert_eq!(thread["params"]["sandbox"], "read-only");

        let resumed = SessionConfig {
            resume: Some("t9".into()),
            ..SessionConfig::default()
        };
        let thread = &Codex::default().open(&resumed)[2];
        assert_eq!(thread["method"], "thread/resume");
        assert_eq!(thread["params"]["threadId"], "t9");
        assert!(
            thread["params"].get("sandbox").is_none(),
            "the author's own settings decide"
        );
    }

    #[test]
    fn a_refused_thread_ends_the_session_and_unknown_lines_stay_raw() {
        let mut codex = Codex::default();
        let read = codex.read(r#"{"id":2,"error":{"code":-32600,"message":"no such thread"}}"#);
        assert_eq!(read.events, vec![Event::Exited("no such thread".into())]);
        let read = codex.read(r#"{"method":"brand/new","params":{"a":1}}"#);
        assert!(matches!(read.events.as_slice(), [Event::Raw(_)]));
        let read = codex.read(r#"{"method":"item/tool/requestUserInput","id":"q1","params":{}}"#);
        assert_eq!(read.replies[0]["id"], "q1");
        assert!(read.replies[0]["error"].is_object());
    }

    #[test]
    fn classifies_versions_and_logins() {
        let codex = Codex::default();
        assert_eq!(
            codex.classify_version("codex-cli 0.166.0\n"),
            CliState::Ready("0.166.0".into())
        );
        assert_eq!(
            codex.classify_version("codex-cli 0.170.1"),
            CliState::Untested("0.170.1".into())
        );
        assert!(matches!(codex.classify_version("codex"), CliState::Unreadable(_)));
        assert_eq!(
            codex.classify_login("Logged in using ChatGPT\n"),
            Login::LoggedIn {
                method: "ChatGPT".into(),
                plan: None,
                subscription: true
            }
        );
        assert!(matches!(
            codex.classify_login("Logged in using an API key - sk-***"),
            Login::LoggedIn {
                subscription: false,
                ..
            }
        ));
        assert_eq!(codex.classify_login("Not logged in"), Login::LoggedOut);
    }
}
