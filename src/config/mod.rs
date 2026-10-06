//! Configuration: theme, document settings, and override resolution.

pub mod front_matter;
pub mod layer;
pub mod merge;
pub mod resolve;
pub mod resolved;
#[cfg(test)]
mod schema_tests;
pub mod source;
pub mod template;
pub mod theme;
pub mod values;

use serde::{Deserialize, Deserializer};

/// The bundled default theme. It is complete: every theme field has a value.
pub const DEFAULT_THEME: &str = include_str!("../../themes/default.json");

/// Deserializes an optional field that may be omitted but not set to `null`.
pub fn non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
