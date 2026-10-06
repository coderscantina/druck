//! Validation of individual theme layers against the typed theme.

use serde_json::Value;

use super::DEFAULT_THEME;
use super::merge::merge;
use super::theme::{THEME_VERSION, Theme};

/// A problem with one property of a configuration input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyError {
    /// Dotted property path, such as `styles.body.size`. `None` for the whole input.
    pub property: Option<String>,
    pub message: String,
}

impl PropertyError {
    pub fn new(property: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            property: Some(property.into()),
            message: message.into(),
        }
    }
}

/// The bundled default theme as JSON.
pub fn default_theme_value() -> Value {
    serde_json::from_str(DEFAULT_THEME).expect("bundled default theme is valid JSON")
}

/// Deserializes a complete theme value, reporting the offending property path.
pub fn deserialize_theme(value: Value) -> Result<Theme, PropertyError> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let path = error.path().to_string();
        PropertyError {
            property: (path != ".").then_some(path),
            message: error.into_inner().to_string(),
        }
    })
}

/// Checks a theme file's top-level shape and schema version.
pub fn check_theme_header(layer: &Value) -> Result<(), PropertyError> {
    let Value::Object(fields) = layer else {
        return Err(PropertyError {
            property: None,
            message: "a theme must be a JSON object".into(),
        });
    };
    match fields.get("version") {
        None => Err(PropertyError::new(
            "version",
            format!("missing theme version (supported: {THEME_VERSION})"),
        )),
        Some(Value::Number(n)) if n.as_u64() == Some(THEME_VERSION) => Ok(()),
        Some(other) => Err(PropertyError::new(
            "version",
            format!("unsupported theme version {other} (supported: {THEME_VERSION})"),
        )),
    }
}

/// Validates a partial theme on its own: its version, and its values merged onto the
/// bundled default. Cross-references such as tokens are checked after full resolution.
pub fn check_theme_layer(layer: &Value) -> Result<(), PropertyError> {
    check_theme_header(layer)?;
    let mut merged = default_theme_value();
    merge(&mut merged, layer.clone());
    deserialize_theme(merged).map(drop)
}
