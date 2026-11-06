//! The session each project resumes, kept in the app's data folder, never in the project.
//!
//! The file maps a project's path to the id of its conversation and the model last chosen. An id is a handle the
//! CLI understands, not a credential.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::persist::{atomic, recovery};

const FILE: &str = "agent-sessions.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remembered {
    pub session: Option<String>,
    pub model: Option<String>,
}

/// The file, as written.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Record {
    projects: BTreeMap<String, Remembered>,
}

pub struct Store {
    file: PathBuf,
}

impl Store {
    /// The store in the app's data folder.
    pub fn open() -> Option<Self> {
        Some(Self::in_dir(&recovery::data_dir()?))
    }

    pub fn in_dir(dir: &Path) -> Self {
        Self { file: dir.join(FILE) }
    }

    /// What is kept for `project`. A missing file keeps nothing, and one that cannot be read is an error.
    pub fn get(&self, project: &Path) -> io::Result<Remembered> {
        Ok(self.read()?.projects.remove(&key(project)).unwrap_or_default())
    }

    /// Keeps `remembered` for `project`. Nothing at all removes its entry. A file that cannot be read is left as it is,
    /// since writing over it would lose every other project's entry.
    pub fn set(&self, project: &Path, remembered: &Remembered) -> io::Result<()> {
        let mut all = self.read()?;
        if remembered == &Remembered::default() {
            all.projects.remove(&key(project));
        } else {
            all.projects.insert(key(project), remembered.clone());
        }
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        atomic::write(&self.file, serde_json::to_string_pretty(&all)?.as_bytes())
    }

    fn read(&self) -> io::Result<Record> {
        match fs::read_to_string(&self.file) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Record::default()),
            Err(error) => Err(error),
        }
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
        assert_eq!(Store::in_dir(&dir).get(&project).expect("get"), remembered);
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
        assert_eq!(store.get(&two).expect("get"), Remembered::default());
        assert_eq!(fs::read_dir(&one).expect("list").count(), 0);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn forgetting_removes_the_entry_and_a_broken_file_is_an_error_left_as_it_is() {
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
        assert_eq!(store.get(&project).expect("get"), Remembered::default());
        fs::write(dir.join(FILE), "{ not json").expect("break it");
        assert!(store.get(&project).is_err());
        assert!(store.set(&project, &Remembered::default()).is_err());
        assert_eq!(fs::read_to_string(dir.join(FILE)).expect("file"), "{ not json");
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
