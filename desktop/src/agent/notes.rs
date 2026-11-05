//! Advice that changes nothing: margin notes, kept in `.druck/notes.json` and anchored by the text they quote. Without
//! gpui. The app is the file's only writer, so nothing here merges.

use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::persist::atomic;

const FOLDER: &str = ".druck";
const FILE: &str = "notes.json";

/// One note. `chapter` is the file's path relative to the project folder, as the agent named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: u64,
    pub chapter: String,
    pub quote: String,
    pub title: String,
    pub text: String,
    pub suggestion: Option<String>,
}

impl Note {
    /// A note from what the agent passed to the tool. `None` if it is not shaped as the tool asks.
    pub fn from_input(id: u64, input: &Value) -> Option<Self> {
        let field = |name: &str| input[name].as_str().map(str::to_owned);
        Some(Self {
            id,
            chapter: field("chapter")?,
            quote: field("quote")?,
            title: field("title")?,
            text: field("text")?,
            suggestion: field("suggestion").filter(|suggestion| !suggestion.is_empty()),
        })
    }

    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "chapter": self.chapter,
            "quote": self.quote,
            "title": self.title,
            "text": self.text,
            "suggestion": self.suggestion,
        })
    }

    /// Where the quoted text stands in `text`, if it is still there.
    pub fn anchor(&self, text: &str) -> Option<Range<usize>> {
        text.find(&self.quote).map(|at| at..at + self.quote.len())
    }
}

/// A project's notes.
pub struct Notes {
    file: PathBuf,
    notes: Vec<Note>,
}

impl Notes {
    /// Reads the notes of the project in `base`. A missing or unreadable file holds none.
    pub fn open(base: &Path) -> Self {
        let file = base.join(FOLDER).join(FILE);
        let notes = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value["notes"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|note| Note::from_input(note["id"].as_u64()?, note))
            .collect();
        Self { file, notes }
    }

    pub fn all(&self) -> &[Note] {
        &self.notes
    }

    pub fn get(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|note| note.id == id)
    }

    /// Adds what the agent passed to the tool. Returns whether it was a note.
    pub fn add(&mut self, input: &Value) -> io::Result<bool> {
        let id = self.notes.iter().map(|note| note.id).max().unwrap_or(0) + 1;
        let Some(note) = Note::from_input(id, input) else {
            return Ok(false);
        };
        self.notes.push(note);
        self.save()?;
        Ok(true)
    }

    pub fn remove(&mut self, id: u64) -> io::Result<()> {
        self.notes.retain(|note| note.id != id);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(folder) = self.file.parent() {
            fs::create_dir_all(folder)?;
        }
        let notes: Vec<Value> = self.notes.iter().map(Note::to_json).collect();
        let text = serde_json::to_string_pretty(&json!({ "notes": notes })).map_err(io::Error::other)?;
        atomic::write(&self.file, format!("{text}\n").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Value {
        json!({"chapter": "manuscript/a.md", "quote": "the lamps", "title": "Tighter?", "text": "Two lamps.", "suggestion": "the lamp"})
    }

    #[test]
    fn notes_survive_a_reload_and_leave_with_a_dismiss() {
        let folder = std::env::temp_dir().join(format!("druck-notes-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let mut notes = Notes::open(&folder);
        assert!(notes.add(&input()).unwrap());
        assert!(!notes.add(&json!({"chapter": "only"})).unwrap());
        let read = Notes::open(&folder);
        assert_eq!(read.all(), notes.all());
        assert_eq!(read.all()[0].suggestion.as_deref(), Some("the lamp"));
        notes.remove(read.all()[0].id).unwrap();
        assert!(Notes::open(&folder).all().is_empty());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_note_finds_its_quote_until_the_text_is_gone() {
        let note = Note::from_input(1, &input()).unwrap();
        assert_eq!(note.anchor("All the lamps were lit."), Some(4..13));
        assert_eq!(note.anchor("All were lit."), None);
    }
}
