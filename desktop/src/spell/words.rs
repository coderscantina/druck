//! The project's own words: a plain text file in the project, one word per line, which the writer can edit, share, and
//! keep in version control. The system's dictionary stays untouched. A word counts in any capitalization.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::persist::atomic;

/// The file's name: in the project's folder, or beside a single file.
pub const FILE_NAME: &str = "druck-words.txt";

/// The words of one document, kept up to date with the file.
#[derive(Debug, Default)]
pub struct WordList {
    path: PathBuf,
    /// When the file changed, when it was last read.
    read_at: Option<SystemTime>,
    words: HashSet<String>,
}

impl WordList {
    pub fn open(folder: &Path) -> Self {
        let mut list = Self {
            path: folder.join(FILE_NAME),
            ..Self::default()
        };
        list.reload();
        list
    }

    /// Reads the file again if another program changed it. Returns whether the words changed.
    pub fn reload(&mut self) -> bool {
        let changed = std::fs::metadata(&self.path).and_then(|file| file.modified()).ok();
        if changed == self.read_at {
            return false;
        }
        let words = std::fs::read_to_string(&self.path)
            .map(|text| parse(&text))
            .unwrap_or_default();
        self.read_at = changed;
        let different = words != self.words;
        self.words = words;
        different
    }

    pub fn contains(&self, word: &str) -> bool {
        self.words.contains(&word.to_lowercase())
    }

    /// Adds `word` as written, on a line of its own, after what the file holds now, and writes it.
    pub fn add(&mut self, word: &str) -> io::Result<()> {
        self.reload();
        if self.contains(word) {
            return Ok(());
        }
        let mut text = std::fs::read_to_string(&self.path).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(word);
        text.push('\n');
        atomic::write(&self.path, text.as_bytes())?;
        self.read_at = None;
        self.reload();
        Ok(())
    }
}

fn parse(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("druck-words-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a test folder");
        dir
    }

    #[test]
    fn a_word_added_is_on_its_own_line_and_counts_in_any_capitalization() {
        let dir = folder("add");
        std::fs::write(dir.join(FILE_NAME), "Ijob").expect("a file");
        let mut list = WordList::open(&dir);
        list.add("Zachäus").expect("written");
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE_NAME)).expect("read"),
            "Ijob\nZachäus\n"
        );
        assert!(list.contains("zachäus") && list.contains("IJOB"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_change_by_another_program_is_read_and_kept_when_adding() {
        let dir = folder("outside");
        let mut list = WordList::open(&dir);
        assert!(!list.contains("Kephas"));
        std::fs::write(dir.join(FILE_NAME), "Kephas\n").expect("a file");
        assert!(list.reload());
        assert!(list.contains("Kephas"));
        list.add("Emmaus").expect("written");
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE_NAME)).expect("read"),
            "Kephas\nEmmaus\n"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
