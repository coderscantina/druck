//! Unstable internals for `druck-desktop`, behind the `editor` feature. Outside `API_VERSION`: anything here
//! may change in any release, and the desktop, which pins the path dependency, follows.
//!
//! Each item says what the desktop needs it for. Everything else it uses is the public API.

/// Citation syntax: the source layer must tint exactly what the parser reads as a citation.
pub use crate::bibliography::syntax;
/// Comment finding: the source layer must cut a comment out of a block exactly where the parser does.
pub use crate::markdown::comments;
/// Inline mark pairing: the source layer must pair `^x^`, `~x~`, and `{todo:` as the parser does.
pub use crate::markdown::marks;

/// The theme as a JSON value: the theme file code edits and saves partial themes over the bundled default.
pub mod theme_value {
    pub use crate::config::layer::default_theme_value;
    pub use crate::config::merge::merge;
}

/// Where named themes are kept: the theme file code lists, finds, and creates them.
pub mod theme_path {
    pub use crate::theme_path::*;
}

/// The resolved configuration: the theme validator runs `resolve` as a render does, and the preview reads the
/// body style and the document's title and authors from `Config`.
pub mod resolved_config {
    pub use crate::config::front_matter::{FrontMatter, split as split_front_matter};
    pub use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
    pub use crate::config::resolved::Config;
    pub use crate::config::source::InputSource;
    pub use crate::config::theme::{FontStyle, Weight};
    /// The session reads `druck.yaml` and front matter as the engine does, so the app has no second YAML reader.
    pub use crate::pipeline::parse_settings;

    pub fn of<'a>(configured: &'a crate::Configured<'_>) -> &'a Config {
        &configured.config
    }
}

/// Which theme file a selection or a document means: the Styles tab edits that file.
pub mod engine {
    pub use crate::pipeline::{document_theme, theme_file};
}
