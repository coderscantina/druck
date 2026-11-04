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
