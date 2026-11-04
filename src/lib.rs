//! Druck's typesetting engine: Markdown and JSON themes in, PDF and EPUB out.
//!
//! [`Renderer`] is the entry point. Build an [`input::Input`], then either call [`Renderer::render`], or go
//! through [`Renderer::job`] to read the warnings and the layout before writing the PDF. The engine reads files
//! only through a [`resource::Resolver`] that the caller chooses, and never prints or exits.
//!
//! # Rendering
//!
//! Markdown in, PDF bytes and warnings out. A [`resource::MemoryResolver`] with nothing in it renders a document
//! that uses the bundled theme and fonts; [`resource::FileResolver`] reads from disk.
//!
//! ```no_run
//! use druck::cancel::Cancel;
//! use druck::input::{Date, DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
//! use druck::resource::{MemoryResolver, Origin};
//! use druck::{PdfOptions, Renderer};
//!
//! # fn main() -> Result<(), druck::Failure> {
//! let input = Input {
//!     text: DocumentText::Single {
//!         text: "# Hello\n\nA paragraph.\n".to_owned(),
//!         origin: Origin::Document("/book".into()),
//!         name: "hello.md".to_owned(),
//!     },
//!     theme: ThemeSelection::Document { dirs: ThemeDirs::default() },
//!     overrides: Overrides::default(),
//!     today: Date::iso("2026-11-04").expect("a valid day"),
//! };
//! let renderer = Renderer::new();
//! let pdf = renderer.render(&MemoryResolver::new(), &input, &PdfOptions::default(), &Cancel::default())?;
//! assert!(pdf.bytes.starts_with(b"%PDF-"));
//! # Ok(())
//! # }
//! ```
//!
//! # Layout, then export
//!
//! A job runs in stages. [`Job::configure`] resolves and checks the configuration, so a configuration error never
//! reaches layout. [`Configured::layout`] returns a [`output::Layout`]: an immutable display list that owns its
//! fonts and images, to inspect or draw. [`output::Layout::pdf`] writes it without shaping or paginating again.
//!
//! ```no_run
//! # use druck::cancel::Cancel;
//! # use druck::input::{Date, DocumentText, Input, Overrides, ThemeDirs, ThemeSelection};
//! # use druck::resource::{MemoryResolver, Origin};
//! # use druck::{LayoutOptions, PdfOptions, Renderer};
//! # fn main() -> Result<(), druck::Failure> {
//! # let input = Input {
//! #     text: DocumentText::Single { text: "# Hello\n".to_owned(), origin: Origin::Document("/book".into()), name: "hello.md".to_owned() },
//! #     theme: ThemeSelection::Document { dirs: ThemeDirs::default() },
//! #     overrides: Overrides::default(),
//! #     today: Date::iso("2026-11-04").expect("a valid day"),
//! # };
//! # let (renderer, resolver, cancel) = (Renderer::new(), MemoryResolver::new(), Cancel::default());
//! let configured = renderer.job(&resolver, &input, &cancel).configure()?;
//! for warning in configured.warnings() {
//!     eprintln!("{warning}");
//! }
//! let layout = configured.layout(&LayoutOptions { source_map: true })?;
//! println!("{} pages", layout.pages().len());
//! let pdf = layout.pdf(&PdfOptions::default(), &cancel)?;
//! # Ok(())
//! # }
//! ```
//!
//! The `examples` folder has six programs that CI runs: `render`, `theme` (a JSON theme and overrides),
//! `in_memory` (a resolver holding the document's resources), `inspect` (the display list and the source map),
//! `cancel`, and `concurrent` (one renderer, four jobs).
//!
//! # Threads
//!
//! The crate starts no threads and needs no async runtime. [`Renderer`], [`output::Layout`], [`input::Input`],
//! [`cancel::Cancel`], and both resolvers are `Send + Sync`: share one renderer and one resolver between jobs on
//! any number of threads. [`Job`] and [`Configured`] are `Send` but not `Sync`, so a job runs on one thread at a
//! time. A layout outlives its job, the input, the resolver, and the renderer.
//!
//! Run about as many jobs at once as the machine has fast cores. On an M1 Pro (8 fast cores), a 203-page book took
//! 0.37 s alone and 0.70 s with four jobs at once, and throughput rose from 2.6 jobs/s on one thread to 5.6 on four
//! and stayed there on eight, where each job took 1.5 s. Budget about 50 MB of live memory per 200 pages in flight,
//! and expect a long-lived process to hold several times that, since the allocator keeps freed memory.
//!
//! Layout reads a deeply nested theme, which a debug build does in up to a megabyte of stack. A thread of 8 MB or
//! more is safe; a Windows main thread or a small pool thread may not be. Release builds need far less.
//!
//! # Resources and reuse
//!
//! Every job reads each resource it uses through the resolver again, because a path does not promise unchanged
//! bytes. A renderer keeps prepared fonts and decoded images by content hash: unchanged bytes are reused,
//! changed bytes are prepared again. What none of the last two jobs used is dropped. A [`resource::Resolver`] is
//! called from several threads at once and must serve a consistent file to each call.
//!
//! # Versions
//!
//! [`API_VERSION`] counts breaking changes to the public modules: a release that keeps it keeps every public
//! signature. The package version is a date and says nothing about compatibility. [`THEME_SCHEMA_VERSIONS`]
//! lists the theme `version` values this release reads, and a theme with another one fails in
//! [`Job::configure`] at its `version` property. The two numbers change independently. The `editor` feature is
//! unstable and outside both.

mod bibliography;
mod citations;
mod config;
mod date;
mod document;
mod epub;
mod highlight;
mod image;
mod index;
mod installed;
mod layout;
mod markdown;
mod pdf;
mod pipeline;
mod project;
mod source_map;
mod spacing;
mod statistics;
mod tags;
mod text;
mod theme_path;

pub mod cancel;
pub mod diagnostic;
pub mod input;
pub mod output;
mod renderer;
pub mod resource;

pub use renderer::*;

/// Bumped on any breaking change to the public modules.
pub const API_VERSION: u32 = 1;

/// The theme schema versions this release accepts.
pub const THEME_SCHEMA_VERSIONS: &[u64] = &[config::theme::THEME_VERSION];

#[cfg(feature = "editor")]
#[doc(hidden)]
pub mod editor;

#[cfg(test)]
mod cancel_tests;
#[cfg(test)]
mod diagnostic_tests;
#[cfg(test)]
mod renderer_tests;
#[cfg(test)]
mod source_map_tests;
