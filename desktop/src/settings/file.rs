//! `settings.json`, without gpui. One table says how each setting reads and writes. The file holds only what differs from
//! the defaults, and a change touches only its own bytes, so what the author wrote by hand stays, unknown keys too.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::strings::Key;
pub use crate::editor::placeholders::Display as PlaceholderDisplay;
use crate::persist::atomic;
use crate::shell::strings::LanguageSetting;
use crate::theme_file::patch;
use crate::tokens::{ACCENTS, Scheme};

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

/// How long the lines in Write are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineLength {
    /// The book's measure.
    #[default]
    Book,
    /// About this many characters.
    Characters(u32),
}

/// The shortest and longest line a person can ask for.
const LINE_LENGTHS: std::ops::RangeInclusive<u64> = 20..=240;

/// What the author can set. Adding a setting is a field here, a line in [`FIELDS`], and a row in `window::ROWS`.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub appearance: Appearance,
    /// The accent as `0xrrggbb`, or `None` for the default.
    pub accent: Option<u32>,
    pub language: LanguageSetting,
    pub line_length: LineLength,
    pub typewriter: bool,
    pub dim_paragraphs: bool,
    pub selection_toolbar: bool,
    pub binder: bool,
    /// Typesetting warnings stay marked on the pages with the Proof tab closed, for a proof pass.
    pub warning_marks: bool,
    pub spelling: bool,
    /// The writing partner: Claude Code, spawned on request. Off leaves no trace on screen and spawns nothing.
    pub agent: bool,
    /// How a placeholder's value shows in Write.
    pub placeholders: PlaceholderDisplay,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            accent: None,
            language: LanguageSetting::System,
            line_length: LineLength::Book,
            typewriter: false,
            dim_paragraphs: false,
            selection_toolbar: true,
            binder: false,
            warning_marks: false,
            spelling: true,
            agent: false,
            placeholders: PlaceholderDisplay::Chip,
        }
    }
}

/// One member of the file.
struct Field {
    key: &'static str,
    /// Takes the value in, or says what is wrong with it.
    read: fn(&mut Settings, &Value) -> Result<(), String>,
    write: fn(&Settings) -> Value,
}

/// A member that is on or off.
macro_rules! switch {
    ($key:literal, $field:ident) => {
        Field {
            key: $key,
            read: |settings, value| {
                settings.$field = value
                    .as_bool()
                    .ok_or_else(|| wanted($key, crate::t!(Key::WantedSwitch)))?;
                Ok(())
            },
            write: |settings| json!(settings.$field),
        }
    };
}

const FIELDS: &[Field] = &[
    Field {
        key: "appearance",
        read: |settings, value| {
            settings.appearance = match value.as_str() {
                Some("system") => Appearance::System,
                Some("dark") => Appearance::Dark,
                Some("light") => Appearance::Light,
                _ => return Err(wanted("appearance", crate::t!(Key::WantedAppearance))),
            };
            Ok(())
        },
        write: |settings| {
            json!(match settings.appearance {
                Appearance::System => "system",
                Appearance::Dark => "dark",
                Appearance::Light => "light",
            })
        },
    },
    Field {
        key: "accent",
        read: |settings, value| {
            let color = value.as_str().and_then(parse_hex);
            settings.accent = Some(color.ok_or_else(|| wanted("accent", crate::t!(Key::WantedAccent)))?);
            Ok(())
        },
        write: |settings| settings.accent.map_or(Value::Null, |color| json!(hex(color))),
    },
    Field {
        key: "language",
        read: |settings, value| {
            settings.language = match value.as_str() {
                Some("system") => LanguageSetting::System,
                Some("english") => LanguageSetting::English,
                Some("german") => LanguageSetting::German,
                _ => return Err(wanted("language", crate::t!(Key::WantedLanguage))),
            };
            Ok(())
        },
        write: |settings| {
            json!(match settings.language {
                LanguageSetting::System => "system",
                LanguageSetting::English => "english",
                LanguageSetting::German => "german",
            })
        },
    },
    Field {
        key: "line_length",
        read: |settings, value| {
            settings.line_length = match (value.as_str(), value.as_u64()) {
                (Some("book"), _) => LineLength::Book,
                (_, Some(count)) if LINE_LENGTHS.contains(&count) => LineLength::Characters(count as u32),
                _ => {
                    return Err(wanted("line_length", crate::t!(Key::WantedLineLength)));
                }
            };
            Ok(())
        },
        write: |settings| match settings.line_length {
            LineLength::Book => json!("book"),
            LineLength::Characters(count) => json!(count),
        },
    },
    switch!("typewriter_scrolling", typewriter),
    switch!("dim_other_paragraphs", dim_paragraphs),
    switch!("selection_toolbar", selection_toolbar),
    switch!("binder", binder),
    switch!("warning_marks", warning_marks),
    switch!("spelling", spelling),
    switch!("agent", agent),
    Field {
        key: "placeholders",
        read: |settings, value| {
            settings.placeholders = match value.as_str() {
                Some("chip") => PlaceholderDisplay::Chip,
                Some("underline") => PlaceholderDisplay::Underline,
                _ => return Err(wanted("placeholders", crate::t!(Key::WantedPlaceholders))),
            };
            Ok(())
        },
        write: |settings| {
            json!(match settings.placeholders {
                PlaceholderDisplay::Chip => "chip",
                PlaceholderDisplay::Underline => "underline",
            })
        },
    },
];

fn wanted(key: &str, what: &str) -> String {
    crate::t!(Key::ShouldBe, key = key, what = what)
}

/// A colour written `#rrggbb`.
pub fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().strip_prefix('#')?;
    (digits.len() == 6 && digits.is_ascii())
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

pub fn hex(color: u32) -> String {
    format!("#{color:06x}")
}

/// Whether a colour is the default accent, which leaves the file.
fn is_default_accent(color: u32) -> bool {
    color == ACCENTS[0]
}

/// The settings a text says, or what is wrong with it. Unknown keys are left alone, for what a later version writes.
pub fn parse(text: &str) -> Result<Settings, String> {
    let root: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let Value::Object(members) = root else {
        return Err(crate::t!(Key::OneObject).to_owned());
    };
    let mut settings = Settings::default();
    for field in FIELDS {
        if let Some(value) = members.get(field.key) {
            (field.read)(&mut settings, value)?;
        }
    }
    Ok(settings)
}

/// Reads the file. One that is not there holds the defaults.
pub fn load(file: &Path) -> Result<Settings, String> {
    match fs::read_to_string(file) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(error.to_string()),
    }
}

/// The text with `after` in it, changing only the members that differ from `before`. A value back at its default leaves
/// the file. Fails on a text that is not valid, which is then left as it is.
fn changed(text: &str, before: &Settings, after: &Settings) -> Result<String, String> {
    parse(text)?;
    let defaults = Settings::default();
    let mut text = text.to_owned();
    for field in FIELDS {
        let value = (field.write)(after);
        if value == (field.write)(before) {
            continue;
        }
        let back_to_default = value == (field.write)(&defaults);
        text = if back_to_default {
            patch::remove(&text, &[field.key])?
        } else {
            patch::set(&text, &[field.key], &value)?
        };
    }
    Ok(text)
}

/// Writes the difference between two settings to the file, whole or not at all. An invalid file is never overwritten.
pub fn write_changes(file: &Path, before: &Settings, after: &Settings) -> Result<(), String> {
    let current = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{\n}\n".to_owned(),
        Err(error) => return Err(error.to_string()),
    };
    let text = changed(&current, before, after)?;
    if text == current {
        return Ok(());
    }
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    atomic::write(file, text.as_bytes()).map_err(|error| error.to_string())
}

/// `settings.json` in the user's config folder, beside the common themes.
pub fn settings_file() -> Option<PathBuf> {
    Some(
        druck::resource::FileResolver::shared_themes()?
            .parent()?
            .join("settings.json"),
    )
}

impl Settings {
    /// The accent to write for a choice in the window: the default leaves the file.
    pub fn set_accent(&mut self, color: u32) {
        self.accent = (!is_default_accent(color)).then_some(color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("druck-settings-{name}-{}", std::process::id()));
        fs::remove_dir_all(&folder).ok();
        folder
    }

    #[test]
    fn a_missing_file_holds_the_defaults() {
        assert_eq!(load(&folder("missing").join("settings.json")), Ok(Settings::default()));
    }

    #[test]
    fn values_are_read_and_unknown_keys_ignored() {
        let settings = parse(
            r##"{ "appearance": "dark", "accent": "#2F8FB8", "line_length": 66, "binder": true, "later": { "x": 1 } }"##,
        )
        .expect("valid");
        assert_eq!(settings.appearance, Appearance::Dark);
        assert_eq!(settings.accent, Some(0x2f8fb8));
        assert_eq!(settings.line_length, LineLength::Characters(66));
        assert!(settings.binder);
        assert!(settings.selection_toolbar);
    }

    #[test]
    fn a_wrong_value_is_named() {
        let problem = parse(r#"{ "appearance": "blue" }"#).expect_err("invalid");
        assert!(problem.contains("appearance"), "{problem}");
        assert!(parse(r#"{ "line_length": 3 }"#).is_err());
        assert!(parse(r#"{ "accent": "red" }"#).is_err());
        assert!(parse("[]").is_err());
    }

    #[test]
    fn only_changed_values_are_written_and_the_rest_keeps_its_bytes() {
        let file = folder("changed").join("settings.json");
        let before = Settings::default();
        let binder = Settings {
            binder: true,
            ..before.clone()
        };
        write_changes(&file, &before, &binder).expect("writes");
        assert_eq!(fs::read_to_string(&file).expect("reads"), "{ \"binder\": true }\n");

        let by_hand = "{\n    \"later\": [ 1,2 ],\n    \"binder\": true\n}\n";
        fs::write(&file, by_hand).expect("writes");
        let dark = Settings {
            appearance: Appearance::Dark,
            ..binder.clone()
        };
        write_changes(&file, &binder, &dark).expect("writes");
        assert_eq!(
            fs::read_to_string(&file).expect("reads"),
            "{\n    \"later\": [ 1,2 ],\n    \"binder\": true,\n    \"appearance\": \"dark\"\n}\n"
        );
        fs::remove_dir_all(file.parent().expect("folder")).ok();
    }

    #[test]
    fn a_value_back_at_its_default_leaves_the_file() {
        let file = folder("default").join("settings.json");
        let before = Settings::default();
        let after = Settings {
            typewriter: true,
            ..before.clone()
        };
        write_changes(&file, &before, &after).expect("writes");
        write_changes(&file, &after, &before).expect("writes");
        assert_eq!(
            parse(&fs::read_to_string(&file).expect("reads")),
            Ok(Settings::default())
        );
        assert!(!fs::read_to_string(&file).expect("reads").contains("typewriter"));
        fs::remove_dir_all(file.parent().expect("folder")).ok();
    }

    #[test]
    fn an_invalid_file_is_reported_and_never_overwritten() {
        let file = folder("invalid").join("settings.json");
        fs::create_dir_all(file.parent().expect("folder")).expect("folder");
        fs::write(&file, "{ \"binder\": tru").expect("writes");
        let before = Settings::default();
        let after = Settings {
            binder: true,
            ..before.clone()
        };
        assert!(load(&file).is_err());
        assert!(write_changes(&file, &before, &after).is_err());
        assert_eq!(fs::read_to_string(&file).expect("reads"), "{ \"binder\": tru");
        fs::remove_dir_all(file.parent().expect("folder")).ok();
    }

    #[test]
    fn the_default_accent_is_not_kept_as_a_choice() {
        let mut settings = Settings::default();
        settings.set_accent(ACCENTS[3]);
        assert_eq!(settings.accent, Some(ACCENTS[3]));
        settings.set_accent(ACCENTS[0]);
        assert_eq!(settings.accent, None);
    }
}
