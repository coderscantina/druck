//! The trial of a fix: the book is laid out with the fix written into a copy in memory, and the warnings before and
//! after say whether it is kept. Nothing is written anywhere. A [`Bench`] holds the copy with every fix kept so far, so
//! each fix is tried on top of the ones before it. Holds no gpui types and asks no agent, so a search without one can
//! call it as it is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use druck::cancel::Cancel;
use druck::diagnostic::{Code, Diagnostic, Source};
use druck::output::Rect;
use druck::resource::normalize;

use super::fix::{self, Fix};
use crate::preview::job::{Layout, Typesetter};
use crate::preview::proof;
use crate::session::{Joined, Kind, Session};
use crate::source::{BlockKind, Blocks, Edit};
use crate::theme_file::patch;

/// The book a trial works on: its text and its theme file's text, as they were when the trial began.
#[derive(Clone)]
pub struct Book {
    pub kind: Kind,
    pub joined: Joined,
    /// The theme file the book names, with its text. `None` for the built-in theme, which has no value to change.
    pub theme: Option<(PathBuf, String)>,
}

impl Book {
    /// The book as the session has it, unsaved text included.
    pub fn of(session: &Session) -> Self {
        let theme = session.theme_file().and_then(|file| {
            let text = std::fs::read_to_string(&file).ok()?;
            Some((file, text))
        });
        Self {
            kind: session.kind().clone(),
            joined: session.joined(),
            theme,
        }
    }

    /// The book as it is on disk, at a project folder or a single file.
    pub fn open(path: &Path) -> Result<Self, String> {
        Session::open(path).map(|session| Self::of(&session))
    }

    fn base(&self) -> PathBuf {
        self.kind.base_dir()
    }

    /// A chapter file's path and text, by its path from the book's folder.
    pub fn file(&self, file: &str) -> Option<(PathBuf, &str)> {
        let path = normalize(&self.base().join(file));
        match &self.kind {
            Kind::Single { path: single } => (*single == path).then_some((path, self.joined.text.as_str())),
            Kind::Project { .. } => {
                let text = self.joined.file(&path)?;
                Some((path, text))
            }
        }
    }

    /// A file's path from the book's folder, as a fix names it.
    fn relative(&self, path: &Path) -> String {
        druck::diagnostic::path_from(path, &self.base())
    }

    /// A copy with `fixes` written in, and what was written into each chapter file. The reason it cannot be made is
    /// for whoever asked for the fix.
    fn with(&self, fixes: &[Fix]) -> Result<(Self, Written), String> {
        let mut by_file: HashMap<PathBuf, Vec<&Fix>> = HashMap::new();
        let mut theme = self.theme.clone();
        for fix in fixes {
            match fix {
                Fix::Theme { property, value } => {
                    let (_, text) = theme
                        .as_mut()
                        .ok_or("the book uses the built-in theme, which has no file to hold a value.")?;
                    let keys: Vec<&str> = property.split('.').collect();
                    *text = patch::set(text, &keys, value)?;
                }
                _ => {
                    let place = fix.place().expect("every other fix stands at a block");
                    let (path, _) =
                        (self.file(&place.file)).ok_or_else(|| format!("{} is no chapter of the book.", place.file))?;
                    by_file.entry(path).or_default().push(fix);
                }
            }
        }
        let mut written = Written::default();
        let mut texts: HashMap<PathBuf, String> = HashMap::new();
        for (path, fixes) in by_file {
            let (_, text) = self.file(&self.relative(&path)).expect("found above");
            let edits = fix::edits(text, &fixes)?;
            texts.insert(path.clone(), fix::apply(text, &edits));
            written.0.insert(path, edits);
        }
        let joined = match &self.kind {
            Kind::Single { path } => match texts.remove(path) {
                Some(text) => Joined {
                    text,
                    ..self.joined.clone()
                },
                None => self.joined.clone(),
            },
            Kind::Project { .. } => self.joined.with_files(|path, _| texts.get(path).cloned()),
        };
        let book = Self {
            kind: self.kind.clone(),
            joined,
            theme,
        };
        Ok((book, written))
    }
}

/// The edits a set of fixes made to each chapter file, which say where a line of the copy was in the book.
#[derive(Default)]
struct Written(HashMap<PathBuf, Vec<Edit>>);

/// A warning as a trial compares it: what it is and which block it is about, without the page, which a fix moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The rule broken, as a short name, or `other`.
    pub kind: &'static str,
    /// The chapter file from the book's folder, and the block's line in the book before any fix. A warning about a
    /// whole page has neither.
    pub at: Option<(String, usize)>,
    /// The page, counted from 1 through the file.
    pub page: Option<usize>,
    /// For a warning about a whole page: the first block that starts on it, as a file and line like `at`. It is where
    /// a directive for that page can stand, and no part of what the warning is.
    pub first_block: Option<(String, usize)>,
    /// The engine's words, in English.
    pub message: String,
}

impl Warning {
    /// Two warnings with one key are the same warning before and after a fix. A page's warning has no block, so it is
    /// the same as any other of its kind: a short page that moved on by one counts as the same short page.
    fn key(&self) -> (&'static str, Option<&(String, usize)>, &str) {
        let words = if self.kind == OTHER { self.message.as_str() } else { "" };
        (self.kind, self.at.as_ref(), words)
    }

    /// One line for a list: the kind, then the place.
    pub fn line(&self) -> String {
        let page = self.page.map(|page| format!(" page {page}")).unwrap_or_default();
        match (&self.at, &self.first_block) {
            (Some((file, line)), _) => format!("{} {file}:{line}{page}", self.kind),
            (None, _) if self.kind == OTHER => self.message.clone(),
            (None, Some((file, line))) => format!("{}{page}, its first block is {file}:{line}", self.kind),
            (None, None) => format!("{}{page}", self.kind),
        }
    }
}

/// The kind of a warning that is no typesetting warning, such as a missing image. A trial counts it, and no fix is
/// asked for it.
pub const OTHER: &str = "other";

/// Whether a diagnostic is a rule the pages break, which is what a fix is for.
pub fn is_typesetting(diagnostic: &Diagnostic) -> bool {
    diagnostic.is_warning() && kind_of(diagnostic.code.as_ref()) != OTHER
}

/// The short name of a typesetting rule.
fn kind_of(code: Option<&Code>) -> &'static str {
    match code {
        Some(Code::PageShort { .. }) => "page-short",
        Some(Code::PageUneven { .. }) => "page-uneven",
        Some(Code::ChapterEndShort { .. }) => "chapter-end-short",
        Some(Code::OneLinerAtTop { .. }) => "one-liner-at-top",
        Some(Code::OneLinerAtBottom { .. }) => "one-liner-at-bottom",
        Some(Code::ListItemAtBottom { .. }) => "list-item-at-bottom",
        Some(Code::ListItemAtTop { .. }) => "list-item-at-top",
        Some(Code::HeadingAtFoot { .. }) => "heading-at-foot",
        Some(Code::LooseLine { .. }) => "loose-line",
        Some(Code::TooManyHyphens { .. }) => "too-many-hyphens",
        Some(Code::ShortLastLine { .. }) => "short-last-line",
        _ => OTHER,
    }
}

/// The warnings of a layout of `copy`, the book with fixes written in, with every line as the book has it.
fn warnings_of(copy: &Book, written: &Written, book: &Book, layout: &Layout) -> Vec<Warning> {
    // A line of the copy as a line of the book, which is what the next fix names.
    let in_book = |path: &Path, line: usize| {
        let file = copy.relative(path);
        let line = match (written.0.get(path), book.file(&file)) {
            (Some(edits), Some((_, text))) => fix::line_before(text, edits, line),
            _ => line,
        };
        (file, line)
    };
    (layout.warnings.iter())
        .filter(|diagnostic| diagnostic.is_warning())
        .map(|diagnostic| {
            let at = match (&diagnostic.source, &diagnostic.location) {
                (Some(Source::Document(path)), Some(location)) => Some(in_book(path, location.line as usize)),
                _ => None,
            };
            let first_block = (diagnostic.page)
                .filter(|_| at.is_none())
                .and_then(|page| first_block_on(copy, layout, page))
                .map(|(path, line)| in_book(&path, line));
            Warning {
                kind: kind_of(diagnostic.code.as_ref()),
                at,
                page: diagnostic.page.map(|page| page + 1),
                first_block,
                message: diagnostic.message.clone(),
            }
        })
        .collect()
}

/// Where each page starts: the file, the book's line, and how far into that line. Two layouts with the same starts
/// hold the same text on every page.
fn page_starts(copy: &Book, written: &Written, book: &Book, layout: &Layout) -> Vec<Option<(PathBuf, usize, usize)>> {
    (0..layout.laid.pages().len())
        .map(|page| {
            let (path, offset) = proof::first_place(&layout.laid, page)?;
            let file = copy.relative(&path);
            let (_, text) = copy.file(&file)?;
            let line = fix::line_of(text, offset);
            let into = offset - text[..offset.min(text.len())].rfind('\n').map_or(0, |end| end + 1);
            let line = match (written.0.get(&path), book.file(&file)) {
                (Some(edits), Some((_, text))) => fix::line_before(text, edits, line),
                _ => line,
            };
            Some((path, line, into))
        })
        .collect()
}

/// The file and line of the first block that starts on a page. A paragraph that runs on from the page before is not it.
fn first_block_on(copy: &Book, layout: &Layout, page: usize) -> Option<(PathBuf, usize)> {
    let (path, offset) = proof::first_place(&layout.laid, page)?;
    let (_, text) = copy.file(&copy.relative(&path))?;
    let blocks = Blocks::parse(text);
    let list = blocks.as_slice();
    let at = blocks.index_at(offset);
    let runs_on = text[list[at].range.start..offset.min(text.len())].contains(char::is_alphanumeric);
    let first = (list[at + usize::from(runs_on)..].iter()).find(|block| block.kind != BlockKind::Blank)?;
    Some((path, fix::line_of(text, first.range.start)))
}

/// What a fix did to the warnings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Warnings the book had that the copy with the fix has no more.
    pub cleared: Vec<Warning>,
    /// Warnings the copy has that the book had not.
    pub caused: Vec<Warning>,
    /// The page a page break took a warning from without mending it: the page before a break the author set may end
    /// where it likes. Its warnings are neither cleared nor caused.
    pub excused: Vec<Warning>,
    /// How many warnings the copy has.
    pub remaining: usize,
    /// A directive moved no text to another page. It can clear a warning that way, where a rule excuses a page after
    /// a break the author set, and then it has only hidden the warning.
    pub unchanged: bool,
}

impl Outcome {
    /// The rule for keeping a fix: it clears a warning, causes no new one, and does more than hide the warning.
    pub fn keeps(&self) -> bool {
        !self.cleared.is_empty() && self.caused.is_empty() && !self.unchanged
    }
}

/// Compares the warnings before and after a fix, each kind and block by how often it stands. Of several warnings with
/// one key, those on the same page pair up first, so what is reported as cleared or caused names the right page.
pub fn compare(before: &[Warning], after: &[Warning]) -> Outcome {
    let mut cleared: Vec<&Warning> = before.iter().collect();
    let mut caused: Vec<&Warning> = after.iter().collect();
    let same_page = |a: &Warning, b: &Warning| a.key() == b.key() && a.page == b.page;
    let same = |a: &Warning, b: &Warning| a.key() == b.key();
    for pairs in [&same_page as &dyn Fn(&Warning, &Warning) -> bool, &same] {
        cleared.retain(|warning| match caused.iter().position(|other| pairs(warning, other)) {
            Some(at) => {
                caused.remove(at);
                false
            }
            None => true,
        });
    }
    Outcome {
        cleared: cleared.into_iter().cloned().collect(),
        caused: caused.into_iter().cloned().collect(),
        excused: Vec::new(),
        remaining: after.len(),
        unchanged: false,
    }
}

/// A page of a trial, and where on it the block of the fix stands, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub page: usize,
    pub mark: Option<Rect>,
}

/// One fix, tried.
pub struct Tried {
    pub fix: Fix,
    pub outcome: Outcome,
    /// The pages without the fix and with it.
    pub before: Arc<Layout>,
    pub after: Arc<Layout>,
    /// The page to look at on each side: the one the fix's block stands on, or for a theme value the first warning
    /// it clears.
    pub shown: Option<(Shown, Shown)>,
}

/// One layout of the copy: its pages, its warnings, and where each page starts.
#[derive(Clone)]
struct Laid {
    layout: Arc<Layout>,
    warnings: Vec<Warning>,
    starts: Vec<Option<(PathBuf, usize, usize)>>,
}

/// The copy of the book that fixes are tried on, with every fix kept so far written in.
pub struct Bench {
    typesetter: Typesetter,
    book: Book,
    kept: Vec<Fix>,
    /// The book with every kept fix, laid out.
    standing: Option<Laid>,
}

impl Bench {
    pub fn new(book: Book) -> Self {
        Self {
            typesetter: Typesetter::new(book.kind.clone(), None),
            book,
            kept: Vec::new(),
            standing: None,
        }
    }

    pub fn book(&self) -> &Book {
        &self.book
    }

    fn lay_out(&mut self, fixes: &[Fix], cancel: &Cancel) -> Result<(Laid, Book), String> {
        let (copy, written) = self.book.with(fixes)?;
        self.typesetter.set_theme_text(copy.theme.clone());
        let layout = self
            .typesetter
            .run(copy.joined.clone(), cancel)
            .map_err(|failure| format!("the book does not set with it: {failure}"))?;
        let laid = Laid {
            warnings: warnings_of(&copy, &written, &self.book, &layout),
            starts: page_starts(&copy, &written, &self.book, &layout),
            layout: Arc::new(layout),
        };
        Ok((laid, copy))
    }

    /// The warnings of the book with every fix kept so far.
    pub fn warnings(&mut self, cancel: &Cancel) -> Result<&[Warning], String> {
        if self.standing.is_none() {
            let (laid, _) = self.lay_out(&self.kept.clone(), cancel)?;
            self.standing = Some(laid);
        }
        Ok(self.standing.as_ref().map_or(&[], |laid| &laid.warnings))
    }

    /// Tries one fix on top of the kept ones, and keeps it if [`Outcome::keeps`] says so. An error is why the fix
    /// could not be tried at all.
    pub fn try_fix(&mut self, fix: Fix, cancel: &Cancel) -> Result<Tried, String> {
        if self.kept.contains(&fix) {
            return Err("this fix is kept already.".to_owned());
        }
        self.warnings(cancel)?;
        let before = self.standing.clone().expect("laid out above");
        let mut fixes = self.kept.clone();
        fixes.push(fix.clone());
        let (after, copy) = self.lay_out(&fixes, cancel)?;
        let mut outcome = Outcome {
            // A theme value sets lines anew within their pages, which page starts do not show.
            unchanged: fix.place().is_some() && before.starts == after.starts,
            ..compare(&before.warnings, &after.warnings)
        };
        let shown = self.shown(&fix, &outcome, &copy, &before.layout, &after.layout);
        if matches!(fix, Fix::PageBreak(_)) {
            // A break cannot lengthen a page. It only excuses the page its block stood on, which now ends at the
            // break, and with it the height that page is measured against across the spread.
            let ended = shown.map(|(before, _)| before.page + 1);
            let excuses = |warning: &Warning| match (warning.kind, warning.page, ended) {
                ("page-short", Some(page), Some(ended)) => page == ended,
                ("page-uneven", Some(page), Some(ended)) => page == ended || page == ended + 1,
                ("page-short" | "page-uneven", _, None) => true,
                _ => false,
            };
            let (excused, cleared) = std::mem::take(&mut outcome.cleared)
                .into_iter()
                .partition(|warning| excuses(warning));
            (outcome.excused, outcome.cleared) = (excused, cleared);
        }
        let tried = Tried {
            fix,
            outcome,
            before: before.layout,
            after: after.layout.clone(),
            shown,
        };
        if tried.outcome.keeps() {
            self.kept = fixes;
            self.standing = Some(after);
        }
        Ok(tried)
    }

    /// Where to look for what a fix did: its block before and after, on the pages it stands on.
    fn shown(
        &self,
        fix: &Fix,
        outcome: &Outcome,
        copy: &Book,
        before: &Layout,
        after: &Layout,
    ) -> Option<(Shown, Shown)> {
        let (file, line) = match fix.place() {
            Some(place) => (place.file.clone(), place.line),
            None => outcome.cleared.iter().find_map(|warning| warning.at.clone())?,
        };
        let (path, text) = self.book.file(&file)?;
        let block = fix::block_at(text, line).ok()?;
        // The same block in the copy, found by its first line, which no fix changes.
        let words = text[block.clone()].lines().next()?;
        let (_, copied) = copy.file(&file)?;
        let moved = nearest(copied, words, block.start)?;
        let on = |layout: &Layout, start: usize| {
            let mark = proof::marks_of(&layout.laid, &path, start..start + words.len())
                .into_iter()
                .next()?;
            Some(Shown {
                page: mark.page,
                mark: Some(mark.rect),
            })
        };
        Some((on(before, self.standing_start(&file, line)?)?, on(after, moved)?))
    }

    /// Where the block at a line of the book starts in the copy with every kept fix.
    fn standing_start(&self, file: &str, line: usize) -> Option<usize> {
        let (standing, _) = self.book.with(&self.kept).ok()?;
        let (_, text) = self.book.file(file)?;
        let block = fix::block_at(text, line).ok()?;
        let words = text[block.clone()].lines().next()?;
        let (_, copied) = standing.file(file)?;
        nearest(copied, words, block.start)
    }
}

/// Where `words` stand in `text`, nearest to `near`.
pub fn nearest(text: &str, words: &str, near: usize) -> Option<usize> {
    text.match_indices(words)
        .map(|(at, _)| at)
        .min_by_key(|at| at.abs_diff(near))
}

/// A book with one warning and fixes for it, for the tests of the trial and of what the app does with it.
#[cfg(test)]
pub mod testing {
    use serde_json::json;

    use super::*;

    /// A book of one file, in a theme file of its own.
    fn book(name: &str, text: &str) -> (PathBuf, Book) {
        let dir = std::env::temp_dir().join(format!("druck-trial-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("plain.json"), json!({"version": 1}).to_string()).expect("theme");
        let file = dir.join("book.md");
        std::fs::write(&file, format!("---\ntheme: ./plain.json\n---\n\n{text}")).expect("file");
        let book = Book::open(&file).expect("the book opens");
        (dir, book)
    }

    fn filler(paragraphs: usize) -> String {
        let sentence = "The lamps were lit along the quay and nobody spoke of the ship that had not come in. ";
        (0..paragraphs)
            .map(|_| format!("{}\n\n", sentence.repeat(3).trim_end()))
            .collect()
    }

    /// The line a block of the book starts on, counting blocks from 0, after the four lines of front matter.
    fn line_of_block(block: usize) -> usize {
        5 + 2 * block
    }

    /// The last block of [`short_ending`]'s book.
    pub const LAST: usize = 33;

    /// A book that ends with three lines on its last page, which is the one warning it has: a chapter's last page
    /// wants five.
    pub fn short_ending(name: &str) -> (PathBuf, Bench, Warning) {
        let (dir, bench, only) = one_warning(name, 25);
        assert_eq!(only.kind, "chapter-end-short");
        let last = ("book.md".to_owned(), line_of_block(LAST));
        assert_eq!(
            only.first_block,
            Some(last),
            "the page's first block is where a fix can stand"
        );
        (dir, bench, only)
    }

    /// A book of a heading, an opening line, `paragraphs` paragraphs, a second heading, and six more, which has one
    /// warning.
    pub fn one_warning(name: &str, paragraphs: usize) -> (PathBuf, Bench, Warning) {
        let text = format!(
            "# One\n\nA short opening.\n\n{}## Two\n\n{}",
            filler(paragraphs),
            filler(6)
        );
        let (dir, book) = book(name, &text);
        let mut bench = Bench::new(book);
        let warnings = bench.warnings(&Cancel::default()).expect("it sets").to_vec();
        let [only] = warnings.as_slice() else {
            panic!("the book has one warning, not {warnings:?}");
        };
        (dir, bench, only.clone())
    }

    /// A fix at a block of the book, as the agent names it.
    pub fn fix_at_block(kind: &str, block: usize) -> serde_json::Value {
        json!({"kind": kind, "file": "book.md", "line": line_of_block(block)})
    }

    pub fn fix(kind: &str, block: usize) -> Fix {
        Fix::from_json(&fix_at_block(kind, block)).expect("a fix")
    }

    /// Every file of a folder with its bytes, to see that nothing was written.
    pub fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files: Vec<_> = (std::fs::read_dir(dir).expect("dir"))
            .map(|entry| entry.expect("entry").path())
            .map(|path| (path.clone(), std::fs::read(&path).expect("file")))
            .collect();
        files.sort();
        files
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn warning(kind: &'static str, at: Option<(&str, usize)>) -> Warning {
        Warning {
            kind,
            at: at.map(|(file, line)| (file.to_owned(), line)),
            page: Some(1),
            first_block: None,
            message: String::new(),
        }
    }

    #[test]
    fn a_fix_is_kept_only_if_it_clears_a_warning_and_causes_none() {
        let loose = warning("loose-line", Some(("a.md", 3)));
        let other = warning("loose-line", Some(("a.md", 9)));
        let short = warning("page-short", None);
        let helps = compare(&[loose.clone(), short.clone()], std::slice::from_ref(&short));
        assert_eq!(helps.cleared, vec![loose.clone()]);
        assert!(helps.keeps());
        let nothing = compare(std::slice::from_ref(&loose), std::slice::from_ref(&loose));
        assert!(!nothing.keeps(), "it clears nothing");
        let trades = compare(std::slice::from_ref(&loose), std::slice::from_ref(&other));
        assert!(!trades.keeps(), "it causes a new one");
        assert_eq!((trades.cleared.len(), trades.caused), (1, vec![other]));
        let more = compare(std::slice::from_ref(&short), &[short.clone(), short.clone()]);
        assert_eq!(more.caused.len(), 1, "a second short page is a new warning");
    }

    #[test]
    fn a_short_page_that_only_moved_is_the_same_warning() {
        let on_page = |page| Warning {
            page: Some(page),
            ..warning("page-short", None)
        };
        let outcome = compare(&[on_page(1)], &[on_page(7)]);
        assert!(outcome.cleared.is_empty() && outcome.caused.is_empty());
        let outcome = compare(&[on_page(2), on_page(4), on_page(9)], &[on_page(4), on_page(9)]);
        assert_eq!(
            outcome.cleared,
            vec![on_page(2)],
            "the page that is no longer short is the one named"
        );
    }

    use super::testing::{LAST, fix, one_warning, short_ending};

    #[test]
    fn a_fix_that_helps_is_kept_and_the_book_on_disk_stays() {
        let (dir, mut bench, warning) = short_ending("helps");
        let on_disk = std::fs::read(dir.join("book.md")).expect("file");
        // The paragraph before the last goes over to the last page, which then holds six lines.
        let page_break = fix("page-break", LAST - 1);
        let tried = bench
            .try_fix(page_break.clone(), &Cancel::default())
            .expect("it can be tried");
        assert_eq!(tried.outcome.cleared, vec![warning]);
        assert_eq!(tried.outcome.caused, vec![]);
        assert_eq!(bench.kept, vec![page_break.clone()]);
        let (before, after) = tried.shown.expect("the paragraph is on the pages");
        assert_eq!(
            (before.page, after.page),
            (1, 2),
            "the paragraph starts the next page, counted from 0"
        );
        assert!(before.mark.is_some() && after.mark.is_some());
        assert!(bench.warnings(&Cancel::default()).expect("it sets").is_empty());
        assert!(
            bench.try_fix(page_break, &Cancel::default()).is_err(),
            "a kept fix is not tried twice"
        );
        assert_eq!(std::fs::read(dir.join("book.md")).expect("file"), on_disk);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_fix_that_only_hides_a_warning_is_not_kept() {
        let (dir, mut bench, warning) = short_ending("hides");
        // A page break where the last page starts anyway: the rule excuses a page after a break, and no line moves.
        let tried = bench
            .try_fix(fix("page-break", LAST), &Cancel::default())
            .expect("it can be tried");
        assert_eq!(tried.outcome.cleared, vec![warning]);
        assert!(tried.outcome.unchanged && !tried.outcome.keeps());
        assert!(bench.kept.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_page_break_excuses_a_short_page_and_that_is_no_fix() {
        // The second heading would stand at the foot of the first page, so the page ends short.
        let (dir, mut bench, warning) = one_warning("excused", 9);
        assert_eq!(warning.kind, "page-short");
        let tried = bench
            .try_fix(fix("page-break", 6), &Cancel::default())
            .expect("it can be tried");
        assert_eq!(tried.outcome.excused, vec![warning]);
        assert!(tried.outcome.cleared.is_empty() && !tried.outcome.keeps());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_fix_that_clears_nothing_is_not_kept() {
        let (dir, mut bench, _) = short_ending("idle");
        // A paragraph of one line is never split, so keeping it together moves nothing.
        let tried = bench
            .try_fix(fix("keep", 1), &Cancel::default())
            .expect("it can be tried");
        assert!(tried.outcome.cleared.is_empty() && tried.outcome.caused.is_empty());
        assert!(!tried.outcome.keeps());
        assert!(bench.kept.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_fix_that_causes_a_new_warning_is_not_kept() {
        let (dir, mut bench, warning) = short_ending("causes");
        // A page break in the middle of the second page mends the ending and leaves that page short.
        let tried = bench
            .try_fix(fix("page-break", 12), &Cancel::default())
            .expect("it can be tried");
        assert_eq!(tried.outcome.cleared, vec![warning.clone()]);
        let caused: Vec<&str> = tried.outcome.caused.iter().map(|warning| warning.kind).collect();
        assert_eq!(caused, ["page-short"]);
        assert!(!tried.outcome.keeps());
        assert!(bench.kept.is_empty());
        let standing = bench.warnings(&Cancel::default()).expect("it sets");
        assert_eq!(standing, [warning], "the copy is as it was before the trial");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_fix_the_engine_refuses_is_an_error_and_a_theme_value_is_tried_in_memory() {
        let (dir, mut bench, _) = short_ending("theme");
        let nowhere = Fix::from_json(&json!({"kind": "page-break", "file": "other.md", "line": 1})).expect("a fix");
        assert!(bench.try_fix(nowhere, &Cancel::default()).is_err());
        let theme_before = std::fs::read(dir.join("plain.json")).expect("theme");
        let tracking = json!({"kind": "theme", "property": "styles.body.tracking", "value": 0.02});
        let tried = bench
            .try_fix(Fix::from_json(&tracking).expect("a fix"), &Cancel::default())
            .expect("a theme value can be tried");
        assert_ne!(
            tried.before.fingerprints, tried.after.fingerprints,
            "wider letters set the pages differently"
        );
        assert_eq!(std::fs::read(dir.join("plain.json")).expect("theme"), theme_before);
        std::fs::remove_dir_all(&dir).ok();
    }
}
