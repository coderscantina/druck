//! From document text to published pages: the engine call, and which request's result counts. Holds no gpui
//! types.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use druck::cancel::Cancel;
use druck::diagnostic::Diagnostic;
use druck::editor::resolved_config::{self, Config, FontStyle, Weight};
use druck::input::{DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
use druck::resource::{Bytes, FileResolver, Origin, clock_date, source_date};
use druck::{Failure, LayoutOptions, Renderer};

use super::raster::{Painter, family_name, fingerprint};

/// A request to start: its number, the job, and the token that stops it.
pub type Started<T> = (u64, T, Cancel);

/// Runs one layout at a time. A request made meanwhile cancels the running layout and waits for it to stop, within a
/// block of text, replacing any request already waiting.
pub struct Scheduler<T> {
    latest: u64,
    running: Option<Cancel>,
    waiting: Option<(u64, T)>,
}

impl<T> Scheduler<T> {
    pub fn new() -> Self {
        Self {
            latest: 0,
            running: None,
            waiting: None,
        }
    }

    /// Registers a request, returning it if it should start now.
    pub fn request(&mut self, job: T) -> Option<Started<T>> {
        self.latest += 1;
        if let Some(running) = &self.running {
            running.cancel();
            self.waiting = Some((self.latest, job));
            return None;
        }
        Some(self.start((self.latest, job)))
    }

    fn start(&mut self, (number, job): (u64, T)) -> Started<T> {
        let cancel = Cancel::default();
        self.running = Some(cancel.clone());
        (number, job, cancel)
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The number of the newest request.
    pub fn latest(&self) -> u64 {
        self.latest
    }

    /// Ends the running layout, returning whether its result counts and the request to start next.
    pub fn finished(&mut self, number: u64) -> (bool, Option<Started<T>>) {
        self.running = None;
        let next = self.waiting.take().map(|waiting| self.start(waiting));
        (number == self.latest, next)
    }
}

/// The result of one layout.
pub struct Layout {
    /// The pages, with what they draw and where their text is written.
    pub laid: druck::output::Layout,
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
    pub files: Vec<Bytes>,
    pub size: f32,
    pub line_height: f32,
}

impl BodyStyle {
    fn of(config: &Config, laid: &druck::output::Layout) -> Self {
        let style = &config.styles.body;
        let mut files: Vec<Bytes> = Vec::new();
        let mut family = String::new();
        for (weight, font_style) in [
            (style.weight, style.style),
            (Weight::BOLD, FontStyle::Normal),
            (style.weight, FontStyle::Italic),
            (Weight::BOLD, FontStyle::Italic),
        ] {
            let italic = font_style == FontStyle::Italic;
            let face = laid
                .font(&style.font, weight.get(), italic)
                .expect("the body font is loaded");
            if family.is_empty() {
                family = family_name(face.data(), face.index()).unwrap_or_else(|| style.font.clone());
            }
            if !files.contains(face.data()) {
                files.push(face.data().clone());
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
    pub typeset: Duration,
    pub fingerprints: Duration,
}

/// Stack of the layout thread. Only the pages touched are committed, so the size costs address space.
const LAYOUT_STACK: usize = 16 * 1024 * 1024;

/// Lays out one document repeatedly. Its renderer keeps fonts and images by content, so each run reads them again
/// and prepares only what changed.
pub struct Typesetter {
    path: PathBuf,
    theme: Option<PathBuf>,
    working_dir: PathBuf,
    renderer: Renderer,
    resolver: FileResolver,
    painter: Option<Arc<Painter>>,
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
            renderer: Renderer::new(),
            resolver: FileResolver::new(),
            painter: None,
        }
    }

    /// Lays out on a thread of its own. The executor's pool threads, libdispatch workers on macOS, have 512 KB
    /// of stack, and a debug build needs about as much for deserializing a theme alone. The schema's nesting
    /// is fixed, so one size covers any input.
    pub fn run(&mut self, text: String, cancel: &Cancel) -> Result<Layout, Failure> {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("druck-layout".into())
                .stack_size(LAYOUT_STACK)
                .spawn_scoped(scope, || self.typeset(text, cancel))
                .expect("the system starts a thread")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }

    fn typeset(&mut self, text: String, cancel: &Cancel) -> Result<Layout, Failure> {
        let start = Instant::now();
        let input = self.input(text)?;
        let configured = self.renderer.job(&self.resolver, &input, cancel).configure()?;
        let configured_at = Instant::now();
        let laid = configured.layout(&LayoutOptions { source_map: true })?;
        let typeset = Instant::now();
        let config = resolved_config::of(&configured);
        let warnings = config.warnings.iter().chain(laid.warnings()).cloned().collect();
        let painter = same_fonts(&mut self.painter, &laid);
        let fingerprints_from = Instant::now();
        let fingerprints = laid.pages().iter().map(fingerprint).collect();
        Ok(Layout {
            body: BodyStyle::of(config, &laid),
            laid,
            painter,
            fingerprints,
            warnings,
            timing: Timing {
                configure: configured_at - start,
                typeset: typeset - configured_at,
                fingerprints: fingerprints_from.elapsed(),
            },
        })
    }

    fn input(&self, text: String) -> Result<Input, Vec<Diagnostic>> {
        let dirs = ThemeDirs {
            project: None,
            shared: FileResolver::shared_themes(),
        };
        let theme = match &self.theme {
            Some(theme) => ThemeSelection::asked(theme, &self.working_dir, dirs),
            None => ThemeSelection::Document { dirs },
        };
        let name = self.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let today = match source_date() {
            Ok(Some(day)) => Ok(day),
            Ok(None) => clock_date(),
            Err(message) => Err(message),
        };
        Ok(Input {
            text: DocumentText::Single {
                text,
                origin: Origin::Document(self.working_dir.clone()),
                name,
            },
            theme,
            overrides: Overrides::default(),
            today: today.map_err(|message| vec![Diagnostic::error(message)])?,
        })
    }
}

/// The painter of the previous layout while the fonts are the same, since it keeps their outlines.
fn same_fonts(painter: &mut Option<Arc<Painter>>, laid: &druck::output::Layout) -> Arc<Painter> {
    match painter {
        Some(painter) if painter.draws(laid) => painter.clone(),
        _ => painter.insert(Arc::new(Painter::new(laid))).clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_request_supersedes_the_running_one() {
        let mut scheduler = Scheduler::new();
        let (first, _, cancel) = scheduler.request("a").expect("idle, so it starts");
        assert!(!cancel.is_cancelled());
        assert!(scheduler.request("b").is_none());
        assert!(cancel.is_cancelled(), "the running layout is stopped");
        assert!(scheduler.request("c").is_none(), "replaces the waiting request");
        let (publish, next) = scheduler.finished(first);
        assert!(!publish);
        let (second, job, cancel) = next.expect("the latest request starts");
        assert_eq!(job, "c");
        assert!(!cancel.is_cancelled(), "a new layout has a token of its own");
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
        let Ok(first) = typesetter.run(text.clone(), &Cancel::default()) else {
            panic!("typesets")
        };
        let Ok(second) = typesetter.run(text + "\n\nOne more paragraph.\n", &Cancel::default()) else {
            panic!("typesets")
        };
        assert!(Arc::ptr_eq(&first.painter, &second.painter));
        assert_eq!(
            first.fingerprints[0], second.fingerprints[0],
            "an unchanged page keeps its fingerprint"
        );
    }

    #[test]
    fn a_replaced_font_makes_a_new_painter() {
        let dir = std::env::temp_dir().join(format!("druck-job-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        let fixture = |name: &str| Path::new(env!("CARGO_MANIFEST_DIR")).join("../fonts").join(name);
        let font = dir.join("body.otf");
        std::fs::copy(fixture("LibertinusSerif-Regular.otf"), &font).expect("font");
        let theme = dir.join("theme.json");
        std::fs::write(
            &theme,
            r#"{"version": 1, "fonts": {"Body": {"regular": "body.otf"}}, "styles": {"body": {"font": "Body"}}}"#,
        )
        .expect("theme");
        let mut typesetter = Typesetter::new(&dir.join("document.md"), Some(theme));
        let run = |typesetter: &mut Typesetter| {
            let Ok(layout) = typesetter.run("Body text.\n".to_owned(), &Cancel::default()) else {
                panic!("typesets")
            };
            layout
        };
        let (first, second) = (run(&mut typesetter), run(&mut typesetter));
        assert!(Arc::ptr_eq(&first.painter, &second.painter));

        std::fs::copy(fixture("LibertinusSerif-Bold.otf"), &font).expect("font");
        let replaced = run(&mut typesetter);
        std::fs::remove_dir_all(&dir).expect("temporary folder");
        assert!(!Arc::ptr_eq(&first.painter, &replaced.painter));
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
        let Ok(layout) = Typesetter::new(&document, Some(theme)).run(text, &Cancel::default()) else {
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
            .spawn(move || {
                let cancel = Cancel::default();
                Typesetter::new(&sample("report.md"), None).run(text, &cancel).is_ok()
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
        assert!(layout, "typesets");
    }

    #[test]
    fn a_cancelled_run_publishes_nothing_and_the_next_one_works() {
        let mut typesetter = Typesetter::new(&sample("report.md"), None);
        let text = std::fs::read_to_string(sample("report.md")).expect("sample");
        let cancelled = Cancel::default();
        cancelled.cancel();
        let result = typesetter.run(text.clone(), &cancelled);
        assert!(matches!(result, Err(Failure::Cancelled)));
        assert!(typesetter.run(text, &Cancel::default()).is_ok());
    }

    #[test]
    fn source_offsets_count_the_front_matter_the_engine_strips() {
        let text = "---\ntitle: A title\n---\n\nFirst word here.\n";
        let mut typesetter = Typesetter::new(&sample("report.md"), None);
        let Ok(layout) = typesetter.run(text.to_owned(), &Cancel::default()) else {
            panic!("typesets")
        };
        let map = layout.laid.source_map().expect("the app asks for a map");
        let source = layout
            .laid
            .pages()
            .iter()
            .flat_map(|page| &page.items)
            .find_map(|item| match item {
                druck::output::Item::Text {
                    run, source: Some(id), ..
                } if run.text == "word" => Some(*id),
                _ => None,
            });
        let at = map
            .position(source.expect("the word is on a page"), 0)
            .expect("written");
        assert_eq!(&text[at.offset as usize..at.offset as usize + 4], "word");
    }

    #[test]
    fn invalid_text_reports_diagnostics() {
        let mut typesetter = Typesetter::new(&sample("report.md"), None);
        let result = typesetter.run("---\ntitle: [unclosed\n---\n\nBody\n".to_owned(), &Cancel::default());
        let Err(Failure::Errors(diagnostics)) = result else {
            panic!("front matter is invalid")
        };
        assert!(!diagnostics.is_empty());
    }
}
