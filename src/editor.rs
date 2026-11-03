//! Unstable internals for `druck-desktop`, behind the `editor` feature. Outside `API_VERSION`: anything here
//! may change in any release, and the desktop, which pins the path dependency, follows.

use std::path::Path;

pub use crate::bibliography::syntax;
pub use crate::markdown::marks;

/// The theme as a JSON value, for the theme editor.
pub mod theme_value {
    pub use crate::config::layer::default_theme_value;
    pub use crate::config::merge::merge;
}

/// Finding themes by name.
pub mod theme_path {
    pub use crate::theme_path::*;
}

/// The resolved configuration, whose fields the inspector and the galley read.
pub mod resolved_config {
    pub use crate::config::front_matter::FrontMatter;
    pub use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
    pub use crate::config::resolved::{Config, FontFiles};
    pub use crate::config::theme::{Face, FontStyle, Lang, Weight};
    pub use crate::config::values::Color;

    pub fn of<'a>(configured: &'a crate::Configured<'_>) -> &'a Config {
        &configured.config
    }
}

/// The document's absolute path and its text.
pub fn document_text<'a>(configured: &'a crate::Configured<'_>) -> (&'a Path, &'a str) {
    (&configured.document.path, &configured.document.text)
}

/// Page and glyph data for the desktop painter. Temporary: `Layout`, `Face`, and `Picture` replace all of this once
/// the display list is public.
pub mod engine {
    pub use crate::image::{Image, Pixels};
    pub use crate::page::{Item, Output, Page, Rect};
    pub use crate::pipeline::{document_theme, theme_file};
    pub use crate::text::{FaceId, FontSet, Fonts, ShapedRun};

    /// A layout's pages and the fonts its runs name.
    pub fn into_parts(layout: crate::output::Layout) -> (Output, FontSet) {
        (layout.laid, layout.fonts)
    }
}
