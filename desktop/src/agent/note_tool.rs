//! The small local MCP server that gives the CLI a tool for leaving a margin note.
//!
//! The app starts the CLI with `--mcp-config` naming its own executable with [`FLAG`], so the author's settings stay as
//! they are. The server only checks a note and answers: the app takes the note from the tool call in the CLI's stream
//! and stays the one writer of `.druck/notes.json`. Messages are one JSON object per line on stdin and stdout.

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::{Component, Path};

use serde_json::{Value, json};

/// The argument that runs the app as this server.
pub const FLAG: &str = "--agent-note-tool";
/// The tool's full name as the CLI calls it: `mcp__<server>__<tool>`.
pub const TOOL: &str = "mcp__druck__leave_note";

const SERVER: &str = "druck";
const SHORT_NAME: &str = "leave_note";

/// What the CLI is told to do with this tool and with the files. For the model, so never translated.
pub const SYSTEM_PROMPT: &str = "You work inside Druck, a writing app for long-form authors. The book's Markdown files \
are the truth. To change text, edit the files with your normal tools: the author reviews each change in the text and \
accepts or rejects it passage by passage, so change only what was asked and leave every other byte alone. A new chapter \
is a new Markdown file in manuscript/, added to the chapters list in druck.yaml. When the author asks for advice, \
feedback, or a review rather than a change, do not edit anything: call mcp__druck__leave_note once per point, quoting \
the exact text it is about. In your replies, be brief and say what you did.";

/// The value for `--mcp-config`: this executable as a server, for this session only.
pub fn mcp_config(executable: &Path) -> String {
    json!({"mcpServers": {SERVER: {"type": "stdio", "command": executable, "args": [FLAG]}}}).to_string()
}

/// The flags that give the CLI the tool for this session only, which the author's settings never see. Without this
/// executable's own path there is no tool, and the rest works.
pub fn cli_args() -> Vec<OsString> {
    let Ok(executable) = std::env::current_exe() else {
        return Vec::new();
    };
    vec![
        "--mcp-config".into(),
        mcp_config(&executable).into(),
        "--allowedTools".into(),
        TOOL.into(),
        "--append-system-prompt".into(),
        SYSTEM_PROMPT.into(),
    ]
}

/// Answers the CLI until it closes stdin. `base` is the project folder, the CLI's working directory.
pub fn serve(base: &Path) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(message) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if let Some(reply) = answer(&message, base) {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// The reply to one message. A notification has none.
fn answer(message: &Value, base: &Path) -> Option<Value> {
    let id = message.get("id")?.clone();
    let params = &message["params"];
    let result = match message["method"].as_str()? {
        "initialize" => json!({
            "protocolVersion": params["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER, "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": [tool()]}),
        "tools/call" if params["name"] == SHORT_NAME => call(&params["arguments"], base),
        method => {
            let error = json!({"code": -32601, "message": format!("unknown method {method}")});
            return Some(json!({"jsonrpc": "2.0", "id": id, "error": error}));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn tool() -> Value {
    json!({
        "name": SHORT_NAME,
        "description": "Leave a margin note beside a passage of the book, for advice that changes nothing. The author can \
            apply its suggestion, reply, or dismiss it.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "chapter": {"type": "string", "description": "The chapter's file, relative to the project folder."},
                "quote": {"type": "string", "description": "The exact text of the passage, copied from the file. A short phrase is enough, on one line."},
                "title": {"type": "string", "description": "What the note is about, in a word or two."},
                "text": {"type": "string", "description": "The advice, in one short sentence, in the book's language."},
                "suggestion": {"type": "string", "description": "Optional: the text to replace the quote with if the author applies the note."},
            },
            "required": ["chapter", "quote", "title", "text"],
        },
    })
}

fn call(input: &Value, base: &Path) -> Value {
    let (text, is_error) = match check(input, base) {
        Ok(()) => ("The note stands beside the passage.".to_owned(), false),
        Err(problem) => (problem, true),
    };
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

/// What makes a note unusable, told to the model so it can try again.
fn check(input: &Value, base: &Path) -> Result<(), String> {
    for field in ["chapter", "quote", "title", "text"] {
        if input[field].as_str().is_none_or(|value| value.trim().is_empty()) {
            return Err(format!("{field} is required."));
        }
    }
    let chapter = Path::new(input["chapter"].as_str().unwrap_or_default());
    if chapter.is_absolute() || chapter.components().any(|part| part == Component::ParentDir) {
        return Err("chapter must be relative to the project folder.".to_owned());
    }
    let text =
        std::fs::read_to_string(base.join(chapter)).map_err(|error| format!("cannot read that chapter: {error}"))?;
    if !text.contains(input["quote"].as_str().unwrap_or_default()) {
        return Err("quote is not in that chapter. Copy it exactly from the file.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params})
    }

    #[test]
    fn it_answers_the_handshake_and_lists_its_tool() {
        let base = Path::new(".");
        let reply = answer(&request("initialize", json!({"protocolVersion": "2025-06-18"})), base).unwrap();
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(reply["id"], 7);
        assert_eq!(answer(&json!({"method": "notifications/initialized"}), base), None);
        let list = answer(&request("tools/list", json!({})), base).unwrap();
        assert_eq!(list["result"]["tools"][0]["name"], SHORT_NAME);
        assert!(answer(&request("nonsense", json!({})), base).unwrap()["error"].is_object());
    }

    #[test]
    fn a_note_must_quote_a_chapter_of_the_project() {
        let folder = std::env::temp_dir().join(format!("druck-note-tool-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.md"), "The lamps were lit.\n").unwrap();
        let call_with = |chapter: &str, quote: &str| {
            let arguments = json!({"chapter": chapter, "quote": quote, "title": "T", "text": "Advice."});
            let params = json!({"name": SHORT_NAME, "arguments": arguments});
            answer(&request("tools/call", params), &folder).unwrap()["result"]["isError"].clone()
        };
        assert_eq!(call_with("a.md", "The lamps"), false);
        assert_eq!(call_with("a.md", "The candles"), true);
        assert_eq!(call_with("../a.md", "The lamps"), true);
        assert_eq!(call_with("missing.md", "The lamps"), true);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_flags_leave_the_authors_settings_and_servers_alone() {
        let args = cli_args();
        assert!(args.contains(&OsString::from("--mcp-config")));
        for flag in ["--strict-mcp-config", "--settings", "--setting-sources", "--tools"] {
            assert!(!args.contains(&OsString::from(flag)), "{flag}");
        }
    }

    #[test]
    fn the_config_names_this_executable_for_the_session() {
        let config: Value = serde_json::from_str(&mcp_config(Path::new("/Apps/Druck App/druck"))).unwrap();
        assert_eq!(config["mcpServers"]["druck"]["command"], "/Apps/Druck App/druck");
        assert_eq!(config["mcpServers"]["druck"]["args"][0], FLAG);
        assert_eq!(TOOL, format!("mcp__{SERVER}__{SHORT_NAME}"));
    }
}
