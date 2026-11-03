//! From document text to published pages: the engine call, and which request's result counts. Holds no gpui
//! types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use druck::config::resolved::{Config, FontFiles};
use druck::config::theme::Face;
use druck::diagnostic::Diagnostic;
use druck::page;
use druck::pipeline::{self, Request};

use super::raster::{Painter, fingerprint};

/// Runs one layout at a time. A request made meanwhile waits, replacing any request already waiting, and
/// makes the running layout's result obsolete. A running layout cannot be stopped.
pub struct Scheduler<T> {
    latest: u64,
    running: bool,
    waiting: Option<(u64, T)>,
}

impl<T> Scheduler<T> {
    pub fn new() -> Self {
        Self {
            latest: 0,
            running: false,
            waiting: None,
        }
    }

    /// Registers a request, returning it with its number if it should start now.
    pub fn request(&mut self, job: T) -> Option<(u64, T)> {
        self.latest += 1;
        if self.running {
            self.waiting = Some((self.latest, job));
            return None;
        }
        self.running = true;
        Some((self.latest, job))
    }

    /// Ends the running layout, returning whether its result counts and the request to start next.
    pub fn finished(&mut self, number: u64) -> (bool, Option<(u64, T)>) {
        let next = self.waiting.take();
        self.running = next.is_some();
        (number == self.latest, next)
    }
}

/// The result of one layout.
pub struct Layout {
    pub output: page::Output,
    pub painter: Arc<Painter>,
    /// One per page, see [`fingerprint`].
    pub fingerprints: Vec<u64>,
    /// Configuration and typesetting warnings.
    pub warnings: Vec<Diagnostic>,
    pub timing: Timing,
}

/// Where a layout's time went.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub configure: Duration,
    pub fonts: Duration,
    pub typeset: Duration,
    pub fingerprints: Duration,
}

/// The faces a configuration asks for, which decide whether loaded fonts can be reused.
#[derive(PartialEq)]
struct FontRequest {
    fonts: BTreeMap<String, FontFiles>,
    installed: BTreeMap<String, BTreeMap<Face, String>>,
    requested: BTreeMap<String, BTreeMap<Face, String>>,
    excluded: BTreeMap<String, Vec<Face>>,
}

impl FontRequest {
    fn of(config: &Config) -> Self {
        Self {
            fonts: config.fonts.clone(),
            installed: config.installed_fonts.clone(),
            requested: config.requested_fonts.clone(),
            excluded: config.excluded_fonts.clone(),
        }
    }
}

/// Lays out one document repeatedly, keeping its fonts while the configuration asks for the same ones.
pub struct Typesetter {
    path: PathBuf,
    working_dir: PathBuf,
    loaded: Option<(FontRequest, Arc<Painter>)>,
}

impl Typesetter {
    /// `path` is the document the text belongs to: relative paths in it start from its folder.
    pub fn new(path: &Path) -> Self {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        let working_dir = path.parent().map(Path::to_owned).unwrap_or_default();
        Self {
            path,
            working_dir,
            loaded: None,
        }
    }

    pub fn run(&mut self, text: String) -> Result<Layout, Vec<Diagnostic>> {
        let start = Instant::now();
        let request = Request {
            input: &self.path,
            theme: None,
            overrides: &[],
        };
        let (config, document) = pipeline::configure(&request, &self.working_dir, text)?;
        let configured = Instant::now();
        let painter = self.painter(&config)?;
        let fonts_ready = Instant::now();
        let output = pipeline::typeset_with(&config, &document, painter.fonts())?;
        let typeset = Instant::now();
        let fingerprints = output.pages.iter().map(fingerprint).collect();
        let warnings = config.warnings.iter().chain(&output.warnings).cloned().collect();
        Ok(Layout {
            output,
            painter,
            fingerprints,
            warnings,
            timing: Timing {
                configure: configured - start,
                fonts: fonts_ready - configured,
                typeset: typeset - fonts_ready,
                fingerprints: typeset.elapsed(),
            },
        })
    }

    fn painter(&mut self, config: &Config) -> Result<Arc<Painter>, Vec<Diagnostic>> {
        let wanted = FontRequest::of(config);
        if let Some((request, painter)) = &self.loaded
            && *request == wanted
        {
            return Ok(painter.clone());
        }
        let painter = Arc::new(Painter::new(pipeline::load_fonts(config)?));
        self.loaded = Some((wanted, painter.clone()));
        Ok(painter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_request_supersedes_the_running_one() {
        let mut scheduler = Scheduler::new();
        let (first, _) = scheduler.request("a").expect("idle, so it starts");
        assert!(scheduler.request("b").is_none());
        assert!(scheduler.request("c").is_none(), "replaces the waiting request");
        let (publish, next) = scheduler.finished(first);
        assert!(!publish);
        let (second, job) = next.expect("the latest request starts");
        assert_eq!(job, "c");
        assert!(scheduler.finished(second).0);
        assert!(scheduler.request("d").is_some(), "idle again");
    }

    fn sample(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples").join(name)
    }

    #[test]
    fn repeated_runs_reuse_the_painter_and_fonts() {
        let mut typesetter = Typesetter::new(&sample("report.md"));
        let text = std::fs::read_to_string(sample("report.md")).expect("sample");
        let Ok(first) = typesetter.run(text.clone()) else {
            panic!("typesets")
        };
        let Ok(second) = typesetter.run(text + "\n\nOne more paragraph.\n") else {
            panic!("typesets")
        };
        assert!(Arc::ptr_eq(&first.painter, &second.painter));
        assert_eq!(
            first.fingerprints[0], second.fingerprints[0],
            "an unchanged page keeps its fingerprint"
        );
    }

    #[test]
    fn invalid_text_reports_diagnostics() {
        let mut typesetter = Typesetter::new(&sample("report.md"));
        let result = typesetter.run("---\ntitle: [unclosed\n---\n\nBody\n".to_owned());
        let Err(diagnostics) = result else {
            panic!("front matter is invalid")
        };
        assert!(!diagnostics.is_empty());
    }
}
