//! Druck's typesetting engine: Markdown and JSON themes in, PDF and EPUB out.
//!
//! [`Renderer`] is the entry point. Build an [`input::Input`], then either call [`Renderer::render`], or go
//! through [`Renderer::job`] to read the warnings and the layout before writing the PDF.
//!
//! `API_VERSION` counts breaking changes to what this page lists. The package version says nothing about
//! compatibility.

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
mod page;
mod pdf;
mod pipeline;
mod project;
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
mod acceptance_tests;
