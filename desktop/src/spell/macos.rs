//! `NSSpellChecker`, with each paragraph checked in its own language.
//!
//! The checker's shared `language` is read when AppKit works through a request, not when the request is made, and a call
//! for suggestions in another language switches it for a moment, both measured here. So this checker never sets it: every
//! call names its language, through `checkSpellingOfString:startingAt:language:...` and
//! `guessesForWordRange:inString:language:...`. Even then a call made while another thread uses the checker can come back
//! in the wrong language, so every call into it takes [`CHECKER`]. The checks are synchronous and run on a thread of their
//! own, so the main thread only waits on the lock for a suggestion while one paragraph is checked.
//!
//! A paragraph's language comes from `NLLanguageRecognizer`, limited to the book's language and the system's preferred
//! languages, since unlimited it reads "The Bundestag met." as Dutch. Short paragraphs and unsure answers keep the book's.
//!
//! The word list is not given to the checker: the system's learned words stay untouched, and the galley drops the project's
//! words from the results itself.

use std::ops::Range;
use std::sync::Mutex;
use std::sync::mpsc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{NSArray, NSDictionary, NSLocale, NSNumber, NSRange, NSString};

use super::{Checker, Done};

#[link(name = "NaturalLanguage", kind = "framework")]
unsafe extern "C" {}

/// Held for every call into `NSSpellChecker`.
static CHECKER: Mutex<()> = Mutex::new(());

/// A paragraph with fewer words keeps the book's language.
const FEWEST_WORDS: usize = 4;
/// How sure the recognizer must be to check a paragraph in another language than the book's.
const SURE: f64 = 0.9;

struct Job {
    text: String,
    language: String,
    done: Done,
}

pub struct SystemChecker {
    jobs: mpsc::Sender<Job>,
    /// One document tag for the app's one window, so the checker keeps nothing between unrelated requests of ours.
    tag: isize,
}

impl SystemChecker {
    pub fn new() -> Self {
        let tag = {
            let _held = lock();
            NSSpellChecker::uniqueSpellDocumentTag()
        };
        let (jobs, inbox) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("spelling".to_owned())
            .spawn(move || {
                let recognizer = Recognizer::new();
                for job in inbox {
                    let language = recognizer.paragraph_language(&job.text, &job.language);
                    (job.done)(misspelled(&job.text, &language, tag));
                }
            })
            .expect("a thread for spelling");
        Self { jobs, tag }
    }
}

impl Checker for SystemChecker {
    fn check(&self, text: String, language: &str, done: Done) {
        let job = Job {
            text,
            language: language.to_owned(),
            done,
        };
        // The thread ends only when this checker is dropped.
        self.jobs.send(job).ok();
    }

    fn suggestions(&self, word: &str, paragraph: &str, language: &str) -> Vec<String> {
        let language = Recognizer::new().paragraph_language(paragraph, language);
        let string = NSString::from_str(word);
        let range = NSRange::new(0, string.length());
        let _held = lock();
        NSSpellChecker::sharedSpellChecker()
            .guessesForWordRange_inString_language_inSpellDocumentWithTag(
                range,
                &string,
                Some(&NSString::from_str(&language)),
                self.tag,
            )
            .map(|guesses| guesses.iter().map(|guess| guess.to_string()).collect())
            .unwrap_or_default()
    }
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    CHECKER.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The misspelled words of `text` in `language`, as byte ranges.
fn misspelled(text: &str, language: &str, tag: isize) -> Vec<Range<usize>> {
    let string = NSString::from_str(text);
    let language = NSString::from_str(language);
    let mut found = Vec::new();
    let mut from = 0;
    let _held = lock();
    let checker = NSSpellChecker::sharedSpellChecker();
    loop {
        // SAFETY: a null word count is allowed.
        let range = unsafe {
            checker.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                &string,
                from,
                Some(&language),
                false,
                tag,
                std::ptr::null_mut(),
            )
        };
        if range.length == 0 {
            break;
        }
        found.push(range);
        from = (range.location + range.length) as isize;
    }
    drop(_held);
    byte_ranges(text, found)
}

/// `NLLanguageRecognizer`, kept by one thread.
struct Recognizer {
    recognizer: Retained<AnyObject>,
    /// The base languages of the system's preferred languages.
    system: Vec<String>,
}

impl Recognizer {
    fn new() -> Self {
        // SAFETY: `new` on an `NSObject` subclass gives a retained object.
        let recognizer: Retained<AnyObject> = unsafe { msg_send![class!(NLLanguageRecognizer), new] };
        let system = NSLocale::preferredLanguages()
            .iter()
            .map(|language| base(&language.to_string()).to_owned())
            .collect();
        Self { recognizer, system }
    }

    /// The language to check a paragraph in: the one it is written in where it has [`FEWEST_WORDS`] and the
    /// recognizer is [`SURE`], else the book's.
    fn paragraph_language(&self, text: &str, book: &str) -> String {
        let candidates = candidates(book, &self.system);
        if candidates.len() < 2 || words(text) < FEWEST_WORDS {
            return book.to_owned();
        }
        let best = self.likeliest(text, &candidates);
        chosen(book, best.as_ref().map(|(language, sure)| (language.as_str(), *sure)))
    }

    /// The likeliest of `candidates` for `text`, and how sure the recognizer is.
    fn likeliest(&self, text: &str, candidates: &[String]) -> Option<(String, f64)> {
        let constraints: Vec<Retained<NSString>> = candidates.iter().map(|code| NSString::from_str(code)).collect();
        let constraints = NSArray::from_retained_slice(&constraints);
        let recognizer = &*self.recognizer;
        // SAFETY: the methods and argument types are `NLLanguageRecognizer`'s, `NLLanguage` being an `NSString`.
        let hypotheses: Retained<NSDictionary<NSString, NSNumber>> = unsafe {
            let _: () = msg_send![recognizer, reset];
            let _: () = msg_send![recognizer, setLanguageConstraints: &*constraints];
            let _: () = msg_send![recognizer, processString: &*NSString::from_str(text)];
            msg_send![recognizer, languageHypothesesWithMaximum: 1usize]
        };
        let (languages, chances) = hypotheses.to_vecs();
        languages
            .first()
            .zip(chances.first())
            .map(|(language, chance)| (language.to_string(), chance.doubleValue()))
    }
}

/// The book's base language first, then the system's others, each once.
fn candidates(book: &str, system: &[String]) -> Vec<String> {
    let mut candidates = vec![base(book).to_lowercase()];
    for language in system {
        let language = language.to_lowercase();
        if !candidates.contains(&language) {
            candidates.push(language);
        }
    }
    candidates
}

/// The recognizer's answer where it is sure of another language than the book's, the book's language as given otherwise,
/// so a region such as `de-AT` stays.
fn chosen(book: &str, best: Option<(&str, f64)>) -> String {
    match best {
        Some((language, sure)) if sure >= SURE && !base(language).eq_ignore_ascii_case(base(book)) => {
            language.to_owned()
        }
        _ => book.to_owned(),
    }
}

/// `de` of `de-AT` or `de_AT`.
fn base(language: &str) -> &str {
    language.split(['-', '_']).next().unwrap_or(language)
}

fn words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|word| word.chars().any(char::is_alphabetic))
        .count()
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
    use std::time::Duration;

    use super::*;

    fn check(checker: &SystemChecker, text: &str, language: &str) -> Vec<String> {
        let (tx, rx) = mpsc::channel();
        checker.check(
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

    fn misspelled(text: &str, language: &str) -> Vec<String> {
        check(&SystemChecker::new(), text, language)
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
    fn a_paragraph_in_another_language_is_checked_in_that_language() {
        let english = "He walked home through the quiet streets and teh rain.";
        let candidates = candidates("de-AT", &["en".to_owned()]);
        assert_eq!(candidates, ["de", "en"]);
        let recognizer = Recognizer {
            system: vec!["en".to_owned()],
            ..Recognizer::new()
        };
        assert_eq!(recognizer.paragraph_language(english, "de-AT"), "en");
        assert_eq!(
            recognizer.paragraph_language("Er ging durch die stillen Straßen nach Hause.", "de-AT"),
            "de-AT"
        );
        assert_eq!(recognizer.paragraph_language("The house.", "de"), "de");
    }

    #[test]
    fn the_recognizer_decides_only_when_sure_of_another_language() {
        assert_eq!(chosen("de-AT", Some(("en", 0.99))), "en");
        assert_eq!(chosen("de-AT", Some(("en", 0.6))), "de-AT");
        assert_eq!(chosen("de-AT", Some(("de", 0.99))), "de-AT");
        assert_eq!(chosen("en_GB", None), "en_GB");
    }

    #[test]
    fn checks_keep_their_language_while_suggestions_ask_in_another() {
        let checker = SystemChecker::new();
        let asking = std::thread::spawn(|| {
            let checker = SystemChecker::new();
            for _ in 0..40 {
                checker.suggestions("Müdigkiet", "Die Müdigkiet kam am Abend.", "de");
            }
        });
        for _ in 0..40 {
            assert_eq!(check(&checker, "The house and teh function.", "en"), ["teh"]);
        }
        asking.join().expect("suggestions asked");
    }

    #[test]
    fn suggestions_come_in_the_paragraphs_language() {
        let checker = SystemChecker::new();
        let guesses = checker.suggestions("Müdigkiet", "Müdigkiet", "de");
        assert!(guesses.iter().any(|guess| guess == "Müdigkeit"), "{guesses:?}");
    }
}
