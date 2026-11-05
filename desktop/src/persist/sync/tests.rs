//! The sync against real files in a temporary folder, as the watcher would report them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::session::Session;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A project on disk, open in a session, with a sync whose kept copies go to a folder of their own.
struct Fixture {
    dir: PathBuf,
    data: PathBuf,
    session: Session,
    sync: FileSync,
}

const MANIFEST: &str = "title: A book\nchapters:\n  - manuscript/one.md\n  - manuscript/two.md\n";

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "druck-sync-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::remove_dir_all(&base).ok();
        let dir = druck::resource::normalize(&base.join("book"));
        let data = base.join("data");
        fs::create_dir_all(dir.join("manuscript")).unwrap();
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join("druck.yaml"), MANIFEST).unwrap();
        fs::write(dir.join("manuscript/one.md"), "# One\n\nFirst line.\n\nSecond line.\n").unwrap();
        fs::write(dir.join("manuscript/two.md"), "# Two\n\nOther text.\n").unwrap();
        let session = Session::open(&dir).unwrap();
        let sync = FileSync::new(Some(Copies::new(&data, &dir)));
        Self {
            dir,
            data,
            session,
            sync,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn one(&self) -> PathBuf {
        self.path("manuscript/one.md")
    }

    fn id(&self, name: &str) -> ChapterId {
        self.session.chapter_of_file(&self.path(name)).expect("a chapter")
    }

    fn text(&self, name: &str) -> String {
        self.session.file_text(&self.path(name)).expect("text")
    }

    /// Types in the text as the galley does, replacing `old` with `new`.
    fn type_in(&mut self, name: &str, old: &str, new: &str) {
        let id = self.id(name);
        let at = self.text(name).find(old).expect("the text to change");
        self.session
            .edit_chapter(id, vec![Edit::new(at..at + old.len(), new)], EditKind::Typing)
            .unwrap();
    }

    /// Another program replaces the file, then the watcher reports it.
    fn outside(&mut self, name: &str, text: &str) -> Option<FileChange> {
        let path = self.path(name);
        fs::write(&path, text).unwrap();
        self.sync
            .on_disk(&mut self.session, &path, Some(text.as_bytes().to_vec()))
    }

    fn save(&mut self) {
        for outgoing in self.session.outgoing() {
            let result = outgoing.write();
            if self.sync.finish_write(&mut self.session, &outgoing, result) {
                let disk = fs::read(&outgoing.path).ok();
                self.sync.on_disk(&mut self.session, &outgoing.path, disk);
            }
        }
    }

    fn disk(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap()
    }

    fn kept(&self) -> Vec<String> {
        let mut texts: Vec<String> = walk(&self.data.join("kept"))
            .into_iter()
            .map(|path| fs::read_to_string(path).unwrap())
            .collect();
        texts.sort();
        texts
    }
}

use crate::source::Edit;

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.path().is_dir() {
            found.extend(walk(&entry.path()));
        } else {
            found.push(entry.path());
        }
    }
    found
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(parent) = self.dir.parent() {
            fs::remove_dir_all(parent).ok();
        }
    }
}

const ONE: &str = "manuscript/one.md";

#[test]
fn a_clean_buffer_takes_the_new_text_with_its_caret_on_the_same_words() {
    let mut fixture = Fixture::new();
    let id = fixture.id(ONE);
    let second = fixture.text(ONE).find("Second").unwrap();
    fixture.session.active_chapter_mut().view.selection = crate::source::Selection::caret(second);
    assert_eq!(fixture.session.active(), id);

    let new = "# One\n\nAdded above.\n\nFirst line.\n\nSecond line.\n";
    let change = fixture.outside(ONE, new).unwrap();

    assert_eq!(change.kind, ChangeKind::Reloaded);
    assert_eq!(fixture.text(ONE), new);
    assert!(!fixture.session.is_dirty());
    assert_eq!(
        fixture.session.active_chapter().view.selection.head,
        new.find("Second").unwrap()
    );
}

#[test]
fn a_dirty_buffer_merges_and_stays_unsaved_until_written() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "First line, mine.");
    let change = fixture
        .outside(ONE, "# One\n\nFirst line.\n\nSecond line, theirs.\n")
        .unwrap();

    assert_eq!(change.kind, ChangeKind::Merged);
    let merged = "# One\n\nFirst line, mine.\n\nSecond line, theirs.\n";
    assert_eq!(fixture.text(ONE), merged);
    assert!(fixture.session.is_dirty());
    fixture.save();
    assert_eq!(fixture.disk(ONE), merged);
    assert!(!fixture.session.is_dirty());
}

#[test]
fn overlapping_edits_wait_and_nothing_overwrites_the_file() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    let theirs = "# One\n\nTheirs.\n\nSecond line.\n";
    let change = fixture.outside(ONE, theirs).unwrap();

    assert_eq!(change.kind, ChangeKind::Conflict);
    assert_eq!(fixture.sync.conflicts().count(), 1);
    assert!(fixture.text(ONE).contains("Mine."));
    fixture.save();
    assert_eq!(fixture.disk(ONE), theirs, "a conflict holds saves back");
}

#[test]
fn keeping_mine_writes_it_and_keeps_a_copy_of_theirs() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    let theirs = "# One\n\nTheirs.\n\nSecond line.\n";
    fixture.outside(ONE, theirs);
    let path = fixture.one();
    let kept = fixture.sync.resolve(&mut fixture.session, &path, Choice::Mine).unwrap();

    assert!(kept.is_some());
    assert_eq!(fixture.kept(), vec![theirs.to_owned()]);
    fixture.save();
    assert_eq!(fixture.disk(ONE), "# One\n\nMine.\n\nSecond line.\n");
    assert_eq!(fixture.sync.conflicts().count(), 0);
}

#[test]
fn taking_theirs_keeps_a_copy_of_mine_and_undo_brings_mine_back() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    let mine = fixture.text(ONE);
    let theirs = "# One\n\nTheirs.\n\nSecond line.\n";
    fixture.outside(ONE, theirs);
    let path = fixture.one();
    fixture
        .sync
        .resolve(&mut fixture.session, &path, Choice::Theirs)
        .unwrap();

    assert_eq!(fixture.text(ONE), theirs);
    assert!(!fixture.session.is_dirty(), "the file already holds it");
    assert_eq!(fixture.kept(), vec![mine.clone()]);
    let buffer = fixture.session.active_buffer_mut();
    assert!(buffer.undo().is_none());
    buffer.undo();
    assert_eq!(buffer.text(), mine);
}

#[test]
fn seeing_both_writes_both_and_the_comments_set_nothing() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    fixture.outside(ONE, "# One\n\nTheirs.\n\nSecond line.\n");
    let path = fixture.one();
    fixture.sync.resolve(&mut fixture.session, &path, Choice::Both).unwrap();
    fixture.save();
    let disk = fixture.disk(ONE);
    assert!(disk.contains("Mine.\n"), "{disk}");
    assert!(disk.contains("Theirs.\n"), "{disk}");
    assert!(disk.contains("<!-- <<<<<<< yours -->"), "{disk}");
}

#[test]
fn seeing_both_names_the_agent_and_the_author_in_the_books_language() {
    let mut fixture = Fixture::new();
    fs::write(fixture.path("druck.yaml"), format!("{MANIFEST}lang: de\n")).unwrap();
    fixture.session = Session::open(&fixture.dir).unwrap();
    fixture.sync.set_source(Source::Agent);
    fixture.type_in(ONE, "First line.", "Mine.");
    let change = fixture
        .outside(ONE, "# One\n\nTheirs.\n\nSecond line.\n")
        .expect("a conflict");
    assert_eq!(change.source, Source::Agent);
    let path = fixture.one();
    fixture.sync.resolve(&mut fixture.session, &path, Choice::Both).unwrap();
    let text = fixture.text(ONE);
    assert!(text.contains("<!-- <<<<<<< deins -->\n"), "{text}");
    assert!(text.contains("<!-- >>>>>>> der Agent -->\n"), "{text}");
}

#[test]
fn typing_that_removes_the_overlap_settles_the_conflict() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    fixture.outside(ONE, "# One\n\nTheirs.\n\nSecond line.\n");
    assert!(!fixture.sync.can_settle_a_conflict(&fixture.session));

    fixture.type_in(ONE, "Mine.", "First line.");
    fixture.type_in(ONE, "Second line.", "Second line, mine.");
    assert!(fixture.sync.can_settle_a_conflict(&fixture.session));
    fixture.sync.settle_conflicts(&mut fixture.session);

    assert_eq!(fixture.sync.conflicts().count(), 0);
    assert_eq!(fixture.text(ONE), "# One\n\nTheirs.\n\nSecond line, mine.\n");
}

#[test]
fn a_newer_change_from_outside_replaces_the_conflict_it_clears() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    fixture.outside(ONE, "# One\n\nTheirs.\n\nSecond line.\n");
    let change = fixture
        .outside(ONE, "# One\n\nFirst line.\n\nSecond line, theirs.\n")
        .unwrap();
    assert_eq!(change.kind, ChangeKind::Merged);
    assert_eq!(fixture.sync.conflicts().count(), 0);
    assert_eq!(fixture.text(ONE), "# One\n\nMine.\n\nSecond line, theirs.\n");
}

#[test]
fn an_autosave_never_overwrites_a_change_it_has_not_seen() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "First line, mine.");
    // The file changes after the last look and before the write.
    fs::write(fixture.one(), "# One\n\nFirst line.\n\nSecond line, theirs.\n").unwrap();
    fixture.save();

    let merged = "# One\n\nFirst line, mine.\n\nSecond line, theirs.\n";
    assert_eq!(fixture.text(ONE), merged, "the write came back as a merge");
    assert!(fixture.disk(ONE).contains("theirs"));
    fixture.save();
    assert_eq!(fixture.disk(ONE), merged);
}

#[test]
fn our_own_write_is_recognized_by_its_content() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Typed.");
    fixture.save();
    let revision = fixture.session.active_buffer().revision();
    let path = fixture.one();
    let disk = fs::read(&path).unwrap();
    assert!(!fixture.sync.differs(&fixture.session, &path, Some(&disk)));
    assert!(fixture.sync.on_disk(&mut fixture.session, &path, Some(disk)).is_none());
    assert_eq!(fixture.session.active_buffer().revision(), revision);
}

#[test]
fn a_change_after_a_reject_does_not_bring_back_what_was_rejected() {
    let mut fixture = Fixture::new();
    let path = fixture.path("manuscript/one.md");
    let agent = "# One\n\nFirst line, shorter now and for good.\n\nSecond line.\n";
    fs::write(&path, agent).unwrap();
    let change = fixture.sync.on_disk(&mut fixture.session, &path, fs::read(&path).ok());
    assert!(change.is_some());
    // The author rejects it, and the rejected text is saved.
    let rejected = "# One\n\nFirst line.\n\nSecond line.\n";
    fixture.session.disown_versions(&path);
    fixture.session.set_synced(&path, rejected.into(), true);
    fs::write(&path, rejected).unwrap();
    let edits = crate::persist::merge::edits_between(agent, rejected);
    fixture
        .session
        .apply_to_file(&path, &edits, EditKind::Discrete)
        .unwrap();
    fixture.session.mark_clean(&path);
    // The agent tries again from that text, closer to its first try than to what it started from.
    let again = "# One\n\nFirst line, shorter now.\n\nSecond line.\n";
    fs::write(&path, again).unwrap();
    fixture.sync.on_disk(&mut fixture.session, &path, fs::read(&path).ok());
    assert_eq!(fixture.session.file_text(&path).as_deref(), Some(again));
}

#[test]
fn a_write_that_started_from_an_older_read_does_not_take_back_what_was_saved() {
    let mut fixture = Fixture::new();
    let read_by_the_agent = fixture.text(ONE);
    fixture.type_in(ONE, "First line.", "First line, typed.");
    fixture.save();
    fixture.type_in(ONE, "First line, typed.", "First line, typed again.");
    fixture.save();
    let theirs = read_by_the_agent.replace("Second line.", "Second line, by the agent.");

    let change = fixture.outside(ONE, &theirs).unwrap();
    assert_eq!(change.kind, ChangeKind::Merged);
    assert_eq!(
        fixture.text(ONE),
        "# One\n\nFirst line, typed again.\n\nSecond line, by the agent.\n"
    );
    fixture.save();
    assert!(fixture.disk(ONE).contains("typed again"));
    assert!(fixture.disk(ONE).contains("by the agent"));
}

#[test]
fn deleting_text_saved_earlier_in_the_session_stays_deleted() {
    let mut fixture = Fixture::new();
    let passage = " A long passage typed and saved early in the session, then deleted by a tool.";
    fixture.type_in(ONE, "First line.", &format!("First line.{passage}"));
    fixture.save();
    fixture.type_in(ONE, "Second line.", "Second line!");
    fixture.save();
    let theirs = fixture.text(ONE).replace(passage, "");

    let change = fixture.outside(ONE, &theirs).unwrap();
    assert_eq!(change.kind, ChangeKind::Reloaded);
    assert_eq!(fixture.text(ONE), theirs);
}

#[test]
fn removing_both_markers_from_outside_is_taken_as_it_is() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    fixture.outside(ONE, "# One\n\nTheirs.\n\nSecond line.\n");
    let path = fixture.one();
    fixture.sync.resolve(&mut fixture.session, &path, Choice::Both).unwrap();
    fixture.save();
    let unmarked: String = (fixture.disk(ONE).lines())
        .filter(|line| !line.starts_with("<!-- "))
        .map(|line| format!("{line}\n"))
        .collect();

    let change = fixture.outside(ONE, &unmarked).unwrap();
    assert_eq!(change.kind, ChangeKind::Reloaded);
    assert_eq!(fixture.text(ONE), unmarked);
    assert!(fixture.sync.conflicts().next().is_none());
}

#[test]
fn git_putting_an_older_version_back_is_taken_as_it_is() {
    let mut fixture = Fixture::new();
    let committed = fixture.text(ONE);
    fixture.type_in(ONE, "First line.", "First line, typed.");
    fixture.save();

    let change = fixture.outside(ONE, &committed).unwrap();
    assert_eq!(change.kind, ChangeKind::Reloaded);
    assert_eq!(fixture.text(ONE), committed);
    assert!(!fixture.session.is_dirty());
}

#[test]
fn an_outside_change_is_one_undo_step_that_stops_first() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Typed.");
    fixture.save();
    let typed = fixture.text(ONE);
    let new = "# One\n\nTyped.\n\nSecond line, theirs.\n";
    fixture.outside(ONE, new);

    let buffer = fixture.session.active_buffer_mut();
    assert!(buffer.undo_is_outside_change());
    assert!(buffer.undo().is_none());
    assert_eq!(buffer.text(), new, "the first undo only stops");
    assert!(buffer.undo().is_some());
    assert_eq!(buffer.text(), typed);
    buffer.redo();
    assert_eq!(buffer.text(), new);
}

#[test]
fn a_removed_clean_chapter_stays_a_broken_row_and_returns_with_its_file() {
    let mut fixture = Fixture::new();
    let revision = fixture.session.revision();
    fs::remove_file(fixture.path("manuscript/two.md")).unwrap();
    let path = fixture.path("manuscript/two.md");
    let change = fixture.sync.on_disk(&mut fixture.session, &path, None).unwrap();
    assert_eq!(change.kind, ChangeKind::Removed);
    let id = fixture.id("manuscript/two.md");
    assert!(
        fixture
            .session
            .contents()
            .book
            .iter()
            .any(|row| row.id == id && row.broken)
    );
    assert_ne!(fixture.session.revision(), revision);

    let back = fixture.text("manuscript/two.md");
    fixture.outside("manuscript/two.md", &back);
    assert!(!fixture.session.contents().book.iter().any(|row| row.broken));
    assert!(!fixture.session.is_dirty());
}

#[test]
fn a_removed_note_leaves_the_session() {
    let mut fixture = Fixture::new();
    fs::write(fixture.path("notes/idea.md"), "An idea.\n").unwrap();
    fixture.session.discover_files();
    let path = fixture.path("notes/idea.md");
    assert!(fixture.session.knows_file(&path));
    fs::remove_file(&path).unwrap();
    fixture.sync.on_disk(&mut fixture.session, &path, None);
    assert!(!fixture.session.knows_file(&path));
}

#[test]
fn a_removed_file_with_unsaved_text_asks_and_mine_brings_it_back() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    let path = fixture.one();
    fs::remove_file(&path).unwrap();
    let change = fixture.sync.on_disk(&mut fixture.session, &path, None).unwrap();
    assert_eq!(change.kind, ChangeKind::Conflict);
    assert!(fixture.sync.conflict_is_removal(&path));
    fixture.save();
    assert!(!path.exists(), "a deleted file does not come back without asking");

    fixture.sync.resolve(&mut fixture.session, &path, Choice::Mine).unwrap();
    fixture.save();
    assert!(fixture.disk(ONE).contains("Mine."));
}

#[test]
fn a_removed_file_with_unsaved_text_can_go_with_a_copy_kept() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    let mine = fixture.text(ONE);
    let path = fixture.one();
    fs::remove_file(&path).unwrap();
    fixture.sync.on_disk(&mut fixture.session, &path, None);
    fixture
        .sync
        .resolve(&mut fixture.session, &path, Choice::Theirs)
        .unwrap();
    assert_eq!(fixture.kept(), vec![mine]);
    assert!(!fixture.session.is_dirty());
    fixture.save();
    assert!(!path.exists());
}

#[test]
fn a_rename_keeps_the_chapter_and_its_undo_and_the_manifest_follows() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Typed.");
    fixture.save();
    let id = fixture.id(ONE);
    let new = fixture.path("manuscript/renamed.md");
    fs::rename(fixture.one(), &new).unwrap();

    let ready = Ready {
        paths: vec![fixture.one(), new.clone()],
        everything: false,
        resource: false,
    };
    let report = fixture.sync.process(&mut fixture.session, &ready);
    assert!(report.files_changed);
    assert_eq!(fixture.session.chapter_of_file(&new), Some(id));
    assert!(
        fs::read_to_string(fixture.path("druck.yaml"))
            .unwrap()
            .contains("manuscript/renamed.md")
    );
    assert!(fixture.session.active_buffer_mut().undo().is_some());
    assert!(!fixture.session.contents().book.iter().any(|row| row.broken));
}

#[test]
fn a_changed_manifest_changes_the_book() {
    let mut fixture = Fixture::new();
    let first = fixture.session.book()[0];
    fs::write(
        fixture.path("druck.yaml"),
        "title: Another\nchapters:\n  - manuscript/two.md\n  - manuscript/one.md\n",
    )
    .unwrap();
    let ready = Ready {
        paths: vec![fixture.path("druck.yaml")],
        everything: false,
        resource: false,
    };
    let report = fixture.sync.process(&mut fixture.session, &ready);
    assert!(report.manifest);
    assert_eq!(fixture.session.book()[1], first);
    assert_eq!(fixture.session.details().title, "Another");
}

#[test]
fn our_own_manifest_write_is_no_change() {
    let mut fixture = Fixture::new();
    let two = fixture.id("manuscript/two.md");
    fixture.session.move_chapter(two, 0).unwrap();
    let manifest = fixture.path("druck.yaml");
    let disk = fs::read(&manifest).unwrap();
    assert!(!fixture.sync.differs(&fixture.session, &manifest, Some(&disk)));
}

#[test]
fn a_new_file_appears_and_a_folder_that_went_keeps_all_the_text() {
    let mut fixture = Fixture::new();
    let new = fixture.path("manuscript/new.md");
    fs::write(&new, "# New\n").unwrap();
    let report = fixture.sync.process(
        &mut fixture.session,
        &Ready {
            paths: vec![new.clone()],
            everything: false,
            resource: false,
        },
    );
    assert!(report.files_changed);
    assert!(fixture.session.knows_file(&new));

    fixture.type_in(ONE, "First line.", "Mine.");
    let moved = fixture.dir.with_file_name("book-moved");
    fs::rename(&fixture.dir, &moved).unwrap();
    fixture.sync.process(
        &mut fixture.session,
        &Ready {
            paths: Vec::new(),
            everything: true,
            resource: false,
        },
    );
    assert!(fixture.sync.root_is_gone(&fixture.session));
    assert!(fixture.text(ONE).contains("Mine."));
    fixture.save();
    assert!(fixture.sync.problems().count() >= 1, "the save failed and says so");
    assert!(fixture.text(ONE).contains("Mine."), "a failed save keeps the text");
    assert!(!fixture.dir.exists(), "a save does not make up the folder");

    fs::rename(&moved, &fixture.dir).unwrap();
    assert!(fixture.sync.check_root(&fixture.session));
    fixture.save();
    assert_eq!(fixture.sync.problems().count(), 0);
    assert!(fixture.disk(ONE).contains("Mine."));
}

#[test]
fn a_failed_save_keeps_the_text_and_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new();
    fs::set_permissions(fixture.one(), fs::Permissions::from_mode(0o444)).unwrap();
    fixture.type_in(ONE, "First line.", "Mine.");
    fixture.save();
    assert!(matches!(fixture.sync.problems().next(), Some((_, Problem::Save(_)))));
    assert!(fixture.text(ONE).contains("Mine."));
    assert!(fixture.session.is_dirty());
    assert!(!fixture.disk(ONE).contains("Mine."));

    fs::set_permissions(fixture.one(), fs::Permissions::from_mode(0o644)).unwrap();
    fixture.save();
    assert_eq!(fixture.sync.problems().count(), 0);
    assert!(fixture.disk(ONE).contains("Mine."));
}

#[test]
fn text_that_turns_unreadable_stays_and_nothing_writes_over_it() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    let path = fixture.one();
    let bytes = vec![0xff, 0xfe, 0x00];
    fs::write(&path, &bytes).unwrap();
    fixture.sync.on_disk(&mut fixture.session, &path, Some(bytes.clone()));
    assert!(matches!(
        fixture.sync.problems().next(),
        Some((_, Problem::Unreadable(_)))
    ));
    assert!(fixture.text(ONE).contains("Mine."));
    fixture.save();
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_crash_copy_merges_with_what_the_file_holds_by_now() {
    let fixture = Fixture::new();
    let base = fixture.text(ONE);
    let copies = fixture.sync.copies().unwrap().clone();
    copies
        .store(&fixture.one(), &base, &base.replace("First line.", "Unsaved."))
        .unwrap();
    let on_disk_by_now = base.replace("Second line.", "Second line, theirs.");
    fs::write(fixture.one(), &on_disk_by_now).unwrap();

    let mut session = Session::open(&fixture.dir).unwrap();
    let mut sync = FileSync::new(Some(copies.clone()));
    for copy in copies.load() {
        session.restore_text(&copy.path, &copy.base, &copy.text).unwrap();
        sync.on_disk(&mut session, &copy.path, fs::read(&copy.path).ok());
    }
    let path = fixture.one();
    assert_eq!(
        session.file_text(&path).unwrap(),
        "# One\n\nUnsaved.\n\nSecond line, theirs.\n"
    );
    assert!(session.is_dirty());
}

#[test]
fn a_single_file_applies_a_change_to_the_section_it_falls_in_and_keeps_the_others_history() {
    let base = std::env::temp_dir().join(format!(
        "druck-sync-single-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).unwrap();
    let path = druck::resource::normalize(&base.join("doc.md"));
    fs::write(&path, "# A\n\nFirst.\n\n# B\n\nSecond.\n").unwrap();
    let mut session = Session::open(&path).unwrap();
    let mut sync = FileSync::new(None);
    let second = session.book()[1];
    session.set_active(second);
    let at = session.active_buffer().text().find("Second").unwrap();
    session
        .edit_chapter(second, vec![Edit::new(at..at + 6, "Typed")], EditKind::Typing)
        .unwrap();

    let theirs = "# A\n\nFirst, theirs.\n\n# B\n\nSecond.\n";
    fs::write(&path, theirs).unwrap();
    let change = sync
        .on_disk(&mut session, &path, Some(theirs.as_bytes().to_vec()))
        .unwrap();

    assert_eq!(change.kind, ChangeKind::Merged);
    assert_eq!(
        session.file_text(&path).unwrap(),
        "# A\n\nFirst, theirs.\n\n# B\n\nTyped.\n"
    );
    assert_eq!(session.book().len(), 2);
    assert_eq!(session.active(), second);
    session.active_buffer_mut().undo();
    assert_eq!(
        session.file_text(&path).unwrap(),
        "# A\n\nFirst, theirs.\n\n# B\n\nSecond.\n"
    );
    fs::remove_dir_all(base).ok();
}

#[test]
fn the_pacer_saves_after_a_pause_and_keeps_saving_through_long_typing() {
    let start = Instant::now();
    let mut pacer = Pacer::default();
    assert_eq!(pacer.next(start), Next::Idle);
    pacer.edited(start);
    assert_eq!(pacer.next(start), Next::Wait(RECOVERY_PAUSE));
    assert_eq!(pacer.next(start + RECOVERY_PAUSE), Next::Recover);
    assert_eq!(pacer.next(start + SAVE_PAUSE), Next::Save);
    assert_eq!(pacer.next(start + SAVE_PAUSE), Next::Idle);

    let mut pacer = Pacer::default();
    let mut at = start;
    let mut saved = false;
    while at < start + SAVE_MOST + Duration::from_secs(1) {
        pacer.edited(at);
        at += Duration::from_millis(100);
        while let Next::Recover | Next::Save = {
            let next = pacer.next(at);
            saved |= next == Next::Save;
            next
        } {}
    }
    assert!(saved);
}

#[test]
fn the_pacer_tries_a_failed_save_again() {
    let start = Instant::now();
    let mut pacer = Pacer::default();
    pacer.retry_later(start);
    assert_eq!(pacer.next(start), Next::Wait(RETRY));
    assert_eq!(pacer.next(start + RETRY), Next::Save);
    assert_eq!(pacer.next(start + RETRY), Next::Idle);
}

#[test]
fn closing_writes_what_changed_and_says_so() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    assert_eq!(fixture.sync.write_all(&mut fixture.session), Closing::Saved);
    assert!(fixture.disk(ONE).contains("Mine."));
    assert!(!fixture.session.is_dirty());
}

#[test]
fn closing_keeps_the_text_and_asks_only_when_the_write_fails() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new();
    fs::set_permissions(fixture.one(), fs::Permissions::from_mode(0o444)).unwrap();
    fixture.type_in(ONE, "First line.", "Mine.");
    let closing = fixture.sync.write_all(&mut fixture.session);
    assert!(
        matches!(&closing, Closing::Failed { file, .. } if *file == fixture.one()),
        "{closing:?}"
    );
    assert!(fixture.text(ONE).contains("Mine."));
    assert!(!fixture.disk(ONE).contains("Mine."));
    assert!(
        fixture
            .sync
            .copies()
            .unwrap()
            .load()
            .iter()
            .any(|copy| copy.text.contains("Mine."))
    );
}

#[test]
fn closing_with_a_conflict_the_footer_holds_asks_nothing_and_keeps_both_versions() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    let theirs = "# One\n\nTheirs.\n\nSecond line.\n";
    fixture.outside(ONE, theirs);
    assert_eq!(fixture.sync.write_all(&mut fixture.session), Closing::Held);
    assert_eq!(fixture.disk(ONE), theirs, "their version stays in the file");
    let copies = fixture.sync.copies().unwrap().load();
    assert!(
        copies.iter().any(|copy| copy.text.contains("Mine.")),
        "mine is in a recovery copy"
    );
    assert_eq!(
        fixture.sync.conflicts().count(),
        1,
        "the footer still holds the question"
    );
}

#[test]
fn closing_merges_a_change_that_arrived_between_the_last_look_and_the_write() {
    let mut fixture = Fixture::new();
    fixture.type_in(ONE, "First line.", "Mine.");
    // Written without the watcher having reported it, so the session still holds the old file's hash.
    fs::write(fixture.one(), "# One\n\nFirst line.\n\nSecond line, changed.\n").unwrap();
    assert_eq!(fixture.sync.write_all(&mut fixture.session), Closing::Saved);
    let disk = fixture.disk(ONE);
    assert!(
        disk.contains("Mine.") && disk.contains("Second line, changed."),
        "{disk}"
    );
}

#[test]
fn a_git_merge_that_overlaps_holds_both_sides_until_a_choice() {
    let mut fixture = Fixture::new();
    let path = fixture.one();
    let ours = fixture.text("manuscript/one.md");
    let base = ours.replace("First line", "Older line");
    let theirs = ours.replace("First line", "Their line");
    fs::write(&path, &theirs).unwrap();
    fixture.session.restart_sync(&path, &base);
    let change = fixture
        .sync
        .on_disk(&mut fixture.session, &path, Some(theirs.clone().into_bytes()))
        .expect("a change");
    assert_eq!(change.kind, ChangeKind::Conflict);
    assert_eq!(fixture.text("manuscript/one.md"), ours);

    // Nothing may treat this as settled, though the text was saved before the merge began.
    assert_eq!(fixture.sync.write_all(&mut fixture.session), Closing::Held);
    assert_eq!(fixture.disk("manuscript/one.md"), theirs);
    let copies = fixture.sync.copies().expect("copies").load();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].text, ours);

    fixture.sync.resolve(&mut fixture.session, &path, Choice::Mine).unwrap();
    assert_eq!(fixture.sync.write_all(&mut fixture.session), Closing::Saved);
    assert_eq!(fixture.disk("manuscript/one.md"), ours);
    assert_eq!(fixture.kept().len(), 1);
}

/// An outside change merges with the author's unsaved edit in another sentence of the same line, and asks in the same
/// one.
#[test]
fn an_outside_change_merges_by_sentences_and_asks_in_the_same_sentence() {
    let mut fixture = Fixture::new();
    let path = fixture.one();
    let base = "# One\n\nFirst line. Of words.\n\nSecond line.\n";
    let ours = "# One\n\nFirst line. Of my words.\n\nSecond line.\n";
    let theirs = "# One\n\nFirst long line. Of words.\n\nSecond line.\n";
    fixture.type_in("manuscript/one.md", "First line.", "First line. Of my words.");
    fixture.save();
    assert_eq!(fixture.text("manuscript/one.md"), ours);
    fs::write(&path, theirs).unwrap();
    fixture.session.restart_sync(&path, base);
    let change = fixture
        .sync
        .on_disk(&mut fixture.session, &path, Some(theirs.as_bytes().to_vec()))
        .expect("a change");
    assert_eq!(change.kind, ChangeKind::Merged);
    assert_eq!(fixture.sync.write_all(&mut fixture.session), Closing::Saved);
    let merged = "# One\n\nFirst long line. Of my words.\n\nSecond line.\n";
    assert_eq!(fixture.disk("manuscript/one.md"), merged);

    fixture.type_in("manuscript/one.md", "Of my words.", "Of nothing.");
    let theirs = "# One\n\nFirst long line. Of all words.\n\nSecond line.\n";
    fs::write(&path, theirs).unwrap();
    let change = fixture
        .sync
        .on_disk(&mut fixture.session, &path, Some(theirs.as_bytes().to_vec()))
        .expect("a change");
    assert_eq!(change.kind, ChangeKind::Conflict);
    let passage = fixture.sync.conflict_passage(&fixture.session, &path).unwrap();
    assert_eq!(passage.mine, "First long line. Of nothing.\n");
    assert_eq!(passage.theirs, "First long line. Of all words.\n");
}
