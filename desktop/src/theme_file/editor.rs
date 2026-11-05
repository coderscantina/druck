//! The theme the inspector edits: one file, the changes the panels make to it, and the rule for common themes.
//! Every change is a text splice checked through the engine at once. Writing it is a [`Commit`] the caller runs, since
//! the probe typeset and the fsync take too long for the window's thread. A common theme opened from a project is
//! not changed before the author has chosen between a fork in the project and the shared file. Holds no gpui types,
//! and every folder it may write to is given to it, so tests run on scratch copies.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::library;
use super::strings::Key;
use super::tokens::{self, Group};
use super::writer::Commit;
use super::{ThemeFile, patch};
use crate::theme_file::validate::Problem;

/// The folders a theme may be in, and so the only places an edit may write to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirs {
    /// The open project, whose `themes/` folder holds its themes.
    pub project: Option<PathBuf>,
    /// The common theme folder of this machine.
    pub common: Option<PathBuf>,
}

/// Where the open theme is, which decides whether editing it needs a decision first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Project,
    Common,
    /// A file named on the command line, outside both folders.
    Loose,
}

/// One edit a panel asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Set(Vec<String>, Value),
    Remove(Vec<String>),
    /// The whole text, from the JSON view.
    Text(String),
    RenameToken(Group, String, String),
    RenameCustomStyle(String, String),
    /// A font file for a face of a family: copied into the theme's folder and named in the theme.
    AddFont {
        family: String,
        face: String,
        from: PathBuf,
    },
}

impl Change {
    pub fn set(path: &[&str], value: Value) -> Self {
        Self::Set(path.iter().map(|key| (*key).to_owned()).collect(), value)
    }

    pub fn remove(path: &[&str]) -> Self {
        Self::Remove(path.iter().map(|key| (*key).to_owned()).collect())
    }

    /// The dotted property the change concerns, for those who react to it.
    pub fn property(&self) -> String {
        match self {
            Self::Set(path, _) | Self::Remove(path) => path.join("."),
            Self::Text(_) => String::new(),
            Self::RenameToken(group, _, to) => format!("tokens.{}.{to}", group.key()),
            Self::RenameCustomStyle(_, to) => format!("custom-styles.{to}"),
            Self::AddFont { family, face, .. } => format!("fonts.{family}.{face}"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The change is made. It reaches the disk when its [`Commit`] has run.
    Saved,
    /// The theme is a common one and the author has not chosen yet.
    NeedsDecision,
}

pub struct Editor {
    file: Result<ThemeFile, String>,
    dirs: Dirs,
    place: Place,
    /// The folder every write stays inside: the project, the common folder, or for a loose file its own folder.
    root: Option<PathBuf>,
    /// A change waiting for the author's choice about a common theme.
    pending: Option<Change>,
    /// The author chose to change the shared file.
    shared: bool,
}

impl Editor {
    pub fn open(path: Option<&Path>, dirs: Dirs) -> Self {
        let file = path.map_or_else(|| Err(String::new()), ThemeFile::open);
        let place = file.as_ref().map_or(Place::Loose, |file| place_of(file.path(), &dirs));
        let root = file.as_ref().ok().and_then(|file| root_of(file.path(), place, &dirs));
        Self {
            file,
            dirs,
            place,
            root,
            pending: None,
            shared: false,
        }
    }

    pub fn file(&self) -> Result<&ThemeFile, &str> {
        self.file.as_ref().map_err(String::as_str)
    }

    /// Reads the file again, and says whether what is on disk differs from what was open. A file that does not parse
    /// keeps the last good text here.
    pub fn reload(&mut self) -> bool {
        let Ok(file) = &self.file else {
            return false;
        };
        let path = file.path().to_owned();
        if fs::read_to_string(&path).is_ok_and(|text| text == file.text()) {
            return false;
        }
        if let Ok(file) = ThemeFile::open(&path) {
            self.file = Ok(file);
        }
        true
    }

    #[cfg(test)]
    pub fn place(&self) -> Place {
        self.place
    }

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    /// The theme's name: a project theme's folder, a common theme's file stem.
    pub fn name(&self) -> Option<String> {
        let path = self.file.as_ref().ok()?.path();
        match self.place {
            Place::Project => path.parent()?.file_name(),
            Place::Common | Place::Loose => path.file_stem(),
        }
        .map(|name| name.to_string_lossy().into_owned())
    }

    pub fn pending(&self) -> Option<&Change> {
        self.pending.as_ref()
    }

    /// Whether changing the theme asks for a decision first.
    fn needs_decision(&self) -> bool {
        self.place == Place::Common && self.dirs.project.is_some() && !self.shared
    }

    /// Makes a change in the text. On a common theme in a project nothing happens until [`Self::fork`] or
    /// [`Self::edit_shared`], and the change waits. [`Self::take_commit`] says what to write for it.
    pub fn stage(&mut self, change: Change) -> Result<Outcome, String> {
        if self.file.is_err() {
            return Err(self.file.as_ref().err().cloned().unwrap_or_default());
        }
        if self.needs_decision() {
            self.pending = Some(change);
            return Ok(Outcome::NeedsDecision);
        }
        self.write(change)
    }

    /// What to write for the changes staged since the last call.
    pub fn take_commit(&mut self) -> Option<Commit> {
        self.file.as_mut().ok()?.commit()
    }

    /// A commit reached the disk.
    pub fn written(&mut self, text: &str) {
        if let Ok(file) = &mut self.file {
            file.written(text);
        }
    }

    /// Takes back the changes the disk has not seen, once nothing more is on its way. Returns whether there were any.
    pub fn take_back_unwritten(&mut self) -> bool {
        let Ok(file) = &mut self.file else {
            return false;
        };
        if file.text() == file.disk() {
            return false;
        }
        file.revert().is_ok()
    }

    /// Makes a change and writes it on this thread, for tests of what reaches the disk.
    #[cfg(test)]
    pub fn apply(&mut self, change: Change) -> Result<Outcome, String> {
        let outcome = self.stage(change)?;
        self.write_out()?;
        Ok(outcome)
    }

    /// Writes what is staged on this thread. A change that cannot be written is taken back.
    #[cfg(test)]
    pub fn write_out(&mut self) -> Result<(), String> {
        match &mut self.file {
            Ok(file) => file.save().map_err(|problem| problem.to_string()),
            Err(_) => Ok(()),
        }
    }

    /// Copies the common theme with its fonts and images into the project, opens the copy, and makes the waiting
    /// change there. A project theme wins over a common one of the same name, so documents naming the theme
    /// use the copy.
    pub fn fork(&mut self) -> Result<Outcome, String> {
        let (Some(project), Some(common), Some(name)) =
            (self.dirs.project.clone(), self.dirs.common.clone(), self.name())
        else {
            return Err(crate::t!(Key::NoProject).to_owned());
        };
        let forked = library::fork(&project, &common, &name, &name)?;
        self.file = ThemeFile::open(&forked.path);
        self.place = Place::Project;
        self.root = Some(project);
        match self.pending.take() {
            Some(change) => self.write(change),
            None => Ok(Outcome::Saved),
        }
    }

    /// Changes the shared file after all, from now on without asking again.
    pub fn edit_shared(&mut self) -> Result<Outcome, String> {
        self.shared = true;
        match self.pending.take() {
            Some(change) => self.write(change),
            None => Ok(Outcome::Saved),
        }
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// Refuses a path outside the folder this editor may write to, however it was reached.
    fn confine(&self, path: &Path) -> Result<(), String> {
        let path = druck::resource::normalize(path);
        match &self.root {
            Some(root) if path.starts_with(druck::resource::normalize(root)) => Ok(()),
            _ => Err(format!(
                "{} is outside the folders this theme may be changed in",
                path.display()
            )),
        }
    }

    fn write(&mut self, change: Change) -> Result<Outcome, String> {
        if let Ok(file) = &self.file {
            self.confine(file.path())?;
        }
        let file = self.file.as_mut().map_err(|message| message.clone())?;
        let refused = |problems: Vec<Problem>| {
            problems.first().map_or_else(
                || crate::t!(Key::CannotTake).to_owned(),
                |problem| problem.message.clone(),
            )
        };
        match &change {
            Change::Set(path, value) => file.set(&keys(path), value.clone()).map_err(refused)?,
            Change::Remove(path) => file.remove(&keys(path)).map_err(refused)?,
            Change::Text(text) => file.replace_text(text).map_err(refused)?,
            Change::RenameToken(group, from, to) => {
                let theme = file.value().clone();
                file.rewrite(|text| tokens::rename(&theme, text, *group, from, to))
                    .map_err(refused)?;
            }
            Change::RenameCustomStyle(from, to) => rename_custom_style(file, from, to).map_err(refused)?,
            Change::AddFont { family, face, from } => return self.add_font(family, face, from),
        }
        Ok(Outcome::Saved)
    }

    /// Copies a font file next to the theme and names it for a face. A file that is not a font changes nothing.
    fn add_font(&mut self, family: &str, face: &str, from: &Path) -> Result<Outcome, String> {
        let bytes = fs::read(from).map_err(|error| crate::t!(Key::CannotRead, path = from.display(), error = error))?;
        crate::theme_file::font_info::FontInfo::read(&bytes, 0)
            .map_err(|_| crate::t!(Key::NotAFont, path = from.display()))?;
        let theme = self.file.as_ref().map_err(|message| message.clone())?.path().to_owned();
        let folder = theme.parent().unwrap_or(Path::new("")).join("fonts");
        self.confine(&folder)?;
        let file = self.file.as_mut().map_err(|message| message.clone())?;
        let name = from
            .file_name()
            .map_or_else(|| "font.otf".to_owned(), |name| name.to_string_lossy().into_owned());
        let (target, relative) = free_name(&folder, &name, &bytes);
        let copied = if target.is_file() {
            false
        } else {
            fs::create_dir_all(&folder)
                .map_err(|error| crate::t!(Key::CannotCreate, path = folder.display(), error = error))?;
            fs::write(&target, &bytes)
                .map_err(|error| crate::t!(Key::CannotWrite, path = target.display(), error = error))?;
            true
        };
        let result = file.set(&["fonts", family, face], Value::String(relative));
        if let Err(problems) = result {
            if copied {
                fs::remove_file(&target).ok();
            }
            return Err(problems
                .first()
                .map(|problem| problem.message.clone())
                .unwrap_or_default());
        }
        Ok(Outcome::Saved)
    }
}

fn keys(path: &[String]) -> Vec<&str> {
    path.iter().map(String::as_str).collect()
}

/// The folder writes for a theme file stay inside.
fn root_of(path: &Path, place: Place, dirs: &Dirs) -> Option<PathBuf> {
    match place {
        Place::Project => dirs.project.clone(),
        Place::Common => dirs.common.clone(),
        Place::Loose => path.parent().map(Path::to_owned),
    }
}

/// Where a theme file is.
pub fn place_of(path: &Path, dirs: &Dirs) -> Place {
    if let Some(project) = &dirs.project
        && path.starts_with(project.join("themes"))
    {
        return Place::Project;
    }
    if let Some(common) = &dirs.common
        && path.starts_with(common)
    {
        return Place::Common;
    }
    Place::Loose
}

/// `fonts/<name>` in the theme's folder. A name taken by other bytes gets a number, and one taken by the same
/// bytes is reused.
fn free_name(folder: &Path, name: &str, bytes: &[u8]) -> (PathBuf, String) {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let extension = path
        .extension()
        .map_or_else(String::new, |extension| format!(".{}", extension.to_string_lossy()));
    let mut candidate = name.to_owned();
    let mut number = 1;
    loop {
        let target = folder.join(&candidate);
        if !target.exists() || fs::read(&target).is_ok_and(|existing| existing == bytes) {
            return (target, format!("fonts/{candidate}"));
        }
        number += 1;
        candidate = format!("{stem}-{number}{extension}");
    }
}

/// Renames a custom style and what names it as a base, a marker style, or a slot style.
fn rename_custom_style(file: &mut ThemeFile, from: &str, to: &str) -> Result<(), Vec<Problem>> {
    if !to.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || to.is_empty() {
        return Err(Problem::internal(format!(
            "\"{to}\" is not a style name: use letters, digits, hyphens, and underscores"
        )));
    }
    if file
        .value()
        .get("custom-styles")
        .and_then(|styles| styles.get(to))
        .is_some()
    {
        return Err(Problem::internal(crate::t!(Key::StyleExists, name = to)));
    }
    file.rewrite(|text| {
        let renamed = patch::rename_key(text, &["custom-styles", from], to)?;
        Ok(patch::rename_values(
            &renamed,
            from,
            to,
            Some(&["based-on", "marker", "style"]),
        ))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::theme_file::tests::Scratch;

    const THEME: &str = "{\n  \"version\": 1,\n  \"tokens\": { \"sizes\": { \"big\": \"20pt\" } },\n  \"styles\": { \"title\": { \"size\": \"$sizes.big\" } },\n  \"custom-styles\": { \"lead\": { \"based-on\": \"body\" }, \"lead2\": { \"based-on\": \"lead\" } }\n}\n";

    fn dirs(scratch: &Scratch) -> Dirs {
        Dirs {
            project: Some(scratch.0.join("project")),
            common: Some(scratch.0.join("common")),
        }
    }

    #[test]
    fn a_common_theme_in_a_project_waits_for_the_choice_and_a_fork_leaves_the_shared_file_alone() {
        let scratch = Scratch::new();
        let common = scratch.write("common/mine.json", THEME);
        let mut editor = Editor::open(Some(&common), dirs(&scratch));
        assert_eq!(editor.place(), Place::Common);
        let change = Change::set(&["styles", "title", "align"], json!("right"));
        assert_eq!(editor.apply(change.clone()), Ok(Outcome::NeedsDecision));
        assert_eq!(fs::read_to_string(&common).unwrap(), THEME);
        assert_eq!(editor.fork(), Ok(Outcome::Saved));
        editor.write_out().unwrap();
        assert_eq!(editor.place(), Place::Project);
        assert_eq!(fs::read_to_string(&common).unwrap(), THEME, "shared file untouched");
        let forked = scratch.0.join("project/themes/mine/theme.json");
        assert!(fs::read_to_string(forked).unwrap().contains("\"align\": \"right\""));
    }

    #[test]
    fn editing_the_shared_file_after_all_asks_only_once() {
        let scratch = Scratch::new();
        let common = scratch.write("common/mine.json", THEME);
        let mut editor = Editor::open(Some(&common), dirs(&scratch));
        editor
            .apply(Change::set(&["styles", "title", "align"], json!("right")))
            .unwrap();
        assert_eq!(editor.edit_shared(), Ok(Outcome::Saved));
        editor.write_out().unwrap();
        assert!(fs::read_to_string(&common).unwrap().contains("\"align\": \"right\""));
        assert_eq!(
            editor.apply(Change::set(&["styles", "title", "align"], json!("left"))),
            Ok(Outcome::Saved)
        );
    }

    #[test]
    fn a_theme_without_a_project_around_it_changes_at_once() {
        let scratch = Scratch::new();
        let common = scratch.write("common/mine.json", THEME);
        let mut editor = Editor::open(
            Some(&common),
            Dirs {
                project: None,
                common: Some(scratch.0.join("common")),
            },
        );
        assert_eq!(
            editor.apply(Change::set(&["styles", "title", "align"], json!("right"))),
            Ok(Outcome::Saved)
        );
    }

    #[test]
    fn renaming_a_token_and_a_custom_style_updates_what_names_them() {
        let scratch = Scratch::new();
        let path = scratch.write("project/themes/t/theme.json", THEME);
        let mut editor = Editor::open(Some(&path), dirs(&scratch));
        editor
            .apply(Change::RenameToken(Group::Sizes, "big".into(), "large".into()))
            .unwrap();
        editor
            .apply(Change::RenameCustomStyle("lead".into(), "intro".into()))
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"large\": \"20pt\""));
        assert!(text.contains("\"size\": \"$sizes.large\""));
        assert!(text.contains("\"intro\": { \"based-on\": \"body\" }"));
        assert!(text.contains("\"lead2\": { \"based-on\": \"intro\" }"));
        assert!(
            editor
                .apply(Change::RenameCustomStyle("intro".into(), "lead2".into()))
                .is_err()
        );
    }

    #[test]
    fn a_token_still_in_use_cannot_be_deleted() {
        let scratch = Scratch::new();
        let path = scratch.write("project/themes/t/theme.json", THEME);
        let mut editor = Editor::open(Some(&path), dirs(&scratch));
        let refused = editor.apply(Change::remove(&["tokens", "sizes", "big"])).unwrap_err();
        assert!(refused.contains("big"), "{refused}");
        assert_eq!(fs::read_to_string(&path).unwrap(), THEME);
    }

    #[test]
    fn adding_a_font_copies_it_into_the_themes_folder_and_a_non_font_changes_nothing() {
        let scratch = Scratch::new();
        let path = scratch.write("project/themes/t/theme.json", THEME);
        let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fonts/LibertinusSerif-Italic.otf");
        let mut editor = Editor::open(Some(&path), dirs(&scratch));
        editor
            .apply(Change::AddFont {
                family: "Mine".into(),
                face: "regular".into(),
                from: font,
            })
            .unwrap();
        assert!(
            scratch
                .0
                .join("project/themes/t/fonts/LibertinusSerif-Italic.otf")
                .is_file()
        );
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("\"regular\": \"fonts/LibertinusSerif-Italic.otf\"")
        );
        let text = scratch.write("notes.txt", "not a font");
        let before = fs::read_to_string(&path).unwrap();
        assert!(
            editor
                .apply(Change::AddFont {
                    family: "Other".into(),
                    face: "regular".into(),
                    from: text,
                })
                .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert!(!scratch.0.join("project/themes/t/fonts/notes.txt").exists());
    }

    #[test]
    fn a_change_the_engine_resolves_but_cannot_typeset_is_refused() {
        let scratch = Scratch::new();
        let path = scratch.write("project/themes/t/theme.json", THEME);
        let mut editor = Editor::open(Some(&path), dirs(&scratch));
        // The bundled italic has no glyph for the bullet of a nested list, which only a layout finds out.
        let refused = editor
            .apply(Change::set(&["styles", "list", "style"], json!("italic")))
            .unwrap_err();
        assert!(refused.contains("no glyph"), "{refused}");
        let refused = editor
            .apply(Change::set(&["styles", "body", "weight"], json!(600)))
            .unwrap_err();
        assert!(refused.contains("600"), "{refused}");
        assert_eq!(fs::read_to_string(&path).unwrap(), THEME);
        assert!(
            !path.with_file_name(".theme.druck-check.json").exists(),
            "the trial file is removed"
        );
    }

    #[test]
    fn a_theme_that_already_fails_to_typeset_can_still_be_mended() {
        let scratch = Scratch::new();
        let broken = THEME
            .replace(
                "\"version\": 1",
                "\"version\": 1, \"styles\": { \"list\": { \"style\": \"italic\" } }",
            )
            .replace("\"styles\": { \"title\": { \"size\": \"$sizes.big\" } },", "");
        let path = scratch.write("project/themes/t/theme.json", &broken);
        let mut editor = Editor::open(Some(&path), dirs(&scratch));
        assert_eq!(
            editor.apply(Change::set(&["styles", "body", "align"], json!("left"))),
            Ok(Outcome::Saved)
        );
        assert_eq!(
            editor.apply(Change::remove(&["styles", "list", "style"])),
            Ok(Outcome::Saved)
        );
    }

    /// Every file below a folder with its size and time of change.
    fn listing(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
        let mut files = Vec::new();
        let mut folders = vec![dir.to_owned()];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
                let path = entry.path();
                match entry.metadata() {
                    Ok(meta) if meta.is_dir() => folders.push(path),
                    Ok(meta) => files.push((path, meta.len(), meta.modified().unwrap())),
                    Err(_) => {}
                }
            }
        }
        files.sort();
        files
    }

    #[test]
    fn no_write_reaches_a_path_outside_the_temp_root() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let outside: Vec<PathBuf> = [
            library::common_dir(),
            Some(repo.join("fonts")),
            Some(repo.join("samples")),
            Some(repo.join("themes")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let before: Vec<_> = outside.iter().map(|dir| listing(dir)).collect();

        let scratch = Scratch::new();
        let font = repo.join("fonts/LibertinusSerif-Italic.otf");
        let dirs = dirs(&scratch);
        let common_dir = dirs.common.clone().unwrap();
        let project_dir = dirs.project.clone().unwrap();

        // Edits to a project theme, a fork of a common one, and the common folder's own operations.
        let project = scratch.write("project/themes/t/theme.json", THEME);
        let mut editor = Editor::open(Some(&project), dirs.clone());
        editor
            .apply(Change::set(&["styles", "title", "align"], json!("right")))
            .unwrap();
        editor
            .apply(Change::RenameToken(Group::Sizes, "big".into(), "large".into()))
            .unwrap();
        editor
            .apply(Change::RenameCustomStyle("lead".into(), "intro".into()))
            .unwrap();
        editor
            .apply(Change::AddFont {
                family: "../../escape".into(),
                face: "regular".into(),
                from: font.clone(),
            })
            .unwrap();
        let shared = scratch.write("common/shared.json", THEME);
        let mut forking = Editor::open(Some(&shared), dirs.clone());
        forking
            .apply(Change::set(&["styles", "title", "align"], json!("left")))
            .unwrap();
        forking.fork().unwrap();
        forking.write_out().unwrap();
        library::create(&common_dir, "fresh").unwrap();
        library::duplicate(&common_dir, "fresh", "fresh-copy").unwrap();
        library::rename(&common_dir, "fresh-copy", "renamed").unwrap();
        library::delete(&common_dir, "renamed").unwrap();

        // Names that climb out of a folder are refused before anything is written.
        assert!(library::create(&common_dir, "../escape").is_err());
        assert!(library::rename(&common_dir, "fresh", "../escape").is_err());
        assert!(library::duplicate(&common_dir, "fresh", "../escape").is_err());
        assert!(library::fork(&project_dir, &common_dir, "shared", "../escape").is_err());
        assert!(library::fork(&project_dir, &common_dir, "../shared", "x").is_err());
        assert!(editor.confine(&project_dir.join("themes/../../outside.json")).is_err());
        assert!(
            editor.confine(&common_dir.join("any.json")).is_err(),
            "a project theme may not write to the common folder"
        );
        assert!(editor.confine(&project_dir.join("themes/t/fonts/a.otf")).is_ok());

        // A file named on the command line may change only inside its own folder.
        let loose = scratch.write("elsewhere/mine.json", THEME);
        let mut loose_editor = Editor::open(Some(&loose), dirs);
        assert_eq!(loose_editor.place(), Place::Loose);
        loose_editor
            .apply(Change::set(&["styles", "title", "align"], json!("right")))
            .unwrap();
        assert!(
            loose_editor
                .confine(&scratch.0.join("project/themes/t/theme.json"))
                .is_err()
        );

        // Everything written is under the scratch root, and nothing outside it changed.
        let mut written: Vec<String> = fs::read_dir(&scratch.0)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        written.sort();
        assert_eq!(written, ["common", "elsewhere", "project"]);
        assert!(!scratch.0.parent().unwrap().join("escape").exists());
        assert!(!scratch.0.join("..").join("escape.json").exists());
        let after: Vec<_> = outside.iter().map(|dir| listing(dir)).collect();
        assert_eq!(before, after, "a folder outside the temp root changed");
    }
}
