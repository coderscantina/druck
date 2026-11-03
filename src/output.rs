//! The result of layout.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::config::resolved::Config;
use crate::date::Date;
use crate::diagnostic::Diagnostic;
use crate::page;
use crate::pipeline::{self, DocumentFile};
use crate::renderer::{Failure, PdfOptions, Rendered};
use crate::text::FontSet;

pub use crate::config::values::Pt;

/// A laid out document, ready to export.
pub struct Layout {
    pub(crate) config: Arc<Config>,
    pub(crate) document: Arc<DocumentFile>,
    pub(crate) fonts: FontSet,
    pub(crate) laid: page::Output,
    pub(crate) today: Date,
}

impl Layout {
    /// The typesetting rules the layout breaks, since no better layout was found.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.laid.warnings
    }

    /// Writes the PDF, checked against the standards the document must conform to. The warnings of the
    /// result name fonts whose license restricts embedding. `parts` is the page count.
    pub fn pdf(&self, options: &PdfOptions) -> Result<Rendered, Failure> {
        let print = crate::pdf::Print {
            bleed: options.bleed,
            crop_marks: options.crop_marks,
        };
        // Temporary: the PDF writer still shapes a space per run, until it reads the display list alone.
        let fonts = self.fonts.fonts();
        let bytes = pipeline::write_pdf(&self.config, &self.document, &fonts, &self.laid, print, self.today)?;
        let used: BTreeSet<_> = self
            .laid
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter_map(|item| match item {
                page::Item::Text { run, .. } | page::Item::TurnedText { run, .. } => Some(run.face),
                _ => None,
            })
            .collect();
        let warnings = used
            .into_iter()
            .filter_map(|face| self.fonts.embedding_warning(face))
            .map(|warning| Diagnostic::warning(None, warning))
            .collect();
        Ok(Rendered {
            bytes,
            warnings,
            parts: self.laid.pages.len(),
        })
    }
}
