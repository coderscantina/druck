//! The seam between Druck and an agent CLI. Everything that differs between `claude` and `codex` is behind [`Backend`]:
//! the program and its version line, what its login report means, its command line, and the lines that go over stdio.
//! The rest of the agent module works with [`Event`]s and [`PermissionRequest`]s and never asks which CLI runs.

use std::ffi::OsString;

use serde_json::Value;

use super::cli::{CliState, Login};
use super::session::{Event, PermissionRequest, SessionConfig};

/// The author's answer to a waiting tool call.
#[derive(Debug, Clone, Copy)]
pub enum Verdict<'a> {
    Allow,
    /// Allow, and apply the request's own offer to remember the answer while the process runs.
    AllowForSession(&'a Value),
    /// Refuse, with a reason the model sees where the CLI passes one on.
    Deny(&'a str),
}

/// What one stdout line of the CLI means, and what the CLI must be told at once in return.
#[derive(Debug, Default, PartialEq)]
pub struct Read {
    pub events: Vec<Event>,
    pub replies: Vec<Value>,
}

impl Read {
    pub fn events(events: Vec<Event>) -> Self {
        Self {
            events,
            replies: Vec::new(),
        }
    }
}

pub trait Backend: Send {
    /// The CLI's name as its vendor writes it.
    fn name(&self) -> &'static str;
    /// Who documents its setup.
    fn vendor(&self) -> &'static str;
    /// The file of project instructions it reads.
    fn memory_file(&self) -> &'static str;
    /// What the vendor's subscription login is called.
    fn subscription(&self) -> &'static str;
    /// The executable's name on the search path.
    fn program(&self) -> &'static str;
    /// The models the selector offers besides the CLI's own choice.
    fn models(&self) -> &'static [&'static str];

    /// What `<program> --version` printed.
    fn classify_version(&self, output: &str) -> CliState;
    /// The arguments that print the login report.
    fn login_args(&self) -> &'static [&'static str];
    /// What the login report says. It keeps the method and plan only, never an account or a token.
    fn classify_login(&self, output: &str) -> Login;

    /// The command line of one session.
    fn args(&self, config: &SessionConfig) -> Vec<OsString>;
    /// What is written when the process starts.
    fn open(&mut self, config: &SessionConfig) -> Vec<Value>;
    /// The author's prompt. Empty while it has to wait for the CLI, and then a later [`Backend::read`] replies with it.
    fn prompt(&mut self, text: &str) -> Vec<Value>;
    fn answer(&mut self, request: &PermissionRequest, verdict: Verdict) -> Value;
    /// Stops the running turn and keeps the session.
    fn interrupt(&mut self) -> Vec<Value>;
    /// Takes one stdout line. What it does not know becomes [`Event::Raw`], so a CLI update does not break the stream.
    fn read(&mut self, line: &str) -> Read;
}

/// `X.Y.Z` as numbers.
pub fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(|part| part.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}
