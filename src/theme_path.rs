//! Where a named theme can be. Assets keep the selected file's origin.
//!
//! A theme name means a project's `themes/<name>/theme.json` first, then `<name>.json` in the common folder.
//! [`crate::pipeline`] does the lookup through the resolver. The CLI and the desktop app share it.

use std::path::{Path, PathBuf};

/// The file name of a theme inside a project's `themes/<name>/` folder.
pub const PROJECT_FILE: &str = "theme.json";

/// Which of the two places a named theme came from.
#[cfg(feature = "editor")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_letters_digits_hyphens_and_underscores() {
        for name in ["latex-1", "book_2", "A"] {
            assert!(is_name(name), "{name}");
        }
        for name in ["", "latex-1.json", "./latex-1", "themes/latex-1"] {
            assert!(!is_name(name), "{name}");
        }
    }
}
