//! Files compiled into the binary, served to gpui by path.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const FILES: &[(&str, &[u8])] = &[
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
