//! What is wrong with the document, grouped by chapter: the engine's errors and warnings, with the `{todo:}`
//! placeholders beside them. Holds no gpui types.
//!
//! A report is built after each layout from the structured [`Diagnostic`]s, so no message is parsed. The sidebar's dots
//! are read off it and never stored.

use std::collections::HashMap;
use std::path::Path;

use druck::diagnostic::{Diagnostic, Severity, Source, path_from};

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
        for entry in self.sections.iter().flat_map(|section| &section.entries) {
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
fn place(session: &Session, path: &Path, line: usize) -> Option<(ChapterId, usize)> {
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
    let message = super::engine_text::message(diagnostic).replace(&format!("{}/", base.display()), "");
    let kind = match diagnostic.severity {
        Severity::Error => Kind::Error,
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
        },
    )
}

/// The report for the engine's `errors` and `warnings` and the placeholders in the cache. `general` names the section of
/// what belongs to no chapter. `page_place` says where a page's first text is written.
pub fn build(
    session: &Session,
    errors: &[Diagnostic],
    warnings: &[Diagnostic],
    facts: &HashMap<ChapterId, Facts>,
    general: &str,
    page_place: &dyn Fn(usize) -> Option<(ChapterId, usize)>,
) -> Report {
    let mut loose = Vec::new();
    let mut by_chapter: HashMap<ChapterId, Vec<Entry>> = HashMap::new();
    for diagnostic in errors.iter().chain(warnings) {
        match entry(session, diagnostic, page_place) {
            (Some(id), entry) => by_chapter.entry(id).or_default().push(entry),
            (None, entry) => loose.push(entry),
        }
    }
    for id in session.book() {
        let Some(found) = facts.get(id) else { continue };
        let buffer = session.buffer(*id);
        for Todo { offset, text } in &found.todos {
            by_chapter.entry(*id).or_default().push(Entry {
                kind: Kind::Todo,
                line: Some(buffer.line_of(*offset)),
                place: Some((*id, *offset)),
                page: None,
                message: text.clone(),
            });
        }
    }
    let mut sections = Vec::new();
    if !loose.is_empty() {
        sections.push(Section {
            chapter: None,
            title: general.to_owned(),
            entries: loose,
        });
    }
    for id in session.book() {
        let Some(mut entries) = by_chapter.remove(id) else {
            continue;
        };
        // Errors first, then in reading order.
        entries.sort_by_key(|entry| (entry.kind, entry.line));
        sections.push(Section {
            chapter: Some(*id),
            title: session.title_of(*id),
            entries,
        });
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
        let report = build(&session, &[], &problems, &facts_of(&session), "Project", &|_| None);
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
        let open = build(&session, &[], &[], &facts_of(&session), "Project", &|_| None);
        assert_eq!(open.marks(), HashMap::from([(book[0], Mark::Placeholder)]));
        let both = build(
            &session,
            &[],
            &[warning(&session, "one.md", 1)],
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
        let report = build(&session, &[theme], &[], &HashMap::new(), "Project", &|_| None);
        assert_eq!(report.sections[0].chapter, None);
        assert_eq!(report.sections[0].entries[0].message, "theme.json:4: unknown property");
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
        let report = build(&session, &[], &[short], &HashMap::new(), "Project", &|page| {
            (page == 2).then_some((two, 7))
        });
        assert_eq!(report.sections[0].chapter, Some(two));
        let entry = &report.sections[0].entries[0];
        assert_eq!((entry.page, entry.place), (Some(2), Some((two, 7))));
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
