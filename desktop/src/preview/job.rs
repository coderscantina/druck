//! From document text to published pages: the engine call, and which request's result counts. Holds no gpui
//! types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use druck::config::resolved::{Config, FontFiles};
use druck::config::theme::{Face, FontStyle, Weight};
use druck::diagnostic::Diagnostic;
use druck::page;
use druck::pipeline::{self, Request};

use super::raster::{Painter, family_name, fingerprint};

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

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// The number of the newest request.
    pub fn latest(&self) -> u64 {
        self.latest
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
    pub body: BodyStyle,
    pub timing: Timing,
}

/// What the galley needs of the theme's body style, resolved by the engine.
#[derive(Clone, PartialEq)]
pub struct BodyStyle {
    /// The family's name in its font files, which is how the toolkit finds it.
    pub family: String,
    /// The font files of the family's regular, bold, and italic faces.
    pub files: Vec<&'static [u8]>,
    pub size: f32,
    pub line_height: f32,
}

impl BodyStyle {
    fn of(config: &Config, painter: &Painter) -> Self {
        let style = &config.styles.body;
        let fonts = painter.fonts();
        let mut files: Vec<&'static [u8]> = Vec::new();
        let mut family = String::new();
        for (weight, font_style) in [
            (style.weight, style.style),
            (Weight::BOLD, FontStyle::Normal),
            (style.weight, FontStyle::Italic),
            (Weight::BOLD, FontStyle::Italic),
        ] {
            let outlines = fonts.outlines(fonts.face(&style.font, weight, font_style));
            if family.is_empty() {
                family = family_name(outlines.data, outlines.index).unwrap_or_else(|| style.font.clone());
            }
            if !files.iter().any(|file| std::ptr::eq(*file, outlines.data)) {
                files.push(outlines.data);
            }
        }
        Self {
            family,
            files,
            size: style.size.0 as f32,
            line_height: style.line_height as f32,
        }
    }
}

/// Where a layout's time went.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub configure: Duration,
    pub fonts: Duration,
    pub typeset: Duration,
    pub fingerprints: Duration,
}

/// Stack of the layout thread. Only the pages touched are committed, so the size costs address space.
const LAYOUT_STACK: usize = 16 * 1024 * 1024;

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
    theme: Option<PathBuf>,
    working_dir: PathBuf,
    loaded: Option<(FontRequest, Arc<Painter>)>,
}

impl Typesetter {
    /// `path` is the document the text belongs to: relative paths in it start from its folder. `theme`, if any,
    /// replaces the one the document names.
    pub fn new(path: &Path, theme: Option<PathBuf>) -> Self {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        let working_dir = path.parent().map(Path::to_owned).unwrap_or_default();
        Self {
            path,
            theme,
            working_dir,
            loaded: None,
        }
    }

    /// Lays out on a thread of its own. The executor's pool threads, libdispatch workers on macOS, have 512 KB
    /// of stack, and a debug build needs about as much for deserializing a theme alone. The schema's nesting
    /// is fixed, so one size covers any input.
    pub fn run(&mut self, text: String) -> Result<Layout, Vec<Diagnostic>> {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("druck-layout".into())
                .stack_size(LAYOUT_STACK)
                .spawn_scoped(scope, || self.typeset(text))
                .expect("the system starts a thread")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }

    fn typeset(&mut self, text: String) -> Result<Layout, Vec<Diagnostic>> {
        let start = Instant::now();
        let request = Request {
            input: &self.path,
            theme: self.theme.as_deref(),
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
            body: BodyStyle::of(&config, &painter),
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
        let mut typesetter = Typesetter::new(&sample("report.md"), None);
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
    fn a_theme_given_to_the_typesetter_sets_the_body_style() {
        let text = "Body text.\n".to_owned();
        let dir = std::env::temp_dir().join(format!("druck-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        let theme = dir.join("theme.json");
        std::fs::write(
            &theme,
            r#"{"version": 1, "styles": {"body": {"size": "13pt", "line-height": 1.5}}}"#,
        )
        .expect("theme");
        let document = dir.join("document.md");
        let Ok(layout) = Typesetter::new(&document, Some(theme)).run(text) else {
            panic!("typesets")
        };
        std::fs::remove_dir_all(&dir).expect("temporary folder");
        assert_eq!((layout.body.size, layout.body.line_height), (13.0, 1.5));
        assert_eq!(layout.body.family, "Libertinus Serif");
        assert!(!layout.body.files.is_empty());
    }

    #[test]
    fn layout_does_not_depend_on_the_callers_stack() {
        let text = std::fs::read_to_string(sample("report.md")).expect("sample");
        let layout = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || Typesetter::new(&sample("report.md"), None).run(text).is_ok())
            .expect("thread")
            .join()
            .expect("no stack overflow");
        assert!(layout, "typesets");
    }

    #[test]
    fn invalid_text_reports_diagnostics() {
        let mut typesetter = Typesetter::new(&sample("report.md"), None);
        let result = typesetter.run("---\ntitle: [unclosed\n---\n\nBody\n".to_owned());
        let Err(diagnostics) = result else {
            panic!("front matter is invalid")
        };
        assert!(!diagnostics.is_empty());
    }
}
