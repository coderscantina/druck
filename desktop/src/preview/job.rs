//! From document text to published pages: the engine call, and which request's result counts. Holds no gpui
//! types.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use druck::cancel::Cancel;
use druck::diagnostic::Diagnostic;
use druck::editor::resolved_config::{self, Config, FontStyle, Style, Weight};
use druck::input::{DocumentText, Input, Overrides, Project, ThemeDirs, ThemeSelection};
use druck::resource::{Bytes, FileResolver, Origin, clock_date, source_date};
use druck::{Failure, LayoutOptions, Renderer};

use super::fonts::{FontUse, fonts_in_use};
use super::raster::{Painter, family_name, fingerprint};
use crate::session::{Joined, Kind, LiveResolver};

/// What one layout typesets: a text, in a theme, which is the specimen or the document.
#[derive(Clone)]
pub struct Job {
    pub joined: Joined,
    /// The theme file to use instead of the document's.
    pub theme: Option<PathBuf>,
    pub specimen: bool,
}

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
    pub book_type: BookType,
    /// The font families the styles use, for the Fonts tab.
    pub fonts: Vec<FontUse>,
    pub timing: Timing,
}

/// What Write shows of one of the theme's block styles, resolved by the engine. Colours are not here: they follow the
/// appearance.
#[derive(Clone, PartialEq, Debug)]
pub struct Face {
    /// The family's name in its font files, which is how the toolkit finds it.
    pub family: String,
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    pub line_height: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub first_line_indent: f32,
}

/// The theme's type for Write: fonts, sizes, weights, spacing, and hierarchy, with the book's measure.
#[derive(Clone, PartialEq)]
pub struct BookType {
    pub body: Face,
    pub headings: [Face; 6],
    pub quote: Face,
    pub list: Face,
    pub footnote: Face,
    pub code: Face,
    /// The width of the book's prose on the page, in points.
    pub measure: f32,
    /// The font files of every face above, for the toolkit.
    pub files: Vec<Bytes>,
}

impl BookType {
    fn of(config: &Config, laid: &druck::output::Layout) -> Self {
        let mut files: Vec<Bytes> = Vec::new();
        let mut face = |style: &Style| {
            let found = laid.font(&style.font, style.weight.get(), style.style == FontStyle::Italic);
            // Bold and italic within a line come from the same family.
            for (weight, italic) in [(Weight::BOLD, false), (style.weight, true), (Weight::BOLD, true)] {
                if let Some(other) = laid.font(&style.font, weight.get(), italic)
                    && !files.contains(other.data())
                {
                    files.push(other.data().clone());
                }
            }
            if let Some(found) = found
                && !files.contains(found.data())
            {
                files.push(found.data().clone());
            }
            Face {
                family: found
                    .and_then(|found| family_name(found.data(), found.index()))
                    .unwrap_or_else(|| style.font.clone()),
                size: style.size.0 as f32,
                weight: style.weight.get(),
                italic: style.style == FontStyle::Italic,
                line_height: style.line_height as f32,
                space_before: style.space_before.0 as f32,
                space_after: style.space_after.0 as f32,
                first_line_indent: style.first_line_indent.0 as f32,
            }
        };
        let styles = &config.styles;
        Self {
            body: face(&styles.body),
            headings: [
                face(&styles.heading_1),
                face(&styles.heading_2),
                face(&styles.heading_3),
                face(&styles.heading_4),
                face(&styles.heading_5),
                face(&styles.heading_6),
            ],
            quote: face(&styles.quote),
            list: face(&styles.list),
            footnote: face(&styles.footnote),
            code: face(&styles.code_block),
            measure: config.page.prose_width.0 as f32,
            files,
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
    kind: Kind,
    theme: Option<PathBuf>,
    working_dir: PathBuf,
    renderer: Renderer,
    resolver: FileResolver,
    painter: Option<Arc<Painter>>,
}

impl Typesetter {
    /// `kind` is the document the text belongs to, with absolute paths: relative paths in it start from its folder.
    /// `theme`, if any, replaces the one the document names.
    pub fn new(kind: Kind, theme: Option<PathBuf>) -> Self {
        let working_dir = match &kind {
            Kind::Single { path } => path.parent().map(PathBuf::from).unwrap_or_default(),
            Kind::Project { dir } => dir.clone(),
        };
        Self {
            kind,
            theme,
            working_dir,
            renderer: Renderer::new(),
            resolver: FileResolver::new(),
            painter: None,
        }
    }

    /// Typesets the next runs in this theme, or in the document's when `None`.
    pub fn set_theme(&mut self, theme: Option<PathBuf>) {
        self.theme = theme;
    }

    /// Lays out on a thread of its own. The executor's pool threads, libdispatch workers on macOS, have 512 KB
    /// of stack, and a debug build needs about as much for deserializing a theme alone. The schema's nesting
    /// is fixed, so one size covers any input.
    pub fn run(&mut self, joined: Joined, cancel: &Cancel) -> Result<Layout, Failure> {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("druck-layout".into())
                .stack_size(LAYOUT_STACK)
                .spawn_scoped(scope, || self.typeset(joined, cancel))
                .expect("the system starts a thread")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }

    /// The engine's text for chips, for the document as `joined` has it. `None` while it cannot be read.
    pub fn labels(&self, joined: Joined) -> Option<druck::editor::rich::Labels> {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("druck-labels".into())
                .stack_size(LAYOUT_STACK)
                .spawn_scoped(scope, || {
                    let cancel = Cancel::default();
                    let (input, _) = self.input(joined).ok()?;
                    let configured = self.renderer.job(&self.resolver, &input, &cancel).configure().ok()?;
                    druck::editor::rich::labels(&configured).ok()
                })
                .expect("the system starts a thread")
                .join()
                .unwrap_or(None)
        })
    }

    fn typeset(&mut self, joined: Joined, cancel: &Cancel) -> Result<Layout, Failure> {
        let start = Instant::now();
        let (input, unlisted) = self.input(joined)?;
        let configured = self.renderer.job(&self.resolver, &input, cancel).configure()?;
        let configured_at = Instant::now();
        let laid = configured.layout(&LayoutOptions { source_map: true })?;
        let typeset = Instant::now();
        let config = resolved_config::of(&configured);
        let warnings = unlisted
            .iter()
            .chain(&config.warnings)
            .chain(laid.warnings())
            .cloned()
            .collect();
        let painter = same_fonts(&mut self.painter, &laid);
        let fingerprints_from = Instant::now();
        let fingerprints = laid.pages().iter().map(fingerprint).collect();
        Ok(Layout {
            book_type: BookType::of(config, &laid),
            fonts: fonts_in_use(config, &laid),
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

    /// The document as the engine's input, with the warnings a project's manifest gives.
    fn input(&self, joined: Joined) -> Result<(Input, Vec<Diagnostic>), Vec<Diagnostic>> {
        let dirs = ThemeDirs {
            project: matches!(self.kind, Kind::Project { .. }).then(|| self.working_dir.clone()),
            shared: FileResolver::shared_themes(),
        };
        let theme = match &self.theme {
            Some(theme) => ThemeSelection::asked(theme, &self.working_dir, dirs),
            None => ThemeSelection::Document { dirs },
        };
        let today = match source_date() {
            Ok(Some(day)) => Ok(day),
            Ok(None) => clock_date(),
            Err(message) => Err(message),
        };
        let today = today.map_err(|message| vec![Diagnostic::error(message)])?;
        let (text, warnings) = match &self.kind {
            Kind::Single { path } => {
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let text = DocumentText::Single {
                    text: joined.text,
                    origin: Origin::Document(self.working_dir.clone()),
                    name,
                };
                (text, Vec::new())
            }
            Kind::Project { dir } => {
                let project = Project::open(&LiveResolver::new(&joined, &self.resolver), dir)?;
                let warnings = project.warnings().to_vec();
                (project.export(None)?.0, warnings)
            }
        };
        let input = Input {
            text,
            theme,
            overrides: Overrides::default(),
            today,
        };
        Ok((input, warnings))
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
    use std::path::Path;

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
        druck::resource::normalize(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples").join(name))
    }

    fn single(path: PathBuf) -> Kind {
        Kind::Single { path }
    }

    fn text(text: impl Into<String>) -> Joined {
        Joined::single(text.into(), 0)
    }

    #[test]
    fn repeated_runs_reuse_the_painter_and_fonts() {
        let mut typesetter = Typesetter::new(single(sample("report.md")), None);
        let written = std::fs::read_to_string(sample("report.md")).expect("sample");
        let Ok(first) = typesetter.run(text(written.clone()), &Cancel::default()) else {
            panic!("typesets")
        };
        let Ok(second) = typesetter.run(text(written + "\n\nOne more paragraph.\n"), &Cancel::default()) else {
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
        let mut typesetter = Typesetter::new(single(dir.join("document.md")), Some(theme));
        let run = |typesetter: &mut Typesetter| {
            let Ok(layout) = typesetter.run(text("Body text.\n"), &Cancel::default()) else {
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
        let dir = std::env::temp_dir().join(format!("druck-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        let theme = dir.join("theme.json");
        std::fs::write(
            &theme,
            r#"{"version": 1, "styles": {"body": {"size": "13pt", "line-height": 1.5}}}"#,
        )
        .expect("theme");
        let document = dir.join("document.md");
        let Ok(layout) = Typesetter::new(single(document), Some(theme)).run(text("Body text.\n"), &Cancel::default())
        else {
            panic!("typesets")
        };
        std::fs::remove_dir_all(&dir).expect("temporary folder");
        assert_eq!(
            (layout.book_type.body.size, layout.book_type.body.line_height),
            (13.0, 1.5)
        );
        assert_eq!(layout.book_type.body.family, "Libertinus Serif");
        assert!(!layout.book_type.files.is_empty());
    }

    #[test]
    fn layout_does_not_depend_on_the_callers_stack() {
        let written = std::fs::read_to_string(sample("report.md")).expect("sample");
        let layout = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                let cancel = Cancel::default();
                Typesetter::new(single(sample("report.md")), None)
                    .run(text(written), &cancel)
                    .is_ok()
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
        assert!(layout, "typesets");
    }

    #[test]
    fn a_cancelled_run_publishes_nothing_and_the_next_one_works() {
        let mut typesetter = Typesetter::new(single(sample("report.md")), None);
        let written = std::fs::read_to_string(sample("report.md")).expect("sample");
        let cancelled = Cancel::default();
        cancelled.cancel();
        let result = typesetter.run(text(written.clone()), &cancelled);
        assert!(matches!(result, Err(Failure::Cancelled)));
        assert!(typesetter.run(text(written), &Cancel::default()).is_ok());
    }

    #[test]
    fn source_offsets_count_the_front_matter_the_engine_strips() {
        let written = "---\ntitle: A title\n---\n\nFirst word here.\n";
        let mut typesetter = Typesetter::new(single(sample("report.md")), None);
        let Ok(layout) = typesetter.run(text(written), &Cancel::default()) else {
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
        assert_eq!(&written[at.offset as usize..at.offset as usize + 4], "word");
    }

    #[test]
    fn invalid_text_reports_diagnostics() {
        let mut typesetter = Typesetter::new(single(sample("report.md")), None);
        let result = typesetter.run(text("---\ntitle: [unclosed\n---\n\nBody\n"), &Cancel::default());
        let Err(Failure::Errors(diagnostics)) = result else {
            panic!("front matter is invalid")
        };
        assert!(!diagnostics.is_empty());
    }
}
