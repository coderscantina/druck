//! CommonMark parsing into the [document model](crate::document).

use crate::config::source::Source;
use crate::diagnostic::Diagnostic;
use crate::document::Document;

/// Parses the Markdown body that starts at 1-based line `first_line` of the document `source`.
///
/// Every construct that cannot be rendered yet is reported with its location. The result is
/// either the complete document or every such diagnostic, never a document with content dropped.
pub fn parse(body: &str, first_line: u64, source: &Source) -> Result<Document, Vec<Diagnostic>> {
    let _ = (body, first_line, source);
    todo!("milestone 02 parser")
}
