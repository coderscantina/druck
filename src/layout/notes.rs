//! The list, quotation, and code styles inside footnotes: the theme's, scaled by the footnote size over the body size.

use std::collections::HashMap;

use crate::config::resolved::{Config, CustomStyle, Style};
use crate::config::values::Pt;

/// Styles and list metrics for footnote content.
pub(super) struct NoteStyles {
    pub list: Style,
    pub code_block: Style,
    pub quote: Style,
    /// The scaled styles of custom styles based on `list`, by name.
    pub custom_lists: HashMap<String, Style>,
    pub list_indent: Pt,
    pub item_spacing: Pt,
}

impl NoteStyles {
    pub fn new(config: &Config) -> Self {
        let ratio = config.styles.footnote.size.0 / config.styles.body.size.0;
        let scale = |length: Pt| Pt(length.0 * ratio);
        let scaled = |style: &Style| Style {
            size: scale(style.size),
            space_before: scale(style.space_before),
            space_after: scale(style.space_after),
            indent: scale(style.indent),
            first_line_indent: scale(style.first_line_indent),
            ..style.clone()
        };
        Self {
            list: scaled(&config.styles.list),
            code_block: scaled(&config.styles.code_block),
            quote: scaled(&config.styles.quote),
            custom_lists: config
                .custom_styles
                .iter()
                .filter(|(_, custom)| matches!(custom, CustomStyle::List { .. }))
                .map(|(name, custom)| (name.clone(), scaled(custom.style())))
                .collect(),
            list_indent: scale(config.lists.indent),
            item_spacing: scale(config.lists.item_spacing),
        }
    }
}
