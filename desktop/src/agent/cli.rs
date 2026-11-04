//! Finding the user's `claude` CLI, checking that it is a version we tested, and asking which login it reports.

use std::env;
use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The CLI line this protocol code was captured and tested against.
const TESTED_MAJOR: u32 = 2;
const TESTED_MINOR: u32 = 1;
const TESTED_MIN_PATCH: u32 = 317;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliState {
    Missing,
    /// The version output did not look like `X.Y.Z (Claude Code)`.
    Unreadable(String),
    /// A version we have not tested. It may still work.
    Untested(String),
    Ready(String),
}

/// How long the login shell may take to say what its search path is.
const SHELL_WAIT: Duration = Duration::from_secs(3);

/// The search path a terminal has. An app started from the Finder inherits only the system's, without the folders where
/// `claude`, `node`, `git`, and the author's MCP servers live, so the CLI and everything it starts get this one. The
/// login shell knows it, and it is asked once, off the main thread.
pub fn search_path() -> OsString {
    let inherited = env::var_os("PATH").unwrap_or_default();
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let fallbacks = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
        home.join(".claude/local"),
    ];
    let shell = login_shell_path().unwrap_or_default();
    let mut folders: Vec<PathBuf> = Vec::new();
    for folder in env::split_paths(&shell)
        .chain(env::split_paths(&inherited))
        .chain(fallbacks)
    {
        if !folder.as_os_str().is_empty() && !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    env::join_paths(folders).unwrap_or(inherited)
}

/// The search path of the author's login shell, or nothing if it has none, fails, or takes too long.
fn login_shell_path() -> Option<OsString> {
    const MARK: &str = "__DRUCK_PATH__";
    let shell = env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args(["-lc", &format!("printf '\\n{MARK}%s\\n' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    while child.try_wait().ok()?.is_none() {
        if started.elapsed() > SHELL_WAIT {
            child.kill().ok();
            child.wait().ok();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    parse_shell_path(&output, MARK).map(OsString::from)
}

/// The path after the mark, whatever a shell's startup files printed before it.
fn parse_shell_path<'a>(output: &'a str, mark: &str) -> Option<&'a str> {
    output
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(mark))
        .filter(|path| !path.is_empty())
}

/// Finds `claude`: the program `DRUCK_CLAUDE` names if that is set, else on `path`.
pub fn locate(path: &OsStr) -> Option<PathBuf> {
    if let Some(program) = env::var_os("DRUCK_CLAUDE").filter(|program| !program.is_empty()) {
        return Some(PathBuf::from(program));
    }
    env::split_paths(path)
        .map(|dir| dir.join("claude"))
        .find(|candidate| candidate.is_file())
}

/// Runs `<program> --version` and classifies the result.
pub fn check(program: &Path, path: &OsStr) -> CliState {
    let Ok(output) = Command::new(program).arg("--version").env("PATH", path).output() else {
        return CliState::Missing;
    };
    classify(&String::from_utf8_lossy(&output.stdout))
}

/// Which login the CLI says it has. Druck only reports this. It never reads a credential, and the report holds no
/// account name, organisation, or token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// The CLI says nobody is logged in.
    LoggedOut,
    /// A login with this method, such as `claude.ai`, and its plan if the CLI names one.
    LoggedIn { method: String, plan: Option<String> },
    /// The CLI did not answer in a way we know.
    Unknown,
}

/// Runs `<program> auth status`, which prints the CLI's own report as JSON.
pub fn login(program: &Path, path: &OsStr) -> Login {
    match Command::new(program)
        .args(["auth", "status"])
        .env("PATH", path)
        .output()
    {
        Ok(output) => classify_login(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => Login::Unknown,
    }
}

fn classify_login(output: &str) -> Login {
    let Ok(report) = serde_json::from_str::<serde_json::Value>(output) else {
        return Login::Unknown;
    };
    match (report["loggedIn"].as_bool(), report["authMethod"].as_str()) {
        (Some(false), _) => Login::LoggedOut,
        (Some(true), Some(method)) => Login::LoggedIn {
            method: method.to_owned(),
            plan: report["subscriptionType"].as_str().map(str::to_owned),
        },
        _ => Login::Unknown,
    }
}

fn classify(output: &str) -> CliState {
    let version = output.split_whitespace().next().unwrap_or_default();
    match parse_version(version) {
        None => CliState::Unreadable(output.trim().to_owned()),
        Some((major, minor, patch)) => {
            let tested = major == TESTED_MAJOR && minor == TESTED_MINOR && patch >= TESTED_MIN_PATCH;
            if tested {
                CliState::Ready(version.to_owned())
            } else {
                CliState::Untested(version.to_owned())
            }
        }
    }
}

fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(|part| part.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shells_path_is_found_behind_what_its_startup_files_print() {
        let output = "Welcome!\nsome tool 1.2\n\n__M__/opt/homebrew/bin:/usr/bin\n";
        assert_eq!(parse_shell_path(output, "__M__"), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(parse_shell_path("no mark here\n", "__M__"), None);
        assert_eq!(parse_shell_path("__M__\n", "__M__"), None);
    }

    #[test]
    fn classifies_versions() {
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
        let login = classify_login(report);
        assert_eq!(
            login,
            Login::LoggedIn {
                method: "claude.ai".into(),
                plan: Some("max".into())
            }
        );
        assert!(!format!("{login:?}").contains("example.com"));
        assert_eq!(classify_login(r#"{"loggedIn": false}"#), Login::LoggedOut);
        assert_eq!(classify_login("Not logged in"), Login::Unknown);
    }

    #[test]
    fn missing_program_is_reported() {
        assert_eq!(
            check(Path::new("/nonexistent/claude"), OsStr::new("")),
            CliState::Missing
        );
    }

    #[test]
    #[ignore = "needs the installed claude CLI"]
    fn installed_cli_is_ready() {
        let path = search_path();
        let program = locate(&path).expect("claude is installed");
        assert!(matches!(check(&program, &path), CliState::Ready(_)));
    }
}
