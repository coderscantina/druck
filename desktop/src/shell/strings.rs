//! The string tables: English and German for everything the app shows, found by a key the compiler checks.
//!
//! # Adding strings
//!
//! Each folder keeps its own table in a `strings.rs` of its own, so slices do not edit one file. A table is an enum
//! made by [`strings!`](crate::strings), one line per key with both languages:
//!
//! ```ignore
//! crate::strings! {
//!     pub enum Key {
//!         Save => ("Save", "Sichern"),
//!         SavedCount => ("{count} files saved", "{count} Dateien gesichert"),
//!     }
//! }
//! ```
//!
//! A key without both languages does not compile. The macro also adds a test that no text is empty and that both
//! languages name the same `{placeholders}`, so one table per file. Read a key with `t!(Key::Save)`, which gives a
//! `&'static str` in the language now set, or fill placeholders with `t!(Key::SavedCount, count = 3)`, which gives a
//! `String`. Looking a string up at render time is what makes a language change show without a restart.

use std::sync::atomic::{AtomicU8, Ordering};

/// A language the app speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    German,
}

impl Language {
    /// The language of the system's first preferred language, English when it is none the app speaks.
    pub fn of_system() -> Self {
        let preferred = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .or_else(|| std::env::var("LANG").ok());
        preferred.as_deref().map_or(Self::English, Self::from_preferences)
    }

    /// The first language in a list such as `defaults read -g AppleLanguages` prints, or a `LANG` value.
    fn from_preferences(text: &str) -> Self {
        let first = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .find(|word| !word.is_empty());
        match first {
            Some(code) if code.len() >= 2 && code[..2].eq_ignore_ascii_case("de") => Self::German,
            _ => Self::English,
        }
    }
}

/// Which language the user asked for: a fixed one, or whatever the system uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LanguageSetting {
    #[default]
    System,
    English,
    German,
}

impl LanguageSetting {
    pub fn resolve(self) -> Language {
        match self {
            Self::System => Language::of_system(),
            Self::English => Language::English,
            Self::German => Language::German,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language every lookup answers in.
pub fn language() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::German,
        _ => Language::English,
    }
}

/// Sets the language lookups answer in. Views read strings as they render, so they show it on their next paint.
pub fn set_language(language: Language) {
    CURRENT.store(u8::from(language == Language::German), Ordering::Relaxed);
}

/// Tests that change the language, or depend on it, hold this, since the language is one setting for the whole process.
#[cfg(test)]
pub static LANGUAGE_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A table of strings keyed by an enum. Made by [`strings!`](crate::strings).
pub trait Table: Copy + 'static {
    #[allow(dead_code, reason = "the table's test checks every key")]
    const ALL: &'static [Self];

    fn text(self, language: Language) -> &'static str;

    /// The key's name, for messages about a table.
    #[allow(dead_code, reason = "the table's test names the key it fails on")]
    fn name(self) -> &'static str;
}

/// The text of `key` in the language now set.
pub fn lookup<K: Table>(key: K) -> &'static str {
    key.text(language())
}

/// `text` with each `{name}` replaced by its value. A `{name}` with no value stays as written.
pub fn fill(text: &str, values: &[(&str, String)]) -> String {
    let mut out = text.to_owned();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// The `{names}` a text holds, in order.
#[allow(dead_code, reason = "the table's test compares them")]
pub fn placeholders(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        found.push(&after[..close]);
        rest = &after[close + 1..];
    }
    found
}

/// Panics unless every key of the table has text in both languages that names the same placeholders. The test the
/// [`strings!`](crate::strings) macro adds calls this.
#[allow(dead_code, reason = "the table's test calls it")]
pub fn check<K: Table>() {
    for key in K::ALL {
        let (english, german) = (key.text(Language::English), key.text(Language::German));
        assert!(!english.trim().is_empty(), "{} has no English text", key.name());
        assert!(!german.trim().is_empty(), "{} has no German text", key.name());
        assert_eq!(
            placeholders(english),
            placeholders(german),
            "{} names different placeholders in English and German",
            key.name()
        );
    }
}

/// Makes a table: an enum with a text in each language per key. See [the module](crate::shell::strings).
#[macro_export]
macro_rules! strings {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($(#[$doc:meta])* $key:ident => ($en:expr, $de:expr)),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[allow(clippy::enum_variant_names)]
        $vis enum $name { $($(#[$doc])* $key),+ }

        impl $crate::shell::strings::Table for $name {
            const ALL: &'static [Self] = &[$(Self::$key),+];

            fn text(self, language: $crate::shell::strings::Language) -> &'static str {
                match self {
                    $(Self::$key => match language {
                        $crate::shell::strings::Language::English => $en,
                        $crate::shell::strings::Language::German => $de,
                    }),+
                }
            }

            fn name(self) -> &'static str {
                match self { $(Self::$key => stringify!($key)),+ }
            }
        }

        #[cfg(test)]
        mod string_table_tests {
            #[test]
            fn every_key_has_both_languages() {
                $crate::shell::strings::check::<super::$name>();
            }
        }
    };
}

/// The text of a key in the language now set: `t!(Key::Save)` is a `&'static str`, and
/// `t!(Key::Saved, count = 3)` fills the `{count}` of the text and is a `String`.
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::shell::strings::lookup($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::shell::strings::fill(
            $crate::shell::strings::lookup($key),
            &[$((stringify!($name), $value.to_string())),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::strings! {
        enum Sample {
            Plain => ("Open", "Öffnen"),
            Counted => ("{count} files", "{count} Dateien"),
        }
    }

    #[test]
    fn a_key_answers_in_the_language_set() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_language(Language::German);
        assert_eq!(crate::t!(Sample::Plain), "Öffnen");
        set_language(Language::English);
        assert_eq!(crate::t!(Sample::Plain), "Open");
    }

    #[test]
    fn placeholders_are_filled_by_name() {
        assert_eq!(
            fill(Sample::Counted.text(Language::German), &[("count", "3".into())]),
            "3 Dateien"
        );
        assert_eq!(placeholders("{a} and {b}"), ["a", "b"]);
    }

    #[test]
    #[should_panic(expected = "different placeholders")]
    fn a_table_whose_languages_disagree_fails_the_check() {
        #[derive(Clone, Copy)]
        struct Broken;

        impl Table for Broken {
            const ALL: &'static [Self] = &[Self];

            fn text(self, language: Language) -> &'static str {
                match language {
                    Language::English => "{count} files",
                    Language::German => "{n} Dateien",
                }
            }

            fn name(self) -> &'static str {
                "Broken"
            }
        }

        check::<Broken>();
    }

    #[test]
    fn the_first_preferred_language_decides() {
        assert_eq!(
            Language::from_preferences("(\n    \"de-AT\",\n    \"en-US\"\n)"),
            Language::German
        );
        assert_eq!(
            Language::from_preferences("(\n    \"en-GB\",\n    \"de-DE\"\n)"),
            Language::English
        );
        assert_eq!(Language::from_preferences("de_DE.UTF-8"), Language::German);
        assert_eq!(Language::from_preferences(""), Language::English);
    }
}
