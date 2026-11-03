//! Druck command-line interface: reads inputs, presents diagnostics, and sets exit codes.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};

use druck::diagnostic::{Diagnostic, show_relative_to, shown};
use druck::input::{Date, DocumentText, Input, MANIFEST, Overrides, Project, ThemeDirs, ThemeSelection};
use druck::output::Pt;
use druck::resource::{FileResolver, clock_date, normalize, source_date};
use druck::{Configured, EpubOptions, Failure, LayoutOptions, PdfOptions, Renderer};

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

/// A document ready to configure, with what its export or file location decides.
struct Job {
    input: Input,
    working_dir: PathBuf,
    /// The day `SOURCE_DATE_EPOCH` fixes, if it is set.
    source_date: Option<Date>,
    /// The document or manifest, for messages.
    path: PathBuf,
    /// Where a render writes without `-o`.
    output: PathBuf,
    /// Whether a render creates the output folder, as project builds do.
    create_folder: bool,
    print: PdfOptions,
    /// The export to check for open placeholders, if it is final.
    final_export: Option<String>,
    /// Why the export's print settings do not suit an EPUB, if it has any.
    print_conflict: Option<Diagnostic>,
}

impl Job {
    fn open(args: &InputArgs, resolver: &FileResolver) -> Result<Self, Failure> {
        if args.input.is_dir() {
            return Self::open_project(args, resolver);
        }
        if args.export.is_some() {
            usage_error("--export needs a project folder, not a single file");
        }
        let working_dir = current_dir()?;
        show_relative_to(working_dir.clone());
        let path = normalize(&working_dir.join(&args.input));
        let text = DocumentText::read(resolver, &path)?;
        let (input, source_date) = args.input(text, &working_dir, None)?;
        Ok(Self {
            input,
            working_dir,
            source_date,
            output: path.with_extension("pdf"),
            path,
            create_folder: false,
            print: PdfOptions::default(),
            final_export: None,
            print_conflict: None,
        })
    }

    fn open_project(args: &InputArgs, resolver: &FileResolver) -> Result<Self, Failure> {
        let working_dir = current_dir()?;
        show_relative_to(working_dir.clone());
        let dir = normalize(&working_dir.join(&args.input));
        let project = Project::open(resolver, &dir)?;
        for warning in project.warnings() {
            eprintln!("{warning}");
        }
        let name = args.export.as_deref();
        if let Some(name) = name {
            check_export(&project, name);
        }
        let (text, settings) = project.export(name)?;
        let output = settings.output.unwrap_or_else(|| {
            let folder = dir.file_name().and_then(|name| name.to_str());
            let file = name.or(folder).unwrap_or("document");
            dir.join("build").join(format!("{file}.pdf"))
        });
        let (input, source_date) = args.input(text, &working_dir, Some(dir.clone()))?;
        Ok(Self {
            input,
            working_dir,
            source_date,
            path: dir.join(MANIFEST),
            output,
            create_folder: true,
            print: settings.pdf,
            final_export: name.filter(|_| settings.is_final).map(str::to_owned),
            print_conflict: settings.print_conflict,
        })
    }

    fn epub_options(&self) -> EpubOptions {
        EpubOptions {
            modified: self.source_date,
        }
    }

    /// Resolves the configuration, printing its warnings, and checks a final export.
    fn configure<'r>(&'r self, renderer: &'r Renderer, resolver: &'r FileResolver) -> Result<Configured<'r>, Failure> {
        let configured = renderer.job(resolver, &self.input).configure()?;
        for warning in configured.warnings() {
            eprintln!("{warning}");
        }
        if let Some(name) = &self.final_export {
            configured.check_final(name)?;
        }
        Ok(configured)
    }
}

impl InputArgs {
    /// The input for `text`, with the day `SOURCE_DATE_EPOCH` fixes beside it, if it is set. `project` is the
    /// folder of a project document, whose own themes come first.
    fn input(
        &self,
        text: DocumentText,
        working_dir: &Path,
        project: Option<PathBuf>,
    ) -> Result<(Input, Option<Date>), Failure> {
        let dirs = ThemeDirs {
            project,
            shared: FileResolver::shared_themes(),
        };
        let theme = match &self.theme {
            Some(theme) => ThemeSelection::asked(theme, working_dir, dirs),
            None => ThemeSelection::Document { dirs },
        };
        let overrides = Overrides::parse(&self.overrides, working_dir)?;
        let source_date = source_date().map_err(|e| vec![Diagnostic::new(None, e)])?;
        let today = match source_date {
            Some(day) => day,
            None => clock_date().map_err(|e| vec![Diagnostic::new(None, e)])?,
        };
        let input = Input {
            text,
            theme,
            overrides,
            today,
        };
        Ok((input, source_date))
    }
}

/// Exits with a usage error unless the project has an export called `name`.
fn check_export(project: &Project, name: &str) {
    let known: Vec<&str> = project.exports().map(|(name, _)| name).collect();
    if known.is_empty() {
        usage_error(&format!("no export named \"{name}\"; the project has none"));
    }
    if !known.contains(&name) {
        usage_error(&format!(
            "no export named \"{name}\"; the project has: {}",
            known.join(", ")
        ));
    }
}

fn current_dir() -> Result<PathBuf, Vec<Diagnostic>> {
    std::env::current_dir().map_err(|e| {
        vec![Diagnostic::new(
            None,
            format!("cannot determine the working directory: {e}"),
        )]
    })
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
        Err(Failure::Cancelled) => {
            eprintln!("error: cancelled");
            ExitCode::FAILURE
        }
        Err(Failure::Errors(diagnostics)) => {
            for diagnostic in &diagnostics {
                eprintln!("{diagnostic}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Failure> {
    let renderer = Renderer::new();
    let resolver = FileResolver::new();
    match cli.command {
        Command::Check { input, print_config } => {
            let job = Job::open(&input, &resolver)?;
            let configured = job.configure(&renderer, &resolver)?;
            if print_config {
                println!("{}", configured.config_json());
                return Ok(());
            }
            if is_epub(&job.output) {
                if let Some(conflict) = job.print_conflict.clone() {
                    return Err(Failure::Errors(vec![conflict]));
                }
                print_warnings(&configured.epub(&job.epub_options())?.warnings);
            } else {
                let laid = configured.layout(&LayoutOptions::default())?;
                // The PDF is written and dropped, so a standard the document must conform to is checked too.
                laid.pdf(&job.print)?;
                print_warnings(laid.warnings());
            }
            println!("{}: document is valid", shown(&job.path));
            Ok(())
        }
        Command::Render {
            input,
            output,
            bleed,
            crop_marks,
        } => {
            let job = Job::open(&input, &resolver)?;
            let configured = job.configure(&renderer, &resolver)?;
            let create_folder = job.create_folder && output.is_none();
            let output = match output {
                Some(path) => normalize(&job.working_dir.join(path)),
                None => job.output.clone(),
            };
            if create_folder && let Some(folder) = output.parent() {
                std::fs::create_dir_all(folder)
                    .map_err(|e| vec![Diagnostic::new(None, format!("cannot create {}: {e}", shown(folder)))])?;
            }
            if is_epub(&output) {
                if bleed.is_some() || crop_marks {
                    let message = "--bleed and --crop-marks apply to PDF output only, not to EPUB";
                    return Err(Failure::Errors(vec![Diagnostic::new(None, message)]));
                }
                if let Some(conflict) = job.print_conflict.clone() {
                    return Err(Failure::Errors(vec![conflict]));
                }
                return render_epub(&configured, &job.epub_options(), &output);
            }
            let laid = configured.layout(&LayoutOptions::default())?;
            let print = PdfOptions {
                bleed: bleed.unwrap_or(job.print.bleed),
                crop_marks: crop_marks || job.print.crop_marks,
            };
            let pdf = laid.pdf(&print)?;
            print_warnings(&pdf.warnings);
            match laid.warnings().len() {
                0 => {}
                1 => eprintln!("warning: 1 typesetting rule is broken, `druck check` names it"),
                count => eprintln!("warning: {count} typesetting rules are broken, `druck check` lists them"),
            }
            write(&output, &pdf.bytes)?;
            let count = pdf.parts;
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
fn render_epub(configured: &Configured, options: &EpubOptions, output: &Path) -> Result<(), Failure> {
    let written = configured.epub(options)?;
    print_warnings(&written.warnings);
    write(output, &written.bytes)?;
    let count = written.parts;
    println!(
        "{}: wrote {count} section{}",
        shown(output),
        if count == 1 { "" } else { "s" }
    );
    Ok(())
}

fn print_warnings(warnings: &[Diagnostic]) {
    for warning in warnings {
        eprintln!("{warning}");
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Vec<Diagnostic>> {
    std::fs::write(path, bytes).map_err(|e| vec![Diagnostic::new(None, format!("cannot write {}: {e}", shown(path)))])
}
