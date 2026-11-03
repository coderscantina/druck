//! Druck command-line interface: reads inputs, presents diagnostics, and sets exit codes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use druck::config::resolved::Config;
use druck::config::source::{normalize, shown};
use druck::config::values::{Length, Pt};
use druck::diagnostic::Diagnostic;
use druck::pipeline::{DocumentFile, Request, load, prepare, typeset, write_pdf};
use druck::{date, epub, page, pdf};

/// Typeset Markdown documents as PDF and EPUB.
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
    /// Render a document to PDF, or to EPUB for an output path ending in `.epub`.
    Render {
        #[command(flatten)]
        input: InputArgs,
        /// Output path: `.epub` writes an EPUB, anything else a PDF. Defaults to the document path with a
        /// `.pdf` extension.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Extend each page by a bleed for print, as in `--bleed 3mm`. The page size stays the trim size.
        #[arg(long, value_name = "LENGTH", value_parser = parse_bleed)]
        bleed: Option<Pt>,
        /// Draw crop marks at the corners of each page, outside the bleed.
        #[arg(long)]
        crop_marks: bool,
    },
}

#[derive(Args)]
struct InputArgs {
    /// Markdown document.
    input: PathBuf,
    /// Theme name or JSON file, overriding the document's `theme` setting.
    #[arg(long)]
    theme: Option<PathBuf>,
    /// Override a document setting, as in `--set toc=true` or `--set margins.top=2cm`.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    overrides: Vec<String>,
}

impl InputArgs {
    fn request(&self) -> Request<'_> {
        Request {
            input: &self.input,
            theme: self.theme.as_deref(),
            overrides: &self.overrides,
        }
    }
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
            let (config, document) = load(&input.request())?;
            if print_config {
                let json = serde_json::to_string_pretty(&config).expect("configuration serializes");
                println!("{json}");
            } else {
                let (fonts, laid) = typeset(&config, &document)?;
                // The PDF is written and dropped, so a standard the document must conform to is checked too.
                let print = pdf::Print {
                    bleed: Pt(0.0),
                    crop_marks: false,
                };
                write_pdf(&config, &document, &fonts, &laid, print)?;
                for warning in &laid.warnings {
                    eprintln!("{warning}");
                }
                println!("{}: document is valid", shown(&document.path));
            }
            Ok(())
        }
        Command::Render {
            input,
            output,
            bleed,
            crop_marks,
        } => {
            let (config, document) = load(&input.request())?;
            let output = match output {
                Some(path) => normalize(&document.working_dir.join(path)),
                None => document.path.with_extension("pdf"),
            };
            if output
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("epub"))
            {
                if bleed.is_some() || crop_marks {
                    let message = "--bleed and --crop-marks apply to PDF output only, not to EPUB";
                    return Err(vec![Diagnostic::new(None, message)]);
                }
                return render_epub(&config, &document, &output);
            }
            let (fonts, laid) = typeset(&config, &document)?;
            let print = pdf::Print {
                bleed: bleed.unwrap_or(Pt(0.0)),
                crop_marks,
            };
            let pdf = write_pdf(&config, &document, &fonts, &laid, print)?;
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
            match laid.warnings.len() {
                0 => {}
                1 => eprintln!("warning: 1 typesetting rule is broken, `druck check` names it"),
                count => eprintln!("warning: {count} typesetting rules are broken, `druck check` lists them"),
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

/// Reads a bleed as an absolute length, such as `3mm` or `0.125in`.
fn parse_bleed(input: &str) -> Result<Pt, String> {
    match Length::parse(input)? {
        Length::Pt(pt) => Ok(Pt(pt)),
        Length::Em(_) => Err(format!("bleed \"{input}\" needs an absolute unit (pt, mm, cm, or in)")),
    }
}

/// Writes the document as an EPUB. Nothing is laid out, so the fonts are only read to be embedded.
fn render_epub(config: &Config, document: &DocumentFile, output: &Path) -> Result<(), Vec<Diagnostic>> {
    let prepared = prepare(config, document)?;
    let today = date::Date::today().map_err(|e| vec![Diagnostic::new(None, e)])?;
    // `SOURCE_DATE_EPOCH` fixes the build day; without it an ISO `date` does, so the same input gives the
    // same bytes.
    let iso = config
        .metadata
        .date
        .as_deref()
        .and_then(|text| date::Date::iso(text.trim()));
    let modified = match std::env::var_os("SOURCE_DATE_EPOCH") {
        Some(_) => today,
        None => iso.unwrap_or(today),
    };
    let stem = document
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Untitled");
    let written = epub::write(&epub::Book {
        document: &prepared.content,
        cited: &prepared.cited,
        config,
        source: &document.source,
        images: &prepared.images,
        dir: document.path.parent().expect("an absolute file path has a parent"),
        today,
        modified,
        fallback_title: stem,
    })?;
    for warning in &written.warnings {
        eprintln!("warning: {warning}");
    }
    std::fs::write(output, written.bytes)
        .map_err(|e| vec![Diagnostic::new(None, format!("cannot write {}: {e}", shown(output)))])?;
    let count = written.documents;
    println!(
        "{}: wrote {count} section{}",
        shown(output),
        if count == 1 { "" } else { "s" }
    );
    Ok(())
}
