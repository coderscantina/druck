//! From a document path to pages and PDF bytes: the steps the CLI and the desktop app share.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::bibliography::Bibliography;
use crate::config::front_matter::{self, FrontMatter};
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use crate::config::resolved::{Config, FontFiles};
use crate::config::source::{Origin, Resource, Source, normalize, show_relative_to};
use crate::diagnostic::Diagnostic;
use crate::image::Image;
use crate::text::Fonts;
use crate::{citations, date, document, installed, layout, markdown, page, pdf, theme_path};

/// What to typeset: a document, with an optional theme and setting overrides.
pub struct Request<'a> {
    pub input: &'a Path,
    pub theme: Option<&'a Path>,
    pub overrides: &'a [String],
}

/// The document file as read, with where it came from.
pub struct DocumentFile {
    pub path: PathBuf,
    pub working_dir: PathBuf,
    pub source: Source,
    pub text: String,
}

impl DocumentFile {
    /// The Markdown of the document and the line it starts on. A project's joined chapters have no front
    /// matter, so a thematic break at the start of the first one stays a thematic break.
    fn body(&self) -> (&str, u64) {
        if matches!(self.source, Source::Chapters(_)) {
            return (&self.text, 1);
        }
        let split = front_matter::split(&self.text).ok().flatten();
        let body = split.map_or(self.text.as_str(), |split| split.body);
        let first_line = self.text[..self.text.len() - body.len()].matches('\n').count() as u64 + 1;
        (body, first_line)
    }

    /// What to call the document when it has no title: its file name, or a project's folder name.
    pub fn name(&self) -> Option<&str> {
        let name = match self.source {
            Source::Chapters(_) => self.path.parent()?.file_name()?,
            _ => self.path.file_stem()?,
        };
        name.to_str()
    }
}

/// Everything layout needs besides the configuration and fonts.
pub struct Prepared {
    pub content: document::Document,
    pub cited: citations::Cited,
    pub images: Vec<Image>,
    pub theme_images: HashMap<Resource, Image>,
}

/// Parses the Markdown body, loads its images, the theme's, and the bibliography, and formats citations.
pub fn prepare(config: &Config, document: &DocumentFile) -> Result<Prepared, Vec<Diagnostic>> {
    let (body, first_line) = document.body();
    let content = markdown::parse_with(
        body,
        first_line,
        &document.source,
        config.document.lang,
        config.typesetting.no_break_spaces,
    )?;
    let bibliography = load_bibliography(config)?;
    let cited = citations::resolve(&content, bibliography.as_ref(), config, &document.source)?;
    let dir = document.path.parent().expect("an absolute file path has a parent");
    let images = load_images(&content.images, dir, &document.source)?;
    let theme_images = load_theme_images(config)?;
    Ok(Prepared {
        content,
        cited,
        images,
        theme_images,
    })
}

/// Loads the fonts and lays out the document, reporting everything `render` reports before writing.
pub fn typeset(config: &Config, document: &DocumentFile) -> Result<(Fonts, page::Output), Vec<Diagnostic>> {
    let fonts = load_fonts(config)?;
    let laid = typeset_with(config, document, &fonts)?;
    Ok((fonts, laid))
}

/// Loads the faces the configuration names, from their files and among installed fonts.
pub fn load_fonts(config: &Config) -> Result<Fonts, Vec<Diagnostic>> {
    let installed = find_installed(config)?;
    Fonts::load(config, &installed)
}

/// Lays out the document with fonts already loaded, so a long-running process can reuse them.
pub fn typeset_with(config: &Config, document: &DocumentFile, fonts: &Fonts) -> Result<page::Output, Vec<Diagnostic>> {
    let prepared = prepare(config, document)?;
    let today = date::Date::today().map_err(|e| vec![Diagnostic::new(None, e)])?;
    layout::layout(
        &prepared.content,
        &prepared.cited,
        &prepared.images,
        &prepared.theme_images,
        config,
        fonts,
        &document.source,
        today,
    )
}

/// Reads and checks the BibTeX file the configuration names, if any. `load` has checked that it exists.
fn load_bibliography(config: &Config) -> Result<Option<Bibliography>, Vec<Diagnostic>> {
    let Some(resource) = &config.bibliography_file else {
        return Ok(None);
    };
    let path = resource.file().expect("a bibliography is never bundled");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        let message = format!("cannot read bibliography {resource}: {e}");
        vec![Diagnostic::new(None, message).property(Some("bibliography".to_owned()))]
    })?;
    Bibliography::parse(&text, &path).map(Some)
}

/// Reads and decodes the theme images that layout may show. Errors name the property.
fn load_theme_images(config: &Config) -> Result<HashMap<Resource, Image>, Vec<Diagnostic>> {
    let used = layout::theme_images(config);
    let mut images = HashMap::new();
    let mut errors = Vec::new();
    for (name, resource) in config.images.iter().filter(|(_, resource)| used.contains(resource)) {
        let loaded = match resource.file() {
            None => Err(format!("{resource} is not bundled")),
            Some(path) => {
                let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_owned();
                std::fs::read(&path)
                    .map_err(|e| format!("cannot read image {resource}: {e}"))
                    .and_then(|data| Image::decode(data, &extension).map_err(|e| format!("image {resource}: {e}")))
            }
        };
        match loaded {
            Ok(image) => {
                images.insert(resource.clone(), image);
            }
            Err(message) => errors.push(Diagnostic::new(None, message).property(Some(format!("images.{name}")))),
        }
    }
    if errors.is_empty() { Ok(images) } else { Err(errors) }
}

/// Reads and decodes each image file once, relative to the document. Errors name the first reference.
fn load_images(files: &[document::ImageFile], dir: &Path, source: &Source) -> Result<Vec<Image>, Vec<Diagnostic>> {
    let mut images = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for file in files {
        let path = dir.join(&file.path);
        let resource = Resource {
            origin: Origin::Document(dir.to_owned()),
            path: file.path.clone(),
        };
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_owned();
        let loaded = match std::fs::read(&path) {
            Ok(data) => Image::decode(data, &extension).map_err(|e| format!("image {resource}: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!("image not found: {resource}")),
            Err(e) => Err(format!("cannot read image {resource}: {e}")),
        };
        match loaded {
            Ok(image) => images.push(image),
            Err(message) => {
                errors.push(Diagnostic::new(Some(source.clone()), message).at(file.at.line, file.at.column))
            }
        }
    }
    if errors.is_empty() { Ok(images) } else { Err(errors) }
}

/// Reads the document, theme, and overrides, then resolves and checks the configuration. For the CLI:
/// paths in messages become relative to the working directory, and configuration warnings go to stderr.
pub fn load(args: &Request) -> Result<(Config, DocumentFile), Vec<Diagnostic>> {
    let working_dir = current_dir()?;
    show_relative_to(working_dir.clone());
    let path = normalize(&working_dir.join(args.input));
    let text = read(&path, &Source::Document(path.clone()))?;
    let (config, document) = configure(args, &working_dir, text)?;
    for warning in &config.warnings {
        eprintln!("{warning}");
    }
    Ok((config, document))
}

/// Resolves and checks the configuration for a document's `text`, which may be newer than its file.
/// Has no effect on the process, so a long-running one can call it repeatedly from any thread.
pub fn configure(args: &Request, working_dir: &Path, text: String) -> Result<(Config, DocumentFile), Vec<Diagnostic>> {
    let document_path = normalize(&working_dir.join(args.input));
    let document_source = Source::Document(document_path.clone());
    let settings = parse_front_matter(&text, &document_source)?;
    let config = resolve_settings(args, settings, &document_source, None, working_dir)?;
    let document = DocumentFile {
        path: document_path,
        working_dir: working_dir.to_owned(),
        source: document_source,
        text,
    };
    Ok((config, document))
}

/// Resolves and checks the configuration from settings read from `source`, which paths in them are relative
/// to. A document of a project folder gets that folder as `project`, for the themes in it.
pub(crate) fn resolve_settings(
    args: &Request,
    settings: FrontMatter,
    source: &Source,
    project: Option<&Path>,
    working_dir: &Path,
) -> Result<Config, Vec<Diagnostic>> {
    let cli_source = Source::Cli {
        working_dir: working_dir.to_owned(),
    };
    let overrides = parse_overrides(args.overrides, &cli_source)?;

    let theme = selected_theme(args.theme, &settings, source, project, working_dir)
        .map(|path| load_theme(&path))
        .transpose()?;

    let config = resolve(Inputs {
        theme,
        document: SettingsInput {
            source: source.clone(),
            settings,
        },
        overrides: SettingsInput {
            source: cli_source,
            settings: overrides,
        },
    })?;
    check_resources(&config)?;
    Ok(config)
}

/// The theme file a document uses: the one asked for, else the one its settings name.
fn selected_theme(
    asked: Option<&Path>,
    settings: &FrontMatter,
    source: &Source,
    project: Option<&Path>,
    working_dir: &Path,
) -> Option<PathBuf> {
    match (asked, &settings.theme) {
        (Some(path), _) => Some(theme_path::resolve_in(path, working_dir, project)),
        (None, Some(path)) => match source.origin() {
            Origin::Document(dir) => Some(theme_path::resolve_in(Path::new(path), &dir, project)),
            _ => None,
        },
        (None, None) => None,
    }
}

/// The theme file that the front matter of a document's `text` names, if it names one and parses.
pub fn document_theme(path: &Path, text: &str) -> Option<PathBuf> {
    let source = Source::Document(path.to_owned());
    let settings = parse_front_matter(text, &source).ok()?;
    selected_theme(None, &settings, &source, None, path.parent()?)
}

pub fn current_dir() -> Result<PathBuf, Vec<Diagnostic>> {
    std::env::current_dir().map_err(|e| {
        vec![Diagnostic::new(
            None,
            format!("cannot determine the working directory: {e}"),
        )]
    })
}

pub(crate) fn read(path: &Path, source: &Source) -> Result<String, Vec<Diagnostic>> {
    std::fs::read_to_string(path)
        .map_err(|e| vec![Diagnostic::new(Some(source.clone()), format!("cannot read file: {e}"))])
}

fn parse_front_matter(text: &str, source: &Source) -> Result<FrontMatter, Vec<Diagnostic>> {
    let split = front_matter::split(text).map_err(|e| vec![Diagnostic::new(Some(source.clone()), e).at(1, 1)])?;
    let Some(split) = split else {
        return Ok(FrontMatter::default());
    };
    let settings = parse_settings(split.yaml, split.first_line, source)?;
    let project_field = settings.chapters.is_some() || settings.exports.is_some();
    if project_field {
        return Err(vec![
            Diagnostic::new(Some(source.clone()), PROJECT_FIELDS).at(split.first_line, 1),
        ]);
    }
    Ok(settings)
}

const PROJECT_FIELDS: &str = "chapters and exports belong in a project's druck.yaml";

/// Reads settings from YAML that starts at 1-based line `first_line` of `source`.
pub(crate) fn parse_settings(yaml: &str, first_line: u64, source: &Source) -> Result<FrontMatter, Vec<Diagnostic>> {
    serde_saphyr::from_str(yaml).map_err(|error| {
        let mut diagnostic = Diagnostic::new(Some(source.clone()), yaml_message(&error));
        if let Some(location) = error.location() {
            diagnostic = diagnostic.at(location.line() + first_line - 1, location.column());
        }
        vec![diagnostic]
    })
}

/// A YAML error message without the snippet and location, which the caller reports itself.
fn yaml_message(error: &serde_saphyr::Error) -> String {
    let message = error.without_snippet().to_string();
    let message = message.strip_prefix("error: ").unwrap_or(&message);
    let message = match message.split_once(": ") {
        Some((location, rest)) if location.starts_with("line ") => rest,
        _ => message,
    };
    let message = message.lines().next().unwrap_or_default();
    match message.rsplit_once(" at line ") {
        Some((message, _)) => message.to_owned(),
        None => message.to_owned(),
    }
}

/// Builds settings from `--set key=value` pairs. Values use YAML syntax; dots nest keys.
fn parse_overrides(pairs: &[String], source: &Source) -> Result<FrontMatter, Vec<Diagnostic>> {
    let error = |message: String| vec![Diagnostic::new(Some(source.clone()), message)];
    let mut settings = Map::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(error(format!("--set {pair}: expected KEY=VALUE")));
        };
        if key == "theme" {
            return Err(error("--set theme: use --theme to select a theme".into()));
        }
        let mut parsed: Value =
            serde_saphyr::from_str(value).map_err(|e| error(format!("--set {key}: {}", yaml_message(&e))))?;
        if front_matter::TEXT_SETTINGS.contains(&key) && matches!(parsed, Value::Number(_) | Value::Bool(_)) {
            parsed = Value::String(value.trim().to_owned());
        }
        let value = parsed;
        let mut target = &mut settings;
        let mut keys = key.split('.').peekable();
        while let Some(part) = keys.next() {
            if keys.peek().is_none() {
                target.insert(part.to_owned(), value.clone());
                break;
            }
            let entry = target.entry(part).or_insert_with(|| Value::Object(Map::new()));
            target = match entry {
                Value::Object(map) => map,
                _ => return Err(error(format!("--set {key}: \"{part}\" is already set to a value"))),
            };
        }
    }
    if settings.contains_key("chapters") || settings.contains_key("exports") {
        return Err(error(format!("--set: {PROJECT_FIELDS}")));
    }
    serde_path_to_error::deserialize(Value::Object(settings)).map_err(|e| {
        let path = e.path().to_string();
        vec![Diagnostic::new(Some(source.clone()), e.into_inner().to_string()).property((path != ".").then_some(path))]
    })
}

fn load_theme(path: &Path) -> Result<ThemeInput, Vec<Diagnostic>> {
    let source = Source::Theme(path.to_owned());
    let text = read(path, &source)?;
    let value = serde_json::from_str(&text).map_err(|e| {
        let message = e.to_string();
        let message = message.split(" at line ").next().unwrap_or(&message).to_owned();
        vec![Diagnostic::new(Some(source.clone()), message).at(e.line() as u64, e.column() as u64)]
    })?;
    Ok(ThemeInput { source, value })
}

/// Looks up the font families the configuration does not define among installed fonts. The installed
/// fonts are scanned only when a style names such a family.
fn find_installed(config: &Config) -> Result<BTreeMap<String, FontFiles>, Vec<Diagnostic>> {
    if config.installed_fonts.is_empty() {
        return Ok(BTreeMap::new());
    }
    installed::lookup(&installed::system(), &config.installed_fonts)
}

/// Checks that every file-based resource exists. Bundled resources ship with the binary.
fn check_resources(config: &Config) -> Result<(), Vec<Diagnostic>> {
    let missing: Vec<_> = config
        .resources()
        .into_iter()
        .filter_map(|(property, resource)| {
            let file = resource.file()?;
            (!file.is_file())
                .then(|| Diagnostic::new(None, format!("resource not found: {resource}")).property(Some(property)))
        })
        .collect();
    if missing.is_empty() { Ok(()) } else { Err(missing) }
}

/// The laid out document as a PDF, checked against the standards it must conform to.
pub fn write_pdf(
    config: &Config,
    document: &DocumentFile,
    fonts: &Fonts,
    laid: &page::Output,
    print: pdf::Print,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let settings = pdf::Settings {
        lang: config.document.lang,
        print,
        conformance: &config.document.conformance,
        today: date::Date::today().map_err(|e| vec![Diagnostic::new(None, e)])?,
        source: &document.source,
    };
    pdf::write(laid, fonts, &config.metadata, &settings)
}
