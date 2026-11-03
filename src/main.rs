//! Druck command-line interface: reads inputs, presents diagnostics, and sets exit codes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};

use druck::config::resolved::Config;
use druck::config::source::{Source, normalize, show_relative_to, shown};
use druck::config::values::Pt;
use druck::diagnostic::Diagnostic;
use druck::pipeline::{DocumentFile, Request, current_dir, load, prepare, typeset, write_pdf};
use druck::project::{self, Project};
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
        /// `.pdf` extension, or for a project to the export's output or `build/`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Extend each page by a bleed for print, as in `--bleed 3mm`. The page size stays the trim size.
        #[arg(long, value_name = "LENGTH", value_parser = Pt::bleed)]
        bleed: Option<Pt>,
        /// Draw crop marks at the corners of each page, outside the bleed.
        #[arg(long)]
        crop_marks: bool,
    },
}

#[derive(Args)]
struct InputArgs {
    /// Markdown document, or a project folder with a `druck.yaml`.
    input: PathBuf,
    /// Named export of a project folder, from `exports` in its `druck.yaml`.
    #[arg(long, value_name = "NAME")]
    export: Option<String>,
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

/// A document ready to build, with what its export or file location decides.
struct Job {
    config: Config,
    document: DocumentFile,
    /// Where a render writes without `-o`.
    output: PathBuf,
    /// Whether a render creates the output folder, as project builds do.
    create_folder: bool,
    print: pdf::Print,
    /// Why the export's print settings do not suit an EPUB, if it has any.
    print_conflict: Option<Diagnostic>,
}

impl Job {
    fn open(input: &InputArgs) -> Result<Self, Vec<Diagnostic>> {
        if input.input.is_dir() {
            return Self::open_project(input);
        }
        if input.export.is_some() {
            usage_error("--export needs a project folder, not a single file");
        }
        let (config, document) = load(&input.request())?;
        Ok(Self {
            config,
            output: document.path.with_extension("pdf"),
            document,
            create_folder: false,
            print: pdf::Print {
                bleed: Pt(0.0),
                crop_marks: false,
            },
            print_conflict: None,
        })
    }

    fn open_project(input: &InputArgs) -> Result<Self, Vec<Diagnostic>> {
        let working_dir = current_dir()?;
        show_relative_to(working_dir.clone());
        let project = Project::open(&normalize(&working_dir.join(&input.input)))?;
        for warning in &project.warnings {
            eprintln!("{warning}");
        }
        let export = input.export.as_deref().map(|name| match project.exports.get(name) {
            Some(export) => (name, export.clone()),
            None if project.exports.is_empty() => {
                usage_error(&format!("no export named \"{name}\"; the project has none"))
            }
            None => {
                let known = project
                    .exports
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                usage_error(&format!("no export named \"{name}\"; the project has: {known}"))
            }
        });
        let output = project.output(export.as_ref().map(|(name, export)| (*name, &export.value)));
        let print_conflict = export.as_ref().and_then(|(name, export)| {
            let has_print = export.value.bleed.is_some() || export.value.crop_marks;
            has_print.then(|| {
                let message = format!("export \"{name}\": bleed and crop-marks apply to PDF output only, not to EPUB");
                let source = Source::Document(project.dir.join(project::MANIFEST));
                Diagnostic::new(Some(source), message).at(export.referenced.line(), export.referenced.column())
            })
        });
        let settings = export.as_ref().map(|(_, export)| &export.value);
        let print = pdf::Print {
            bleed: settings.and_then(|export| export.bleed).unwrap_or(Pt(0.0)),
            crop_marks: settings.is_some_and(|export| export.crop_marks),
        };
        let final_export = export
            .as_ref()
            .filter(|(_, export)| export.value.is_final)
            .map(|(name, _)| *name);
        let (config, document) = project.configure(&input.request(), settings, &working_dir)?;
        for warning in &config.warnings {
            eprintln!("{warning}");
        }
        if let Some(name) = final_export {
            project::check_final(&config, &document, name)?;
        }
        Ok(Self {
            config,
            document,
            output,
            create_folder: true,
            print,
            print_conflict,
        })
    }
}

/// Reports a wrong use of the command line and exits, as clap does for arguments it rejects itself.
fn usage_error(message: &str) -> ! {
    Cli::command().error(ErrorKind::InvalidValue, message).exit()
}

fn is_epub(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("epub"))
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
            let job = Job::open(&input)?;
            let Job { config, document, .. } = &job;
            if print_config {
                let json = serde_json::to_string_pretty(config).expect("configuration serializes");
                println!("{json}");
                return Ok(());
            }
            if is_epub(&job.output) {
                if let Some(conflict) = job.print_conflict.clone() {
                    return Err(vec![conflict]);
                }
                build_epub(config, document)?;
            } else {
                let (fonts, laid) = typeset(config, document)?;
                // The PDF is written and dropped, so a standard the document must conform to is checked too.
                write_pdf(config, document, &fonts, &laid, job.print)?;
                for warning in &laid.warnings {
                    eprintln!("{warning}");
                }
            }
            println!("{}: document is valid", shown(&document.path));
            Ok(())
        }
        Command::Render {
            input,
            output,
            bleed,
            crop_marks,
        } => {
            let job = Job::open(&input)?;
            let Job { config, document, .. } = &job;
            let create_folder = job.create_folder && output.is_none();
            let output = match output {
                Some(path) => normalize(&document.working_dir.join(path)),
                None => job.output.clone(),
            };
            if create_folder && let Some(folder) = output.parent() {
                std::fs::create_dir_all(folder)
                    .map_err(|e| vec![Diagnostic::new(None, format!("cannot create {}: {e}", shown(folder)))])?;
            }
            if is_epub(&output) {
                if bleed.is_some() || crop_marks {
                    let message = "--bleed and --crop-marks apply to PDF output only, not to EPUB";
                    return Err(vec![Diagnostic::new(None, message)]);
                }
                if let Some(conflict) = job.print_conflict.clone() {
                    return Err(vec![conflict]);
                }
                return render_epub(config, document, &output);
            }
            let (fonts, laid) = typeset(config, document)?;
            let print = pdf::Print {
                bleed: bleed.unwrap_or(job.print.bleed),
                crop_marks: crop_marks || job.print.crop_marks,
            };
            let pdf = write_pdf(config, document, &fonts, &laid, print)?;
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

/// Writes the document as an EPUB.
fn render_epub(config: &Config, document: &DocumentFile, output: &Path) -> Result<(), Vec<Diagnostic>> {
    let written = build_epub(config, document)?;
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

/// Builds the EPUB and prints its warnings. Nothing is laid out, so the fonts are only read to be embedded.
fn build_epub(config: &Config, document: &DocumentFile) -> Result<epub::Written, Vec<Diagnostic>> {
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
    let stem = document.name().unwrap_or("Untitled");
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
    Ok(written)
}
