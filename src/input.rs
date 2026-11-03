//! What to typeset. Inputs are values: nothing here reads the machine unless a caller's resolver does.

use std::path::{Path, PathBuf};

use crate::config::front_matter::FrontMatter;
use crate::config::resolve::SettingsInput;
use crate::config::source::{InputSource, Origin};
use crate::diagnostic::Diagnostic;
use crate::pipeline;
use crate::renderer::PdfOptions;
use crate::resource::Resolver;
use crate::theme_path;

pub use crate::config::front_matter::Export;
pub use crate::date::Date;
pub use crate::project::{MANIFEST, Project};

/// A document, its theme, and setting overrides.
pub struct Input {
    pub text: DocumentText,
    pub theme: ThemeSelection,
    pub overrides: Overrides,
    /// The day of the run, for `{build-date}` and `{year}`. The caller reads the clock and `SOURCE_DATE_EPOCH`,
    /// see [`crate::resource::source_date`].
    pub today: Date,
}

pub enum DocumentText {
    /// One Markdown file with front matter. `origin` is an [`Origin::Document`], the folder its relative paths
    /// start from, and `name` its file name. `text` may be newer than the file.
    Single { text: String, origin: Origin, name: String },
    /// A project: joined chapters without front matter, settings from the manifest.
    Project(Box<Project>),
}

impl DocumentText {
    /// Reads the Markdown file at `path`, which is absolute.
    pub fn read(resolver: &dyn Resolver, path: &Path) -> Result<Self, Vec<Diagnostic>> {
        let origin = Origin::Document(path.parent().unwrap_or(path).to_owned());
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
        let name = name.unwrap_or_default();
        let source = InputSource::Document(path.to_owned());
        let text = pipeline::read_text(resolver, &origin, &name, &source)?;
        Ok(Self::Single { text, origin, name })
    }
}

/// Which theme a document uses.
pub enum ThemeSelection {
    /// Nothing asked for: the document's `theme` setting, relative to the document, looked up in `dirs`.
    Document { dirs: ThemeDirs },
    /// A theme by bare name: from `dirs.project`, then the exact file `name` in `base`, then `dirs.shared`.
    Name {
        name: String,
        base: PathBuf,
        dirs: ThemeDirs,
    },
    /// A theme file, absolute.
    File { path: PathBuf },
    /// A theme in memory, whose assets are relative to `origin`.
    Json { text: String, origin: Origin },
}

impl ThemeSelection {
    /// What `--theme` means: a bare name is looked up, anything else is a path from `base`.
    pub fn asked(selection: &Path, base: &Path, dirs: ThemeDirs) -> Self {
        match selection.to_str().filter(|name| theme_path::is_name(name)) {
            Some(name) => Self::Name {
                name: name.to_owned(),
                base: base.to_owned(),
                dirs,
            },
            None => Self::File {
                path: crate::resource::normalize(&base.join(selection)),
            },
        }
    }
}

/// Where a bare theme name is looked up. No environment or home directory is read: the caller says, as with
/// [`crate::resource::FileResolver::shared_themes`].
#[derive(Clone, Default)]
pub struct ThemeDirs {
    /// A project folder, for its `themes/<name>/theme.json`.
    pub project: Option<PathBuf>,
    /// The folder of themes shared by all projects, for its `<name>.json`.
    pub shared: Option<PathBuf>,
}

/// Settings over the document's own. Relative paths in them start from the working directory they were made for.
#[derive(Default)]
pub struct Overrides {
    settings: FrontMatter,
    working_dir: PathBuf,
}

impl Overrides {
    /// Parses `KEY=VALUE` pairs in `druck render --set` syntax. Values use YAML; dots nest keys.
    pub fn parse(pairs: &[String], working_dir: &Path) -> Result<Self, Vec<Diagnostic>> {
        let source = source(working_dir);
        Ok(Self {
            settings: pipeline::parse_overrides(pairs, &source)?,
            working_dir: working_dir.to_owned(),
        })
    }

    /// Takes settings from a JSON object.
    pub fn from_json(value: serde_json::Value, working_dir: &Path) -> Result<Self, Vec<Diagnostic>> {
        let source = source(working_dir);
        Ok(Self {
            settings: pipeline::overrides_from_value(value, &source)?,
            working_dir: working_dir.to_owned(),
        })
    }

    pub(crate) fn input(&self) -> SettingsInput {
        SettingsInput {
            source: source(&self.working_dir),
            settings: self.settings.clone(),
        }
    }
}

fn source(working_dir: &Path) -> InputSource {
    InputSource::Overrides {
        working_dir: working_dir.to_owned(),
    }
}

/// What a project's export decides besides the document.
pub struct ExportSettings {
    pub pdf: PdfOptions,
    /// Whether open placeholders fail the build, see `Configured::check_final`.
    pub is_final: bool,
    /// Where the export writes, absolute. `None` leaves the choice to the caller.
    pub output: Option<PathBuf>,
    /// Why the export's print settings do not suit an EPUB, if it has any.
    pub print_conflict: Option<Diagnostic>,
}
