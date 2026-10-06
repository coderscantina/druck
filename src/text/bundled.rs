//! Font files compiled into the binary, looked up by the resource path the default theme uses.

macro_rules! bundled {
    ($($file:literal),* $(,)?) => {
        /// The bytes of a bundled font, or `None` for an unknown path.
        pub fn font(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("fonts/", $file) => Some(include_bytes!(concat!("../../fonts/", $file))),)*
                _ => None,
            }
        }
    };
}

bundled!(
    "LibertinusSerif-Regular.otf",
    "LibertinusSerif-Italic.otf",
    "LibertinusSerif-Bold.otf",
    "LibertinusSerif-BoldItalic.otf",
    "LibertinusMono-Regular.otf",
);
