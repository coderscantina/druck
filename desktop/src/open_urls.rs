//! What macOS hands the app when a document is opened with it: a double click in the Finder, a drop on the Dock icon,
//! or Open With. They arrive as `file://` URLs.

use std::path::PathBuf;

/// The paths of the file URLs in `urls`. Any other kind of URL is not a document.
pub fn paths(urls: &[String]) -> Vec<PathBuf> {
    urls.iter().filter_map(|url| path_of(url)).collect()
}

fn path_of(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // A host may stand before the path, `localhost` being the one a Mac writes.
    let path = rest.find('/').map(|slash| &rest[slash..])?;
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' => {
                let hex = path.get(at + 1..at + 3)?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                at += 3;
            }
            byte => {
                decoded.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(decoded).ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_become_paths_with_their_escapes_undone() {
        let urls = [
            "file:///Users/me/My%20Book/k%C3%A4se.md".to_owned(),
            "file://localhost/tmp/a.md".to_owned(),
            "https://example.com/a.md".to_owned(),
            "file:///broken%2".to_owned(),
        ];
        assert_eq!(
            paths(&urls),
            [PathBuf::from("/Users/me/My Book/käse.md"), PathBuf::from("/tmp/a.md")]
        );
    }
}
