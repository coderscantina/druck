//! What the app remembers between runs: the window's size, the recent documents, and the settings. It lives in the
//! operating system's app data folder and never in a project. Holds no gpui types.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::strings::LanguageSetting;
use crate::tokens::Scheme;

/// How many recent documents are kept.
pub const RECENT: usize = 10;

/// Which colors the app uses: the system's or a fixed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

impl Appearance {
    /// The fixed scheme, or `None` to follow the system.
    pub fn scheme(self) -> Option<Scheme> {
        match self {
            Self::System => None,
            Self::Dark => Some(Scheme::Dark),
            Self::Light => Some(Scheme::Light),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// The window's width and height when it last closed.
    pub window: Option<(f32, f32)>,
    /// Most recent first.
    pub recent: Vec<PathBuf>,
    pub appearance: Appearance,
    pub language: LanguageSetting,
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

    fn to_json(&self) -> Value {
        json!({
            "window": self.window.map(|(width, height)| json!({ "width": width, "height": height })),
            "recent": self.recent.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
            "appearance": match self.appearance {
                Appearance::System => "system",
                Appearance::Dark => "dark",
                Appearance::Light => "light",
            },
            "language": match self.language {
                LanguageSetting::System => "system",
                LanguageSetting::English => "english",
                LanguageSetting::German => "german",
            },
        })
    }

    fn from_json(value: &Value) -> Self {
        let window = value.get("window").and_then(|window| {
            let size = |key: &str| {
                window
                    .get(key)?
                    .as_f64()
                    .map(|size| size as f32)
                    .filter(|size| *size > 0.)
            };
            Some((size("width")?, size("height")?))
        });
        let recent = value
            .get("recent")
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .take(RECENT)
                    .collect()
            })
            .unwrap_or_default();
        let appearance = match value.get("appearance").and_then(Value::as_str) {
            Some("dark") => Appearance::Dark,
            Some("light") => Appearance::Light,
            _ => Appearance::System,
        };
        let language = match value.get("language").and_then(Value::as_str) {
            Some("english") => LanguageSetting::English,
            Some("german") => LanguageSetting::German,
            _ => LanguageSetting::System,
        };
        Self {
            window,
            recent,
            appearance,
            language,
        }
    }

    /// Reads the file. One that is missing or unreadable gives the defaults, since a lost setting is not worth a
    /// message.
    pub fn load(file: &Path) -> Self {
        fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|value| Self::from_json(&value))
            .unwrap_or_default()
    }

    /// Writes the file whole or not at all.
    pub fn save(&self, file: &Path) -> Result<(), String> {
        let describe = |error: std::io::Error| format!("cannot save {}: {error}", file.display());
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder).map_err(describe)?;
        }
        let temporary = file.with_extension("json.part");
        let text = serde_json::to_string_pretty(&self.to_json()).map_err(|error| error.to_string())?;
        fs::write(&temporary, text).map_err(describe)?;
        fs::rename(&temporary, file).map_err(describe)
    }
}

/// The folder the operating system gives this app for its own data, such as `~/Library/Application Support/Druck`.
/// `DRUCK_DATA_DIR` names another one, so a measurement or a test leaves the real one alone.
pub fn app_data_dir() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("DRUCK_DATA_DIR") {
        return Some(PathBuf::from(folder));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    if cfg!(target_os = "macos") {
        return Some(home.join("Library/Application Support/Druck"));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    Some(data.join("druck"))
}

/// The file the state is kept in.
pub fn state_file() -> Option<PathBuf> {
    Some(app_data_dir()?.join("state.json"))
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
            appearance: Appearance::Dark,
            language: LanguageSetting::German,
            ..State::default()
        };
        state.remember(Path::new("/books/one"));
        state.save(&file).expect("saves");
        assert_eq!(State::load(&file), state);
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
