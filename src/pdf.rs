//! PDF output: embeds and subsets fonts, writes text as selectable glyph runs, adds links.

use crate::config::front_matter::Metadata;
use crate::config::theme::Lang;
use crate::page::Page;
use crate::text::Fonts;

/// Writes `pages` as a PDF document. Title, authors, and language go into the document info.
pub fn write(pages: &[Page], fonts: &Fonts, metadata: &Metadata, lang: Lang) -> Result<Vec<u8>, String> {
    let _ = (pages, fonts, metadata, lang);
    todo!("milestone 02 PDF output")
}
