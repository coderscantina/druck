//! Live protocol probe: streamed text, a denied edit, an allowed edit, and a resumed session.
//!
//! `cargo run -p druck-desktop --example agent_probe -- <scratch-dir> [transcript-file]`
//!
//! The folder must hold `notes.md`. This spends plan credit, so it uses `haiku`
//! and short prompts. It never touches anything outside the given folder.

#[allow(dead_code)]
#[path = "../src/agent/cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "../src/agent/session.rs"]
mod session;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use session::{Event, PermissionRequest, Session, SessionConfig};

/// What the probe does with permission requests during one turn.
#[derive(Clone, Copy, PartialEq)]
enum Policy {
    Deny,
    Allow,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: agent_probe <scratch-dir> [transcript]"));
    let transcript = args.next().map(PathBuf::from);
    let path = cli::search_path();
    let program = cli::locate(&path).expect("claude not found");
    println!("cli: {:?} {:?}", program, cli::check(&program, &path));

    let config = SessionConfig {
        program,
        cwd: dir.clone(),
        model: Some("haiku".into()),
        extra_args: [
            "--tools",
            "Read,Edit",
            "--strict-mcp-config",
            "--setting-sources",
            "project",
        ]
        .map(Into::into)
        .into(),
        transcript: transcript.clone(),
        ..SessionConfig::default()
    };
    let notes = dir.join("notes.md");
    let before = std::fs::read_to_string(&notes).expect("notes.md");

    let (mut session, events) = Session::spawn(&config).expect("spawn");
    let ask = "Use the Edit tool to change the word Hello to Howdy in notes.md. Do nothing else.";
    let session_id = turn(&mut session, &events, ask, Policy::Deny);
    let after_deny = std::fs::read_to_string(&notes).expect("notes.md");
    println!("file unchanged after deny: {}", after_deny == before);

    turn(&mut session, &events, "Try the same edit again.", Policy::Allow);
    let after_allow = std::fs::read_to_string(&notes).expect("notes.md");
    println!("file changed after allow: {}", after_allow != before);
    drop(session);

    let resumed = SessionConfig {
        resume: Some(session_id),
        transcript,
        ..config
    };
    let (mut session, events) = Session::spawn(&resumed).expect("spawn resumed");
    turn(
        &mut session,
        &events,
        "Which word did I ask you to write? Answer with that one word.",
        Policy::Deny,
    );

    session.send("Count from 1 to 300, separated by spaces.").expect("send");
    let mut cancelled = false;
    println!("\n> cancel after the first delta");
    loop {
        match events.recv().expect("stream ended") {
            Event::TextDelta(_) if !cancelled => {
                session.cancel().expect("cancel");
                cancelled = true;
                println!("  cancel sent");
            }
            Event::TurnFinished { is_error, result, .. } => {
                println!("  finished after cancel error={is_error} {result:?}");
                break;
            }
            Event::Raw(value) if cancelled => println!("  raw {}", value["type"]),
            _ => {}
        }
    }
}

/// Sends one message and prints events until the turn ends. Returns the session id.
fn turn(session: &mut Session, events: &Receiver<Event>, text: &str, policy: Policy) -> String {
    let start = Instant::now();
    let mut first_token = None;
    println!("\n> {text}");
    session.send(text).expect("send");
    loop {
        let ms = start.elapsed().as_millis();
        match events.recv().expect("stream ended") {
            Event::TextDelta(delta) => {
                first_token.get_or_insert(ms);
                println!("  [{ms:>5} ms] text {delta:?}");
            }
            Event::Permission(request) => {
                answer(session, &request, policy);
                println!(
                    "  [{ms:>5} ms] permission {} -> {}",
                    request.tool_name,
                    if policy == Policy::Allow { "allow" } else { "deny" }
                );
            }
            Event::ToolUse { name, input, .. } => println!("  [{ms:>5} ms] tool use {name} {input}"),
            Event::ToolResult {
                content,
                is_error,
                details,
                ..
            } => {
                println!(
                    "  [{ms:>5} ms] tool result error={is_error} {content:?} details={}",
                    details.is_some()
                );
            }
            Event::Started { session_id, model, .. } => println!("  [{ms:>5} ms] started {session_id} {model}"),
            Event::TurnFinished {
                session_id,
                is_error,
                result,
                ..
            } => {
                println!("  [{ms:>5} ms] finished error={is_error} first_token={first_token:?} {result:?}");
                return session_id;
            }
            Event::Error(message) | Event::Exited(message) => panic!("{message}"),
            Event::Raw(value) => println!("  [{ms:>5} ms] raw {}", value["type"]),
        }
    }
}

fn answer(session: &mut Session, request: &PermissionRequest, policy: Policy) {
    match policy {
        Policy::Allow => session.allow(request),
        Policy::Deny => session.deny(request, "The author declined this edit."),
    }
    .expect("answer");
}
