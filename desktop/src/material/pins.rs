//! Material pinned to a paragraph, kept in `.druck/pins.json` and anchored as the agent's margin notes are: by the text
//! they quote, here the paragraph's opening words. Without gpui. The app is the file's only writer.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::persist::atomic;
use crate::source::{BlockKind, Blocks};

const FOLDER: &str = ".druck";
const FILE: &str = "pins.json";
/// How much of a paragraph's opening a pin quotes, at most, in bytes. Enough to be unique in a chapter, short enough
/// that an edit later in the paragraph leaves it.
const QUOTE: usize = 48;

/// One pin: an item of material on a paragraph of a chapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// The chapter's file, from the book's folder.
    pub chapter: String,
    /// The paragraph's opening words.
    pub quote: String,
    /// The item's id: its path from the book's folder, or `@key` for a bibliography entry.
    pub item: String,
    /// The passage of the item that was chosen when it was pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passage: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    pins: Vec<Pin>,
}

/// The paragraph, list, or quotation at `offset`, as a byte range of `text`.
pub fn paragraph_at(text: &str, offset: usize) -> Option<std::ops::Range<usize>> {
    let blocks = Blocks::parse(text);
    let block = blocks.as_slice().get(blocks.index_at(offset.min(text.len())))?;
    matches!(block.kind, BlockKind::Paragraph | BlockKind::List | BlockKind::Quote).then(|| block.range.clone())
}

/// What a pin on the paragraph at `offset` quotes: its opening, cut at a word's end.
pub fn quote_at(text: &str, offset: usize) -> Option<String> {
    let range = paragraph_at(text, offset)?;
    let paragraph = text[range].trim_end();
    if paragraph.len() <= QUOTE {
        return Some(paragraph.to_owned());
    }
    let cut = paragraph.floor_char_boundary(QUOTE);
    let cut = paragraph[..cut].rfind(' ').filter(|space| *space > 0).unwrap_or(cut);
    Some(paragraph[..cut].to_owned())
}

/// Where the paragraph a pin quotes starts in `text`, while the quote is still there.
pub fn anchor(text: &str, quote: &str) -> Option<usize> {
    let at = text.find(quote)?;
    paragraph_at(text, at).map(|range| range.start)
}

/// The pins of a book.
pub struct Pins {
    file: PathBuf,
    pins: Vec<Pin>,
}

impl Pins {
    /// Reads the pins of the book in `base`. A missing or unreadable file holds none.
    pub fn open(base: &Path) -> Self {
        let file = base.join(FOLDER).join(FILE);
        let pins = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<File>(&text).ok())
            .map(|read| read.pins)
            .unwrap_or_default();
        Self { file, pins }
    }

    pub fn all(&self) -> &[Pin] {
        &self.pins
    }

    /// Pins `item` to the paragraph `quote` names, unless it is pinned there already. Returns whether it was added.
    pub fn add(&mut self, pin: Pin) -> io::Result<bool> {
        let there = (self.pins.iter()).any(|other| {
            other.chapter == pin.chapter && other.quote == pin.quote && other.item == pin.item
        });
        if there {
            return Ok(false);
        }
        self.pins.push(pin);
        self.save().map(|()| true)
    }

    pub fn remove(&mut self, pin: &Pin) -> io::Result<()> {
        self.pins.retain(|other| other != pin);
        self.save()
    }

    /// Gives the pins of `chapter` the quotes their paragraphs open with now, after an edit, matched by the old quote.
    /// Returns whether any changed, and writes them if so.
    pub fn requote(&mut self, chapter: &str, quotes: &[(String, String)]) -> io::Result<bool> {
        let mut changed = false;
        for pin in self.pins.iter_mut().filter(|pin| pin.chapter == chapter) {
            if let Some((_, new)) = quotes.iter().find(|(old, new)| *old == pin.quote && old != new) {
                pin.quote = new.clone();
                changed = true;
            }
        }
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    /// With no pins left there is no file, so nothing empty stands in the project or a commit.
    fn save(&self) -> io::Result<()> {
        if self.pins.is_empty() {
            return match fs::remove_file(&self.file) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => {
                    if let Some(folder) = self.file.parent() {
                        fs::remove_dir(folder).ok();
                    }
                    Ok(())
                }
            };
        }
        if let Some(folder) = self.file.parent() {
            fs::create_dir_all(folder)?;
        }
        let file = File {
            pins: self.pins.clone(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
        atomic::write(&self.file, format!("{text}\n").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# Tolls\n\nHis wage was fixed and small: twelve Gulden a year, a house by the gate.\n\nShort one.\n";

    #[test]
    fn a_pin_quotes_its_paragraphs_opening_and_finds_it_again_after_edits_elsewhere() {
        let quote = quote_at(TEXT, 20).unwrap();
        assert_eq!(quote, "His wage was fixed and small: twelve Gulden a");
        assert_eq!(quote_at(TEXT, TEXT.len() - 3).as_deref(), Some("Short one."));
        assert_eq!(quote_at(TEXT, 2), None);
        let edited = TEXT.replace("# Tolls", "# Tolls and tariffs").replace("the gate", "the bridge");
        assert_eq!(anchor(&edited, &quote), Some(edited.find("His").unwrap()));
        assert_eq!(anchor("Gone.\n", &quote), None);
    }

    #[test]
    fn pins_survive_a_reload_add_once_follow_a_requote_and_leave_no_empty_file() {
        let base = std::env::temp_dir().join(format!("druck-pins-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        let mut pins = Pins::open(&base);
        let pin = Pin {
            chapter: "manuscript/05.md".into(),
            quote: "His wage".into(),
            item: "research/interview.md".into(),
            passage: Some("They rounded.".into()),
        };
        assert!(pins.add(pin.clone()).unwrap());
        assert!(!pins.add(pin.clone()).unwrap());
        assert_eq!(Pins::open(&base).all(), [pin.clone()]);
        let quotes = [("His wage".to_owned(), "Her wage".to_owned())];
        assert!(pins.requote("manuscript/05.md", &quotes).unwrap());
        assert_eq!(Pins::open(&base).all()[0].quote, "Her wage");
        let moved = pins.all()[0].clone();
        pins.remove(&moved).unwrap();
        assert!(!base.join(FOLDER).exists());
        fs::remove_dir_all(&base).unwrap();
    }
}
