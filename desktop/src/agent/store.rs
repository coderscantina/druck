//! The session each project resumes, kept in the app's data folder, never in the project.
//!
//! The file maps a project's path to the id of its conversation and the model last chosen. An id is a handle the
//! CLI understands, not a credential.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const FILE: &str = "agent-sessions.json";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Remembered {
    pub session: Option<String>,
    pub model: Option<String>,
}

pub struct Store {
    file: PathBuf,
}

impl Store {
    /// The app's data folder: `DRUCK_DATA_DIR` if set, else the platform's own place.
    pub fn open() -> Option<Self> {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let dir = var("DRUCK_DATA_DIR").or_else(|| {
            if cfg!(target_os = "macos") {
                var("HOME").map(|home| home.join("Library/Application Support/Druck"))
            } else if cfg!(target_os = "windows") {
                var("APPDATA").map(|dir| dir.join("Druck"))
            } else {
                var("XDG_DATA_HOME")
                    .or_else(|| var("HOME").map(|home| home.join(".local/share")))
                    .map(|dir| dir.join("druck"))
            }
        })?;
        Some(Self::in_dir(&dir))
    }

    pub fn in_dir(dir: &Path) -> Self {
        Self { file: dir.join(FILE) }
    }

    /// What is kept for `project`. A missing or unreadable file keeps nothing.
    pub fn get(&self, project: &Path) -> Remembered {
        let all = self.read();
        let entry = &all[key(project)];
        Remembered {
            session: entry["session"].as_str().map(str::to_owned),
            model: entry["model"].as_str().map(str::to_owned),
        }
    }

    /// Keeps `remembered` for `project`. Nothing at all removes its entry.
    pub fn set(&self, project: &Path, remembered: &Remembered) -> io::Result<()> {
        let mut all = self.read();
        let projects = all.as_object_mut().expect("read returns an object");
        if remembered == &Remembered::default() {
            projects.remove(&key(project));
        } else {
            projects.insert(
                key(project),
                json!({"session": remembered.session, "model": remembered.model}),
            );
        }
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        // Written beside the file and renamed over it, so a crash never leaves half of one.
        let temporary = self.file.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temporary, serde_json::to_string_pretty(&json!({"projects": all}))?)?;
        fs::rename(&temporary, &self.file)
    }

    fn read(&self) -> Value {
        let projects = fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value["projects"].as_object().cloned())
            .unwrap_or_default();
        Value::Object(projects)
    }
}

/// A project is the same project however its path is spelled.
fn key(project: &Path) -> String {
    fs::canonicalize(project)
        .unwrap_or_else(|_| project.to_owned())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("druck-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn a_project_gets_back_what_was_kept_after_a_restart() {
        let dir = scratch("kept");
        let project = dir.join("book");
        fs::create_dir_all(&project).expect("project");
        let remembered = Remembered {
            session: Some("abc-123".into()),
            model: Some("haiku".into()),
        };
        Store::in_dir(&dir).set(&project, &remembered).expect("set");
        // A new Store reads the file again, as a restarted app does.
        assert_eq!(Store::in_dir(&dir).get(&project), remembered);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn projects_are_kept_apart_and_nothing_goes_in_the_project() {
        let dir = scratch("apart");
        let (one, two) = (dir.join("one"), dir.join("two"));
        fs::create_dir_all(&one).expect("one");
        fs::create_dir_all(&two).expect("two");
        let store = Store::in_dir(&dir.join("data"));
        store
            .set(
                &one,
                &Remembered {
                    session: Some("s1".into()),
                    model: None,
                },
            )
            .expect("set");
        assert_eq!(store.get(&two), Remembered::default());
        assert_eq!(fs::read_dir(&one).expect("list").count(), 0);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn forgetting_removes_the_entry_and_a_broken_file_keeps_nothing() {
        let dir = scratch("forget");
        let store = Store::in_dir(&dir);
        let project = dir.join("p");
        store
            .set(
                &project,
                &Remembered {
                    session: Some("s".into()),
                    model: None,
                },
            )
            .expect("set");
        store.set(&project, &Remembered::default()).expect("forget");
        assert_eq!(store.get(&project), Remembered::default());
        fs::write(dir.join(FILE), "{ not json").expect("break it");
        assert_eq!(store.get(&project), Remembered::default());
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
