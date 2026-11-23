//! The app's log: one file in the app's data folder, `druck.log`, that never leaves the machine on its own. Panics and
//! the errors the app shows to the author go in. Text from the book never does, only file names.
//!
//! The file is bounded: once it passes [`MAX_BYTES`] the next line starts a new one, and the full one becomes
//! `druck.log.1`, replacing an older. Nothing is written until [`install`] names the folder, so tests and the tools
//! that start this executable again leave no file.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const FILE: &str = "druck.log";
const OLD_FILE: &str = "druck.log.1";
/// Past this size the file is rotated.
const MAX_BYTES: u64 = 256 * 1024;

static LOG: OnceLock<PathBuf> = OnceLock::new();

/// Names the folder the log lives in, once. Later calls change nothing.
pub fn install(folder: &Path) {
    LOG.set(folder.join(FILE)).ok();
}

/// The log file, once installed.
pub fn path() -> Option<&'static Path> {
    LOG.get().map(PathBuf::as_path)
}

fn stamp() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S %:z").to_string()
}

/// Appends one line, stamped with the local time. A message of several lines becomes one. A log that cannot be written
/// is not worth a second error, so a failure is dropped.
pub fn record(message: &str) {
    if let Some(path) = path() {
        let line = message.split_whitespace().collect::<Vec<_>>().join(" ");
        append(path, &format!("{} {line}", stamp()), MAX_BYTES).ok();
    }
}

/// Appends a stamped heading and a block of lines under it as they are. For a panic's backtrace.
pub fn record_block(heading: &str, body: &str) {
    if let Some(path) = path() {
        append(path, &format!("{} {heading}\n{}", stamp(), body.trim_end()), MAX_BYTES).ok();
    }
}

fn append(path: &Path, text: &str, max: u64) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    if fs::metadata(path).is_ok_and(|meta| meta.len() >= max) {
        fs::rename(path, path.with_file_name(OLD_FILE))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{text}")
}

/// The last `bytes` bytes of the log, from the start of a line, for the report. Empty when there is no log.
pub fn tail(path: &Path, bytes: u64) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |meta| meta.len());
    let start = length.saturating_sub(bytes);
    let mut text = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut text).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&text);
    if start == 0 {
        return text.into_owned();
    }
    text.split_once('\n')
        .map_or_else(String::new, |(_, rest)| rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("druck-log-{name}-{}", std::process::id()));
        fs::remove_dir_all(&folder).ok();
        folder
    }

    #[test]
    fn a_full_log_becomes_the_old_one_and_the_next_line_starts_a_new_log() {
        let folder = scratch("rotate");
        let path = folder.join(FILE);
        append(&path, "first line", 20).unwrap();
        append(&path, "second line", 20).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first line\nsecond line\n");
        append(&path, "third line", 20).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "third line\n");
        assert_eq!(
            fs::read_to_string(folder.join(OLD_FILE)).unwrap(),
            "first line\nsecond line\n"
        );
        append(&path, "fourth line that is long", 20).unwrap();
        append(&path, "fifth", 20).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "fifth\n");
        assert_eq!(
            fs::read_to_string(folder.join(OLD_FILE)).unwrap(),
            "third line\nfourth line that is long\n"
        );
        fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_tail_starts_at_a_line_and_the_whole_log_comes_when_it_fits() {
        let folder = scratch("tail");
        let path = folder.join(FILE);
        append(&path, "one", MAX_BYTES).unwrap();
        append(&path, "two", MAX_BYTES).unwrap();
        append(&path, "three", MAX_BYTES).unwrap();
        assert_eq!(tail(&path, 100), "one\ntwo\nthree\n");
        assert_eq!(tail(&path, 10), "three\n");
        assert_eq!(tail(&folder.join("none"), 10), "");
        fs::remove_dir_all(&folder).ok();
    }
}
