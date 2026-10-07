//! Druck command-line interface: reads inputs, presents diagnostics, and sets exit codes.

mod bibliography;
mod citations;
mod config;
mod date;
mod diagnostic;
mod document;
mod image;
mod installed;
mod layout;
mod markdown;
mod page;
mod pdf;
mod statistics;
mod text;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bibliography::Bibliography;
use clap::{Args, Parser, Subcommand};
use serde_json::{Map, Value};

use config::front_matter::{self, FrontMatter};
use config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use config::resolved::{Config, FontFiles};
use config::source::{Origin, Resource, Source, normalize, show_relative_to, shown};
use diagnostic::Diagnostic;
use image::Image;
use text::Fonts;

/// Typeset Markdown documents as PDF.
#[derive(Parser)]
#[command(version = env!("DRUCK_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lay out a document and report every problem `render` would, without writing the PDF.
    Check {
        #[command(flatten)]
        input: InputArgs,
        /// Print the resolved configuration as JSON instead of laying out.
        #[arg(long)]
        print_config: bool,
    },
    /// Render a document to PDF.
    Render {
        #[command(flatten)]
        input: InputArgs,
        /// Output PDF path. Defaults to the document path with a `.pdf` extension.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Args)]
struct InputArgs {
    /// Markdown document.
    input: PathBuf,
    /// Theme JSON file, overriding the document's `theme` setting.
    #[arg(long)]
    theme: Option<PathBuf>,
    /// Override a document setting, as in `--set toc=true` or `--set margins.top=2cm`.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    overrides: Vec<String>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(diagnostics) => {
            for diagnostic in &diagnostics {
                eprintln!("{diagnostic}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Vec<Diagnostic>> {
    match cli.command {
        Command::Check { input, print_config } => {
            let (config, document) = load(&input)?;
            if print_config {
                let json = serde_json::to_string_pretty(&config).expect("configuration serializes");
                println!("{json}");
            } else {
                typeset(&config, &document)?;
                println!("{}: document is valid", shown(&document.path));
            }
            Ok(())
        }
        Command::Render { input, output } => {
            let (config, document) = load(&input)?;
            let output = match output {
                Some(path) => normalize(&document.working_dir.join(path)),
                None => document.path.with_extension("pdf"),
            };
            let (fonts, laid) = typeset(&config, &document)?;
            let pdf = pdf::write(&laid, &fonts, &config.metadata, config.document.lang)
                .map_err(|e| vec![Diagnostic::new(None, e)])?;
            let used: BTreeSet<_> = laid
                .pages
                .iter()
                .flat_map(|page| &page.items)
                .filter_map(|item| match item {
                    page::Item::Text { run, .. } | page::Item::TurnedText { run, .. } => Some(run.face),
                    _ => None,
                })
                .collect();
            for warning in used.into_iter().filter_map(|face| fonts.embedding_warning(face)) {
                eprintln!("warning: {warning}");
            }
            std::fs::write(&output, pdf)
                .map_err(|e| vec![Diagnostic::new(None, format!("cannot write {}: {e}", shown(&output)))])?;
            let count = laid.pages.len();
            println!(
                "{}: wrote {count} page{}",
                shown(&output),
                if count == 1 { "" } else { "s" }
            );
            Ok(())
        }
    }
}

/// The document file as read, with where it came from.
struct DocumentFile {
    path: PathBuf,
    working_dir: PathBuf,
    source: Source,
    text: String,
}

/// Everything layout needs besides the configuration and fonts.
struct Prepared {
    content: document::Document,
    cited: citations::Cited,
    images: Vec<Image>,
    theme_images: HashMap<Resource, Image>,
}

/// Parses the Markdown body, loads its images, the theme's, and the bibliography, and formats citations.
fn prepare(config: &Config, document: &DocumentFile) -> Result<Prepared, Vec<Diagnostic>> {
    let split = front_matter::split(&document.text).ok().flatten();
    let body = split.map_or(document.text.as_str(), |split| split.body);
    let first_line = document.text[..document.text.len() - body.len()].matches('\n').count() as u64 + 1;
    let content = markdown::parse(body, first_line, &document.source)?;
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
fn typeset(config: &Config, document: &DocumentFile) -> Result<(Fonts, page::Output), Vec<Diagnostic>> {
    let installed = find_installed(config)?;
    let fonts = Fonts::load(config, &installed)?;
    let prepared = prepare(config, document)?;
    let today = date::Date::today().map_err(|e| vec![Diagnostic::new(None, e)])?;
    let laid = layout::layout(
        &prepared.content,
        &prepared.cited,
        &prepared.images,
        &prepared.theme_images,
        config,
        &fonts,
        &document.source,
        today,
    )?;
    Ok((fonts, laid))
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

/// Reads and decodes the theme images that the title layout in use shows. Errors name the property.
fn load_theme_images(config: &Config) -> Result<HashMap<Resource, Image>, Vec<Diagnostic>> {
    let used = layout::title_images(config);
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

/// Reads the document, theme, and overrides, then resolves and checks the configuration.
fn load(args: &InputArgs) -> Result<(Config, DocumentFile), Vec<Diagnostic>> {
    let working_dir = std::env::current_dir().map_err(|e| {
        vec![Diagnostic::new(
            None,
            format!("cannot determine the working directory: {e}"),
        )]
    })?;
    show_relative_to(working_dir.clone());
    let document_path = normalize(&working_dir.join(&args.input));
    let document_source = Source::Document(document_path.clone());
    let cli_source = Source::Cli {
        working_dir: working_dir.clone(),
    };

    let text = read(&document_path, &document_source)?;
    let document = parse_front_matter(&text, &document_source)?;
    let overrides = parse_overrides(&args.overrides, &cli_source)?;

    let theme_path = match (&args.theme, &document.theme) {
        (Some(path), _) => Some(normalize(&working_dir.join(path))),
        (None, Some(path)) => document_path.parent().map(|dir| normalize(&dir.join(path))),
        (None, None) => None,
    };
    let theme = theme_path.map(|path| load_theme(&path)).transpose()?;

    let config = resolve(Inputs {
        theme,
        document: SettingsInput {
            source: document_source.clone(),
            settings: document,
        },
        overrides: SettingsInput {
            source: cli_source,
            settings: overrides,
        },
    })?;
    check_resources(&config)?;
    for warning in &config.warnings {
        eprintln!("{warning}");
    }
    let document = DocumentFile {
        path: document_path,
        working_dir,
        source: document_source,
        text,
    };
    Ok((config, document))
}

fn read(path: &Path, source: &Source) -> Result<String, Vec<Diagnostic>> {
    std::fs::read_to_string(path)
        .map_err(|e| vec![Diagnostic::new(Some(source.clone()), format!("cannot read file: {e}"))])
}

fn parse_front_matter(text: &str, source: &Source) -> Result<FrontMatter, Vec<Diagnostic>> {
    let split = front_matter::split(text).map_err(|e| vec![Diagnostic::new(Some(source.clone()), e).at(1, 1)])?;
    let Some(split) = split else {
        return Ok(FrontMatter::default());
    };
    serde_saphyr::from_str(split.yaml).map_err(|error| {
        let mut diagnostic = Diagnostic::new(Some(source.clone()), yaml_message(&error));
        if let Some(location) = error.location() {
            diagnostic = diagnostic.at(location.line() + split.first_line - 1, location.column());
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
