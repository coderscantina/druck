//! Finding the user's `claude` CLI and checking that it is a version we tested.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Finds `claude` on the search path, then in the places GUI apps miss because
/// they do not inherit the shell's `PATH`.
pub fn locate() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let fallbacks = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
        home.join(".claude/local"),
    ];
    let path = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path)
        .chain(fallbacks)
        .map(|dir| dir.join("claude"))
        .find(|candidate| candidate.is_file())
}

/// Runs `<program> --version` and classifies the result.
pub fn check(program: &Path) -> CliState {
    let Ok(output) = Command::new(program).arg("--version").output() else {
        return CliState::Missing;
    };
    classify(&String::from_utf8_lossy(&output.stdout))
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
    fn classifies_versions() {
        assert_eq!(classify("2.1.317 (Claude Code)\n"), CliState::Ready("2.1.317".into()));
        assert_eq!(classify("2.1.400 (Claude Code)"), CliState::Ready("2.1.400".into()));
        assert_eq!(classify("2.1.100 (Claude Code)"), CliState::Untested("2.1.100".into()));
        assert_eq!(classify("3.0.0 (Claude Code)"), CliState::Untested("3.0.0".into()));
        assert_eq!(classify("not a version"), CliState::Unreadable("not a version".into()));
    }

    #[test]
    fn missing_program_is_reported() {
        assert_eq!(check(Path::new("/nonexistent/claude")), CliState::Missing);
    }

    #[test]
    #[ignore = "needs the installed claude CLI"]
    fn installed_cli_is_ready() {
        let program = locate().expect("claude is installed");
        assert!(matches!(check(&program), CliState::Ready(_)));
    }
}
