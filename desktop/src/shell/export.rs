//! Running an export: the document through the public crate to a file, the way `druck render` does it. Holds no gpui
//! types, so it runs on any thread, reports its stage as it goes, and stops when its token is cancelled.

use std::fs;
use std::path::{Path, PathBuf};

use druck::cancel::Cancel;
use druck::diagnostic::Diagnostic;
use druck::input::{DocumentText, Input, MANIFEST, Overrides, Project, ThemeDirs, ThemeSelection};
use druck::resource::{FileResolver, Origin, clock_date, source_date};
use druck::{EpubOptions, Failure, LayoutOptions, Missing, PdfOptions, Renderer};

use super::text::Key;
use crate::session::{Joined, Kind, LiveResolver};

/// What an export makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A project's named export from `druck.yaml`.
    Named(String),
    /// A single file, or a project without exports, as one document.
    Pdf,
    Epub,
}

/// Where a run is. The stages are what the engine reports between its own, since a layout has no progress of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Preparing,
    Typesetting,
    Writing,
}

impl Stage {
    pub const COUNT: usize = 3;
}

/// What a finished run wrote.
#[derive(Debug, Clone)]
pub struct Written {
    pub output: PathBuf,
    /// Pages of a PDF, or sections of an EPUB.
    pub parts: usize,
}

/// Everything a run needs, taken from the session so the run can use another thread.
pub struct Request {
    pub kind: Kind,
    pub joined: Joined,
    pub target: Target,
}

fn is_epub(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("epub"))
}

/// What a project's output is called when no export names it: its folder's name.
fn document_name(dir: &Path) -> &str {
    dir.file_name().and_then(|folder| folder.to_str()).unwrap_or("document")
}

/// Where a project's PDF goes when no export names one, as `druck.yaml` would write it.
pub fn built_in_output(dir: &Path) -> String {
    format!("build/{}.pdf", document_name(dir))
}

fn failed(message: String) -> Failure {
    Failure::Errors(vec![Diagnostic::error(message)])
}

/// Writes the document. `stage` hears each stage as it starts. A cancelled run writes nothing.
pub fn run(request: Request, cancel: &Cancel, stage: &dyn Fn(Stage)) -> Result<Written, Failure> {
    stage(Stage::Preparing);
    let files = FileResolver::new();
    let resolver = LiveResolver::new(&request.joined, &files);
    let today = match source_date() {
        Ok(Some(day)) => day,
        Ok(None) => clock_date().map_err(failed)?,
        Err(message) => return Err(failed(message)),
    };
    let source_day = source_date().ok().flatten();
    let (text, dirs, output, print, final_export) = match &request.kind {
        Kind::Single { path } => {
            let working_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let text = DocumentText::Single {
                text: request.joined.text.clone(),
                origin: Origin::Document(working_dir),
                name,
            };
            let extension = if request.target == Target::Epub { "epub" } else { "pdf" };
            let dirs = ThemeDirs {
                project: None,
                shared: FileResolver::shared_themes(),
            };
            (text, dirs, path.with_extension(extension), PdfOptions::default(), None)
        }
        Kind::Project { dir } => {
            let project = Project::open(&resolver, dir)?;
            let name = match &request.target {
                Target::Named(name) => Some(name.as_str()),
                Target::Pdf | Target::Epub => None,
            };
            if let Some(name) = name
                && !project.exports().any(|(known, _)| known == name)
            {
                return Err(failed(crate::t!(Key::NoExportNamed, name = name, manifest = MANIFEST)));
            }
            let (text, settings) = project.export(name)?;
            let extension = if request.target == Target::Epub { "epub" } else { "pdf" };
            let output = settings.output.unwrap_or_else(|| {
                let file = name.unwrap_or_else(|| document_name(dir));
                dir.join("build").join(format!("{file}.{extension}"))
            });
            if is_epub(&output)
                && let Some(conflict) = settings.print_conflict
            {
                return Err(Failure::Errors(vec![conflict]));
            }
            let dirs = ThemeDirs {
                project: Some(dir.clone()),
                shared: FileResolver::shared_themes(),
            };
            let final_export = name.filter(|_| settings.is_final).map(str::to_owned);
            (text, dirs, output, settings.pdf, final_export)
        }
    };
    let input = Input {
        text,
        theme: ThemeSelection::Document { dirs },
        overrides: Overrides::default(),
        today,
    };
    let renderer = Renderer::new();
    let configured = renderer
        .job(&resolver, &input, cancel)
        .missing(Missing::StandIn)
        .configure()?;
    if let Some(name) = &final_export {
        configured.check_final(name)?;
    }
    stage(Stage::Typesetting);
    let rendered = if is_epub(&output) {
        configured.epub(&EpubOptions { modified: source_day })?
    } else {
        let laid = configured.layout(&LayoutOptions::default())?;
        laid.pdf(&print, cancel)?
    };
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    stage(Stage::Writing);
    write(&output, &rendered.bytes).map_err(failed)?;
    Ok(Written {
        output,
        parts: rendered.parts,
    })
}

/// Writes the file whole or not at all: a partial PDF never replaces the one before it.
fn write(output: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(folder) = output.parent() {
        fs::create_dir_all(folder)
            .map_err(|error| crate::t!(Key::CannotCreateFolder, folder = folder.display(), error = error))?;
    }
    let mut temporary = output.as_os_str().to_owned();
    temporary.push(".part");
    let temporary = PathBuf::from(temporary);
    let cannot_write = |error: std::io::Error| crate::t!(Key::CannotWriteFile, file = output.display(), error = error);
    fs::write(&temporary, bytes).map_err(cannot_write)?;
    fs::rename(&temporary, output).map_err(|error| {
        fs::remove_file(&temporary).ok();
        cannot_write(error)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::session::Session;

    /// Layout needs more stack than a test thread has.
    fn on_big_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(64 * 1024 * 1024)
                .spawn_scoped(scope, work)
                .expect("a thread")
                .join()
                .expect("no panic")
        })
    }

    fn project(name: &str, final_open: bool) -> (PathBuf, Session) {
        let dir = std::env::temp_dir().join(format!("druck-export-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("manuscript")).expect("folder");
        let exports = format!(
            "exports:\n  proof:\n    output: build/proof.pdf\n    final: {final_open}\n  web:\n    output: build/web.epub\n"
        );
        fs::write(
            dir.join("druck.yaml"),
            format!("title: T\nchapters:\n  - manuscript/one.md\n{exports}"),
        )
        .expect("manifest");
        fs::write(dir.join("manuscript/one.md"), "# One\n\nSome text {todo: later}.\n").expect("chapter");
        let session = Session::open(&dir).expect("opens");
        (dir, session)
    }

    fn request(session: &Session, target: Target) -> Request {
        Request {
            kind: session.kind().clone(),
            joined: session.joined(),
            target,
        }
    }

    #[test]
    fn a_named_export_writes_its_file_and_reports_each_stage() {
        let (dir, session) = project("ok", false);
        let stages = Mutex::new(Vec::new());
        let written = on_big_stack(|| {
            run(
                request(&session, Target::Named("proof".into())),
                &Cancel::default(),
                &|stage| {
                    stages.lock().expect("no panic").push(stage);
                },
            )
        })
        .expect("exports");
        assert_eq!(written.output, dir.join("build/proof.pdf"));
        assert!(fs::read(&written.output).expect("file").starts_with(b"%PDF-"));
        assert_eq!(
            *stages.lock().expect("no panic"),
            [Stage::Preparing, Stage::Typesetting, Stage::Writing]
        );
        let written = on_big_stack(|| {
            run(
                request(&session, Target::Named("web".into())),
                &Cancel::default(),
                &|_| {},
            )
        })
        .expect("exports");
        assert!(fs::read(&written.output).expect("file").starts_with(b"PK"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_final_export_with_an_open_placeholder_fails_at_its_line() {
        let (dir, session) = project("final", true);
        let failure = on_big_stack(|| {
            run(
                request(&session, Target::Named("proof".into())),
                &Cancel::default(),
                &|_| {},
            )
        })
        .expect_err("open placeholder");
        let Failure::Errors(errors) = failure else {
            panic!("expected errors");
        };
        assert_eq!(errors[0].location.map(|location| location.line), Some(3));
        assert!(!dir.join("build/proof.pdf").exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_cancelled_run_writes_nothing() {
        let (dir, session) = project("cancel", false);
        let cancel = Cancel::default();
        cancel.cancel();
        let failure = on_big_stack(|| run(request(&session, Target::Named("proof".into())), &cancel, &|_| {}));
        assert!(matches!(failure, Err(Failure::Cancelled)));
        assert!(!dir.join("build").exists());
        fs::remove_dir_all(dir).ok();
    }
}
