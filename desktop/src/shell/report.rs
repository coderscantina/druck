//! What is wrong with the document, grouped by chapter: the engine's errors and warnings, with the `{todo:}`
//! placeholders beside them. Holds no gpui types.
//!
//! A report is built after each layout from the structured [`Diagnostic`]s, so no message is parsed. The sidebar's dots
//! are read off it and never stored.

use std::collections::HashMap;
use std::path::Path;

use druck::diagnostic::{Code, Diagnostic, Severity, Source, path_from};

use super::facts::{Facts, Todo};
use crate::session::{ChapterId, Kind as DocumentKind, Session};
use crate::sidebar::Mark;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Error,
    Warning,
    /// An open `{todo:}` placeholder.
    Todo,
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    /// The 1-based line in the chapter, when the engine or the placeholder says one.
    pub line: Option<usize>,
    /// The place in the chapter a click opens.
    pub place: Option<(ChapterId, usize)>,
    /// The page, for a typesetting warning, as its physical index.
    pub page: Option<usize>,
    pub message: String,
    /// A refused export's error about a problem the list shows in its chapter too, which counts once.
    pub repeated: bool,
}

/// The entries of one chapter, or of what belongs to no chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// `None` for settings, themes, and anything else that is not a chapter's text.
    pub chapter: Option<ChapterId>,
    pub title: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub sections: Vec<Section>,
}

/// How many of each kind a report holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub errors: usize,
    pub warnings: usize,
    pub todos: usize,
}

impl Report {
    /// Every entry, in the order the list shows them.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.sections.iter().flat_map(|section| &section.entries)
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for entry in self.entries().filter(|entry| !entry.repeated) {
            match entry.kind {
                Kind::Error => counts.errors += 1,
                Kind::Warning => counts.warnings += 1,
                Kind::Todo => counts.todos += 1,
            }
        }
        counts
    }

    /// The dot each chapter shows: a problem if it has an error, else a placeholder if one is open. A warning shows no
    /// dot: a page problem waits for Publish.
    pub fn marks(&self) -> HashMap<ChapterId, Mark> {
        let mut marks = HashMap::new();
        for section in &self.sections {
            let Some(chapter) = section.chapter else { continue };
            let kinds = section.entries.iter().map(|entry| entry.kind);
            let mark = if kinds.clone().any(|kind| kind == Kind::Error) {
                Mark::Problem
            } else if kinds.clone().any(|kind| kind == Kind::Todo) {
                Mark::Placeholder
            } else {
                continue;
            };
            marks.insert(chapter, mark);
        }
        marks
    }
}

/// The chapter and offset a line of a file is at. A single file's lines count in the whole file, which its sections
/// split without losing a line.
pub fn place(session: &Session, path: &Path, line: usize) -> Option<(ChapterId, usize)> {
    match session.kind() {
        DocumentKind::Project { .. } => {
            let id = *session
                .book()
                .iter()
                .find(|id| session.chapter(**id).is_some_and(|chapter| chapter.path() == path))?;
            Some((id, session.buffer(id).offset_of_line(line)))
        }
        DocumentKind::Single { path: file } => {
            if file != path {
                return None;
            }
            let mut before = 0;
            for (at, id) in session.book().iter().enumerate() {
                let text = session.buffer(*id).text();
                let lines = text.matches('\n').count() + usize::from(!text.ends_with('\n'));
                if line <= before + lines || at + 1 == session.book().len() {
                    return Some((
                        *id,
                        session.buffer(*id).offset_of_line(line.saturating_sub(before).max(1)),
                    ));
                }
                before += lines;
            }
            None
        }
    }
}

/// One diagnostic as an entry, and the chapter it belongs to. A warning about a whole page belongs to the chapter
/// `page_place` says the page is in.
fn entry(
    session: &Session,
    diagnostic: &Diagnostic,
    page_place: &dyn Fn(usize) -> Option<(ChapterId, usize)>,
) -> (Option<ChapterId>, Entry) {
    let base = session.base_dir();
    let page = diagnostic.page;
    let line = diagnostic.location.map(|location| location.line as usize);
    let message = super::engine_text::message_in(diagnostic, Some(&base));
    // A file or citation key that is not there is wrong to the author while something stands in for it, so it counts
    // as an error.
    let stands_in = matches!(
        diagnostic.code,
        Some(
            Code::ImageNotFound { .. }
                | Code::FontNotFound { .. }
                | Code::BibliographyNotFound { .. }
                | Code::MissingCitation { .. }
        )
    );
    let kind = match diagnostic.severity {
        Severity::Error => Kind::Error,
        Severity::Warning if stands_in => Kind::Error,
        Severity::Warning => Kind::Warning,
    };
    let found = match (&diagnostic.source, line) {
        (Some(Source::Document(path)), Some(line)) => place(session, path, line),
        _ => page.and_then(page_place),
    };
    if let Some((id, offset)) = found {
        return (
            Some(id),
            Entry {
                kind,
                line,
                place: Some((id, offset)),
                page,
                message,
                repeated: false,
            },
        );
    }
    // Settings and themes: say which file, since no chapter shows it.
    let file = match &diagnostic.source {
        Some(Source::Document(path) | Source::Theme(path) | Source::Bibliography(path)) => Some(path_from(path, &base)),
        _ => None,
    };
    let message = match (file, line) {
        (Some(file), Some(line)) => format!("{file}:{line}: {message}"),
        (Some(file), None) => format!("{file}: {message}"),
        _ => message,
    };
    (
        None,
        Entry {
            kind,
            line: None,
            place: None,
            page,
            message,
            repeated: false,
        },
    )
}

/// Why an export did not run: its errors, which the report lists first under `title`, so the author sees them before
/// the pages' own problems.
pub struct Refused<'a> {
    pub title: String,
    pub errors: &'a [Diagnostic],
}

/// The report for the engine's `errors` and `warnings`, an export that was `refused`, and the placeholders in the cache.
/// Errors and placeholders come before warnings, each group in book order, and within a chapter by page, then line, so
/// the list and the walk over it move forward through the book. A chapter with both stands once in each group.
/// `general` names the section of what belongs to no chapter. `page_place` says where a page's first text is written.
pub fn build(
    session: &Session,
    errors: &[Diagnostic],
    warnings: &[Diagnostic],
    refused: Option<Refused>,
    facts: &HashMap<ChapterId, Facts>,
    general: &str,
    page_place: &dyn Fn(usize) -> Option<(ChapterId, usize)>,
) -> Report {
    // What stops an export, then warnings, each in book order: with what belongs to no chapter first, then the chapters.
    let mut grouped: HashMap<(bool, Option<ChapterId>), Vec<Entry>> = HashMap::new();
    for diagnostic in errors.iter().chain(warnings) {
        let (owner, entry) = entry(session, diagnostic, page_place);
        grouped
            .entry((entry.kind == Kind::Warning, owner))
            .or_default()
            .push(entry);
    }
    for id in session.book() {
        let Some(found) = facts.get(id) else { continue };
        let buffer = session.buffer(*id);
        for Todo { offset, text } in &found.todos {
            grouped.entry((false, Some(*id))).or_default().push(Entry {
                kind: Kind::Todo,
                line: Some(buffer.line_of(*offset)),
                place: Some((*id, *offset)),
                page: None,
                message: text.clone(),
                repeated: false,
            });
        }
    }
    let mut sections = Vec::new();
    if let Some(refused) = refused {
        let entries = (refused.errors.iter())
            .map(|error| {
                let (owner, mut entry) = entry(session, error, page_place);
                // An open placeholder, or a file or key that is not there, stands in its chapter's list too.
                let twin = match &error.code {
                    Some(Code::FinalRefused { cause, .. }) if matches!(**cause, Code::OpenPlaceholder { .. }) => {
                        Kind::Todo
                    }
                    _ => Kind::Error,
                };
                entry.repeated = owner.is_some()
                    && (grouped.get(&(false, owner)).into_iter().flatten())
                        .any(|other| other.kind == twin && other.line == entry.line);
                entry
            })
            .collect();
        sections.push(Section {
            chapter: None,
            title: refused.title,
            entries,
        });
    }
    for warnings in [false, true] {
        for owner in std::iter::once(None).chain(session.book().iter().copied().map(Some)) {
            let Some(mut entries) = grouped.remove(&(warnings, owner)) else {
                continue;
            };
            // Entries with no page, such as errors and placeholders, go by line before those on pages.
            entries.sort_by_key(|entry| (entry.page, entry.line));
            sections.push(Section {
                chapter: owner,
                title: owner.map_or_else(|| general.to_owned(), |id| session.title_of(id)),
                entries,
            });
        }
    }
    Report { sections }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use druck::diagnostic::Location;

    use super::*;
    use crate::shell::facts;

    fn project(name: &str) -> (std::path::PathBuf, Session) {
        let dir = std::env::temp_dir().join(format!("druck-report-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("manuscript")).expect("folder");
        fs::write(
            dir.join("druck.yaml"),
            "title: T\nchapters:\n  - manuscript/one.md\n  - manuscript/two.md\n",
        )
        .expect("manifest");
        fs::write(
            dir.join("manuscript/one.md"),
            "# One\n\nText {todo: check this} more.\n",
        )
        .expect("one");
        fs::write(dir.join("manuscript/two.md"), "# Two\n\nA\n\nB\n\nC\n").expect("two");
        let session = Session::open(&dir).expect("opens");
        (dir, session)
    }

    fn warning(session: &Session, chapter: &str, line: u64) -> Diagnostic {
        let mut diagnostic = Diagnostic::error("a line is loose");
        diagnostic.severity = Severity::Warning;
        diagnostic.source = Some(Source::Document(session.base_dir().join("manuscript").join(chapter)));
        diagnostic.location = Some(Location { line, column: 1 });
        diagnostic
    }

    fn facts_of(session: &Session) -> HashMap<ChapterId, Facts> {
        session
            .book()
            .iter()
            .map(|id| (*id, facts::compute(session.buffer(*id).text(), "en")))
            .collect()
    }

    #[test]
    fn entries_group_by_chapter_in_book_order_with_a_click_target() {
        let (dir, session) = project("group");
        let problems = [warning(&session, "two.md", 5), warning(&session, "two.md", 3)];
        let report = build(&session, &[], &problems, None, &facts_of(&session), "Project", &|_| {
            None
        });
        let titles: Vec<_> = report.sections.iter().map(|section| section.title.as_str()).collect();
        assert_eq!(titles, ["One", "Two"]);
        let two = &report.sections[1];
        assert_eq!(
            two.entries.iter().map(|entry| entry.line).collect::<Vec<_>>(),
            [Some(3), Some(5)]
        );
        let (id, offset) = two.entries[0].place.expect("opens a place");
        assert_eq!(Some(id), two.chapter);
        assert_eq!(&session.buffer(id).text()[offset..offset + 1], "A");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_dot_says_error_over_placeholder_and_a_page_warning_says_nothing() {
        let (dir, session) = project("marks");
        let book = session.book().to_vec();
        let open = build(&session, &[], &[], None, &facts_of(&session), "Project", &|_| None);
        assert_eq!(open.marks(), HashMap::from([(book[0], Mark::Placeholder)]));
        let both = build(
            &session,
            &[],
            &[warning(&session, "one.md", 1)],
            None,
            &facts_of(&session),
            "Project",
            &|_| None,
        );
        assert_eq!(both.marks(), HashMap::from([(book[0], Mark::Placeholder)]));
        assert_eq!(
            both.counts(),
            Counts {
                errors: 0,
                warnings: 1,
                todos: 1
            }
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn what_belongs_to_no_chapter_is_listed_first_with_its_file() {
        let (dir, session) = project("general");
        let mut theme = Diagnostic::error("unknown property");
        theme.source = Some(Source::Theme(session.base_dir().join("theme.json")));
        theme.location = Some(Location { line: 4, column: 2 });
        let report = build(&session, &[theme], &[], None, &HashMap::new(), "Project", &|_| None);
        assert_eq!(report.sections[0].chapter, None);
        assert_eq!(report.sections[0].entries[0].message, "theme.json:4: Unknown property");
        assert!(report.marks().is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_warning_about_a_page_stands_under_the_chapter_the_page_is_in() {
        let (dir, session) = project("page");
        let two = session.book()[1];
        let mut short = Diagnostic::error("page 3 ends short");
        short.severity = Severity::Warning;
        short.page = Some(2);
        let report = build(&session, &[], &[short], None, &HashMap::new(), "Project", &|page| {
            (page == 2).then_some((two, 7))
        });
        assert_eq!(report.sections[0].chapter, Some(two));
        let entry = &report.sections[0].entries[0];
        assert_eq!((entry.page, entry.place), (Some(2), Some((two, 7))));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn errors_and_placeholders_come_before_warnings_each_in_book_order() {
        let (dir, session) = project("order");
        let [one, two] = [session.book()[0], session.book()[1]];
        let mut error = warning(&session, "two.md", 5);
        error.severity = Severity::Error;
        let warnings = [warning(&session, "one.md", 1), warning(&session, "two.md", 3)];
        let report = build(
            &session,
            &[error],
            &warnings,
            None,
            &facts_of(&session),
            "Project",
            &|_| None,
        );
        let order: Vec<_> = report
            .sections
            .iter()
            .map(|section| {
                (
                    section.chapter,
                    section.entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            order,
            [
                (Some(one), vec![Kind::Todo]),
                (Some(two), vec![Kind::Error]),
                (Some(one), vec![Kind::Warning]),
                (Some(two), vec![Kind::Warning]),
            ]
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_refused_exports_errors_come_first_with_their_places() {
        let (dir, session) = project("refused");
        let mut image = Diagnostic::error("export \"print\" is final, but this image is not there: a.png");
        image.source = Some(Source::Document(session.base_dir().join("manuscript/two.md")));
        image.location = Some(Location { line: 5, column: 1 });
        let refused = Refused {
            title: "Export \"print\"".to_owned(),
            errors: &[image],
        };
        let warnings = [warning(&session, "one.md", 1)];
        let report = build(
            &session,
            &[],
            &warnings,
            Some(refused),
            &facts_of(&session),
            "Project",
            &|_| None,
        );
        let titles: Vec<_> = report.sections.iter().map(|section| section.title.as_str()).collect();
        assert_eq!(titles, ["Export \"print\"", "One", "One"]);
        let first = report.entries().next().expect("an entry");
        assert_eq!(first.kind, Kind::Error);
        assert_eq!(first.place.map(|(id, _)| id), Some(session.book()[1]));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_refused_exports_error_about_a_problem_the_chapter_shows_counts_once() {
        let (dir, session) = project("once");
        let refusal = |cause, line| {
            let mut error = Diagnostic::error("export \"print\" is final, but ...");
            error.code = Some(Code::FinalRefused {
                export: "print".into(),
                cause: Box::new(cause),
            });
            error.source = Some(Source::Document(session.base_dir().join("manuscript/one.md")));
            error.location = Some(Location { line, column: 1 });
            error
        };
        let placeholder = refusal(
            Code::OpenPlaceholder {
                text: "check this".into(),
            },
            3,
        );
        let isbn = Diagnostic::error("meta.isbn must be an ISBN");
        let errors = [placeholder, isbn];
        let refused = Refused {
            title: "Export \"print\"".to_owned(),
            errors: &errors,
        };
        let report = build(
            &session,
            &[],
            &[],
            Some(refused),
            &facts_of(&session),
            "Project",
            &|_| None,
        );
        assert_eq!(report.sections[0].entries.len(), 2, "both rows show");
        assert_eq!(
            report.counts(),
            Counts {
                errors: 1,
                warnings: 0,
                todos: 1
            },
            "the placeholder once, and the ISBN, which stands only under the export"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_chapters_entries_go_by_page_then_line() {
        let (dir, session) = project("pages");
        let on_page = |line, page| {
            let mut diagnostic = warning(&session, "two.md", line);
            diagnostic.page = Some(page);
            diagnostic
        };
        let warnings = [
            on_page(3, 4),
            on_page(7, 2),
            warning(&session, "two.md", 5),
            on_page(1, 4),
        ];
        let report = build(&session, &[], &warnings, None, &HashMap::new(), "Project", &|_| None);
        let order: Vec<_> = (report.entries()).map(|entry| (entry.page, entry.line)).collect();
        assert_eq!(
            order,
            [
                (None, Some(5)),
                (Some(2), Some(7)),
                (Some(4), Some(1)),
                (Some(4), Some(3))
            ]
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_single_files_lines_count_across_its_sections() {
        let dir = std::env::temp_dir().join(format!("druck-report-single-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("folder");
        let file = dir.join("doc.md");
        fs::write(&file, "# One\n\nA\n\n# Two\n\nB\n").expect("file");
        let session = Session::open(&file).expect("opens");
        let (id, offset) = place(&session, &file, 7).expect("line 7 is in a section");
        assert_eq!(id, session.book()[1]);
        assert_eq!(&session.buffer(id).text()[offset..offset + 1], "B");
        fs::remove_dir_all(dir).ok();
    }
}
