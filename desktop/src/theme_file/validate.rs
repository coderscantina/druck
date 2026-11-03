//! Checks a theme value through the engine's own resolution, so the app accepts exactly what `druck render`
//! accepts.

use std::fmt;
use std::path::Path;

use druck::editor::resolved_config::{FrontMatter, InputSource, Inputs, SettingsInput, ThemeInput, resolve};
use serde_json::Value;

/// One reason a theme is not valid, with the dotted property it concerns when the engine names one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub property: Option<String>,
    pub message: String,
}

impl Problem {
    pub fn internal(message: String) -> Vec<Self> {
        vec![Self {
            property: None,
            message,
        }]
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.property {
            Some(property) => write!(f, "{property}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// Resolves the theme over the bundled default, as a render would, and checks the files it names.
pub fn check(path: &Path, theme: &Value) -> Vec<Problem> {
    let inputs = Inputs {
        theme: Some(ThemeInput {
            source: InputSource::Theme(path.to_owned()),
            value: theme.clone(),
        }),
        document: SettingsInput {
            source: InputSource::Document(path.to_owned()),
            settings: FrontMatter::default(),
        },
        overrides: SettingsInput {
            source: InputSource::Overrides {
                working_dir: path.parent().unwrap_or(path).to_owned(),
            },
            settings: FrontMatter::default(),
        },
    };
    match resolve(inputs) {
        Ok(config) => config
            .resources()
            .into_iter()
            .filter(|(_, resource)| resource.file().is_some_and(|file| !file.is_file()))
            .map(|(property, resource)| Problem {
                property: Some(property),
                message: format!("resource not found: {resource}"),
            })
            .collect(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .filter(|diagnostic| !diagnostic.is_warning())
            .map(|diagnostic| Problem {
                property: diagnostic.property,
                message: diagnostic.message,
            })
            .collect(),
    }
}

/// Problems the new theme has and the old one did not.
pub fn introduced(path: &Path, old: &Value, new: &Value) -> Vec<Problem> {
    let before = check(path, old);
    check(path, new).into_iter().filter(|p| !before.contains(p)).collect()
}
