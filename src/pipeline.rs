//! From a document to pages and PDF bytes: the steps behind [`crate::Renderer`]. Everything is read through the
//! job's resolver, and fonts and images are reused through the renderer's caches.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::bibliography::Bibliography;
use crate::cancel::{Cancel, CheckPoint};
use crate::config::front_matter::{self, FrontMatter};
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use crate::config::resolved::Config;
use crate::config::source::{InputSource, Origin, Resource, normalize};
use crate::date::Date;
use crate::diagnostic::Diagnostic;
use crate::image::Image;
use crate::input::{Input, ThemeDirs, ThemeSelection};
use crate::renderer::{Failure, Renderer};
use crate::resource::{Bytes, Files, ResolveError, Resolver};
use crate::text::FontSet;
use crate::{citations, document, installed, layout, markdown, output, theme_path};

/// What one job reads from and reuses.
pub struct JobContext<'r> {
    pub renderer: &'r Renderer,
    pub resolver: &'r dyn Resolver,
    pub cancel: &'r Cancel,
    /// This job's place among the renderer's jobs, which decides what its caches keep.
    pub number: u64,
    /// The resources the configuration names, as read for this job.
    pub files: Files,
}

/// The document file as read, with where it came from.
pub struct DocumentFile {
    pub path: PathBuf,
    pub source: InputSource,
    pub text: String,
}

impl DocumentFile {
    /// The Markdown of the document and the line it starts on. A project's joined chapters have no front
    /// matter, so a thematic break at the start of the first one stays a thematic break.
    fn body(&self) -> (&str, u64) {
        if matches!(self.source, InputSource::Chapters(_)) {
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
            InputSource::Chapters(_) => self.path.parent()?.file_name()?,
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

/// Parses the Markdown body into the document model.
pub fn parse(config: &Config, document: &DocumentFile) -> Result<document::Document, Vec<Diagnostic>> {
    let (body, first_line) = document.body();
    markdown::parse_with(
        body,
        first_line,
        &document.source,
        config.document.lang,
        config.typesetting.no_break_spaces,
    )
}

/// Like [`parse`], and records where blocks and text are written, for a source map.
fn parse_mapped(config: &Config, document: &DocumentFile) -> Result<document::Document, Vec<Diagnostic>> {
    let (body, first_line) = document.body();
    markdown::parse_mapped(
        body,
        first_line,
        &document.source,
        config.document.lang,
        config.typesetting.no_break_spaces,
        document.text.len() - body.len(),
    )
}

/// Parses the Markdown body, loads its images, the theme's, and the bibliography, and formats citations.
pub fn prepare(job: &JobContext, config: &Config, document: &DocumentFile) -> Result<Prepared, Failure> {
    prepare_parsed(job, config, document, parse(config, document)?)
}

fn prepare_parsed(
    job: &JobContext,
    config: &Config,
    document: &DocumentFile,
    content: document::Document,
) -> Result<Prepared, Failure> {
    let bibliography = load_bibliography(job, config)?;
    let cited = citations::resolve(&content, bibliography.as_ref(), config, &document.source, job.cancel)?;
    let dir = document.path.parent().expect("an absolute file path has a parent");
    let images = load_images(job, &content.images, dir, &document.source)?;
    let theme_images = load_theme_images(job, config)?;
    Ok(Prepared {
        content,
        cited,
        images,
        theme_images,
    })
}

/// Finds the fonts and lays out the document, reporting everything `render` reports before writing. With
/// `source_map`, the result connects its text to the written text.
pub fn typeset(
    job: &JobContext,
    config: &Config,
    document: &DocumentFile,
    today: Date,
    source_map: bool,
) -> Result<(FontSet, output::Output), Failure> {
    let set = load_fonts(job, config)?;
    let fonts = set.fonts();
    let content = if source_map {
        parse_mapped(config, document)?
    } else {
        parse(config, document)?
    };
    let prepared = prepare_parsed(job, config, document, content)?;
    let laid = layout::layout(
        &prepared.content,
        &prepared.cited,
        &prepared.images,
        &prepared.theme_images,
        config,
        &fonts,
        &document.source,
        &document.path,
        today,
        job.cancel,
    )?;
    Ok((set, laid))
}

/// The lines of running text as a layout with this configuration sets them, without making pages.
#[cfg(feature = "editor")]
pub fn set_lines(
    job: &JobContext,
    config: &Config,
    document: &DocumentFile,
    today: Date,
) -> Result<layout::SetLines, Failure> {
    let set = load_fonts(job, config)?;
    let fonts = set.fonts();
    let content = parse_mapped(config, document)?;
    let prepared = prepare_parsed(job, config, document, content)?;
    layout::set_lines(
        &prepared.content,
        &prepared.cited,
        &prepared.images,
        &prepared.theme_images,
        config,
        &fonts,
        &document.source,
        &document.path,
        today,
        job.cancel,
    )
}

/// The faces the configuration names, from their files and among installed fonts. The installed fonts are looked
/// at only when a style names such a family.
fn load_fonts(job: &JobContext, config: &Config) -> Result<FontSet, Failure> {
    let faces = &job.renderer.faces;
    if config.installed_fonts.is_empty() {
        return FontSet::load(config, &BTreeMap::new(), &job.files, faces, job.number, job.cancel);
    }
    let found = installed::lookup(job.resolver, &config.installed_fonts)?;
    let mut files = job.files.clone();
    files.extend(found.files);
    FontSet::load(config, &found.families, &files, faces, job.number, job.cancel)
}

/// Reads every resource the configuration names, so a missing file fails before layout and everything in the job
/// sees the same bytes. Errors name the resource, its origin, and the property.
pub fn read_resources(resolver: &dyn Resolver, config: &Config, cancel: &Cancel) -> Result<Files, Failure> {
    let mut files = Files::default();
    let mut errors = Vec::new();
    for (property, resource) in config.resources() {
        if resource.origin == Origin::Bundled || files.contains(resource) {
            continue;
        }
        cancel.check(CheckPoint::Resource)?;
        match resolver.read(&resource.origin, &resource.path) {
            Ok(data) => files.insert(resource.clone(), data),
            Err(e) => {
                let message = match e {
                    ResolveError::NotFound(_) => format!("resource not found: {resource}"),
                    e => format!("cannot read {resource}: {e}"),
                };
                errors.push(
                    Diagnostic::new(None, message)
                        .property(Some(property))
                        .resource(resource),
                );
            }
        }
    }
    if errors.is_empty() {
        Ok(files)
    } else {
        Err(errors.into())
    }
}

/// Parses the BibTeX file the configuration names, if any.
pub(crate) fn load_bibliography(job: &JobContext, config: &Config) -> Result<Option<Bibliography>, Failure> {
    let Some(resource) = &config.bibliography_file else {
        return Ok(None);
    };
    job.cancel.check(CheckPoint::Bibliography)?;
    let path = resource.file().expect("a bibliography is never bundled");
    let text = job.files.get(resource).and_then(|data| text(&data)).map_err(|e| {
        let message = format!("cannot read bibliography {resource}: {e}");
        let diagnostic = Diagnostic::new(None, message).property(Some("bibliography".to_owned()));
        vec![diagnostic.resource(resource)]
    })?;
    Ok(Bibliography::parse(&text, &path).map(Some)?)
}

/// Decodes the theme images that layout may show. Errors name the property.
fn load_theme_images(job: &JobContext, config: &Config) -> Result<HashMap<Resource, Image>, Failure> {
    let used = layout::theme_images(config);
    let mut images = HashMap::new();
    let mut errors = Vec::new();
    for (name, resource) in config.images.iter().filter(|(_, resource)| used.contains(resource)) {
        job.cancel.check(CheckPoint::Image)?;
        let loaded = match resource.file() {
            None => Err(format!("{resource} is not bundled")),
            Some(path) => {
                let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
                job.files.get(resource).and_then(|data| {
                    let decoded = job.renderer.images.image(&data, extension, job.number);
                    decoded.map_err(|e| format!("image {resource}: {e}"))
                })
            }
        };
        match loaded {
            Ok(image) => {
                images.insert(resource.clone(), image);
            }
            Err(message) => {
                let property = Some(format!("images.{name}"));
                errors.push(Diagnostic::new(None, message).property(property).resource(resource));
            }
        }
    }
    if errors.is_empty() {
        Ok(images)
    } else {
        Err(errors.into())
    }
}

/// Reads and decodes each image file once, relative to the document. Errors name the first reference.
fn load_images(
    job: &JobContext,
    files: &[document::ImageFile],
    dir: &Path,
    source: &InputSource,
) -> Result<Vec<Image>, Failure> {
    let mut images = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for file in files {
        job.cancel.check(CheckPoint::Image)?;
        let resource = Resource {
            origin: Origin::Document(dir.to_owned()),
            path: file.path.clone(),
        };
        let extension = (Path::new(&file.path).extension())
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let loaded = match job.resolver.read(&resource.origin, &resource.path) {
            Ok(data) => (job.renderer.images)
                .image(&data, extension, job.number)
                .map_err(|e| format!("image {resource}: {e}")),
            Err(ResolveError::NotFound(_)) => Err(format!("image not found: {resource}")),
            Err(e) => Err(format!("cannot read image {resource}: {e}")),
        };
        match loaded {
            Ok(image) => images.push(image),
            Err(message) => {
                let diagnostic = Diagnostic::new(Some(source.clone()), message).resource(&resource);
                errors.push(diagnostic.at(file.at.line, file.at.column));
            }
        }
    }
    if errors.is_empty() {
        Ok(images)
    } else {
        Err(errors.into())
    }
}

/// Resolves and checks the configuration for one Markdown document `name` in the folder `dir`.
pub fn configure(
    resolver: &dyn Resolver,
    input: &Input,
    dir: &Path,
    name: &str,
    text: &str,
) -> Result<(Config, DocumentFile), Vec<Diagnostic>> {
    let document_path = normalize(&dir.join(name));
    let document_source = InputSource::Document(document_path.clone());
    let settings = parse_front_matter(text, &document_source)?;
    let config = resolve_settings(resolver, input, settings, &document_source)?;
    let document = DocumentFile {
        path: document_path,
        source: document_source,
        text: text.to_owned(),
    };
    Ok((config, document))
}

/// Resolves and checks the configuration from settings read from `source`, which paths in them are relative to.
pub(crate) fn resolve_settings(
    resolver: &dyn Resolver,
    input: &Input,
    settings: FrontMatter,
    source: &InputSource,
) -> Result<Config, Vec<Diagnostic>> {
    let theme = selected_theme(resolver, &input.theme, &settings, source)?;
    resolve(Inputs {
        theme,
        document: SettingsInput {
            source: source.clone(),
            settings,
        },
        overrides: input.overrides.input(),
    })
}

/// The theme a document uses: the one asked for, else the one its settings name.
fn selected_theme(
    resolver: &dyn Resolver,
    selection: &ThemeSelection,
    settings: &FrontMatter,
    source: &InputSource,
) -> Result<Option<ThemeInput>, Vec<Diagnostic>> {
    let (path, read) = match selection {
        ThemeSelection::Document { dirs } => {
            let (Some(theme), Origin::Document(dir)) = (&settings.theme, source.origin()) else {
                return Ok(None);
            };
            find_theme(resolver, Path::new(theme), &dir, dirs)
        }
        ThemeSelection::Name { name, base, dirs } => find_theme(resolver, Path::new(name), base, dirs),
        ThemeSelection::File { path } => (path.clone(), read_theme_file(resolver, path)),
        ThemeSelection::Json { text, origin } => {
            let path = origin.file("theme.json").unwrap_or_default();
            return parse_theme(text, InputSource::Theme(path)).map(Some);
        }
    };
    let source = InputSource::Theme(path);
    let error = |message: String| {
        vec![Diagnostic::new(
            Some(source.clone()),
            format!("cannot read file: {message}"),
        )]
    };
    let data = read.map_err(|e| error(e.to_string()))?;
    let theme = self::text(&data).map_err(error)?;
    parse_theme(&theme, source).map(Some)
}

/// The theme file a selection means, with the result of reading it. A bare name means a theme in the project,
/// then an exact local file, then a theme in the shared folder. Anything else is a path from `base`.
fn find_theme(
    resolver: &dyn Resolver,
    selection: &Path,
    base: &Path,
    dirs: &ThemeDirs,
) -> (PathBuf, Result<Bytes, ResolveError>) {
    let local = normalize(&base.join(selection));
    let read = |path: PathBuf| {
        let data = read_theme_file(resolver, &path);
        (path, data)
    };
    let exists = |found: &(PathBuf, Result<Bytes, ResolveError>)| !matches!(found.1, Err(ResolveError::NotFound(_)));
    let Some(name) = selection.to_str().filter(|name| theme_path::is_name(name)) else {
        return read(local);
    };
    if let Some(project) = &dirs.project {
        let found = read(theme_path::project_file(project, name));
        if exists(&found) {
            return found;
        }
    }
    let Some(shared) = &dirs.shared else {
        return read(local);
    };
    let found = read(local);
    if exists(&found) {
        return found;
    }
    read(theme_path::common_file(shared, name))
}

/// Reads a theme file through its own origin, the folder its assets are relative to.
fn read_theme_file(resolver: &dyn Resolver, path: &Path) -> Result<Bytes, ResolveError> {
    match (path.parent(), path.file_name().and_then(|name| name.to_str())) {
        (Some(dir), Some(name)) => resolver.read(&Origin::Theme(dir.to_owned()), name),
        _ => Err(ResolveError::NotFound("not a file".to_owned())),
    }
}

/// The theme file that the front matter of a document's `text` names, if it names one and parses.
#[cfg(feature = "editor")]
pub fn document_theme(resolver: &dyn Resolver, path: &Path, text: &str, dirs: &ThemeDirs) -> Option<PathBuf> {
    let source = InputSource::Document(path.to_owned());
    let settings = parse_front_matter(text, &source).ok()?;
    let theme = settings.theme?;
    Some(find_theme(resolver, Path::new(&theme), path.parent()?, dirs).0)
}

/// The file a theme selection means, whether or not it can be read.
#[cfg(any(test, feature = "editor"))]
pub fn theme_file(resolver: &dyn Resolver, selection: &Path, base: &Path, dirs: &ThemeDirs) -> PathBuf {
    find_theme(resolver, selection, base, dirs).0
}

/// Text from the bytes of a file, in the words the standard library has for what is not text.
pub(crate) fn text(data: &[u8]) -> Result<String, String> {
    std::str::from_utf8(data)
        .map(str::to_owned)
        .map_err(|_| "stream did not contain valid UTF-8".to_owned())
}

/// The text of a file, with its reading errors at `source`.
pub(crate) fn read_text(
    resolver: &dyn Resolver,
    origin: &Origin,
    path: &str,
    source: &InputSource,
) -> Result<String, Vec<Diagnostic>> {
    let error = |message: String| {
        vec![Diagnostic::new(
            Some(source.clone()),
            format!("cannot read file: {message}"),
        )]
    };
    let data = resolver.read(origin, path).map_err(|e| error(e.to_string()))?;
    text(&data).map_err(error)
}

fn parse_front_matter(text: &str, source: &InputSource) -> Result<FrontMatter, Vec<Diagnostic>> {
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
pub fn parse_settings(yaml: &str, first_line: u64, source: &InputSource) -> Result<FrontMatter, Vec<Diagnostic>> {
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
pub(crate) fn parse_overrides(pairs: &[String], source: &InputSource) -> Result<FrontMatter, Vec<Diagnostic>> {
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
    overrides_from_value(Value::Object(settings), source)
}

/// Builds settings from a JSON object of settings.
pub(crate) fn overrides_from_value(settings: Value, source: &InputSource) -> Result<FrontMatter, Vec<Diagnostic>> {
    if settings.get("chapters").is_some() || settings.get("exports").is_some() {
        let message = format!("--set: {PROJECT_FIELDS}");
        return Err(vec![Diagnostic::new(Some(source.clone()), message)]);
    }
    serde_path_to_error::deserialize(settings).map_err(|e| {
        let path = e.path().to_string();
        vec![Diagnostic::new(Some(source.clone()), e.into_inner().to_string()).property((path != ".").then_some(path))]
    })
}

fn parse_theme(text: &str, source: InputSource) -> Result<ThemeInput, Vec<Diagnostic>> {
    let value = serde_json::from_str(text).map_err(|e| {
        let message = e.to_string();
        let message = message.split(" at line ").next().unwrap_or(&message).to_owned();
        vec![Diagnostic::new(Some(source.clone()), message).at(e.line() as u64, e.column() as u64)]
    })?;
    Ok(ThemeInput { source, value })
}
