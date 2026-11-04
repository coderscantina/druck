//! A tool call waiting for the author's answer, and what the panel shows to help give it.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::diff::{self, DiffLine};
use super::session::PermissionRequest;

/// How much of a command or other input the panel shows.
const PREVIEW_LIMIT: usize = 600;

/// What to show under the tool's name.
#[derive(Debug, Clone, PartialEq)]
pub enum Preview {
    Diff(Vec<DiffLine>),
    /// A command, or the input of a tool the panel does not know.
    Text(String),
    /// A new file or an empty one.
    Nothing,
}

/// The CLI's offer to remember an answer for as long as its process runs, in words for a button.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRule {
    pub label: SessionLabel,
    /// The suggestion as the CLI sent it, passed back unchanged when the author chooses it.
    pub update: Value,
}

/// What a session rule does, for the strings table to word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLabel {
    AcceptEdits,
    Mode(String),
    Rule(String),
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Approval {
    pub request: PermissionRequest,
    /// The file or command, relative to the project folder where it can be.
    pub target: String,
    /// The file an edit or write changes.
    pub file: Option<PathBuf>,
    pub preview: Preview,
    pub session_rule: Option<SessionRule>,
}

impl Approval {
    /// `read` gives a file's text, for the diff of what a tool would change.
    pub fn new(request: PermissionRequest, base: &Path, read: &dyn Fn(&Path) -> Option<String>) -> Self {
        let input = &request.input;
        let file = input["file_path"].as_str().map(PathBuf::from);
        let preview = match (request.tool_name.as_str(), &file) {
            ("Edit", Some(file)) => match (input["old_string"].as_str(), input["new_string"].as_str()) {
                (Some(old), Some(new)) => Preview::Diff(diff::edit(read(file).as_deref(), old, new)),
                _ => text_preview(input),
            },
            ("Write", Some(file)) => match input["content"].as_str() {
                Some(content) => {
                    let current = read(file);
                    if current.is_none() && content.is_empty() {
                        Preview::Nothing
                    } else {
                        Preview::Diff(diff::diff(current.as_deref().unwrap_or_default(), content))
                    }
                }
                None => text_preview(input),
            },
            ("Bash", _) => Preview::Text(clip(input["command"].as_str().unwrap_or_default())),
            _ => text_preview(input),
        };
        Self {
            target: target(&request.tool_name, input, base),
            session_rule: session_rule(&request.suggestions),
            request,
            file,
            preview,
        }
    }
}

fn text_preview(input: &Value) -> Preview {
    match input {
        Value::Null => Preview::Nothing,
        input => Preview::Text(clip(&serde_json::to_string_pretty(input).unwrap_or_default())),
    }
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(PREVIEW_LIMIT) {
        Some((end, _)) => format!("{}\u{2026}", &text[..end]),
        None => text.to_owned(),
    }
}

/// What a tool call is about, in a few words: a file relative to the project, a command's first line, a pattern.
pub fn target(tool: &str, input: &Value, base: &Path) -> String {
    for key in ["file_path", "path", "notebook_path"] {
        if let Some(path) = input[key].as_str() {
            return Path::new(path)
                .strip_prefix(base)
                .map_or_else(|_| path.to_owned(), |relative| relative.display().to_string());
        }
    }
    let first_line = |text: &str| {
        text.lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(80)
            .collect::<String>()
    };
    for key in ["command", "pattern", "url", "query", "description", "prompt"] {
        if let Some(text) = input[key].as_str() {
            return first_line(text);
        }
    }
    tool.to_owned()
}

/// The suggestion that lasts for the CLI's process. One that writes to a settings file would outlive the panel, and
/// the panel offers nothing that does.
fn session_rule(suggestions: &[Value]) -> Option<SessionRule> {
    let update = suggestions.iter().find(|s| s["destination"] == "session")?;
    let label = match update["type"].as_str() {
        Some("setMode") if update["mode"] == "acceptEdits" => SessionLabel::AcceptEdits,
        Some("setMode") => SessionLabel::Mode(update["mode"].as_str().unwrap_or_default().to_owned()),
        Some("addRules") => {
            let rules: Vec<String> = update["rules"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|rule| match rule["ruleContent"].as_str() {
                    Some(content) => format!("{}({content})", rule["toolName"].as_str().unwrap_or_default()),
                    None => rule["toolName"].as_str().unwrap_or_default().to_owned(),
                })
                .collect();
            SessionLabel::Rule(rules.join(", "))
        }
        _ => SessionLabel::Other,
    };
    Some(SessionRule {
        label,
        update: update.clone(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn request(tool: &str, input: Value, suggestions: Vec<Value>) -> PermissionRequest {
        PermissionRequest {
            request_id: "r1".into(),
            tool_use_id: Some("t1".into()),
            tool_name: tool.into(),
            input,
            suggestions,
        }
    }

    #[test]
    fn an_edit_shows_its_diff_and_a_path_inside_the_project() {
        let captured: Value = serde_json::from_str(
            include_str!("../../tests/fixtures/agent-turns.jsonl")
                .lines()
                .find(|line| line.contains("can_use_tool"))
                .expect("request"),
        )
        .expect("json");
        let request = request(
            "Edit",
            captured["request"]["input"].clone(),
            captured["request"]["permission_suggestions"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        let file = "# Notes\n\nHello world.\n\nA second line.\n";
        let approval = Approval::new(request, Path::new("/work/project"), &|_| Some(file.to_owned()));
        assert_eq!(approval.target, "notes.md");
        let Preview::Diff(lines) = &approval.preview else {
            panic!("a diff")
        };
        assert!(lines.iter().any(|line| matches!(line, DiffLine::Line { mark: diff::Mark::Added, number: 3, text, .. } if text == "Howdy world.")));
        let rule = approval.session_rule.expect("the CLI offered one");
        assert_eq!(rule.label, SessionLabel::AcceptEdits);
        assert_eq!(rule.update["mode"], "acceptEdits");
    }

    #[test]
    fn a_write_over_an_existing_file_diffs_against_it() {
        let input = json!({"file_path": "/work/p/a.md", "content": "one\ntwo\n"});
        let approval = Approval::new(request("Write", input, vec![]), Path::new("/work/p"), &|_| {
            Some("one\n".into())
        });
        let Preview::Diff(lines) = approval.preview else {
            panic!("a diff")
        };
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, DiffLine::Line { mark: diff::Mark::Added, text, .. } if text == "two"))
        );
    }

    #[test]
    fn a_suggestion_that_outlives_the_session_is_not_offered() {
        let local = json!({"type": "setMode", "mode": "acceptEdits", "destination": "localSettings"});
        let approval = Approval::new(request("Edit", json!({}), vec![local]), Path::new("/p"), &|_| None);
        assert_eq!(approval.session_rule, None);
    }

    #[test]
    fn a_rule_suggestion_names_the_rule() {
        let rule = json!({"type": "addRules", "destination": "session", "behavior": "allow",
            "rules": [{"toolName": "Bash", "ruleContent": "ls:*"}]});
        let approval = Approval::new(
            request("Bash", json!({"command": "ls -la\nsecond line"}), vec![rule]),
            Path::new("/p"),
            &|_| None,
        );
        assert_eq!(approval.target, "ls -la");
        assert_eq!(
            approval.session_rule.expect("rule").label,
            SessionLabel::Rule("Bash(ls:*)".into())
        );
    }
}
