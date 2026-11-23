//! A single file's change from outside, undone and redone as one step.

use std::fs;
use std::path::PathBuf;

use crate::persist::merge::edits_between;
use crate::session::{Session, Undone};
use crate::source::{Edit, EditKind};

const TEXT: &str = "# One\n\nFirst.\n\n# Two\n\nSecond.\n\n# Three\n\nThird.\n";

/// A single file in a folder of its own, open in a session.
struct Single {
    dir: PathBuf,
    path: PathBuf,
    session: Session,
}

impl Single {
    fn new(name: &str) -> Self {
        Self::with(name, TEXT)
    }

    fn with(name: &str, text: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("druck-outside-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let path = druck::resource::normalize(&dir.join("doc.md"));
        fs::write(&path, text).unwrap();
        let session = Session::open(&path).unwrap();
        Self { dir, path, session }
    }

    fn text(&self) -> String {
        self.session.file_text(&self.path).unwrap()
    }

    /// Another program writes `theirs`, and the session takes it in as the sync does.
    fn outside(&mut self, theirs: &str) {
        let edits = edits_between(&self.text(), theirs);
        let path = self.path.clone();
        self.session.apply_to_file(&path, &edits, EditKind::External).unwrap();
    }

    /// Types in the section at `index`, replacing `old` with `new`.
    fn type_in(&mut self, index: usize, old: &str, new: &str) {
        let id = self.session.book()[index];
        self.session.set_active(id);
        let at = self.session.buffer(id).text().find(old).unwrap();
        (self.session)
            .edit_chapter(id, vec![Edit::new(at..at + old.len(), new)], EditKind::Typing)
            .unwrap();
    }

    /// Undo as the galley, its notes, and a table's cells do it.
    fn undo(&mut self) {
        self.session.undo();
    }

    fn redo(&mut self) {
        self.session.redo();
    }
}

impl Drop for Single {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

#[test]
fn one_undo_takes_back_an_outside_change_in_every_section_and_redo_brings_it_back() {
    let mut single = Single::new("sections");
    let theirs = TEXT
        .replace("First.", "First, theirs.")
        .replace("Third.", "Third, theirs.");
    let third = single.session.book()[2];
    single.session.set_active(third);
    single.outside(&theirs);
    assert_eq!(single.text(), theirs);

    single.undo();
    assert_eq!(single.text(), TEXT);
    single.redo();
    assert_eq!(single.text(), theirs);
}

#[test]
fn an_outside_change_that_moves_headings_is_one_step_and_keeps_the_typing_before_it() {
    let mut single = Single::new("headings");
    single.type_in(1, "Second.", "Typed.");
    let mine = single.text();
    // A heading turns into a paragraph, and a new one starts a section.
    let theirs = mine
        .replace("# Two", "Two")
        .replace("Third.", "Third.\n\n# Four\n\nFourth.");
    single.outside(&theirs);
    assert_eq!(single.session.book().len(), 3);
    assert_eq!(single.text(), theirs);

    single.undo();
    assert_eq!(single.text(), mine);
    assert_eq!(single.session.book().len(), 3);
    single.undo();
    assert_eq!(single.text(), TEXT, "the typing before the change keeps its own step");
    single.redo();
    single.redo();
    assert_eq!(single.text(), theirs);
}

#[test]
fn typing_after_an_outside_change_comes_back_first_and_elsewhere_stays() {
    let mut single = Single::new("after");
    let theirs = TEXT.replace("First.", "First, theirs.");
    single.outside(&theirs);
    single.type_in(2, "Third.", "Third, mine.");
    single.type_in(0, "First, theirs.", "First, both.");

    single.undo();
    assert_eq!(single.text(), theirs.replace("Third.", "Third, mine."));
    single.undo();
    assert_eq!(
        single.text(),
        TEXT.replace("Third.", "Third, mine."),
        "the typing in a section the change left alone stays"
    );
}

#[test]
fn typing_right_after_an_outside_change_is_its_own_step_and_comes_back_first() {
    let mut single = Single::new("coalesce");
    single.type_in(2, "Third.", "Third, mine.");
    let mine = single.text();
    let theirs = mine.replace("First.", "First, theirs.");
    single.outside(&theirs);
    // Within a second, at the caret the typing before left: it would join that step.
    single.type_in(2, "mine.", "mine, again.");

    single.undo();
    assert_eq!(single.text(), theirs);
    single.undo();
    assert_eq!(single.text(), mine);

    // Typing after an undo is newer than the change too, also where it would join the step below.
    single.redo();
    single.type_in(2, "mine.", "mine, again.");
    single.undo();
    single.type_in(2, "mine.", "mine, once more.");
    single.undo();
    assert_eq!(single.text(), theirs);
}

#[test]
fn a_heading_typed_in_a_new_section_leaves_the_change_unreachable() {
    let mut single = Single::new("resplit");
    let theirs = TEXT.replace("First.", "First, theirs.");
    single.outside(&theirs);
    single.type_in(0, "First, theirs.", "First.\n\n# Half\n\nMore.");
    // Leaving the section resplits the file, and the new section's buffer goes.
    assert!(single.session.resplit());

    assert!(!single.session.undo_is_outside_file_change());
    assert!(!single.session.undo_outside_file_change());
}

#[test]
fn a_change_from_an_unreadable_file_is_no_undo_step() {
    let mut single = Single::new("unreadable");
    let path = single.path.clone();
    single.session.mark_unreadable(&path, "not UTF-8".into());
    let theirs = TEXT.replace("Second.", "Second, readable again.");
    fs::write(&path, &theirs).unwrap();
    single.outside(&theirs);
    single.session.set_synced(&path, theirs.as_str().into(), true);
    single.session.mark_clean(&path);

    assert!(!single.session.undo_outside_file_change());
    assert_eq!(single.text(), theirs);
    single.session.save().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), theirs);
}

#[test]
fn a_write_of_the_undone_version_leaves_the_redone_one_unsaved() {
    let mut single = Single::new("redo-save");
    let path = single.path.clone();
    let theirs = TEXT.replace("First.", "First, theirs.");
    fs::write(&path, &theirs).unwrap();
    single.outside(&theirs);
    single.session.set_synced(&path, theirs.as_str().into(), true);
    single.session.mark_clean(&path);

    single.undo();
    let outgoing = single.session.outgoing();
    assert_eq!(outgoing.len(), 1);
    // The write is under way while the author redoes.
    single.redo();
    outgoing[0].write().unwrap();
    single.session.written(&outgoing[0]);
    assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);

    assert!(single.session.file_is_dirty(&path), "the disk holds the undone version");
    single.session.save().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), theirs);
}

#[test]
fn undo_takes_typing_after_the_change_then_the_change_then_typing_before_it() {
    let mut single = Single::new("order");
    let third = single.session.book()[2];
    single.session.set_active(third);
    // Discrete steps stay apart, as typing with a pause between does.
    let edit = |single: &mut Single, old: &str, new: &str| {
        let at = single.session.buffer(third).text().find(old).unwrap();
        (single.session)
            .edit_chapter(third, vec![Edit::new(at..at + old.len(), new)], EditKind::Discrete)
            .unwrap();
    };
    edit(&mut single, "Third.", "Third, A.");
    let theirs = single.text().replace("First.", "First, theirs.");
    single.outside(&theirs);
    edit(&mut single, "Third, A.", "Third, AB.");

    single.undo();
    assert_eq!(single.text(), theirs);
    single.undo();
    assert_eq!(single.text(), TEXT.replace("Third.", "Third, A."));
    single.undo();
    assert_eq!(single.text(), TEXT);
}

#[test]
fn every_surface_undoes_the_files_outside_change_before_typing_from_before_it() {
    let mut single = Single::new("surfaces");
    single.type_in(0, "First.", "First, mine.");
    let mine = single.text();
    let theirs = mine.replace("Third.", "Third, theirs.");
    single.outside(&theirs);

    assert!(matches!(single.session.undo(), Some(Undone::File)));
    assert_eq!(single.text(), mine);
    assert!(matches!(single.session.undo(), Some(Undone::Own(_))));
    assert_eq!(single.text(), TEXT);
    assert!(matches!(single.session.redo(), Some(Undone::Own(_))));
    assert!(matches!(single.session.redo(), Some(Undone::File)));
    assert_eq!(single.text(), theirs);
}

#[test]
fn an_outside_change_ends_the_step_an_offer_from_before_it_would_join() {
    let mut single = Single::new("offer");
    single.type_in(0, "First.", "First, mine.");
    let one = single.session.book()[0];
    let (revision, clock) = (
        single.session.buffer(one).revision(),
        single.session.buffer(one).clock(),
    );
    assert!(single.session.buffer(one).amends(clock));
    let theirs = single.text().replace("Third.", "Third, theirs.");
    single.outside(&theirs);
    let buffer = single.session.buffer(one);
    assert_eq!(buffer.revision(), revision, "the change left the section alone");
    assert!(!buffer.amends(clock), "a removal accepted now is a step of its own");
}

#[test]
fn a_section_back_from_an_undo_stays_apart_from_typing_since_above_it() {
    let mut single = Single::with("swap-apart", "# One\n\nFirst.\n\nTwo\n===\n\nSecond.\n");
    single.outside("# One\n\nFirst.\n\n# Two\n\nSecond.\n");
    let one = single.session.book()[0];
    let end = single.session.buffer(one).len();
    (single.session)
        .edit_chapter(one, vec![Edit::new(end..end, "\nAdded.")], EditKind::Typing)
        .unwrap();
    assert_eq!(single.text(), "# One\n\nFirst.\n\n\nAdded.\n# Two\n\nSecond.\n");

    let two = single.session.book()[1];
    single.session.set_active(two);
    single.undo();
    assert_eq!(single.text(), "# One\n\nFirst.\n\n\nAdded.\n\nTwo\n===\n\nSecond.\n");
    assert!(
        !single.session.resplit(),
        "the paragraph does not take the setext heading's words"
    );
}
