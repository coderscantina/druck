//! Carrying what an agent wrote to a file into the session's open buffer of that file.
//!
//! TEMPORARY, until the Persistence slice reloads files that change on disk into their buffers and merges them. It
//! stands in for that merge so the agent's edits show up in the editor now, and it has the one property that merge
//! must keep: text the author typed is never lost.
//!
//! The agent's patch is applied to the buffer, not the file's text put in its place. Each hunk is found in the buffer
//! by its lines, wherever the author's edits have moved it, and replaced as an ordinary edit with one undo step. A
//! hunk whose lines the author changed too is left alone and counted, so the buffer keeps the author's version.

use std::path::Path;

use super::changes::{self, Outcome, Tracked};
use crate::session::{ChapterId, Session};
use crate::source::{Edit, EditKind};

/// What carrying a patch did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Carried {
    /// Hunks no open buffer could take because the author edited their lines. The buffer keeps the author's text.
    pub conflicts: usize,
    /// Why an edit was refused, if one was.
    pub errors: Vec<String>,
}

/// The chapters of the session that are the file at `path`. A single file's sections all are.
pub fn chapters_at(session: &Session, path: &Path) -> Vec<ChapterId> {
    let contents = session.contents();
    let rows = [
        &contents.book,
        &contents.not_in_book,
        &contents.notes,
        &contents.research,
        &contents.manifest,
    ];
    let wanted = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    rows.into_iter()
        .flatten()
        .map(|row| row.id)
        .filter(|id| {
            session.chapter(*id).is_some_and(|chapter| {
                let own = std::fs::canonicalize(chapter.path()).unwrap_or_else(|_| chapter.path().to_owned());
                own == wanted
            })
        })
        .collect()
}

/// Carries the hunks that are not in a buffer yet into the buffers of `path`, and binds each hunk to its chapter.
pub fn carry(session: &mut Session, path: &Path, hunks: &mut [Tracked]) -> Carried {
    let chapters = chapters_at(session, path);
    let mut pending: Vec<usize> = (0..hunks.len()).filter(|at| hunks[*at].chapter.is_none()).collect();
    let mut carried = Carried::default();
    for id in &chapters {
        if pending.is_empty() {
            break;
        }
        let text = session.buffer(*id).text().to_owned();
        let mut subset: Vec<Tracked> = pending.iter().map(|at| hunks[*at].clone()).collect();
        let outcomes = changes::merge(&mut subset, &text);
        let mut edits = Vec::new();
        let mut conflicted = Vec::new();
        for ((at, mut tracked), outcome) in pending.iter().copied().zip(subset).zip(outcomes) {
            match outcome {
                Outcome::Replace(replacement) => edits.push(Edit::new(replacement.range, replacement.text)),
                Outcome::Already => {}
                Outcome::Conflict => {
                    conflicted.push(at);
                    continue;
                }
            }
            tracked.chapter = Some(*id);
            hunks[at] = tracked;
        }
        pending = conflicted;
        if !edits.is_empty() {
            edits.sort_by_key(|edit| edit.range.start);
            if let Err(error) = session.edit_chapter(*id, edits, EditKind::Discrete) {
                carried.errors.push(error.to_string());
            }
        }
    }
    // A hunk no open chapter could take stays with the first, where it reads as changed since. A file with no open
    // chapter has nothing to conflict with.
    if let Some(first) = chapters.first() {
        carried.conflicts = pending.len();
        for at in pending {
            hunks[at].chapter = Some(*first);
        }
    }
    carried
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::agent::changes::{Hunk, Presence};

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("druck-carry-{name}-{}", std::process::id()));
            fs::remove_dir_all(&dir).ok();
            fs::create_dir_all(dir.join("manuscript")).expect("folder");
            fs::write(dir.join("druck.yaml"), "chapters:\n  - manuscript/one.md\n").expect("manifest");
            fs::write(
                dir.join("manuscript/one.md"),
                "# One\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n",
            )
            .expect("chapter");
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    /// The patch the CLI reports for changing the second paragraph on disk.
    fn second_paragraph(path: &Path) -> (PathBuf, Vec<Tracked>) {
        let patch = json!([{"oldStart": 3, "oldLines": 5, "newStart": 3, "newLines": 5, "lines": [
            " First paragraph.", " ", "-Second paragraph.", "+Second paragraph, rewritten.", " ", " Third paragraph."]}]);
        let hunks = Hunk::parse_all(&patch).into_iter().map(Tracked::new).collect();
        (path.join("manuscript/one.md"), hunks)
    }

    fn open(folder: &Folder) -> (Session, ChapterId) {
        let session = Session::open(&folder.0).expect("opens");
        let id = session.book()[0];
        (session, id)
    }

    #[test]
    fn the_agents_edit_arrives_and_the_authors_unsaved_edit_stays() {
        let folder = Folder::new("both");
        let (mut session, id) = open(&folder);
        // The author types at the top while the agent works on the file on disk.
        session
            .edit_chapter(id, vec![Edit::new(0..0, "Typed by the author. ")], EditKind::Typing)
            .expect("edit");
        let (file, mut hunks) = second_paragraph(&folder.0);
        let carried = carry(&mut session, &file, &mut hunks);
        assert_eq!(carried, Carried::default());
        assert_eq!(
            session.buffer(id).text(),
            "Typed by the author. # One\n\nFirst paragraph.\n\nSecond paragraph, rewritten.\n\nThird paragraph.\n"
        );
        assert_eq!(hunks[0].chapter, Some(id));
    }

    #[test]
    fn an_edit_to_the_same_lines_keeps_the_authors_text() {
        let folder = Folder::new("overlap");
        let (mut session, id) = open(&folder);
        let at = session.buffer(id).text().find("Second").expect("text");
        session
            .edit_chapter(
                id,
                vec![Edit::new(at..at, "My own opening of the second. ")],
                EditKind::Typing,
            )
            .expect("edit");
        let before = session.buffer(id).text().to_owned();
        let (file, mut hunks) = second_paragraph(&folder.0);
        let carried = carry(&mut session, &file, &mut hunks);
        assert_eq!(carried.conflicts, 1);
        assert_eq!(session.buffer(id).text(), before);
        // The hunk reads as changed since, so the panel offers no revert over the author's words.
        assert_eq!(hunks[0].presence(&before), Presence::Moved);
    }

    #[test]
    fn a_buffer_that_already_has_the_edit_is_not_edited_twice() {
        let folder = Folder::new("twice");
        let (mut session, id) = open(&folder);
        let (file, mut hunks) = second_paragraph(&folder.0);
        carry(&mut session, &file, &mut hunks);
        let once = session.buffer(id).text().to_owned();
        let (_, mut again) = second_paragraph(&folder.0);
        assert_eq!(carry(&mut session, &file, &mut again), Carried::default());
        assert_eq!(session.buffer(id).text(), once);
    }

    #[test]
    fn a_revert_is_one_undo_step_and_undo_brings_the_edit_back() {
        let folder = Folder::new("undo");
        let (mut session, id) = open(&folder);
        let original = session.buffer(id).text().to_owned();
        let (file, mut hunks) = second_paragraph(&folder.0);
        carry(&mut session, &file, &mut hunks);
        let agent_text = session.buffer(id).text().to_owned();
        let revert = hunks[0].revert(&agent_text).expect("revert");
        session
            .edit_chapter(id, vec![Edit::new(revert.range, revert.text)], EditKind::Discrete)
            .expect("edit");
        assert_eq!(session.buffer(id).text(), original);
        session.active_buffer_mut().undo().expect("one step to undo");
        assert_eq!(session.buffer(id).text(), agent_text);
    }

    #[test]
    fn a_file_the_session_does_not_have_open_binds_nothing() {
        let folder = Folder::new("closed");
        let (mut session, _) = open(&folder);
        let (_, mut hunks) = second_paragraph(&folder.0);
        let carried = carry(&mut session, &folder.0.join("manuscript/other.md"), &mut hunks);
        assert_eq!(carried, Carried::default());
        assert_eq!(hunks[0].chapter, None);
    }
}
