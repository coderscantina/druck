//! Checks a theme value through the engine's own resolution, so the app accepts exactly what `druck render`
//! accepts.

use std::fmt;
use std::path::Path;

use druck::editor::installed_fonts;
use druck::editor::resolved_config::{FrontMatter, InputSource, Inputs, SettingsInput, ThemeInput, resolve};
use druck::resource::FileResolver;
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
        Ok(config) => {
            let missing = config
                .resources()
                .into_iter()
                .filter(|(_, resource)| resource.file().is_some_and(|file| !file.is_file()))
                .map(|(property, resource)| Problem {
                    property: Some(property),
                    message: format!("resource not found: {resource}"),
                });
            // A family the theme neither defines nor finds installed would stop every layout.
            let absent = match installed_fonts::lookup(&FileResolver::new(), &config.installed_fonts) {
                Ok(_) => Vec::new(),
                Err(diagnostics) => diagnostics
                    .into_iter()
                    .map(|diagnostic| Problem {
                        property: diagnostic.property,
                        message: diagnostic.message,
                    })
                    .collect(),
            };
            missing.chain(absent).collect()
        }
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

/// A short document with every kind of text a style sets, to try a theme on. The specimen is the long one.
const PROBE: &str = "---\ntitle: Probe\nsubtitle: Probe\nauthor: Probe\ndate: 2026-01-01\n---\n\n# One\n\nBody text with *emphasis*, **strong**, `code`, and a note.[^1]\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n\n> A quotation.\n\n> [!NOTE]\n> A callout.\n\n- one\n  - two\n    - three\n\n1. first\n   1. nested\n\n```rust\nfn main() {}\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n* * *\n\n[^1]: A footnote.\n";

/// Typesets a short probe document in a theme as the CLI would, which finds what resolving cannot: a weight the font
/// lacks, a font that fails to load, a glyph a bullet needs that the font does not have. The text is tried from a
/// file beside the theme, so paths in it mean what they will mean, and the file is removed again.
pub fn renders(path: &Path, text: &str) -> Result<(), Problem> {
    use druck::Failure;
    use druck::cancel::Cancel;

    use crate::preview::job::Typesetter;
    use crate::session::{Joined, Kind};

    let folder = path.parent().unwrap_or(path);
    let trial = folder.join(format!(".{}.druck-check.json", path.file_stem().unwrap_or_default().to_string_lossy()));
    if std::fs::write(&trial, text).is_err() {
        // Nothing can be tried where the theme cannot be written, and saving it will say so.
        return Ok(());
    }
    let outcome = Typesetter::new(
        Kind::Single {
            path: folder.join("specimen.md"),
        },
        Some(trial.clone()),
    )
    .run(Joined::single(PROBE.to_owned(), 0), &Cancel::default());
    std::fs::remove_file(&trial).ok();
    match outcome {
        Ok(_) => Ok(()),
        Err(Failure::Errors(errors)) => Err(errors
            .into_iter()
            .find(|diagnostic| !diagnostic.is_warning())
            .map_or_else(
                || Problem {
                    property: None,
                    message: "the theme does not typeset".to_owned(),
                },
                |diagnostic| Problem {
                    property: diagnostic.property,
                    message: diagnostic.message,
                },
            )),
        Err(Failure::Cancelled) => Ok(()),
    }
}
