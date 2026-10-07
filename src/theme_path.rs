//! Local theme selection at the CLI boundary. Assets keep the selected file's origin.

use std::path::{Path, PathBuf};

use crate::config::source::normalize;

pub fn resolve(selection: &Path, base: &Path) -> PathBuf {
    select(selection, base, directory().as_deref())
}

fn select(selection: &Path, base: &Path, directory: Option<&Path>) -> PathBuf {
    let local = normalize(&base.join(selection));
    let is_name = selection.to_str().is_some_and(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    });
    if is_name
        && !local.is_file()
        && let Some(directory) = directory
    {
        return directory.join(selection).with_extension("json");
    }
    local
}

fn directory() -> Option<PathBuf> {
    config_root(
        cfg!(windows),
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
    )
    .map(|root| root.join("druck/themes"))
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
    fn only_bare_names_use_the_shared_directory() {
        let root = std::env::temp_dir();
        let base = root.join("druck-theme-documents");
        let shared = root.join("druck-theme-config");
        assert_eq!(
            select(Path::new("latex-1"), &base, Some(&shared)),
            shared.join("latex-1.json")
        );
        for path in ["latex-1.json", "./latex-1", "../latex-1", "themes/latex-1"] {
            assert_eq!(
                select(Path::new(path), &base, Some(&shared)),
                normalize(&base.join(path))
            );
        }
        assert_eq!(
            select(&root.join("absolute.json"), &base, Some(&shared)),
            root.join("absolute.json")
        );
    }
}
