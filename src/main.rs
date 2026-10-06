//! Kyber command-line interface: reads inputs, presents diagnostics, and sets exit codes.

mod config;
mod diagnostic;
mod document;
mod layout;
mod markdown;
mod page;
mod pdf;
mod text;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde_json::{Map, Value};

use config::front_matter::{self, FrontMatter};
use config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use config::resolved::Config;
use config::source::Source;
use diagnostic::Diagnostic;
use text::Fonts;

/// Typeset Markdown documents as PDF.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate a document's configuration and theme without rendering.
    Check {
        #[command(flatten)]
        input: InputArgs,
        /// Print the resolved configuration as JSON.
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
            let (config, _) = load(&input)?;
            if print_config {
                let json = serde_json::to_string_pretty(&config).expect("configuration serializes");
                println!("{json}");
            } else {
                println!("{}: configuration is valid", input.input.display());
            }
            Ok(())
        }
        Command::Render { input, output } => {
            let (config, document) = load(&input)?;
            let output = match output {
                Some(path) => document.working_dir.join(path),
                None => document.path.with_extension("pdf"),
            };
            let fonts = Fonts::load(&config)?;
            let pages = render(&config, &fonts, &document)?;
            let pdf = pdf::write(&pages, &fonts, &config.metadata, config.document.lang)
                .map_err(|e| vec![Diagnostic::new(None, e)])?;
            std::fs::write(&output, pdf)
                .map_err(|e| vec![Diagnostic::new(None, format!("cannot write {}: {e}", output.display()))])?;
            let count = pages.len();
            println!(
                "{}: wrote {count} page{}",
                output.display(),
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

/// Parses the Markdown body and lays it out.
fn render(config: &Config, fonts: &Fonts, document: &DocumentFile) -> Result<Vec<page::Page>, Vec<Diagnostic>> {
    let split = front_matter::split(&document.text).ok().flatten();
    let body = split.map_or(document.text.as_str(), |split| split.body);
    let first_line = document.text[..document.text.len() - body.len()].matches('\n').count() as u64 + 1;
    let content = markdown::parse(body, first_line, &document.source)?;
    layout::layout(&content, config, fonts, &document.source)
}

/// Reads the document, theme, and overrides, then resolves and checks the configuration.
fn load(args: &InputArgs) -> Result<(Config, DocumentFile), Vec<Diagnostic>> {
    let working_dir = std::env::current_dir().map_err(|e| {
        vec![Diagnostic::new(
            None,
            format!("cannot determine the working directory: {e}"),
        )]
    })?;
    let document_path = working_dir.join(&args.input);
    let document_source = Source::Document(document_path.clone());
    let cli_source = Source::Cli {
        working_dir: working_dir.clone(),
    };

    let text = read(&document_path, &document_source)?;
    let document = parse_front_matter(&text, &document_source)?;
    let overrides = parse_overrides(&args.overrides, &cli_source)?;

    let theme_path = match (&args.theme, &document.theme) {
        (Some(path), _) => Some(working_dir.join(path)),
        (None, Some(path)) => document_path.parent().map(|dir| dir.join(path)),
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
        let value: Value =
            serde_saphyr::from_str(value).map_err(|e| error(format!("--set {key}: {}", yaml_message(&e))))?;
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
