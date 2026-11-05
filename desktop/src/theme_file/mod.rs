//! A theme JSON file the app edits. Changes are partial: only the touched member changes in the text, and
//! every change is checked through the engine before it can reach disk. The check that resolves the theme runs at
//! once, so a change is refused where it is made. The probe typeset and the write run later, in order, on
//! [`writer::Writer`]. Holds no gpui types.

pub mod blocks;
pub mod editor;
pub mod font_info;
pub mod geometry;
pub mod library;
pub mod patch;
mod strings;
pub mod tokens;
mod validate;
pub mod writer;

use std::fs;
use std::path::{Path, PathBuf};

use druck::editor::theme_value::{default_theme_value, merge};
use serde_json::Value;

pub use validate::Problem;
use writer::Commit;

/// The bundled specimen: every element of a document, to see a theme on and to try a change against.
pub const SPECIMEN: &str = include_str!("../../assets/specimen.md");

pub struct ThemeFile {
    path: PathBuf,
    text: String,
    /// The file's own value: what it sets, nothing inherited.
    own: Value,
    /// The file merged over the bundled default: what a document using it gets.
    effective: Value,
    /// What the engine says about `own`, kept so a change is only checked once.
    problems: Vec<Problem>,
    /// The text last written, which is what the file holds unless another program changed it.
    disk: String,
    /// The text last handed out by [`Self::commit`], so a change that leaves the text as it was is not written again.
    queued: String,
}

impl ThemeFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let text = fs::read_to_string(&path)
            .map_err(|e| crate::t!(strings::Key::CannotRead, path = path.display(), error = e))?;
        let mut file = Self {
            path,
            own: Value::Null,
            effective: Value::Null,
            problems: Vec::new(),
            disk: text.clone(),
            queued: text.clone(),
            text,
        };
        file.load()?;
        file.problems = validate::check(&file.path, &file.own);
        if !file.own.is_object() {
            return Err(format!("{}: a theme must be a JSON object", file.path.display()));
        }
        Ok(file)
    }

    /// What the engine reports about the file as it stands. A file with problems still opens, so they can
    /// be fixed in place.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the text differs from what was written last.
    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        self.text != self.disk
    }

    /// The value this file sets at a path, if it sets one.
    pub fn get(&self, path: &[&str]) -> Option<&Value> {
        lookup(&self.own, path)
    }

    /// The value a document gets at a path: this file's, or the bundled default's.
    pub fn effective(&self, path: &[&str]) -> Option<&Value> {
        lookup(&self.effective, path)
    }

    /// Sets one property. A value the engine rejects leaves the file as it was.
    pub fn set(&mut self, path: &[&str], value: Value) -> Result<(), Vec<Problem>> {
        let text = patch::set(&self.text, path, &value).map_err(Problem::internal)?;
        self.replace(text, path, Some(&value))
    }

    /// The file's own value: what it sets, nothing inherited.
    pub fn value(&self) -> &Value {
        &self.own
    }

    /// The file merged over the bundled default.
    pub fn merged(&self) -> &Value {
        &self.effective
    }

    /// Takes the text a change produces from the current text, if it is valid JSON that adds no problem the
    /// file did not have. For changes that touch more than one property, such as renaming a token.
    pub fn rewrite(&mut self, change: impl FnOnce(&str) -> Result<String, String>) -> Result<(), Vec<Problem>> {
        let text = change(&self.text).map_err(Problem::internal)?;
        self.accept(text)
    }

    /// Replaces the whole text, as the JSON view does. It must be a JSON object that adds no problem.
    pub fn replace_text(&mut self, text: &str) -> Result<(), Vec<Problem>> {
        self.accept(text.to_owned())
    }

    /// Removes one property, which returns it to the inherited value.
    pub fn remove(&mut self, path: &[&str]) -> Result<(), Vec<Problem>> {
        let text = patch::remove(&self.text, path).map_err(Problem::internal)?;
        self.replace(text, path, None)
    }

    /// What to write for the changes made since the last call, or nothing if the text is as it was.
    pub fn commit(&mut self) -> Option<Commit> {
        if self.text == self.queued {
            return None;
        }
        self.queued.clone_from(&self.text);
        Some(Commit {
            path: self.path.clone(),
            text: self.text.clone(),
        })
    }

    /// Where [`Commit::run`] compares a text that fails to typeset: what the file held at the last write.
    pub fn disk(&self) -> &str {
        &self.disk
    }

    /// A commit reached the disk.
    pub fn written(&mut self, text: &str) {
        self.disk = text.to_owned();
    }

    /// Takes back every change the disk has not seen, after one was refused or could not be written.
    pub fn revert(&mut self) -> Result<(), String> {
        self.text.clone_from(&self.disk);
        self.queued.clone_from(&self.disk);
        self.load()?;
        self.problems = validate::check(&self.path, &self.own);
        Ok(())
    }

    /// Writes the changes now, on this thread, and takes them back if they cannot be written.
    #[cfg(test)]
    pub fn save(&mut self) -> Result<(), Problem> {
        let Some(commit) = self.commit() else {
            return Ok(());
        };
        match commit.run(&self.disk) {
            Ok(()) => {
                self.disk = commit.text;
                Ok(())
            }
            Err(problem) => {
                self.revert().ok();
                Err(problem)
            }
        }
    }

    /// Takes new text if it parses, holds `expected` at `path`, and adds no problem the old text did not have.
    fn replace(&mut self, text: String, path: &[&str], expected: Option<&Value>) -> Result<(), Vec<Problem>> {
        let own = Self::parse(&text)?;
        if lookup(&own, path) != expected {
            return Err(Problem::internal(format!(
                "the edit of {} did not apply",
                path.join(".")
            )));
        }
        self.take(text, own)
    }

    fn accept(&mut self, text: String) -> Result<(), Vec<Problem>> {
        let own = Self::parse(&text)?;
        self.take(text, own)
    }

    fn parse(text: &str) -> Result<Value, Vec<Problem>> {
        let own: Value = serde_json::from_str(text).map_err(|e| Problem::internal(e.to_string()))?;
        if own.is_object() {
            Ok(own)
        } else {
            Err(Problem::internal("a theme must be a JSON object".to_owned()))
        }
    }

    fn take(&mut self, text: String, own: Value) -> Result<(), Vec<Problem>> {
        let now = validate::check(&self.path, &own);
        let introduced: Vec<Problem> = now
            .iter()
            .filter(|problem| !self.problems.contains(problem))
            .cloned()
            .collect();
        if !introduced.is_empty() {
            return Err(introduced);
        }
        if text == self.text {
            return Ok(());
        }
        self.text = text;
        self.load().map_err(Problem::internal)?;
        self.problems = now;
        Ok(())
    }

    fn load(&mut self) -> Result<(), String> {
        self.own = serde_json::from_str(&self.text).map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.effective = default_theme_value();
        merge(&mut self.effective, self.own.clone());
        Ok(())
    }
}

fn lookup<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, key| value.get(*key))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;

    /// A fresh directory under the system temp folder, removed on drop.
    pub struct Scratch(pub PathBuf);

    impl Scratch {
        pub fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "druck-desktop-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    const THEME: &str = "{\n  \"version\": 1,\n  \"tokens\": { \"sizes\": { \"big\": \"20pt\" } },\n  \"styles\": {\n    \"heading-1\": { \"size\": \"$sizes.big\", \"color\": \"#7a2a2a\" }\n  }\n}\n";

    #[test]
    fn a_change_is_partial_and_keeps_token_references_as_written() {
        let scratch = Scratch::new();
        let path = scratch.write("t.json", THEME);
        let mut file = ThemeFile::open(&path).unwrap();
        file.set(&["styles", "heading-1", "align"], json!("right")).unwrap();
        file.save().unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert_eq!(
            saved,
            THEME.replace(
                "\"color\": \"#7a2a2a\" }",
                "\"color\": \"#7a2a2a\", \"align\": \"right\" }"
            )
        );
        assert!(saved.contains("\"$sizes.big\""));
    }

    #[test]
    fn removing_a_property_returns_the_inherited_value_and_the_earlier_file() {
        let scratch = Scratch::new();
        let path = scratch.write("t.json", THEME);
        let mut file = ThemeFile::open(&path).unwrap();
        let inherited = file.effective(&["styles", "heading-2", "size"]).cloned();
        file.set(&["styles", "heading-2", "size"], json!("30pt")).unwrap();
        assert_eq!(file.effective(&["styles", "heading-2", "size"]), Some(&json!("30pt")));
        file.remove(&["styles", "heading-2", "size"]).unwrap();
        assert_eq!(file.effective(&["styles", "heading-2", "size"]).cloned(), inherited);
        assert!(file.get(&["styles", "heading-2"]).is_none());
        file.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), THEME);
    }

    #[test]
    fn an_invalid_value_is_reported_with_its_property_and_never_reaches_disk() {
        let scratch = Scratch::new();
        let path = scratch.write("t.json", THEME);
        let mut file = ThemeFile::open(&path).unwrap();
        for (property, value) in [
            ("size", json!("huge")),
            ("size", json!("$sizes.missing")),
            ("align", json!("diagonal")),
            ("line-height", json!(9)),
        ] {
            let problems = file.set(&["styles", "heading-1", property], value).unwrap_err();
            assert!(!problems.is_empty(), "{property}");
        }
        let problems = file.set(&["styles", "heading-1", "line-height"], json!(9)).unwrap_err();
        assert_eq!(problems[0].property.as_deref(), Some("styles.heading-1.line-height"));
        assert!(!file.is_dirty());
        file.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), THEME);
    }

    #[test]
    fn a_problem_the_file_already_had_does_not_block_other_edits() {
        let scratch = Scratch::new();
        let text = "{ \"version\": 1, \"styles\": { \"body\": { \"font\": \"$fonts.nope\" } } }";
        let path = scratch.write("t.json", text);
        let mut file = ThemeFile::open(&path).unwrap();
        assert!(!file.problems().is_empty());
        file.set(&["styles", "heading-1", "align"], json!("right")).unwrap();
        assert!(file.set(&["styles", "heading-1", "align"], json!("diagonal")).is_err());
    }

    #[test]
    fn open_rejects_what_is_not_json_and_reports_what_is_not_a_valid_theme() {
        let scratch = Scratch::new();
        assert!(ThemeFile::open(&scratch.write("a.json", "[1]")).is_err());
        assert!(
            !ThemeFile::open(&scratch.write("b.json", "{ \"version\": 2 }"))
                .unwrap()
                .problems()
                .is_empty()
        );
        assert!(ThemeFile::open(&scratch.0.join("missing.json")).is_err());
    }
}
