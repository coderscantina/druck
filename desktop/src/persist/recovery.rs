//! The last-resort copies, in the OS's app data folder and never in the project. Holds no gpui types.
//!
//! - A recovery copy holds a file's unsaved text. It goes when a save succeeds, so one that is still there at the next
//!   open means the app did not get to save it, and the text comes back.
//! - A kept copy holds a version a person chose against when settling a conflict, or the outside change an undo took
//!   back. These stay, so nothing is lost for good by a decision made in a hurry.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::atomic::{self, content_hash};

const FORMAT: u64 = 1;

/// The app's data folder. `DRUCK_DATA_DIR` replaces it, for tests and scratch runs.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DRUCK_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return Some(home?.join("Library/Application Support/Druck"));
    }
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("Druck"));
    }
    let share = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| Some(home?.join(".local/share")));
    Some(share?.join("druck"))
}

/// A file's name in a folder keyed by its path, the same for the same path on every run.
fn key(path: &Path) -> String {
    format!("{:016x}", content_hash(path.as_os_str().as_encoded_bytes()))
}

/// A file's unsaved text, as found after a crash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    pub path: PathBuf,
    /// The text the unsaved text started from.
    pub base: String,
    pub text: String,
}

/// The copies of one document.
#[derive(Clone)]
pub struct Copies {
    recovery: PathBuf,
    kept: PathBuf,
}

impl Copies {
    /// The copies of the document at `location`, under `data`.
    pub fn new(data: &Path, location: &Path) -> Self {
        let name = location.file_stem().unwrap_or_default().to_string_lossy();
        let document = format!("{name}-{}", key(location));
        Self {
            recovery: data.join("recovery").join(&document),
            kept: data.join("kept").join(document),
        }
    }

    /// Stores a file's unsaved text, replacing any copy before it.
    pub fn store(&self, path: &Path, base: &str, text: &str) -> io::Result<()> {
        fs::create_dir_all(&self.recovery)?;
        let record = json!({ "format": FORMAT, "path": path, "base": base, "text": text });
        atomic::write_quick(&self.file_for(path), record.to_string().as_bytes())
    }

    /// Clears a file's copy after its text reached the disk.
    pub fn clear(&self, path: &Path) {
        fs::remove_file(self.file_for(path)).ok();
    }

    /// Clears every recovery copy, when the person chose to discard the unsaved text.
    pub fn clear_all(&self) {
        fs::remove_dir_all(&self.recovery).ok();
    }

    fn file_for(&self, path: &Path) -> PathBuf {
        self.recovery.join(format!("{}.json", key(path)))
    }

    /// Every copy the last run left behind. One that cannot be read is left where it is, for a person to look at.
    pub fn load(&self) -> Vec<Recovered> {
        let mut found = Vec::new();
        for entry in fs::read_dir(&self.recovery).into_iter().flatten().flatten() {
            let record = fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            let Some(record) = record else {
                continue;
            };
            let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_owned);
            if let (Some(path), Some(base), Some(body)) = (text("path"), text("base"), text("text")) {
                found.push(Recovered {
                    path: path.into(),
                    base,
                    text: body,
                });
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found
    }

    /// Keeps a version of a file for good, and returns where it went. `label` says whose it was.
    pub fn keep(&self, path: &Path, label: &str, text: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.kept)?;
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let extension = path.extension().unwrap_or_default().to_string_lossy();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let target = self.kept.join(format!("{stem}.{now}.{label}.{extension}"));
        atomic::write(&target, text.as_bytes())?;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("druck-recovery-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn a_copy_comes_back_until_it_is_cleared() {
        let dir = data("back");
        let copies = Copies::new(&dir, Path::new("/books/one"));
        let path = Path::new("/books/one/manuscript/größe.md");
        copies.store(path, "base", "base and mörë\n").unwrap();
        copies.store(path, "base", "newer\n").unwrap();
        assert_eq!(
            copies.load(),
            vec![Recovered {
                path: path.to_owned(),
                base: "base".to_owned(),
                text: "newer\n".to_owned()
            }]
        );
        copies.clear(path);
        assert!(copies.load().is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn documents_keep_their_copies_apart() {
        let dir = data("apart");
        let one = Copies::new(&dir, Path::new("/books/one"));
        let two = Copies::new(&dir, Path::new("/books/two"));
        one.store(Path::new("/books/one/a.md"), "", "text").unwrap();
        assert!(two.load().is_empty());
        assert_eq!(one.load().len(), 1);
        one.clear_all();
        assert!(one.load().is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_kept_copy_stays_with_its_name() {
        let dir = data("kept");
        let copies = Copies::new(&dir, Path::new("/books/one"));
        let kept = copies.keep(Path::new("/books/one/a.md"), "theirs", "text").unwrap();
        assert!(kept.starts_with(&copies.kept));
        assert!(kept.to_string_lossy().ends_with(".theirs.md"));
        assert_eq!(fs::read_to_string(&kept).unwrap(), "text");
        fs::remove_dir_all(dir).ok();
    }
}
