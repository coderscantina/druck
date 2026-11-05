//! Files compiled into the binary, served to gpui by path.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const FILES: &[(&str, &[u8])] = &[
    ("icons/book.svg", include_bytes!("../assets/icons/book.svg")),
    ("icons/close.svg", include_bytes!("../assets/icons/close.svg")),
    ("icons/contents.svg", include_bytes!("../assets/icons/contents.svg")),
    ("icons/file.svg", include_bytes!("../assets/icons/file.svg")),
    ("icons/more.svg", include_bytes!("../assets/icons/more.svg")),
    ("icons/plus.svg", include_bytes!("../assets/icons/plus.svg")),
    ("icons/title.svg", include_bytes!("../assets/icons/title.svg")),
    ("icons/details.svg", include_bytes!("../assets/icons/details.svg")),
    (
        "icons/diagnostics.svg",
        include_bytes!("../assets/icons/diagnostics.svg"),
    ),
    ("icons/export.svg", include_bytes!("../assets/icons/export.svg")),
    ("icons/fonts.svg", include_bytes!("../assets/icons/fonts.svg")),
    ("icons/images.svg", include_bytes!("../assets/icons/images.svg")),
    ("icons/selection.svg", include_bytes!("../assets/icons/selection.svg")),
    ("icons/styles.svg", include_bytes!("../assets/icons/styles.svg")),
    (
        "icons/chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    (
        "icons/chevron-left.svg",
        include_bytes!("../assets/icons/chevron-left.svg"),
    ),
    (
        "icons/chevron-right.svg",
        include_bytes!("../assets/icons/chevron-right.svg"),
    ),
    (
        "icons/block-break.svg",
        include_bytes!("../assets/icons/block-break.svg"),
    ),
    (
        "icons/block-bullets.svg",
        include_bytes!("../assets/icons/block-bullets.svg"),
    ),
    (
        "icons/block-callout.svg",
        include_bytes!("../assets/icons/block-callout.svg"),
    ),
    ("icons/block-code.svg", include_bytes!("../assets/icons/block-code.svg")),
    (
        "icons/block-comment.svg",
        include_bytes!("../assets/icons/block-comment.svg"),
    ),
    (
        "icons/block-directive.svg",
        include_bytes!("../assets/icons/block-directive.svg"),
    ),
    (
        "icons/block-footnote.svg",
        include_bytes!("../assets/icons/block-footnote.svg"),
    ),
    ("icons/block-h1.svg", include_bytes!("../assets/icons/block-h1.svg")),
    ("icons/block-h2.svg", include_bytes!("../assets/icons/block-h2.svg")),
    ("icons/block-h3.svg", include_bytes!("../assets/icons/block-h3.svg")),
    ("icons/block-h4.svg", include_bytes!("../assets/icons/block-h4.svg")),
    ("icons/block-h5.svg", include_bytes!("../assets/icons/block-h5.svg")),
    ("icons/block-h6.svg", include_bytes!("../assets/icons/block-h6.svg")),
    ("icons/block-meta.svg", include_bytes!("../assets/icons/block-meta.svg")),
    (
        "icons/block-numbers.svg",
        include_bytes!("../assets/icons/block-numbers.svg"),
    ),
    (
        "icons/block-quote.svg",
        include_bytes!("../assets/icons/block-quote.svg"),
    ),
    ("icons/block-raw.svg", include_bytes!("../assets/icons/block-raw.svg")),
    (
        "icons/block-table.svg",
        include_bytes!("../assets/icons/block-table.svg"),
    ),
    ("icons/block-text.svg", include_bytes!("../assets/icons/block-text.svg")),
    ("icons/mark-bold.svg", include_bytes!("../assets/icons/mark-bold.svg")),
    (
        "icons/mark-italic.svg",
        include_bytes!("../assets/icons/mark-italic.svg"),
    ),
    ("icons/mark-link.svg", include_bytes!("../assets/icons/mark-link.svg")),
    (
        "icons/mark-smallcaps.svg",
        include_bytes!("../assets/icons/mark-smallcaps.svg"),
    ),
    (
        "icons/mark-strike.svg",
        include_bytes!("../assets/icons/mark-strike.svg"),
    ),
    (
        "icons/mark-subscript.svg",
        include_bytes!("../assets/icons/mark-subscript.svg"),
    ),
    (
        "icons/mark-superscript.svg",
        include_bytes!("../assets/icons/mark-superscript.svg"),
    ),
    (
        "icons/mark-underline.svg",
        include_bytes!("../assets/icons/mark-underline.svg"),
    ),
    ("icons/search.svg", include_bytes!("../assets/icons/search.svg")),
    ("icons/agent.svg", include_bytes!("../assets/icons/agent.svg")),
    (
        "icons/block-reference.svg",
        include_bytes!("../assets/icons/block-reference.svg"),
    ),
    (
        "icons/align-default.svg",
        include_bytes!("../assets/icons/align-default.svg"),
    ),
    ("icons/align-left.svg", include_bytes!("../assets/icons/align-left.svg")),
    (
        "icons/align-center.svg",
        include_bytes!("../assets/icons/align-center.svg"),
    ),
    (
        "icons/align-right.svg",
        include_bytes!("../assets/icons/align-right.svg"),
    ),
    (
        "icons/align-justify.svg",
        include_bytes!("../assets/icons/align-justify.svg"),
    ),
    ("icons/pencil.svg", include_bytes!("../assets/icons/pencil.svg")),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::new_static(name))
            .collect())
    }
}
