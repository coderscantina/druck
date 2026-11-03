//! Local theme selection at the CLI boundary. Assets keep the selected file's origin.
//!
//! A theme name means a project's `themes/<name>/theme.json` first, then `<name>.json` in the common folder
//! of the user's config. The CLI and the desktop app share this lookup.

use std::path::{Path, PathBuf};

use crate::config::source::normalize;

/// The file name of a theme inside a project's `themes/<name>/` folder.
pub const PROJECT_FILE: &str = "theme.json";

/// Which of the two places a named theme came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Project,
    Common,
}

pub fn project_file(project: &Path, name: &str) -> PathBuf {
    project.join("themes").join(name).join(PROJECT_FILE)
}

pub fn common_file(common: &Path, name: &str) -> PathBuf {
    common.join(name).with_extension("json")
}

/// Whether `name` can name a theme: ASCII letters, digits, hyphens, and underscores.
pub fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// The existing theme file of a name. A project theme wins over a common one of the same name.
pub fn locate(name: &str, project: Option<&Path>, common: Option<&Path>) -> Option<(Scope, PathBuf)> {
    let project = project.map(|project| (Scope::Project, project_file(project, name)));
    let common = common.map(|common| (Scope::Common, common_file(common, name)));
    project.into_iter().chain(common).find(|(_, path)| path.is_file())
}

pub fn resolve(selection: &Path, base: &Path) -> PathBuf {
    resolve_in(selection, base, None)
}

/// Like [`resolve`] for a document of the project folder `project`, whose own themes come first.
pub fn resolve_in(selection: &Path, base: &Path, project: Option<&Path>) -> PathBuf {
    select(selection, base, project, directory().as_deref())
}

fn select(selection: &Path, base: &Path, project: Option<&Path>, directory: Option<&Path>) -> PathBuf {
    let local = normalize(&base.join(selection));
    let Some(name) = selection.to_str().filter(|name| is_name(name)) else {
        return local;
    };
    if let Some((_, path)) = locate(name, project, None) {
        return path;
    }
    match directory {
        Some(directory) if !local.is_file() => common_file(directory, name),
        _ => local,
    }
}

/// The folder of named themes shared by all projects, if the platform has a config folder.
pub fn directory() -> Option<PathBuf> {
    config_root(
        cfg!(windows),
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
    )
    .map(|root| root.join("druck").join("themes"))
}

fn config_root(
    windows: bool,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
    appdata: Option<PathBuf>,
) -> Option<PathBuf> {
    if windows {
        appdata.filter(|path| path.is_absolute())
    } else {
        xdg.filter(|path| path.is_absolute())
            .or_else(|| home.filter(|path| path.is_absolute()).map(|path| path.join(".config")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_absolute_os_config_roots_and_ignores_relative_xdg_values() {
        let root = std::env::temp_dir();
        let home = root.join("home");
        let xdg = root.join("xdg");
        let appdata = root.join("appdata");
        assert_eq!(
            config_root(false, Some(xdg.clone()), Some(home.clone()), None),
            Some(xdg)
        );
        assert_eq!(
            config_root(false, Some("relative".into()), Some(home.clone()), None),
            Some(home.join(".config"))
        );
        assert_eq!(
            config_root(true, None, Some(home), Some(appdata.clone())),
            Some(appdata)
        );
        assert_eq!(config_root(false, None, None, None), None);
    }

    #[test]
    fn a_project_theme_wins_over_a_local_file_and_a_common_theme() {
        let root = std::env::temp_dir().join(format!("druck-theme-lookup-{}", std::process::id()));
        let project = root.join("project");
        let common = root.join("common");
        std::fs::create_dir_all(project.join("themes/book")).unwrap();
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(project.join("themes/book").join(PROJECT_FILE), "{}").unwrap();
        std::fs::write(project.join("book"), "{}").unwrap();
        std::fs::write(common.join("book.json"), "{}").unwrap();
        std::fs::write(common.join("other.json"), "{}").unwrap();

        let found = locate("book", Some(&project), Some(&common)).unwrap();
        assert_eq!(found, (Scope::Project, project_file(&project, "book")));
        let found = locate("other", Some(&project), Some(&common)).unwrap();
        assert_eq!(found, (Scope::Common, common_file(&common, "other")));
        assert_eq!(
            select(Path::new("book"), &project, Some(&project), Some(&common)),
            project_file(&project, "book")
        );
        assert_eq!(
            select(Path::new("book"), &project, None, Some(&common)),
            project.join("book")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn only_bare_names_use_the_shared_directory() {
        let root = std::env::temp_dir();
        let base = root.join("druck-theme-documents");
        let shared = root.join("druck-theme-config");
        assert_eq!(
            select(Path::new("latex-1"), &base, None, Some(&shared)),
            shared.join("latex-1.json")
        );
        for path in ["latex-1.json", "./latex-1", "../latex-1", "themes/latex-1"] {
            assert_eq!(
                select(Path::new(path), &base, None, Some(&shared)),
                normalize(&base.join(path))
            );
        }
        assert_eq!(
            select(&root.join("absolute.json"), &base, None, Some(&shared)),
            root.join("absolute.json")
        );
    }
}
