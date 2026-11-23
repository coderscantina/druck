//! Claude Code: `claude -p` speaking `stream-json` on both pipes, with tool approvals as control requests.
//!
//! Checked against the CLI's own `--help` at 2.1.319. `--permission-prompt-tool stdio` sends each approval to the host.
//! Druck's tools come in through `--mcp-config` for one run, so the author's settings stay as they are, and without
//! `--strict-mcp-config` their own servers still load.

use std::ffi::OsString;
use std::path::Path;

use serde_json::{Value, json};

use super::backend::{Backend, Read, Verdict, parse_version};
use super::cli::{CliState, Login};
use super::session::{Access, Event, PermissionRequest, SessionConfig};
use super::tools;

/// The CLI line this protocol code was captured and tested against.
const TESTED: (u32, u32) = (2, 1);
const TESTED_MIN_PATCH: u32 = 317;

/// The CLI's own tools that write files, which a session that may only read goes without.
const WRITING_TOOLS: &str = "Edit,Write,MultiEdit,NotebookEdit";

#[derive(Default)]
pub struct Claude {
    next_request: u32,
}

impl Backend for Claude {
    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn vendor(&self) -> &'static str {
        "Anthropic"
    }

    fn memory_file(&self) -> &'static str {
        "CLAUDE.md"
    }

    fn subscription(&self) -> &'static str {
        "Claude"
    }

    fn program(&self) -> &'static str {
        "claude"
    }

    fn models(&self) -> &'static [&'static str] {
        &["haiku", "sonnet", "opus"]
    }

    fn classify_version(&self, output: &str) -> CliState {
        let version = output.split_whitespace().next().unwrap_or_default();
        match parse_version(version) {
            None => CliState::Unreadable(output.trim().to_owned()),
            Some((major, minor, patch)) if (major, minor) == TESTED && patch >= TESTED_MIN_PATCH => {
                CliState::Ready(version.to_owned())
            }
            Some(_) => CliState::Untested(version.to_owned()),
        }
    }

    fn login_args(&self) -> &'static [&'static str] {
        &["auth", "status"]
    }

    fn classify_login(&self, output: &str) -> Login {
        let Ok(report) = serde_json::from_str::<Value>(output) else {
            return Login::Unknown;
        };
        match (report["loggedIn"].as_bool(), report["authMethod"].as_str()) {
            (Some(false), _) => Login::LoggedOut,
            (Some(true), Some(method)) => Login::LoggedIn {
                method: method.to_owned(),
                plan: report["subscriptionType"].as_str().map(str::to_owned),
                subscription: method == "claude.ai",
            },
            _ => Login::Unknown,
        }
    }

    fn args(&self, config: &SessionConfig) -> Vec<OsString> {
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
        match config.access {
            Access::AsConfigured => {}
            Access::Edits => args.extend(["--permission-mode".into(), "acceptEdits".into()]),
            Access::ReadOnly => args.extend(["--disallowedTools".into(), WRITING_TOOLS.into()]),
        }
        if let Some(executable) = &config.tool_server {
            let allowed = tools::NAMES.map(tools::full_name).join(",");
            args.extend([
                "--mcp-config".into(),
                mcp_config(executable, &tools::server_args(config)).into(),
                "--allowedTools".into(),
                allowed.into(),
                "--append-system-prompt".into(),
                tools::SYSTEM_PROMPT.into(),
            ]);
        }
        args.extend(config.extra_args.iter().cloned());
        args
    }

    fn open(&mut self, _: &SessionConfig) -> Vec<Value> {
        Vec::new()
    }

    fn prompt(&mut self, text: &str) -> Vec<Value> {
        vec![json!({"type": "user", "message": {"role": "user", "content": text}})]
    }

    fn answer(&mut self, request: &PermissionRequest, verdict: Verdict) -> Value {
        let response = match verdict {
            Verdict::Allow => {
                json!({"behavior": "allow", "updatedInput": request.input, "toolUseID": request.tool_use_id})
            }
            // Only a suggestion the CLI sent for this request is passed back.
            Verdict::AllowForSession(update) => json!({
                "behavior": "allow",
                "updatedInput": request.input,
                "updatedPermissions": [update],
                "toolUseID": request.tool_use_id,
            }),
            Verdict::Deny(message) => {
                json!({"behavior": "deny", "message": message, "toolUseID": request.tool_use_id})
            }
        };
        json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": request.request_id, "response": response},
        })
    }

    fn interrupt(&mut self) -> Vec<Value> {
        self.next_request += 1;
        let request_id = format!("druck-{}", self.next_request);
        vec![json!({"type": "control_request", "request_id": request_id, "request": {"subtype": "interrupt"}})]
    }

    fn read(&mut self, line: &str) -> Read {
        Read::events(parse_line(line))
    }
}

/// The value for `--mcp-config`: this executable as a server, for this session only.
fn mcp_config(executable: &Path, args: &[&Path]) -> String {
    let server = json!({"type": "stdio", "command": executable, "args": args});
    json!({"mcpServers": {tools::SERVER: server}}).to_string()
}

/// Turns one stdout line into events. Anything not recognised becomes [`Event::Raw`].
pub(super) fn parse_line(line: &str) -> Vec<Event> {
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
    use super::*;

    fn args_of(config: &SessionConfig) -> Vec<OsString> {
        Claude::default().args(config)
    }

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
    fn captured_turns_finish_with_their_session() {
        let finished: Vec<_> = captured_events()
            .into_iter()
            .filter_map(|e| {
                if let Event::TurnFinished {
                    session_id, is_error, ..
                } = e
                {
                    Some((session_id, is_error))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(finished.len(), 4);
        assert!(!finished[0].0.is_empty());
        assert!(!finished[2].1 && finished[3].1);
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
    fn resume_adds_the_session_id() {
        let config = SessionConfig {
            resume: Some("abc".into()),
            ..SessionConfig::default()
        };
        let args = args_of(&config);
        let at = args.iter().position(|a| a == "--resume").expect("flag");
        assert_eq!(args[at + 1], "abc");
    }

    #[test]
    fn the_cli_runs_with_its_own_settings_and_tools() {
        let args = args_of(&SessionConfig::default());
        for flag in ["--tools", "--setting-sources", "--strict-mcp-config", "--settings"] {
            assert!(
                !args.iter().any(|arg| arg == flag),
                "{flag} would change what a terminal does"
            );
        }
    }

    #[test]
    fn a_pass_sets_what_the_session_may_write() {
        let with = |access| {
            args_of(&SessionConfig {
                access,
                ..SessionConfig::default()
            })
        };
        let edits = with(Access::Edits);
        let at = edits.iter().position(|arg| arg == "--permission-mode").expect("flag");
        assert_eq!(edits[at + 1], "acceptEdits");
        let read_only = with(Access::ReadOnly);
        let at = read_only
            .iter()
            .position(|arg| arg == "--disallowedTools")
            .expect("flag");
        assert_eq!(read_only[at + 1], WRITING_TOOLS);
    }

    #[test]
    fn the_tools_come_in_for_one_session_and_leave_the_authors_servers_alone() {
        let args = args_of(&SessionConfig {
            tool_server: Some("/Apps/Druck App/druck".into()),
            document: "/books/My Book".into(),
            ..SessionConfig::default()
        });
        let at = args.iter().position(|arg| arg == "--mcp-config").expect("flag");
        let config: Value = serde_json::from_str(args[at + 1].to_str().unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["druck"]["command"], "/Apps/Druck App/druck");
        assert_eq!(
            config["mcpServers"]["druck"]["args"],
            json!([tools::FLAG, "/books/My Book"])
        );
        let at = args.iter().position(|arg| arg == "--allowedTools").expect("flag");
        assert_eq!(
            args[at + 1],
            "mcp__druck__leave_note,mcp__druck__record_style,mcp__druck__try_fixes"
        );
        assert!(!args.iter().any(|arg| arg == "--strict-mcp-config"));

        let typesetting = args_of(&SessionConfig {
            tool_server: Some("/Apps/Druck App/druck".into()),
            document: "/books/My Book".into(),
            book_copy: Some("/tmp/copy.json".into()),
            ..SessionConfig::default()
        });
        let at = typesetting.iter().position(|arg| arg == "--mcp-config").expect("flag");
        let config: Value = serde_json::from_str(typesetting[at + 1].to_str().unwrap()).unwrap();
        assert_eq!(
            config["mcpServers"]["druck"]["args"],
            json!([tools::FLAG, "/books/My Book", "/tmp/copy.json"]),
            "the trial tool reads the app's copy of the text"
        );
    }

    #[test]
    fn classifies_versions() {
        let classify = |output| Claude::default().classify_version(output);
        assert_eq!(classify("2.1.317 (Claude Code)\n"), CliState::Ready("2.1.317".into()));
        assert_eq!(classify("2.1.400 (Claude Code)"), CliState::Ready("2.1.400".into()));
        assert_eq!(classify("2.1.100 (Claude Code)"), CliState::Untested("2.1.100".into()));
        assert_eq!(classify("3.0.0 (Claude Code)"), CliState::Untested("3.0.0".into()));
        assert_eq!(classify("not a version"), CliState::Unreadable("not a version".into()));
    }

    #[test]
    fn a_login_report_keeps_only_the_method_and_plan() {
        let report = r#"{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
            "email": "someone@example.com", "orgId": "o-1", "subscriptionType": "max"}"#;
        let claude = Claude::default();
        let login = claude.classify_login(report);
        assert_eq!(
            login,
            Login::LoggedIn {
                method: "claude.ai".into(),
                plan: Some("max".into()),
                subscription: true,
            }
        );
        assert!(!format!("{login:?}").contains("example.com"));
        assert_eq!(claude.classify_login(r#"{"loggedIn": false}"#), Login::LoggedOut);
        assert_eq!(claude.classify_login("Not logged in"), Login::Unknown);
    }
}
