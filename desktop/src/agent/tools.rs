//! The small local MCP server that gives the CLI Druck's tools: one leaves a margin note, one records a style decision.
//!
//! The app starts the CLI with its own executable named as a server for one session, with [`FLAG`], so the author's
//! settings stay as they are. The server only checks a call and answers: the app takes the call from the CLI's stream and
//! stays the one writer of `.druck/notes.json` and the style sheet. Messages are one JSON object per line on stdin and
//! stdout.

use std::io::{self, BufRead, Write};
use std::path::{Component, Path};

use serde_json::{Value, json};

/// The argument that runs the app as this server.
pub const FLAG: &str = "--agent-note-tool";
/// The server's name, which is part of each tool's full name.
pub const SERVER: &str = "druck";
pub const LEAVE_NOTE: &str = "leave_note";
pub const RECORD_STYLE: &str = "record_style";
pub const NAMES: [&str; 2] = [LEAVE_NOTE, RECORD_STYLE];

/// A tool's name as a CLI's stream has it: `mcp__<server>__<tool>`.
pub fn full_name(tool: &str) -> String {
    format!("mcp__{SERVER}__{tool}")
}

/// What the CLI is told to do with these tools and with the files. For the model, so never translated.
pub const SYSTEM_PROMPT: &str = "You work inside Druck, a writing app for long-form authors. The book's Markdown files \
are the truth. To change text, edit the files with your normal tools: the author reviews each change in the text and \
accepts or rejects it passage by passage, so change only what was asked and leave every other byte alone. A new chapter \
is a new Markdown file in manuscript/, added to the chapters list in druck.yaml. When the author asks for advice, \
feedback, or a review rather than a change, do not edit anything: call the druck tool leave_note once per point, quoting \
the exact text it is about. The druck tool record_style keeps the book's style sheet: call it only when an editing pass \
tells you to, and never edit the style sheet file yourself. In your replies, be brief and say what you did.";

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
        "tools/list" => json!({"tools": [leave_note(), record_style()]}),
        "tools/call" if params["name"] == LEAVE_NOTE => call(
            check_note(&params["arguments"], base),
            "The note stands beside the passage.",
        ),
        "tools/call" if params["name"] == RECORD_STYLE => {
            call(check_style(&params["arguments"]), "The style sheet has it.")
        }
        method => {
            let error = json!({"code": -32601, "message": format!("unknown method {method}")});
            return Some(json!({"jsonrpc": "2.0", "id": id, "error": error}));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn leave_note() -> Value {
    json!({
        "name": LEAVE_NOTE,
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

fn record_style() -> Value {
    json!({
        "name": RECORD_STYLE,
        "description": "Record one decision in the book's style sheet, which later editing passes follow. Recording an \
            entry again replaces its rule.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "entry": {"type": "string", "description": "What the decision is about, as it is written in the book: a word, a name, a kind of number."},
                "rule": {"type": "string", "description": "The decision, in a few words, in the book's language."},
            },
            "required": ["entry", "rule"],
        },
    })
}

fn call(checked: Result<(), String>, done: &str) -> Value {
    let (text, is_error) = match checked {
        Ok(()) => (done.to_owned(), false),
        Err(problem) => (problem, true),
    };
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn required(input: &Value, fields: &[&str]) -> Result<(), String> {
    match fields
        .iter()
        .find(|field| input[**field].as_str().is_none_or(|value| value.trim().is_empty()))
    {
        Some(field) => Err(format!("{field} is required.")),
        None => Ok(()),
    }
}

fn check_style(input: &Value) -> Result<(), String> {
    required(input, &["entry", "rule"])
}

/// What makes a note unusable, told to the model so it can try again.
fn check_note(input: &Value, base: &Path) -> Result<(), String> {
    required(input, &["chapter", "quote", "title", "text"])?;
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
        assert_eq!(list["result"]["tools"][0]["name"], LEAVE_NOTE);
        assert_eq!(list["result"]["tools"][1]["name"], RECORD_STYLE);
        assert!(answer(&request("nonsense", json!({})), base).unwrap()["error"].is_object());
    }

    #[test]
    fn a_note_must_quote_a_chapter_of_the_project() {
        let folder = std::env::temp_dir().join(format!("druck-note-tool-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.md"), "The lamps were lit.\n").unwrap();
        let call_with = |chapter: &str, quote: &str| {
            let arguments = json!({"chapter": chapter, "quote": quote, "title": "T", "text": "Advice."});
            let params = json!({"name": LEAVE_NOTE, "arguments": arguments});
            answer(&request("tools/call", params), &folder).unwrap()["result"]["isError"].clone()
        };
        assert_eq!(call_with("a.md", "The lamps"), false);
        assert_eq!(call_with("a.md", "The candles"), true);
        assert_eq!(call_with("../a.md", "The lamps"), true);
        assert_eq!(call_with("missing.md", "The lamps"), true);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_style_decision_needs_its_entry_and_rule() {
        let call_with = |arguments: Value| {
            let params = json!({"name": RECORD_STYLE, "arguments": arguments});
            answer(&request("tools/call", params), Path::new("."))?["result"]["isError"].as_bool()
        };
        assert_eq!(call_with(json!({"entry": "e-mail", "rule": "hyphenated"})), Some(false));
        assert_eq!(call_with(json!({"entry": "e-mail", "rule": " "})), Some(true));
    }
}
