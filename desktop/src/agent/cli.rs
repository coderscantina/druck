//! Finding the user's agent CLI, checking that it is a version we tested, and asking which login it reports. What
//! differs between the CLIs is the [`Backend`]'s.

use std::env;
use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::backend::Backend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliState {
    Missing,
    /// The version output did not hold a version.
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

/// Finds the CLI: the program `DRUCK_CLAUDE` or `DRUCK_CODEX` names if that is set, else on `path`.
pub fn locate(backend: &dyn Backend, path: &OsStr) -> Option<PathBuf> {
    let named = format!("DRUCK_{}", backend.program().to_uppercase());
    if let Some(program) = env::var_os(named).filter(|program| !program.is_empty()) {
        return Some(PathBuf::from(program));
    }
    env::split_paths(path)
        .map(|dir| dir.join(backend.program()))
        .find(|candidate| candidate.is_file())
}

/// Runs `<program> --version` and classifies the result.
pub fn check(backend: &dyn Backend, program: &Path, path: &OsStr) -> CliState {
    let Ok(output) = Command::new(program).arg("--version").env("PATH", path).output() else {
        return CliState::Missing;
    };
    backend.classify_version(&String::from_utf8_lossy(&output.stdout))
}

/// Which login the CLI says it has. Druck only reports this. It never reads a credential, and the report holds no
/// account name, organisation, or token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// The CLI says nobody is logged in.
    LoggedOut,
    /// A login with this method, such as `claude.ai`, and its plan if the CLI names one.
    LoggedIn {
        method: String,
        plan: Option<String>,
        /// A subscription login, which the vendor may not allow in an app like this one.
        subscription: bool,
    },
    /// The CLI did not answer in a way we know.
    Unknown,
}

/// Runs the CLI's own login report. Some print it on stderr.
pub fn login(backend: &dyn Backend, program: &Path, path: &OsStr) -> Login {
    match Command::new(program)
        .args(backend.login_args())
        .env("PATH", path)
        .output()
    {
        Ok(output) => {
            let said = [output.stdout, output.stderr].concat();
            backend.classify_login(&String::from_utf8_lossy(&said))
        }
        Err(_) => Login::Unknown,
    }
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
    fn missing_program_is_reported() {
        let claude = super::super::claude::Claude::default();
        assert_eq!(
            check(&claude, Path::new("/nonexistent/claude"), OsStr::new("")),
            CliState::Missing
        );
    }
}
