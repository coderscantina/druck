//! What the app remembers between runs: the window's size, the recent documents, and each mode's zoom. It lives in the
//! operating system's app data folder and never in a project. What the author sets is in `settings.json`, see
//! [`crate::settings`]. Holds no gpui types.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::persist::atomic;
use crate::persist::recovery::data_dir;

/// How many recent documents are kept.
pub const RECENT: usize = 10;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// The window's width and height when it last closed.
    pub window: Option<(f32, f32)>,
    /// Most recent first.
    pub recent: Vec<PathBuf>,
    /// Write's text zoom, where 1 is 100 percent.
    pub write_zoom: Option<f32>,
    /// The pages' zoom. `None` fits them to the pane.
    pub publish_zoom: Option<f32>,
}

impl State {
    /// Puts a document at the front of the recent list.
    pub fn remember(&mut self, path: &Path) {
        self.recent.retain(|other| other != path);
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(RECENT);
    }

    pub fn forget_recent(&mut self) {
        self.recent.clear();
    }

    /// Reads the file. One that is missing or unreadable gives the defaults, since a lost setting is not worth a
    /// message.
    pub fn load(file: &Path) -> Self {
        fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<Record>(&text).ok())
            .map(Self::from)
            .unwrap_or_default()
    }

    /// Writes the file whole or not at all.
    pub fn save(&self, file: &Path) -> Result<(), String> {
        let describe = |error: std::io::Error| format!("cannot save {}: {error}", file.display());
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder).map_err(describe)?;
        }
        let text = serde_json::to_string_pretty(&Record::from(self)).map_err(|error| error.to_string())?;
        atomic::write(file, text.as_bytes()).map_err(describe)
    }
}

/// `state.json`, as written.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Record {
    window: Option<WindowRecord>,
    recent: Vec<String>,
    zoom: ZoomRecord,
}

#[derive(Serialize, Deserialize)]
struct WindowRecord {
    width: f32,
    height: f32,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ZoomRecord {
    write: Option<f32>,
    publish: Option<f32>,
}

impl From<&State> for Record {
    fn from(state: &State) -> Self {
        Self {
            window: state.window.map(|(width, height)| WindowRecord { width, height }),
            recent: (state.recent.iter())
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            zoom: ZoomRecord {
                write: state.write_zoom,
                publish: state.publish_zoom,
            },
        }
    }
}

impl From<Record> for State {
    fn from(record: Record) -> Self {
        let positive = |value: f32| (value > 0.).then_some(value);
        Self {
            window: record
                .window
                .and_then(|window| Some((positive(window.width)?, positive(window.height)?))),
            recent: record.recent.into_iter().map(PathBuf::from).take(RECENT).collect(),
            write_zoom: record.zoom.write.and_then(positive),
            publish_zoom: record.zoom.publish.and_then(positive),
        }
    }
}

/// The file the state is kept in.
pub fn state_file() -> Option<PathBuf> {
    Some(data_dir()?.join("state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("druck-state-{name}-{}", std::process::id()))
            .join("state.json")
    }

    #[test]
    fn the_state_survives_a_save_and_a_load() {
        let file = file("round");
        let mut state = State {
            window: Some((1200., 800.)),
            write_zoom: Some(1.25),
            publish_zoom: Some(0.5),
            ..State::default()
        };
        state.remember(Path::new("/books/one"));
        state.save(&file).expect("saves");
        assert_eq!(State::load(&file), state);
        fs::remove_dir_all(file.parent().expect("folder")).ok();
    }

    #[test]
    fn a_file_written_before_the_records_still_loads() {
        let file = file("old");
        fs::create_dir_all(file.parent().expect("folder")).expect("folder");
        let text =
            r#"{"window":{"width":1000.0,"height":700.0},"recent":["/books/a"],"zoom":{"write":1.5,"publish":null}}"#;
        fs::write(&file, text).expect("file");
        let state = State::load(&file);
        assert_eq!(state.window, Some((1000., 700.)));
        assert_eq!(state.recent, [PathBuf::from("/books/a")]);
        assert_eq!((state.write_zoom, state.publish_zoom), (Some(1.5), None));
        fs::remove_dir_all(file.parent().expect("folder")).ok();
    }

    #[test]
    fn recent_documents_move_to_the_front_without_repeats_and_stop_at_ten() {
        let mut state = State::default();
        for number in 0..12 {
            state.remember(Path::new(&format!("/books/{number}")));
        }
        state.remember(Path::new("/books/5"));
        assert_eq!(state.recent.len(), RECENT);
        assert_eq!(state.recent[0], Path::new("/books/5"));
        assert_eq!(
            state
                .recent
                .iter()
                .filter(|path| **path == Path::new("/books/5"))
                .count(),
            1
        );
    }

    #[test]
    fn a_missing_or_broken_file_gives_the_defaults() {
        assert_eq!(State::load(&file("missing")), State::default());
        let broken = file("broken");
        fs::create_dir_all(broken.parent().expect("folder")).expect("folder");
        fs::write(&broken, "{ not json").expect("file");
        assert_eq!(State::load(&broken), State::default());
        fs::remove_dir_all(broken.parent().expect("folder")).ok();
    }
}
