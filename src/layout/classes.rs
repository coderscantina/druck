//! Checks the custom styles that `{.name}` attributes apply, before layout relies on them.

use crate::config::resolved::{BlockKind, Config};
use crate::config::source::Source;
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Class, Document};

/// Reports, at the attribute, each style name the theme does not define and each style applied to a
/// kind of block it is not for. A style's kind follows its built-in base: headings, lists, or paragraphs.
/// `{.no-drop-cap}` on a paragraph is no theme style; it turns off a chapter's drop capital and lead-in.
pub(super) fn check(document: &Document, config: &Config, source: &Source) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut visit = |class: &Class, kind: BlockKind| {
        let message = match config.custom_styles.get(&class.name) {
            Some(style) if style.kind() == kind => return,
            Some(style) => format!(
                "style \"{}\" is for {} and cannot style {kind}",
                class.name,
                style.kind()
            ),
            None if config.custom_styles.is_empty() => {
                format!("unknown style \"{}\"; the theme defines no custom styles", class.name)
            }
            None => {
                let names: Vec<_> = config.custom_styles.keys().map(String::as_str).collect();
                format!(
                    "unknown style \"{}\"; the theme's custom styles are {}",
                    class.name,
                    names.join(", ")
                )
            }
        };
        errors.push(Diagnostic::new(Some(source.clone()), message).at(class.at.line, class.at.column));
    };
    walk(&document.blocks, &mut visit);
    for footnote in &document.footnotes {
        walk(&footnote.blocks, &mut visit);
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn walk(blocks: &[Block], visit: &mut impl FnMut(&Class, BlockKind)) {
    for block in blocks {
        match block {
            Block::Paragraph { class, .. } => class
                .iter()
                .filter(|class| class.name != super::NO_DROP_CAP)
                .for_each(|class| visit(class, BlockKind::Paragraph)),
            Block::Heading { class, .. } => class.iter().for_each(|class| visit(class, BlockKind::Heading)),
            Block::List { class, items, .. } => {
                class.iter().for_each(|class| visit(class, BlockKind::List));
                items.iter().for_each(|item| walk(item, visit));
            }
            Block::Table { header, rows, .. } => {
                for row in std::iter::once(header).chain(rows) {
                    row.class.iter().for_each(|class| visit(class, BlockKind::Paragraph));
                    for cell in &row.cells {
                        cell.class.iter().for_each(|class| visit(class, BlockKind::Paragraph));
                        walk(&cell.blocks, visit);
                    }
                }
            }
            Block::Quote { blocks, .. }
            | Block::Keep { blocks, .. }
            | Block::Columns { blocks, .. }
            | Block::FullWidth { blocks, .. } => walk(blocks, visit),
            _ => {}
        }
    }
}
