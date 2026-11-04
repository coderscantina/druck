//! Where themes live: a project's `themes/<name>/theme.json` first, then the common folder of the user's
//! config as `<name>.json`. Every function takes its folders as arguments so tests never touch the real one.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

pub use druck::editor::theme_path::Scope;
use druck::editor::theme_path::{self, PROJECT_FILE, common_file as common_path};

use super::patch;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub scope: Scope,
    pub path: PathBuf,
}

/// The common theme folder of this machine, which may not exist yet.
pub fn common_dir() -> Option<PathBuf> {
    druck::resource::FileResolver::shared_themes()
}

/// Names are what `theme_path` accepts as a bare name: ASCII letters, digits, hyphens, underscores.
fn check_name(name: &str) -> Result<(), String> {
    if theme_path::is_name(name) {
        Ok(())
    } else {
        Err(format!(
            "\"{name}\" is not a theme name: use letters, digits, hyphens, and underscores"
        ))
    }
}

/// A theme by name. A project theme wins over a common one of the same name.
pub fn locate(name: &str, project: Option<&Path>, common: Option<&Path>) -> Option<Entry> {
    let project = project.map(|project| (Scope::Project, theme_path::project_file(project, name)));
    let common = common.map(|common| (Scope::Common, common_path(common, name)));
    let (scope, path) = project.into_iter().chain(common).find(|(_, path)| path.is_file())?;
    Some(Entry {
        name: name.to_owned(),
        scope,
        path,
    })
}

/// Every theme by name, sorted, with project themes shadowing common ones.
pub fn list(project: Option<&Path>, common: Option<&Path>) -> Vec<Entry> {
    let mut entries = BTreeMap::new();
    if let Some(common) = common {
        for entry in files(common) {
            let Some(name) = json_name(&entry) else { continue };
            entries.insert(
                name.clone(),
                Entry {
                    name,
                    scope: Scope::Common,
                    path: entry,
                },
            );
        }
    }
    if let Some(project) = project {
        for entry in folders(&project.join("themes")) {
            let path = entry.join(PROJECT_FILE);
            let Some(name) = entry.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if path.is_file() {
                entries.insert(
                    name.clone(),
                    Entry {
                        name,
                        scope: Scope::Project,
                        path,
                    },
                );
            }
        }
    }
    entries.into_values().collect()
}

fn json_name(path: &Path) -> Option<String> {
    (path.extension()? == "json").then_some(())?;
    let name = path.file_stem()?.to_string_lossy().into_owned();
    check_name(&name).is_ok().then_some(name)
}

fn files(dir: &Path) -> Vec<PathBuf> {
    entries(dir, Path::is_file)
}

fn folders(dir: &Path) -> Vec<PathBuf> {
    entries(dir, Path::is_dir)
}

fn entries(dir: &Path, keep: fn(&Path) -> bool) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| keep(path))
        .collect()
}

/// A new, empty common theme.
pub fn create(common: &Path, name: &str) -> Result<PathBuf, String> {
    check_name(name)?;
    let path = common_path(common, name);
    if path.exists() {
        return Err(format!("a theme named \"{name}\" already exists"));
    }
    fs::create_dir_all(common).map_err(|e| format!("cannot create {}: {e}", common.display()))?;
    fs::write(&path, "{\n  \"version\": 1\n}\n").map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// Renames a common theme file. Assets it names stay where they are, so their paths keep resolving.
pub fn rename(common: &Path, from: &str, to: &str) -> Result<PathBuf, String> {
    check_name(to)?;
    let source = common_path(common, from);
    let target = common_path(common, to);
    if !source.is_file() {
        return Err(format!("no common theme named \"{from}\""));
    }
    if target.exists() {
        return Err(format!("a theme named \"{to}\" already exists"));
    }
    fs::rename(&source, &target).map_err(|e| format!("cannot rename {}: {e}", source.display()))?;
    Ok(target)
}

/// A copy of a common theme under another name. The copy sits beside the original, so paths in it keep resolving.
pub fn duplicate(common: &Path, from: &str, to: &str) -> Result<PathBuf, String> {
    check_name(to)?;
    let source = common_path(common, from);
    let target = common_path(common, to);
    if !source.is_file() {
        return Err(format!("no common theme named \"{from}\""));
    }
    if target.exists() {
        return Err(format!("a theme named \"{to}\" already exists"));
    }
    fs::copy(&source, &target).map_err(|e| format!("cannot copy {}: {e}", source.display()))?;
    Ok(target)
}

/// The first of `name-copy`, `name-copy-2`, and so on that no theme in the common folder has.
pub fn free_copy_name(common: &Path, name: &str) -> String {
    let mut candidate = format!("{name}-copy");
    let mut number = 1;
    while common_path(common, &candidate).exists() {
        number += 1;
        candidate = format!("{name}-copy-{number}");
    }
    candidate
}

/// Deletes a common theme file. Fonts and images next to it may be shared with other themes and stay.
pub fn delete(common: &Path, name: &str) -> Result<(), String> {
    let path = common_path(common, name);
    if !path.is_file() {
        return Err(format!("no common theme named \"{name}\""));
    }
    fs::remove_file(&path).map_err(|e| format!("cannot delete {}: {e}", path.display()))
}

/// What copying a theme into a project left behind.
#[derive(Debug, PartialEq, Eq)]
pub struct Forked {
    pub path: PathBuf,
    /// Referenced files that do not exist, as written in the theme. The theme still copies.
    pub missing: Vec<String>,
}

/// Copies a common theme and the fonts and images it names into `<project>/themes/<name>/`. Paths inside
/// the theme folder keep their place, so they still resolve. A path that leaves the folder, or is
/// absolute, is copied to `assets/` in the new folder and rewritten.
pub fn fork(project: &Path, common: &Path, theme: &str, name: &str) -> Result<Forked, String> {
    check_name(name)?;
    let source = common_path(common, theme);
    let text = fs::read_to_string(&source).map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", source.display()))?;
    let folder = project.join("themes").join(name);
    let path = folder.join(PROJECT_FILE);
    if path.exists() {
        return Err(format!("the project already has a theme named \"{name}\""));
    }

    let origin = source.parent().unwrap_or(common);
    let mut text = text;
    let mut copies = Vec::new();
    let mut missing = Vec::new();
    for (property, reference) in references(&value) {
        let from = origin.join(&reference);
        if !from.is_file() {
            missing.push(reference);
            continue;
        }
        if is_inside(&reference) {
            copies.push((from, reference));
            continue;
        }
        let relative = match copies.iter().find(|(copied, _)| *copied == from) {
            Some((_, relative)) => relative.clone(),
            None => {
                let relative = moved(&reference, &copies);
                copies.push((from, relative.clone()));
                relative
            }
        };
        let keys: Vec<&str> = property.iter().map(String::as_str).collect();
        text = patch::set(&text, &keys, &Value::String(relative))?;
    }
    for (from, relative) in &copies {
        let to = folder.join(relative);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::copy(from, &to).map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
    }
    fs::create_dir_all(&folder).map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(Forked { path, missing })
}

/// A relative path with no `..` or root, so it stays inside the theme folder.
fn is_inside(reference: &str) -> bool {
    Path::new(reference)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

/// `assets/<file name>`, numbered when another copy already took that name.
fn moved(reference: &str, copies: &[(PathBuf, String)]) -> String {
    let file = Path::new(reference)
        .file_name()
        .map_or_else(|| "file".to_owned(), |name| name.to_string_lossy().into_owned());
    let mut candidate = format!("assets/{file}");
    let mut number = 1;
    while copies.iter().any(|(_, relative)| *relative == candidate) {
        number += 1;
        candidate = format!("assets/{number}-{file}");
    }
    candidate
}

/// Every file a theme names under `fonts` and `images`, with the property path of the string holding it.
/// Remote and empty paths are the engine's to reject and are skipped here.
fn references(theme: &Value) -> Vec<(Vec<String>, String)> {
    let mut found = Vec::new();
    let mut add = |property: Vec<String>, value: &Value| {
        if let Some(path) = value.as_str().filter(|path| !path.is_empty() && !path.contains("://")) {
            found.push((property, path.to_owned()));
        }
    };
    if let Some(families) = theme.get("fonts").and_then(Value::as_object) {
        for (family, faces) in families {
            for (face, file) in faces.as_object().into_iter().flatten() {
                let property = vec!["fonts".to_owned(), family.clone(), face.clone()];
                match file.get("file") {
                    Some(inner) => add([property, vec!["file".to_owned()]].concat(), inner),
                    None => add(property, file),
                }
            }
        }
    }
    if let Some(images) = theme.get("images").and_then(Value::as_object) {
        for (name, file) in images {
            add(vec!["images".to_owned(), name.clone()], file);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::super::ThemeFile;
    use super::super::tests::Scratch;
    use super::*;

    const WITH_ASSETS: &str = "{\n  \"version\": 1,\n  \"fonts\": {\n    \"Mine\": { \"regular\": \"fonts/a.otf\", \"italic\": { \"file\": \"../shared/b.otf\", \"index\": 1 }, \"bold\": \"../shared/b.otf\" }\n  },\n  \"images\": { \"logo\": \"../shared/logo.svg\" }\n}\n";

    fn common_with_assets(scratch: &Scratch) -> PathBuf {
        let common = scratch.0.join("config/themes");
        scratch.write("config/themes/fonts/a.otf", "a");
        scratch.write("config/shared/b.otf", "b");
        scratch.write("config/shared/logo.svg", "<svg/>");
        scratch.write("config/themes/mine.json", WITH_ASSETS);
        common
    }

    #[test]
    fn a_project_theme_wins_over_a_common_one_of_the_same_name() {
        let scratch = Scratch::new();
        let common = scratch.0.join("common");
        let project = scratch.0.join("project");
        scratch.write("common/book.json", "{ \"version\": 1 }");
        scratch.write("common/other.json", "{ \"version\": 1 }");
        scratch.write("project/themes/book/theme.json", "{ \"version\": 1 }");

        let found = locate("book", Some(&project), Some(&common)).unwrap();
        assert_eq!(
            (found.scope, found.path),
            (Scope::Project, project.join("themes/book/theme.json"))
        );
        assert_eq!(
            locate("other", Some(&project), Some(&common)).unwrap().scope,
            Scope::Common
        );
        assert_eq!(locate("none", Some(&project), Some(&common)), None);

        let names: Vec<_> = list(Some(&project), Some(&common))
            .into_iter()
            .map(|entry| (entry.name, entry.scope))
            .collect();
        assert_eq!(
            names,
            [("book".to_owned(), Scope::Project), ("other".to_owned(), Scope::Common)]
        );
    }

    #[test]
    fn common_themes_are_created_renamed_and_deleted_by_valid_names_only() {
        let scratch = Scratch::new();
        let common = scratch.0.join("common");
        let path = create(&common, "first").unwrap();
        assert!(ThemeFile::open(&path).unwrap().problems().is_empty());
        assert!(create(&common, "first").is_err());
        assert!(create(&common, "../escape").is_err());
        assert!(rename(&common, "first", "second").unwrap().is_file());
        assert!(!path.exists());
        assert!(rename(&common, "first", "third").is_err());
        delete(&common, "second").unwrap();
        assert!(list(None, Some(&common)).is_empty());
    }

    #[test]
    fn a_duplicate_sits_beside_the_original_and_gets_a_free_name() {
        let scratch = Scratch::new();
        let common = scratch.0.join("common");
        scratch.write("common/a.json", "{ \"version\": 1 }");
        assert_eq!(free_copy_name(&common, "a"), "a-copy");
        let copy = duplicate(&common, "a", "a-copy").unwrap();
        assert_eq!(fs::read_to_string(copy).unwrap(), "{ \"version\": 1 }");
        assert_eq!(free_copy_name(&common, "a"), "a-copy-2");
        assert!(duplicate(&common, "a", "a-copy").is_err());
        assert!(duplicate(&common, "missing", "b").is_err());
    }

    #[test]
    fn forking_copies_the_assets_and_every_path_still_resolves() {
        let scratch = Scratch::new();
        let common = common_with_assets(&scratch);
        let project = scratch.0.join("project");
        let forked = fork(&project, &common, "mine", "mine-copy").unwrap();
        assert!(forked.missing.is_empty());
        let folder = project.join("themes/mine-copy");
        assert!(folder.join("fonts/a.otf").is_file());
        assert!(folder.join("assets/b.otf").is_file());
        assert!(folder.join("assets/logo.svg").is_file());

        let text = fs::read_to_string(&forked.path).unwrap();
        assert!(text.contains("\"regular\": \"fonts/a.otf\""));
        assert!(text.contains("\"italic\": { \"file\": \"assets/b.otf\", \"index\": 1 }"));
        assert!(text.contains("\"bold\": \"assets/b.otf\""));
        assert!(text.contains("\"logo\": \"assets/logo.svg\""));
        assert!(ThemeFile::open(&forked.path).unwrap().problems().is_empty());
        assert!(fork(&project, &common, "mine", "mine-copy").is_err());
    }

    #[test]
    fn forking_reports_a_missing_asset_and_still_copies_the_theme() {
        let scratch = Scratch::new();
        let common = scratch.0.join("common");
        scratch.write("common/gone.json", WITH_ASSETS);
        let forked = fork(&scratch.0.join("p"), &common, "gone", "gone").unwrap();
        assert_eq!(forked.missing.len(), 4);
        assert_eq!(fs::read_to_string(forked.path).unwrap(), WITH_ASSETS);
    }
}
