//! The spell checker behind a small seam: the system's on macOS, none elsewhere until a platform has one. What it checks
//! and how the galley shows it is in `editor/spelling.rs`, and the project's own words are in [`words`].

#[cfg(target_os = "macos")]
mod macos;
pub mod words;

use std::ops::Range;
use std::rc::Rc;

/// Called with the misspelled words' byte ranges in the text, on any thread.
pub type Done = Box<dyn FnOnce(Vec<Range<usize>>) + Send>;

/// A platform's spell checker. `language` is the document's `lang`.
pub trait Checker {
    /// Checks `text` off the calling thread and calls `done` when it knows. A language the system has no dictionary for
    /// finds nothing.
    fn check(&self, text: String, language: &str, done: Done);

    /// What the word might have been, best first. Asked for when the writer asks, on the main thread.
    fn suggestions(&self, word: &str, language: &str) -> Vec<String>;
}

/// No spell checker: nothing is ever misspelled.
#[cfg(not(target_os = "macos"))]
pub struct NoChecker;

#[cfg(not(target_os = "macos"))]
impl Checker for NoChecker {
    fn check(&self, _: String, _: &str, done: Done) {
        done(Vec::new());
    }

    fn suggestions(&self, _: &str, _: &str) -> Vec<String> {
        Vec::new()
    }
}

/// The spell checker of this platform.
pub fn system() -> Rc<dyn Checker> {
    #[cfg(target_os = "macos")]
    {
        Rc::new(macos::SystemChecker::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Rc::new(NoChecker)
    }
}
