//! The installed `git`: what gitoxide has no safe way to do. Push and pull, since gitoxide 0.90 has no push and the
//! author's keys and credential helpers live with `git`. Switching and merging, since gitoxide can check out a tree into
//! an empty folder but not move a worktree with the author's changes in it from one tree to another. Holds no gpui types.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::strings::Key;

/// Where `git` may be when an app started from the Finder has only the system's search path.
const FOLDERS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];
/// Local work is quick. Past this something waits for a person it cannot reach.
const LOCAL_LIMIT: Duration = Duration::from_secs(60);
/// A slow connection and a big book.
const NETWORK_LIMIT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(20);

/// The installed `git`, when there is one.
#[derive(Debug, Clone)]
pub struct Git {
    program: PathBuf,
    search_path: std::ffi::OsString,
}

/// What a merge did.
#[derive(Debug, PartialEq, Eq)]
pub enum Merged {
    /// Finished: already there, a fast-forward, or a merge commit.
    Done,
    /// Stopped with both sides overlapping. The repo is in the middle of the merge.
    Conflicts,
}

/// What a command printed when it failed.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The author's uncommitted changes are in the way. Nothing happened.
    LocalChanges,
    /// Git said this.
    Said(String),
}

impl Git {
    /// Looks for `git` where a terminal would find it. On a Mac without the developer tools `/usr/bin/git` is a stand-in
    /// that opens an install dialog when run, so it counts only with the tools there.
    pub fn find() -> Option<Self> {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let mut folders: Vec<PathBuf> = FOLDERS.iter().map(PathBuf::from).collect();
        folders.extend(std::env::split_paths(&inherited));
        folders.dedup();
        let program = (folders.iter())
            .map(|folder| folder.join("git"))
            .find(|program| program.is_file() && is_real(program))?;
        Some(Self {
            program,
            search_path: std::env::join_paths(folders).unwrap_or(inherited),
        })
    }

    fn run(&self, dir: &Path, args: &[&str], input: Option<&str>, limit: Duration) -> Result<Output, String> {
        let mut child = Command::new(&self.program)
            .args(args)
            .current_dir(dir)
            .env("PATH", &self.search_path)
            .env("LC_ALL", "C")
            // No terminal to ask in: credentials come from the author's helpers and keys, or the command fails.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_MERGE_AUTOEDIT", "no")
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| crate::t!(Key::CannotRunGit, error = error))?;
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin
                .write_all(input.as_bytes())
                .map_err(|error| crate::t!(Key::CannotTalkToGit, error = error))?;
        }
        // On their own threads, as a child with a full pipe would never exit.
        let reader = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            thread::spawn(move || {
                let mut text = Vec::new();
                if let Some(mut pipe) = pipe {
                    pipe.read_to_end(&mut text).ok();
                }
                String::from_utf8_lossy(&text).into_owned()
            })
        };
        let stdout = reader(child.stdout.take().map(|pipe| Box::new(pipe) as _));
        let stderr = reader(child.stderr.take().map(|pipe| Box::new(pipe) as _));
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() > limit => {
                    child.kill().ok();
                    child.wait().ok();
                    return Err(crate::t!(Key::GitTooLong, command = args[0]));
                }
                Ok(None) => thread::sleep(POLL),
                Err(error) => return Err(crate::t!(Key::CannotWaitForGit, error = error)),
            }
        };
        Ok(Output {
            success: status.success(),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    fn checked(&self, dir: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
        let output = self.run(dir, args, None, limit)?;
        if output.success {
            Ok(output.stdout)
        } else {
            Err(output.said())
        }
    }

    /// Moves the worktree to a branch. Changes that do not touch what differs come along, as with `git switch`.
    pub fn switch(&self, dir: &Path, branch: &str) -> Result<(), Refused> {
        let output = self
            .run(dir, &["switch", "--", branch], None, LOCAL_LIMIT)
            .map_err(Refused::Said)?;
        if output.success { Ok(()) } else { Err(output.refused()) }
    }

    /// Merges a branch into the current one. `MERGE_HEAD` tells a stop with conflicts from a refusal to begin.
    pub fn merge(&self, dir: &Path, git_dir: &Path, target: &str) -> Result<Merged, Refused> {
        let output = self
            .run(dir, &["merge", "--no-edit", target], None, LOCAL_LIMIT)
            .map_err(Refused::Said)?;
        if output.success {
            Ok(Merged::Done)
        } else if git_dir.join("MERGE_HEAD").exists() {
            Ok(Merged::Conflicts)
        } else {
            Err(output.refused())
        }
    }

    pub fn abort_merge(&self, dir: &Path) -> Result<(), String> {
        self.checked(dir, &["merge", "--abort"], LOCAL_LIMIT).map(|_| ())
    }

    pub fn fetch(&self, dir: &Path) -> Result<(), String> {
        self.checked(dir, &["fetch", "--quiet"], NETWORK_LIMIT).map(|_| ())
    }

    /// Pushes the current branch, to its upstream or, when it has none yet, to `new_upstream` as a new one.
    pub fn push(&self, dir: &Path, new_upstream: Option<(&str, &str)>) -> Result<(), String> {
        let args: Vec<&str> = match new_upstream {
            Some((remote, branch)) => vec!["push", "--quiet", "--set-upstream", remote, branch],
            None => vec!["push", "--quiet"],
        };
        self.checked(dir, &args, NETWORK_LIMIT).map(|_| ())
    }

    /// Writes a signed commit, which only `git` can, and returns its id.
    pub fn signed_commit(&self, dir: &Path, tree: &str, parents: &[String], message: &str) -> Result<String, String> {
        let mut args = vec!["commit-tree", "-S", tree];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        args.extend(["-F", "-"]);
        let output = self.run(dir, &args, Some(message), LOCAL_LIMIT)?;
        if output.success {
            Ok(output.stdout.trim().to_owned())
        } else {
            Err(output.said())
        }
    }
}

struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}

impl Output {
    fn said(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or(crate::t!(Key::GitFailed))
            .trim_start_matches("error: ")
            .trim_start_matches("fatal: ")
            .to_owned()
    }

    fn refused(&self) -> Refused {
        if self.stderr.contains("would be overwritten") {
            Refused::LocalChanges
        } else {
            Refused::Said(self.said())
        }
    }
}

fn is_real(program: &Path) -> bool {
    if cfg!(target_os = "macos") && program == Path::new("/usr/bin/git") {
        return Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_for_local_changes_is_told_apart() {
        let output = |stderr: &str| Output {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        };
        assert_eq!(
            output("error: Your local changes to the following files would be overwritten by checkout:\n\ta.md\n")
                .refused(),
            Refused::LocalChanges
        );
        assert_eq!(
            output("fatal: invalid reference: nope\n").refused(),
            Refused::Said("invalid reference: nope".to_owned())
        );
    }
}
