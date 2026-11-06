//! The entry points: a renderer, and the stages of one job.
//!
//! A [`Renderer`] keeps what is expensive to prepare, by content hash, and can run jobs from several threads at
//! once. A job keeps its own state: [`Job`] and [`Configured`] are `Send` but not `Sync`, and borrow the renderer,
//! resolver, input, and token. A [`crate::cancel::Cancel`] stops a job from any thread. A [`Layout`] owns what it
//! needs and outlives them all.

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cancel::Cancel;
use crate::config::resolved::Config;
use crate::config::source::{InputSource, Origin};
use crate::config::theme::{FontStyle, Weight};
use crate::date::Date;
use crate::diagnostic::Diagnostic;
use crate::image::ImageCache;
use crate::input::{DocumentText, Input};
use crate::output::{Layout, Pt};
use crate::pipeline::{self, DocumentFile, JobContext};
use crate::resource::Resolver;
use crate::text::FontSet;
use crate::text::face::FaceCache;
use crate::{epub, project};

/// Runs jobs, reusing faces and decoded images between them. Every job reads its resources again through its
/// resolver, so a replaced file is seen, and an unchanged one costs a read and a hash. What the last two jobs did
/// not use is forgotten when the next one starts.
#[derive(Default)]
pub struct Renderer {
    pub(crate) faces: FaceCache,
    pub(crate) images: ImageCache,
    jobs: AtomicU64,
}

impl Renderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// The simple path: Markdown and configuration in, PDF bytes and every warning out.
    pub fn render(
        &self,
        resolver: &dyn Resolver,
        input: &Input,
        options: &PdfOptions,
        cancel: &Cancel,
    ) -> Result<Rendered, Failure> {
        let configured = self.job(resolver, input, cancel).configure()?;
        let layout = configured.layout(&LayoutOptions::default())?;
        let mut rendered = layout.pdf(options, cancel)?;
        let mut warnings = configured.warnings().to_vec();
        warnings.extend_from_slice(layout.warnings());
        warnings.append(&mut rendered.warnings);
        rendered.warnings = warnings;
        Ok(rendered)
    }

    /// The explicit path, one stage at a time. A job is one document on one thread, and `cancel` stops all its
    /// stages. [`Layout::pdf`] takes its own token, since a layout outlives its job.
    pub fn job<'r>(&'r self, resolver: &'r dyn Resolver, input: &'r Input, cancel: &'r Cancel) -> Job<'r> {
        Job {
            renderer: self,
            resolver,
            input,
            cancel,
            one_thread: PhantomData,
        }
    }

    /// Numbers a new job and forgets what neither of the two jobs before it used.
    fn begin(&self) -> u64 {
        let number = self.jobs.load(Ordering::Relaxed) + 1;
        let oldest = number.saturating_sub(2);
        self.faces.sweep(oldest);
        self.images.sweep(oldest);
        number
    }

    /// Counts a job that ran its course. A cancelled one does not age what the caches hold, so a burst of them
    /// costs no decoded image.
    fn count<T>(&self, result: Result<T, Failure>) -> Result<T, Failure> {
        if !matches!(result, Err(Failure::Cancelled)) {
            self.jobs.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
}

/// One document on its way to being configured.
pub struct Job<'r> {
    renderer: &'r Renderer,
    resolver: &'r dyn Resolver,
    input: &'r Input,
    cancel: &'r Cancel,
    one_thread: PhantomData<Cell<()>>,
}

impl<'r> Job<'r> {
    /// Resolves and checks the configuration, and reads every resource it names. Errors here never reach layout.
    pub fn configure(self) -> Result<Configured<'r>, Failure> {
        let Self {
            renderer,
            resolver,
            input,
            cancel,
            ..
        } = self;
        let (config, document) = match &input.text {
            DocumentText::Single { text, origin, name } => {
                let Origin::Document(dir) = origin else {
                    let message = "a document's origin must be a document folder";
                    return Err(vec![Diagnostic::new(Some(InputSource::Document(name.into())), message)].into());
                };
                pipeline::configure(resolver, input, dir, name, text)?
            }
            DocumentText::Project(project) => project.configure(resolver, input)?,
        };
        let (files, missing) = pipeline::read_resources(resolver, &config, cancel)?;
        let mut config = config;
        config.warnings.extend(missing);
        Ok(Configured {
            job: JobContext {
                renderer,
                resolver,
                cancel,
                number: renderer.begin(),
                files,
            },
            config,
            document,
            today: input.today,
            one_thread: PhantomData,
        })
    }
}

/// A document with its configuration resolved and its resources read.
pub struct Configured<'r> {
    pub(crate) job: JobContext<'r>,
    pub(crate) config: Config,
    pub(crate) document: DocumentFile,
    pub(crate) today: Date,
    one_thread: PhantomData<Cell<()>>,
}

impl Configured<'_> {
    /// What is wrong with the configuration without stopping the job.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.config.warnings
    }

    /// The resolved configuration as pretty JSON, in schema order.
    pub fn config_json(&self) -> String {
        serde_json::to_string_pretty(&self.config).expect("configuration serializes")
    }

    /// Fails with one error per open placeholder, and per image, bibliography, or font file that is not there, at its
    /// real location, naming the final `export`.
    pub fn check_final(&self, export: &str) -> Result<(), Failure> {
        let mut errors = project::check_final(&self.config, &self.document, export)
            .err()
            .unwrap_or_default();
        errors.extend(pipeline::missing_for_final(
            &self.job,
            &self.config,
            &self.document,
            export,
        )?);
        if errors.is_empty() { Ok(()) } else { Err(errors.into()) }
    }

    /// The document's fonts, loaded as layout loads them, for a client that sets text in them before or without a
    /// layout, such as when the layout fails.
    pub fn fonts(&self) -> Result<DocumentFonts, Failure> {
        Ok(DocumentFonts {
            fonts: pipeline::load_fonts(&self.job, &self.config)?,
        })
    }

    /// Lays out the document, reporting everything a render reports before writing.
    pub fn layout(&self, options: &LayoutOptions) -> Result<Layout, Failure> {
        let typeset = pipeline::typeset(&self.job, &self.config, &self.document, self.today, options.source_map);
        let (fonts, laid) = self.job.renderer.count(typeset)?;
        Ok(Layout {
            laid,
            fonts,
            metadata: self.config.metadata.clone(),
            lang: self.config.document.lang,
            conformance: self.config.document.conformance.clone(),
            today: self.today,
            source: self.document.source.clone(),
        })
    }

    /// Writes an EPUB, which needs no layout: a reader sets the text itself, so there is no `Layout::epub`. `parts`
    /// is the number of content documents.
    pub fn epub(&self, options: &EpubOptions) -> Result<Rendered, Failure> {
        self.job.renderer.count(self.write_epub(options))
    }

    fn write_epub(&self, options: &EpubOptions) -> Result<Rendered, Failure> {
        let (config, document) = (&self.config, &self.document);
        let prepared = pipeline::prepare(&self.job, config, document)?;
        let iso = config.metadata.date.as_deref().and_then(|text| Date::iso(text.trim()));
        let written = epub::write(&epub::Book {
            document: &prepared.content,
            cited: &prepared.cited,
            config,
            source: &document.source,
            images: &prepared.images,
            files: &self.job.files,
            today: self.today,
            modified: options.modified.or(iso).unwrap_or(self.today),
            fallback_title: document.name().unwrap_or("Untitled"),
            cancel: self.job.cancel,
        })?;
        Ok(Rendered {
            bytes: written.bytes,
            warnings: written.warnings,
            parts: written.documents,
        })
    }
}

/// Finished output.
#[derive(Debug)]
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub warnings: Vec<Diagnostic>,
    /// The pages of a PDF, or the content documents of an EPUB.
    pub parts: usize,
}

/// The fonts of a configured document, see [`Configured::fonts`].
pub struct DocumentFonts {
    fonts: FontSet,
}

impl DocumentFonts {
    /// The face a theme font family uses for text of `weight` (100 to 900) and slant, as [`Layout::font`] chooses it.
    pub fn font(&self, family: &str, weight: u16, italic: bool) -> Option<&crate::output::Face> {
        let style = if italic { FontStyle::Italic } else { FontStyle::Normal };
        let id = self.fonts.nearest(family, Weight::of_font(weight), style)?;
        Some(self.fonts.face_of(id))
    }
}

/// What a layout includes besides its pages.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutOptions {
    /// Whether to connect the text items to the written text, see [`Layout::source_map`]. A batch render needs no
    /// map, and without it the parser records nothing.
    pub source_map: bool,
}

/// Export settings. They never change the layout.
#[derive(Debug, Clone, Copy)]
pub struct PdfOptions {
    /// How far the bleed box reaches past the trim box on each side.
    pub bleed: Pt,
    /// Whether crop marks show the trim box at the corners, outside the bleed.
    pub crop_marks: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            bleed: Pt(0.0),
            crop_marks: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct EpubOptions {
    /// The day of `dcterms:modified`. Without it an ISO `date` setting decides, then `Input::today`, so the same
    /// input gives the same bytes. A build with `SOURCE_DATE_EPOCH` passes that day.
    pub modified: Option<Date>,
}

/// Why a job produced no output.
#[derive(Debug)]
pub enum Failure {
    Cancelled,
    /// Every error of the stage that failed, in reading order.
    Errors(Vec<Diagnostic>),
}

/// One line per error, as a terminal shows them with every path absolute.
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Errors(errors) => {
                for (index, error) in errors.iter().enumerate() {
                    if index > 0 {
                        f.write_str("\n")?;
                    }
                    std::fmt::Display::fmt(error, f)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Failure {}

impl From<Vec<Diagnostic>> for Failure {
    fn from(errors: Vec<Diagnostic>) -> Self {
        Self::Errors(errors)
    }
}

#[cfg(test)]
impl Failure {
    /// The errors of a job that nobody cancels.
    pub(crate) fn into_errors(self) -> Vec<Diagnostic> {
        match self {
            Self::Errors(errors) => errors,
            Self::Cancelled => panic!("nothing cancels this job"),
        }
    }
}

/// What the documented sharing rests on: a renderer, a layout, and resolvers go between threads and are shared, a
/// job moves to one thread and stays there.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    const fn moved<T: Send>() {}
    shared::<Renderer>();
    shared::<Layout>();
    shared::<Input>();
    shared::<crate::resource::FileResolver>();
    shared::<crate::resource::MemoryResolver>();
    shared::<crate::cancel::Cancel>();
    moved::<Job<'static>>();
    moved::<Configured<'static>>();
};

/// Fails to compile when its type argument is `Sync`, because `_` then has two answers.
trait NotSync<Answer> {
    const HOLDS: () = ();
}
impl<T: ?Sized> NotSync<()> for T {}
impl<T: ?Sized + Sync> NotSync<u8> for T {}
const _: () = <Job<'static> as NotSync<_>>::HOLDS;
const _: () = <Configured<'static> as NotSync<_>>::HOLDS;
