//! `NSSpellChecker`. AppKit's header documents one background path, `requestCheckingOfString:...`: it takes the request,
//! does the work elsewhere, and calls the completion handler "in an arbitrary context". It says nothing about calling the
//! synchronous methods off the main thread, so the requests and the suggestions are made on the main thread, where they
//! cost little, and all checking happens on AppKit's own background queue.
//!
//! That path has no language argument, and an orthography in its options is ignored while the checker identifies languages
//! by itself, tried and measured here. So this process's checker has identification off and the document's language set,
//! which the header discourages only in favour of identification. The setting is not kept after the app quits. English
//! inside a German book counts as misspelled, which the project's word list answers.
//!
//! The word list is not given to the checker: the system's learned words stay untouched, and the galley drops the project's
//! words from the results itself.

use std::cell::Cell;
use std::ops::Range;
use std::sync::Mutex;

use block2::RcBlock;
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{NSArray, NSOrthography, NSRange, NSString, NSTextCheckingResult, NSTextCheckingTypes};

use super::{Checker, Done};

/// `NSTextCheckingTypeSpelling`.
const SPELLING: NSTextCheckingTypes = 1 << 1;

pub struct SystemChecker {
    tag: Cell<Option<isize>>,
    /// The language the checker is set to.
    language: std::cell::RefCell<String>,
}

impl SystemChecker {
    pub fn new() -> Self {
        Self {
            tag: Cell::new(None),
            language: std::cell::RefCell::default(),
        }
    }

    fn set_language(&self, checker: &NSSpellChecker, language: &str) {
        let mut set = self.language.borrow_mut();
        if *set != language {
            checker.setAutomaticallyIdentifiesLanguages(false);
            checker.setLanguage(&NSString::from_str(language));
            language.clone_into(&mut set);
        }
    }

    /// One document tag for the app's one window, so the checker keeps nothing between unrelated requests of ours.
    fn tag(&self) -> isize {
        self.tag.get().unwrap_or_else(|| {
            let tag = NSSpellChecker::uniqueSpellDocumentTag();
            self.tag.set(Some(tag));
            tag
        })
    }
}

impl Checker for SystemChecker {
    fn check(&self, text: String, language: &str, done: Done) {
        let checker = NSSpellChecker::sharedSpellChecker();
        self.set_language(&checker, language);
        let string = NSString::from_str(&text);
        let range = NSRange::new(0, string.length());
        // The handler is a `Fn`, and AppKit calls it once.
        let done = Mutex::new(Some((done, text)));
        let handler = RcBlock::new(
            move |_: isize,
                  results: std::ptr::NonNull<NSArray<NSTextCheckingResult>>,
                  _: std::ptr::NonNull<NSOrthography>,
                  _: isize| {
                let Some((done, text)) = done.lock().ok().and_then(|mut done| done.take()) else {
                    return;
                };
                // SAFETY: AppKit passes a valid array for the duration of the call.
                let results = unsafe { results.as_ref() };
                let ranges: Vec<NSRange> = results.iter().map(|result| result.range()).collect();
                done(byte_ranges(&text, ranges));
            },
        );
        // SAFETY: no options, so there is no generic to get wrong.
        unsafe {
            checker.requestCheckingOfString_range_types_options_inSpellDocumentWithTag_completionHandler(
                &string,
                range,
                SPELLING,
                None,
                self.tag(),
                Some(&handler),
            );
        }
    }

    fn suggestions(&self, word: &str, language: &str) -> Vec<String> {
        let string = NSString::from_str(word);
        let range = NSRange::new(0, string.length());
        NSSpellChecker::sharedSpellChecker()
            .guessesForWordRange_inString_language_inSpellDocumentWithTag(
                range,
                &string,
                Some(&NSString::from_str(language)),
                self.tag(),
            )
            .map(|guesses| guesses.iter().map(|guess| guess.to_string()).collect())
            .unwrap_or_default()
    }
}

/// Turns the checker's UTF-16 ranges, in rising order, into byte ranges of `text`.
fn byte_ranges(text: &str, ranges: Vec<NSRange>) -> Vec<Range<usize>> {
    let mut found = Vec::with_capacity(ranges.len());
    let mut chars = text.char_indices();
    let (mut utf16, mut byte) = (0, 0);
    let mut advance_to = |target: usize| {
        while utf16 < target {
            match chars.next() {
                Some((at, character)) => {
                    utf16 += character.len_utf16();
                    byte = at + character.len_utf8();
                }
                None => break,
            }
        }
        byte
    };
    for range in ranges {
        let start = advance_to(range.location);
        let end = advance_to(range.location + range.length);
        found.push(start..end);
    }
    found
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// The checker's language is the process's, so tests take turns.
    static TURN: Mutex<()> = Mutex::new(());

    fn misspelled(text: &str, language: &str) -> Vec<String> {
        let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (tx, rx) = mpsc::channel();
        SystemChecker::new().check(
            text.to_owned(),
            language,
            Box::new(move |ranges| {
                tx.send(ranges).ok();
            }),
        );
        let ranges = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the checker answers off the main thread");
        ranges.into_iter().map(|range| text[range].to_owned()).collect()
    }

    #[test]
    fn utf16_ranges_become_byte_ranges_past_umlauts_and_emoji() {
        let text = "Größe 😀 Fehhler";
        let ranges = byte_ranges(text, vec![NSRange::new(0, 5), NSRange::new(9, 7)]);
        assert_eq!(&text[ranges[0].clone()], "Größe");
        assert_eq!(&text[ranges[1].clone()], "Fehhler");
    }

    #[test]
    fn german_finds_typos_and_leaves_umlauts_and_common_compounds_alone() {
        let found = misspelled(
            "Das Gärtnerhäuschen und die Größe sind richtig, Müdigkiet und Fehhler nicht.",
            "de",
        );
        assert_eq!(found, ["Müdigkiet", "Fehhler"]);
    }

    #[test]
    fn the_documents_language_decides_not_the_texts() {
        assert_eq!(misspelled("The house and teh function.", "en"), ["teh"]);
        assert!(misspelled("Das Haus und die Größe.", "en").contains(&"Größe".to_owned()));
        assert!(misspelled("The house is fine.", "de").contains(&"house".to_owned()));
    }

    #[test]
    fn suggestions_come_in_the_language_asked_for() {
        let guesses = SystemChecker::new().suggestions("Müdigkiet", "de");
        assert!(guesses.iter().any(|guess| guess == "Müdigkeit"), "{guesses:?}");
    }
}
