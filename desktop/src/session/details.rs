//! The document's title, authors, and so on, as `druck.yaml` or front matter says them, and writing them back. A write
//! is one text splice, checked with the engine's own parser before anything changes. Holds no gpui types.

use std::fs;
use std::path::{Path, PathBuf};

use super::strings::Key;
use druck::editor::resolved_config::{FrontMatter, InputSource, parse_settings, split_front_matter};
use druck::input::MANIFEST;

use super::yaml;
use super::{Kind, Session};
use crate::persist::atomic::{self, Outcome, content_hash};
use crate::source::{Edit, EditKind};

/// Where a new key goes among the keys already written.
pub(crate) const ORDER: &[&str] = &[
    "title", "subtitle", "author", "date", "lang", "cover", "theme", "meta", "chapters", "exports",
];

/// The languages the engine sets, as `lang` writes them, with what the panel shows.
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("de", "Deutsch")];

/// A `meta` row. A value with several lines cannot be typed into one field, so it shows but stays as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaRow {
    pub key: String,
    pub value: String,
    pub lines: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Details {
    pub title: String,
    pub subtitle: String,
    /// Names joined with commas. A list in the file reads the same.
    pub authors: String,
    pub date: String,
    pub language: String,
    /// The cover path as written.
    pub cover: String,
    pub meta: Vec<MetaRow>,
    /// Why the settings cannot be read. They stay as they were until the file is fixed.
    pub problem: Option<String>,
}

/// The fields with one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Title,
    Subtitle,
    Authors,
    Date,
    Language,
    Cover,
    Theme,
}

impl Field {
    fn key(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Subtitle => "subtitle",
            Self::Authors => "author",
            Self::Date => "date",
            Self::Language => "lang",
            Self::Cover => "cover",
            Self::Theme => "theme",
        }
    }

    /// What the field says in parsed settings.
    fn read(self, settings: &FrontMatter) -> String {
        match self {
            Self::Title => settings.title.clone().unwrap_or_default(),
            Self::Subtitle => settings.subtitle.clone().unwrap_or_default(),
            Self::Authors => settings
                .author
                .as_ref()
                .map(|names| names.0.join(", "))
                .unwrap_or_default(),
            Self::Date => settings.date.clone().unwrap_or_default(),
            Self::Language => settings
                .lang
                .as_ref()
                .and_then(|lang| serde_json::to_value(lang).ok())
                .and_then(|lang| lang.as_str().map(str::to_owned))
                .unwrap_or_default(),
            Self::Cover => settings.cover.clone().unwrap_or_default(),
            Self::Theme => settings.theme.clone().unwrap_or_default(),
        }
    }

    /// The value as YAML.
    fn yaml(self, value: &str) -> String {
        if self == Self::Authors {
            let names: Vec<String> = value.split(',').map(|name| name.trim().to_owned()).collect();
            if names.len() > 1 {
                return yaml::flow_list(&names);
            }
        }
        yaml::scalar(value)
    }
}

/// What parsed settings say, as the panel shows it.
pub(super) fn read(settings: &FrontMatter) -> Details {
    let meta = settings.meta.iter().filter_map(|(key, value)| {
        let row = |value: String, lines| MetaRow {
            key: key.clone(),
            value,
            lines,
        };
        match serde_json::to_value(value).ok()? {
            serde_json::Value::String(text) => Some(row(text, false)),
            serde_json::Value::Array(lines) => {
                let lines: Vec<&str> = lines.iter().filter_map(serde_json::Value::as_str).collect();
                Some(row(lines.join(", "), true))
            }
            _ => None,
        }
    });
    Details {
        title: Field::Title.read(settings),
        subtitle: Field::Subtitle.read(settings),
        authors: Field::Authors.read(settings),
        date: Field::Date.read(settings),
        language: Field::Language.read(settings),
        cover: Field::Cover.read(settings),
        meta: meta.collect(),
        problem: None,
    }
}

/// Parses settings text, where blank text means no settings.
pub(super) fn parse(text: &str, path: &Path) -> Result<FrontMatter, String> {
    if text.trim().is_empty() {
        return Ok(FrontMatter::default());
    }
    let source = InputSource::Document(path.to_owned());
    let base = path.parent().unwrap_or(path);
    parse_settings(text, 1, &source).map_err(|problems| {
        problems
            .first()
            .map(|problem| problem.display_from(base).to_string())
            .unwrap_or_else(|| "the settings cannot be read".to_owned())
    })
}

pub(super) fn applied(text: &str, edit: &Edit) -> String {
    format!("{}{}{}", &text[..edit.range.start], edit.text, &text[edit.range.end..])
}

impl Session {
    /// Sets one field. Empty text removes the key.
    pub fn set_field(&mut self, field: Field, value: &str) -> Result<(), String> {
        let value = value.trim();
        self.write_settings(
            |text| {
                if value.is_empty() {
                    yaml::remove(text, field.key())
                } else {
                    yaml::set(text, field.key(), &field.yaml(value), ORDER).map(Some)
                }
            },
            |settings| field.read(settings) == normalized(field, value),
        )
    }

    /// Sets a `meta` value. Empty text writes an empty value, so a suggested row can wait for its text.
    pub fn set_meta(&mut self, key: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        let written = if value.is_empty() {
            "\"\"".to_owned()
        } else {
            yaml::scalar(value)
        };
        self.write_settings(
            |text| yaml::set_nested(text, "meta", key, &written, ORDER).map(Some),
            |settings| {
                settings
                    .meta
                    .get(key)
                    .and_then(|found| serde_json::to_value(found).ok())
                    .is_some_and(|found| found == value)
            },
        )
    }

    pub fn remove_meta(&mut self, key: &str) -> Result<(), String> {
        self.write_settings(
            |text| yaml::remove_nested(text, "meta", key),
            |settings| !settings.meta.contains_key(key),
        )
    }

    /// Sets the cover to an image at `path`, copying it to `assets/` if it is outside a project, so the project stays
    /// whole. A single file refers to it as it is.
    pub fn set_cover(&mut self, path: &Path) -> Result<(), String> {
        let base = self.base_dir();
        let inside = path.strip_prefix(&base).ok().map(Path::to_path_buf);
        let relative = match (inside, &self.kind) {
            (Some(relative), _) => relative,
            (None, Kind::Project { .. }) => {
                let name = path
                    .file_name()
                    .ok_or_else(|| crate::t!(Key::NotAFile, path = path.display()))?;
                let assets = base.join("assets");
                let describe = |error: std::io::Error| crate::t!(Key::CannotCopy, path = path.display(), error = error);
                fs::create_dir_all(&assets).map_err(describe)?;
                fs::copy(path, assets.join(name)).map_err(describe)?;
                PathBuf::from("assets").join(name)
            }
            (None, Kind::Single { .. }) => path.to_path_buf(),
        };
        self.set_field(Field::Cover, &relative.to_string_lossy())
    }

    /// The cover image as a file, if one is set.
    pub fn cover_file(&self) -> Option<PathBuf> {
        let cover = &self.details.cover;
        (!cover.is_empty()).then(|| self.base_dir().join(cover))
    }

    /// Applies a splice to the settings text. `check` must agree with what the engine reads from the result, or
    /// nothing changes.
    pub(crate) fn write_settings(
        &mut self,
        edit: impl Fn(&str) -> Result<Option<Edit>, String>,
        check: impl Fn(&FrontMatter) -> bool,
    ) -> Result<(), String> {
        match &self.kind {
            Kind::Project { dir } => {
                let manifest = dir.join(MANIFEST);
                self.patch_manifest(&manifest, edit, check)?;
            }
            Kind::Single { path } => {
                let path = path.clone();
                self.patch_front_matter(&path, edit, check)?;
            }
        }
        self.refresh_details();
        Ok(())
    }

    /// Reads the manifest from disk, so an edit made elsewhere is not lost, splices it, and writes it.
    pub(super) fn patch_manifest(
        &mut self,
        manifest: &Path,
        edit: impl Fn(&str) -> Result<Option<Edit>, String>,
        check: impl Fn(&FrontMatter) -> bool,
    ) -> Result<(), String> {
        let text = fs::read_to_string(manifest)
            .map_err(|error| crate::t!(Key::CannotReadManifest, manifest = MANIFEST, error = error))?;
        let Some(change) = edit(&text)? else {
            return Ok(());
        };
        let patched = applied(&text, &change);
        let settings = parse(&patched, manifest)?;
        if !check(&settings) {
            return Err(crate::t!(Key::CannotChangeManifest, manifest = MANIFEST));
        }
        let expected = Some(content_hash(text.as_bytes()));
        match atomic::write_if(manifest, patched.as_bytes(), expected) {
            Ok(Outcome::Written) => {}
            Ok(Outcome::Changed(_)) => return Err(crate::t!(Key::ManifestChanged, manifest = MANIFEST)),
            Err(error) => return Err(crate::t!(Key::CannotWriteManifest, manifest = MANIFEST, error = error)),
        }
        self.set_synced(manifest, patched.into(), true);
        self.bump_revision();
        self.settings = Ok(settings);
        Ok(())
    }

    /// Splices the front matter of a single file, as an edit undo can take back. A file without any gets one.
    fn patch_front_matter(
        &mut self,
        path: &Path,
        edit: impl Fn(&str) -> Result<Option<Edit>, String>,
        check: impl Fn(&FrontMatter) -> bool,
    ) -> Result<(), String> {
        let first = self.book[0];
        let text = self.buffer(first).text().to_owned();
        let found = split_front_matter(&text)?;
        let yaml_text = found.as_ref().map_or("", |split| split.yaml);
        let Some(change) = edit(yaml_text)? else {
            return Ok(());
        };
        let patched = applied(yaml_text, &change);
        let settings = parse(&patched, path)?;
        if !check(&settings) {
            return Err(crate::t!(Key::CannotChangeFrontMatter).to_owned());
        }
        let edit = match &found {
            Some(split) => {
                let start = split.yaml.as_ptr() as usize - text.as_ptr() as usize;
                if patched.trim().is_empty() {
                    // Nothing is left between the fences, so the fences go too.
                    let body = split.body.as_ptr() as usize - text.as_ptr() as usize;
                    let blank = split.body.len() - split.body.trim_start_matches(['\n', '\r']).len();
                    let fences = start - "---\n".len()..body + blank;
                    Edit::new(fences, "")
                } else {
                    Edit::new(start + change.range.start..start + change.range.end, change.text)
                }
            }
            None => {
                let at = if text.starts_with('\u{feff}') {
                    '\u{feff}'.len_utf8()
                } else {
                    0
                };
                Edit::new(at..at, format!("---\n{patched}---\n\n"))
            }
        };
        self.edit_chapter(first, vec![edit], EditKind::Discrete)
            .map_err(|error| error.to_string())
    }
}

/// What the engine reads back after a value is written.
fn normalized(field: Field, value: &str) -> String {
    if field == Field::Authors {
        let names: Vec<&str> = value.split(',').map(str::trim).collect();
        return names.join(", ");
    }
    value.to_owned()
}
