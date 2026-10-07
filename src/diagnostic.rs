//! User-facing diagnostics tied to an input and, where known, a location or property.

use std::fmt;

use crate::config::source::Source;

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// The input responsible, if a single one is.
    pub source: Option<Source>,
    /// 1-based line and column in the source file.
    pub location: Option<(u64, u64)>,
    /// Dotted property path, such as `styles.body.size`.
    pub property: Option<String>,
    pub message: String,
    /// A warning does not stop the run.
    pub warning: bool,
}

impl Diagnostic {
    pub fn new(source: Option<Source>, message: impl Into<String>) -> Self {
        Self {
            source,
            location: None,
            property: None,
            message: message.into(),
            warning: false,
        }
    }

    pub fn warning(source: Option<Source>, message: impl Into<String>) -> Self {
        Self {
            warning: true,
            ..Self::new(source, message)
        }
    }

    pub fn at(mut self, line: u64, column: u64) -> Self {
        self.location = Some((line, column));
        self
    }

    pub fn property(mut self, property: Option<String>) -> Self {
        self.property = property;
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.warning { "warning: " } else { "error: " })?;
        if let Some(source) = &self.source {
            write!(f, "{source}")?;
            if let Some((line, column)) = self.location {
                write!(f, ":{line}:{column}")?;
            }
            f.write_str(": ")?;
        }
        if let Some(property) = &self.property {
            write!(f, "{property}: ")?;
        }
        f.write_str(&self.message)
    }
}
