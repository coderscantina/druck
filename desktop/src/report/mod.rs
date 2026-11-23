//! Errors stay on the machine, and reporting one is a click. [`log`] keeps panics and the errors shown to the author in
//! a file in the app's folder. The Help menu's report opens a new GitHub issue in the browser with the version, the
//! system, and the end of that log, and shows the file in the Finder to attach. Nothing is sent from here.
//!
//! A panic also leaves a marker, so the next launch offers the report once, at the window's foot.

pub mod issue;
pub mod log;
pub mod strings;

use std::fs;
use std::panic;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, actions};

use crate::persist::recovery;

actions!(report, [ReportProblem]);

/// The marker a panic leaves in the data folder.
const MARKER: &str = "panicked";
/// How much of the log goes in the issue, before the address is cut to fit.
const LOG_BYTES: u64 = 6000;
/// A panic message is cut here, since a failed text operation can quote the text it was on.
const MESSAGE_CHARS: usize = 200;

/// The last launch ended in a panic and the notice has not been shown yet.
static PANIC_NOTICE: AtomicBool = AtomicBool::new(false);

/// Starts the log in the app's folder, takes the marker of a panic in the last run, and hooks panics. The default
/// hook runs after ours, so the message still reaches the terminal and the process ends as it would have.
pub fn install() {
    let Some(folder) = recovery::data_dir() else {
        return;
    };
    log::install(&folder);
    PANIC_NOTICE.store(take_marker(&folder), Ordering::Relaxed);
    let default = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let message = payload_text(info.payload());
        let place = info.location().map_or_else(String::new, |at| format!(" at {at}"));
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::record_block(&format!("panic{place}: {}", cut(&message)), &backtrace.to_string());
        fs::create_dir_all(&folder)
            .and_then(|()| fs::write(folder.join(MARKER), ""))
            .ok();
        default(info);
    }));
    #[cfg(debug_assertions)]
    if std::env::var_os("DRUCK_TEST_PANIC").is_some() {
        // A panic away from the window's thread, so a run can look at the log and the notice after it.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(2));
            panic!("a panic asked for by DRUCK_TEST_PANIC");
        });
    }
}

fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown payload".to_owned())
}

/// The message's first line, cut.
fn cut(message: &str) -> String {
    let line = message.lines().next().unwrap_or_default();
    match line.char_indices().nth(MESSAGE_CHARS) {
        Some((end, _)) => format!("{}...", &line[..end]),
        None => line.to_owned(),
    }
}

/// Whether a panic left its marker, which is removed so the notice comes once.
fn take_marker(folder: &Path) -> bool {
    fs::remove_file(folder.join(MARKER)).is_ok()
}

/// Whether the notice about the last run's panic is due. True once per launch.
pub fn take_panic_notice() -> bool {
    PANIC_NOTICE.swap(false, Ordering::Relaxed)
}

/// Records an error the app showed to the author. Pass file names, never text from the book.
pub fn error(message: &str) {
    log::record(&format!("error: {message}"));
}

pub fn install_actions(cx: &mut App) {
    cx.on_action(|_: &ReportProblem, cx| open(cx));
}

/// Opens the browser on a new issue and shows the log in the Finder.
pub fn open(cx: &mut App) {
    let log = log::path();
    let tail = log.map(|path| log::tail(path, LOG_BYTES)).unwrap_or_default();
    let address = issue::url(
        &issue::Facts {
            version: druck::editor::VERSION,
            system: &system(),
        },
        "Problem report",
        &tail,
    );
    #[cfg(debug_assertions)]
    if let Some(file) = std::env::var_os("DRUCK_REPORT_URL_FILE") {
        fs::write(file, address).ok();
        return;
    }
    cx.open_url(&address);
    if let Some(log) = log.filter(|log| log.exists()) {
        cx.reveal_path(log);
    }
}

#[cfg(target_os = "macos")]
fn system() -> String {
    let version = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_default();
    format!("macOS {}", version.trim())
}

#[cfg(not(target_os = "macos"))]
fn system() -> String {
    std::env::consts::OS.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panics_marker_is_taken_once() {
        let folder = std::env::temp_dir().join(format!("druck-marker-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        assert!(!take_marker(&folder));
        fs::write(folder.join(MARKER), "").unwrap();
        assert!(take_marker(&folder));
        assert!(!take_marker(&folder));
        fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_panic_message_is_cut_to_its_first_line_and_a_length() {
        assert_eq!(cut("short\nsecond"), "short");
        let long = "ä".repeat(300);
        assert_eq!(cut(&long), format!("{}...", "ä".repeat(MESSAGE_CHARS)));
    }
}
