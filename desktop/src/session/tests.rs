//! The session against real files in a temporary folder, and through the engine where the behaviour is what the pages
//! show.

use std::fs;
use std::path::{Path, PathBuf};

use druck::cancel::Cancel;
use druck::output::Item;
use druck::resource::normalize;

use super::details::Field;
use super::*;
use crate::preview::job::Typesetter;
use crate::source::buffer::Selection as Caret;

/// A folder that removes itself.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("druck-session-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("temporary folder");
        Self(normalize(&dir))
    }

    fn write(&self, path: &str, text: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().expect("a file has a folder")).expect("folder");
        fs::write(&path, text).expect("file");
        path
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join(path)).expect("file")
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

const MANIFEST_TEXT: &str = "# The book\ntitle: A small book # working title\nlang: en\nchapters:\n  # opening\n  - manuscript/one.md\n  - manuscript/two.md\n  - manuscript/three.md\nexports:\n  print:\n    final: true\n";

fn project(name: &str) -> (Folder, Session) {
    let folder = Folder::new(name);
    folder.write("druck.yaml", MANIFEST_TEXT);
    folder.write("manuscript/one.md", "# One {#sec:one}\n\nFirst chapter text.\n");
    folder.write("manuscript/two.md", "<!-- notes -->\n\n# Two\n\nSecond chapter text.\n");
    folder.write("manuscript/three.md", "# Three {-}\n\nThird chapter text.");
    folder.write("manuscript/stray.md", "# A stray chapter\n");
    folder.write("notes/ideas.md", "An idea.\n");
    folder.write("notes/archive/old.md", "An old idea.\n");
    folder.write("notes/picture.png", "not text");
    folder.write("research/source.md", "A source.\n");
    let session = Session::open(&folder.0).expect("opens");
    (folder, session)
}

fn typing(session: &mut Session, id: ChapterId, at: usize, text: &str) {
    session
        .edit_chapter(id, vec![Edit::new(at..at, text)], EditKind::Typing)
        .expect("edit");
}

fn titles(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|row| row.title.as_str()).collect()
}

fn layout(session: &Session) -> druck::output::Layout {
    let mut typesetter = Typesetter::new(session.kind().clone(), None);
    match typesetter.run(session.joined(), &Cancel::default()) {
        Ok(layout) => layout.laid,
        Err(failure) => panic!("typesets: {failure}"),
    }
}

fn page_text(layout: &druck::output::Layout) -> String {
    let mut text = String::new();
    for item in layout.pages().iter().flat_map(|page| &page.items) {
        if let Item::Text { run, .. } = item {
            text.push_str(&run.text);
        }
    }
    text
}

#[test]
fn a_project_opens_with_its_chapters_and_groups() {
    let (_folder, session) = project("open");
    let contents = session.contents();
    assert_eq!(titles(&contents.book), ["One", "Two", "Three"]);
    assert_eq!(titles(&contents.not_in_book), ["A stray chapter"]);
    assert_eq!(titles(&contents.notes), ["archive/old", "ideas"]);
    assert_eq!(titles(&contents.research), ["source"]);
    assert_eq!(session.details().title, "A small book");
    assert_eq!(session.active(), session.book()[0]);
}

#[test]
fn numbers_count_only_numbered_chapters() {
    let (_folder, session) = project("numbers");
    let numbers: Vec<_> = session.contents().book.iter().map(|row| row.number).collect();
    assert_eq!(numbers, [Some(1), Some(2), None]);
}

#[test]
fn a_missing_chapter_is_a_broken_row_and_the_pages_name_it() {
    let folder = Folder::new("missing");
    folder.write(
        "druck.yaml",
        "chapters:\n  - manuscript/there.md\n  - manuscript/gone.md\n",
    );
    folder.write("manuscript/there.md", "Text.\n");
    let session = Session::open(&folder.0).expect("opens");
    let contents = session.contents();
    assert!(!contents.book[0].broken);
    assert!(contents.book[1].broken);
    let mut typesetter = Typesetter::new(session.kind().clone(), None);
    let Err(druck::Failure::Errors(problems)) = typesetter.run(session.joined(), &Cancel::default()) else {
        panic!("a missing chapter is an error");
    };
    assert!(problems[0].message.contains("gone.md"));
}

#[test]
fn a_broken_manifest_opens_itself_in_the_editor() {
    let folder = Folder::new("broken");
    folder.write("druck.yaml", "title: [unclosed\n");
    folder.write("manuscript/a.md", "Text.\n");
    let mut session = Session::open(&folder.0).expect("a manifest error does not stop the app");
    assert!(session.problem().is_some());
    assert_eq!(session.active_chapter().group(), Group::Manifest);
    assert!(session.details().problem.is_some());
    assert_eq!(titles(&session.contents().not_in_book), ["A"]);
    assert!(session.move_chapter(session.active(), 0).is_err());

    // Fixing it in the editor brings the book back.
    let id = session.active();
    let end = session.buffer(id).len();
    let fix = "title: Fixed\nchapters:\n  - manuscript/a.md\n";
    session
        .edit_chapter(id, vec![Edit::new(0..end, fix)], EditKind::Discrete)
        .expect("edit");
    session.save().expect("saves");
    assert!(session.problem().is_none());
    assert_eq!(titles(&session.contents().book), ["A"]);
    assert_eq!(session.active(), session.book()[0]);
    assert!(session.contents().manifest.is_empty());
}

#[test]
fn a_folder_without_a_manifest_is_an_error_and_a_manifest_path_opens_its_folder() {
    let folder = Folder::new("nomanifest");
    assert!(Session::open(&folder.0).is_err());
    assert!(Session::open(Path::new("no/such/document.md")).is_err());
    folder.write("druck.yaml", "chapters:\n  - a.md\n");
    folder.write("a.md", "Text\n");
    let session = Session::open(&folder.0.join("druck.yaml")).expect("opens");
    assert_eq!(session.location(), folder.0);
}

#[test]
fn a_single_file_splits_in_view_and_joins_to_the_same_bytes() {
    let folder = Folder::new("single");
    let text = "---\ntitle: Whole\n---\n\nIntro.\n\n# One\n\nText.\n\n<!-- about two -->\n\n# Two\n\nMore.\n";
    let path = folder.write("whole.md", text);
    let mut session = Session::open(&path).expect("opens");
    assert_eq!(titles(&session.contents().book), [FRONT_MATTER, "One", "Two"]);
    assert_eq!(session.joined().text, text);
    assert!(!session.is_dirty());

    let two = session.book()[2];
    assert!(session.buffer(two).text().starts_with("<!-- about two -->"));
    let end = session.buffer(two).len();
    typing(&mut session, two, end, "Added.\n");
    assert_eq!(session.save(), Ok(1));
    assert_eq!(folder.read("whole.md"), format!("{text}Added.\n"));
}

#[test]
fn offsets_in_a_single_file_map_through_its_sections() {
    let folder = Folder::new("offsets");
    let text = "Intro.\n\n# One\n\nText.\n\n# Two\n\nMore.\n";
    let path = folder.write("whole.md", text);
    let session = Session::open(&path).expect("opens");
    let at = text.find("More").expect("in the text");
    let (id, local) = session.locate(&path, at).expect("in the file");
    assert_eq!(id, session.book()[2]);
    assert_eq!(&session.buffer(id).text()[local..local + 4], "More");
    assert_eq!(session.file_position(id, local), Some((path.clone(), at)));
    let joined = session.joined();
    assert_eq!(joined.map.to_joined(id, local), Some(at));
    assert_eq!(joined.map.chapter_at(at), Some((id, local)));
}

#[test]
fn offsets_in_a_project_are_the_chapter_files() {
    let (folder, session) = project("project-offsets");
    let path = folder.0.join("manuscript/two.md");
    let (id, offset) = session.locate(&path, 20).expect("a chapter of the book");
    assert_eq!(id, session.book()[1]);
    assert_eq!(session.file_position(id, offset), Some((path, 20)));
    assert_eq!(session.locate(&folder.0.join("notes/ideas.md"), 0), None);
    let note = session.contents().notes[0].id;
    assert_eq!(session.file_position(note, 0), None);
}

#[test]
fn the_joined_text_follows_the_engines_rule() {
    let (_folder, session) = project("joined");
    let joined = session.joined();
    assert_eq!(
        joined.text,
        "# One {#sec:one}\n\nFirst chapter text.\n\n<!-- notes -->\n\n# Two\n\nSecond chapter text.\n\n# Three {-}\n\nThird chapter text.\n\n"
    );
    let two = session.book()[1];
    let start = joined.map.to_joined(two, 0).expect("in the book");
    assert!(joined.text[start..].starts_with("<!-- notes"));
}

#[test]
fn pages_show_unsaved_text_and_name_the_chapter_it_is_in() {
    let (folder, mut session) = project("live");
    let two = session.book()[1];
    let at = session.buffer(two).text().find("Second").expect("in the text");
    typing(&mut session, two, at, "Unsaved ");
    let laid = layout(&session);
    assert!(page_text(&laid).contains("Unsaved"));
    assert_eq!(
        folder.read("manuscript/two.md"),
        "<!-- notes -->\n\n# Two\n\nSecond chapter text.\n"
    );

    let map = laid.source_map().expect("the app asks for a map");
    let source = (laid.pages().iter().flat_map(|page| &page.items)).find_map(|item| match item {
        Item::Text {
            run, source: Some(id), ..
        } if run.text == "Unsaved" => Some(*id),
        _ => None,
    });
    let position = map.position(source.expect("on a page"), 0).expect("written");
    let chapter = &map.chapters()[position.chapter.0 as usize];
    let (found, offset) = session
        .locate(&chapter.path, position.offset as usize)
        .expect("the engine's path is the session's");
    assert_eq!(found, two);
    assert_eq!(offset, at);

    let (file, in_file) = session.file_position(two, at).expect("in the book");
    let located = laid.locate(druck::output::SourcePos {
        chapter: position.chapter,
        offset: in_file as u32,
    });
    assert_eq!(file, chapter.path);
    assert!(located.is_some());
}

#[test]
fn a_caret_in_a_chapters_opening_comment_shows_at_its_first_text() {
    let (_folder, session) = project("comment-caret");
    let two = session.book()[1];
    let (file, offset) = session.page_position(two, 3).expect("in the book");
    assert_eq!(&session.buffer(two).text()[offset..offset + 5], "# Two");
    let laid = layout(&session);
    let map = laid.source_map().expect("the app asks for a map");
    let chapter = map
        .chapters()
        .iter()
        .position(|chapter| chapter.path == file)
        .expect("a chapter");
    let located = laid.locate(druck::output::SourcePos {
        chapter: druck::output::ChapterId(chapter as u32),
        offset: offset as u32,
    });
    let located = located.expect("on a page");
    let Item::Text { run, .. } = &laid.pages()[located.page].items[located.item] else {
        panic!("text");
    };
    assert_eq!(run.text, "2", "the heading's number is its block's first text");
    let inside = session.buffer(two).text().find("Second").expect("in the text");
    assert_eq!(session.page_position(two, inside).expect("in the book").1, inside);
}

#[test]
fn an_edit_changes_only_its_chapters_file() {
    let (folder, mut session) = project("one-file");
    let one_before = folder.read("manuscript/one.md");
    let three_before = folder.read("manuscript/three.md");
    let manifest_before = folder.read("druck.yaml");
    let two = session.book()[1];
    let end = session.buffer(two).len();
    typing(&mut session, two, end, "More.\n");
    assert_eq!(session.save(), Ok(1));
    assert!(folder.read("manuscript/two.md").ends_with("More.\n"));
    assert_eq!(folder.read("manuscript/one.md"), one_before);
    assert_eq!(folder.read("manuscript/three.md"), three_before);
    assert_eq!(folder.read("druck.yaml"), manifest_before);
    assert_eq!(session.save(), Ok(0));
}

#[test]
fn each_chapter_keeps_its_undo_history_and_caret() {
    let (_folder, mut session) = project("undo");
    let (one, two) = (session.book()[0], session.book()[1]);
    typing(&mut session, one, 0, "A");
    session.active_chapter_mut().view.selection = Caret::caret(1);
    session.set_active(two);
    typing(&mut session, two, 0, "B");
    session.active_chapter_mut().view.selection = Caret::caret(1);
    session.set_active(one);
    assert_eq!(session.active_chapter().view.selection.head, 1);
    assert!(session.active_buffer_mut().undo().is_some());
    assert!(session.buffer(one).text().starts_with("# One"));
    assert!(
        session.buffer(two).text().starts_with('B'),
        "the other chapter keeps its edit"
    );
    session.set_active(two);
    assert!(session.active_buffer_mut().undo().is_some());
    assert!(session.buffer(two).text().starts_with("<!--"));
}

#[test]
fn a_new_heading_in_a_single_file_makes_a_section_once_the_caret_leaves() {
    let folder = Folder::new("resplit");
    let text = "# One\n\nText.\n\n# Two\n\nMore.\n";
    let path = folder.write("whole.md", text);
    let mut session = Session::open(&path).expect("opens");
    let (one, two) = (session.book()[0], session.book()[1]);
    let end = session.buffer(two).len();
    typing(&mut session, two, end, "\n# Three\n\nNew.\n");
    assert_eq!(session.book().len(), 2, "not while typing");
    session.set_active(two);
    session.active_chapter_mut().view.selection = Caret::caret(session.buffer(two).len());
    assert!(session.resplit());
    assert_eq!(titles(&session.contents().book), ["One", "Two", "Three"]);
    assert_eq!(session.book()[0], one, "an unchanged section keeps its buffer");
    assert_eq!(session.active(), session.book()[2], "the caret stays with its text");
    assert!(session.is_dirty());
    session.save().expect("saves");
    assert_eq!(folder.read("whole.md"), format!("{text}\n# Three\n\nNew.\n"));
    assert!(!session.is_dirty());
    assert!(!session.resplit());
}

#[test]
fn a_removed_heading_in_a_single_file_merges_the_sections() {
    let folder = Folder::new("merge");
    let path = folder.write("whole.md", "# One\n\nText.\n\n# Two\n\nMore.\n");
    let mut session = Session::open(&path).expect("opens");
    let two = session.book()[1];
    session
        .edit_chapter(two, vec![Edit::new(0..2, "")], EditKind::Deleting)
        .expect("edit");
    let before = session.revision();
    assert!(session.resplit());
    assert_eq!(titles(&session.contents().book), ["One"]);
    assert!(session.revision() > before);
    assert_eq!(session.joined().text, "# One\n\nText.\n\nTwo\n\nMore.\n");
}

#[test]
fn the_revision_moves_with_the_book_and_not_with_a_note() {
    let (_folder, mut session) = project("revision");
    let start = session.revision();
    let note = session.contents().notes[0].id;
    session.set_active(note);
    assert_eq!(session.active_buffer().text(), "An old idea.\n", "read when opened");
    typing(&mut session, note, 0, "X");
    assert_eq!(session.revision(), start, "a note is never built");

    let one = session.book()[0];
    typing(&mut session, one, 0, "X");
    assert!(session.revision() > start);
    let moved = session.revision();
    session.move_chapter(one, 2).expect("moves");
    assert!(session.revision() > moved);
}

#[test]
fn reordering_writes_the_manifest_and_keeps_comments() {
    let (folder, mut session) = project("reorder");
    let three = session.book()[2];
    session.move_chapter(three, 0).expect("moves");
    assert_eq!(titles(&session.contents().book), ["Three", "One", "Two"]);
    assert_eq!(
        folder.read("druck.yaml"),
        "# The book\ntitle: A small book # working title\nlang: en\nchapters:\n  - manuscript/three.md\n  # opening\n  - manuscript/one.md\n  - manuscript/two.md\nexports:\n  print:\n    final: true\n"
    );
    let reopened = Session::open(&folder.0).expect("opens");
    assert_eq!(titles(&reopened.contents().book), ["Three", "One", "Two"]);
    assert!(page_text(&layout(&session)).find("Third") < page_text(&layout(&session)).find("First"));
}

#[test]
fn moving_to_the_end_and_to_the_same_place_work() {
    let (folder, mut session) = project("move-end");
    let one = session.book()[0];
    session.move_chapter(one, 3).expect("moves");
    assert_eq!(titles(&session.contents().book), ["Two", "Three", "One"]);
    let manifest = folder.read("druck.yaml");
    session.move_chapter(one, 3).expect("nothing to do");
    assert_eq!(folder.read("druck.yaml"), manifest);
}

#[test]
fn removing_and_adding_moves_between_the_lists_and_keeps_the_file() {
    let (folder, mut session) = project("remove");
    let two = session.book()[1];
    session.remove_from_book(two).expect("removes");
    let contents = session.contents();
    assert_eq!(titles(&contents.book), ["One", "Three"]);
    assert!(titles(&contents.not_in_book).contains(&"Two"));
    assert!(!folder.read("druck.yaml").contains("two.md"));
    assert!(folder.read("manuscript/two.md").contains("Second"));
    assert!(!page_text(&layout(&session)).contains("Second"));

    session.add_to_book(two).expect("adds");
    assert_eq!(titles(&session.contents().book), ["One", "Three", "Two"]);
    assert!(page_text(&layout(&session)).contains("Second"));
}

#[test]
fn a_note_listed_in_the_manifest_joins_the_book_with_its_text() {
    let (folder, mut session) = project("note-listed");
    folder.write("druck.yaml", "chapters:\n  - notes/ideas.md\n");
    session.read_manifest();
    assert_eq!(session.buffer(session.book()[0]).text(), "An idea.\n");
    assert!(page_text(&layout(&session)).contains("idea"));
}

#[test]
fn the_last_chapter_cannot_be_removed() {
    let folder = Folder::new("last");
    folder.write("druck.yaml", "chapters:\n  - a.md\n");
    folder.write("a.md", "Text\n");
    let mut session = Session::open(&folder.0).expect("opens");
    assert!(session.remove_from_book(session.book()[0]).is_err());
}

#[test]
fn renaming_changes_the_heading_as_an_undoable_edit() {
    let (folder, mut session) = project("rename");
    let one = session.book()[0];
    session.rename(one, "Opening").expect("renames");
    assert_eq!(
        session.buffer(one).text(),
        "# Opening {#sec:one}\n\nFirst chapter text.\n"
    );
    assert_eq!(session.title_of(one), "Opening");
    assert_eq!(
        folder.read("manuscript/one.md"),
        "# One {#sec:one}\n\nFirst chapter text.\n",
        "until saved"
    );
    assert!(session.rename(one, "  ").is_err());
}

#[test]
fn a_chapter_without_a_heading_renames_its_file() {
    let folder = Folder::new("rename-file");
    folder.write("druck.yaml", "chapters:\n  - manuscript/00-front.md # first\n");
    folder.write("manuscript/00-front.md", "Imprint text\n");
    let mut session = Session::open(&folder.0).expect("opens");
    let id = session.book()[0];
    assert_eq!(session.title_of(id), "Front");
    session.rename(id, "Title page").expect("renames");
    assert_eq!(session.title_of(id), "Title page");
    assert!(folder.0.join("manuscript/title-page.md").exists());
    assert!(!folder.0.join("manuscript/00-front.md").exists());
    assert_eq!(folder.read("druck.yaml"), "chapters:\n  - manuscript/title-page.md\n");
    assert!(page_text(&layout(&session)).contains("Imprint"));
}

#[test]
fn a_new_section_in_a_project_is_a_new_listed_file_after_the_chapter() {
    let (folder, mut session) = project("new-section");
    let (one, chapter) = (session.book()[0], template("chapter"));
    let id = session.new_section(chapter, Some("Fresh start"), one).expect("adds");
    assert_eq!(session.active(), id);
    assert_eq!(titles(&session.contents().book), ["One", "Fresh start", "Two", "Three"]);
    assert_eq!(
        folder.read("manuscript/fresh-start.md"),
        "# Fresh start\n\nStart writing here.\n"
    );
    assert!(
        folder
            .read("druck.yaml")
            .contains("- manuscript/one.md\n  - manuscript/fresh-start.md\n")
    );
    let second = session.new_section(chapter, Some("Fresh start"), id).expect("adds");
    assert_eq!(
        session.chapter(second).expect("known").path(),
        folder.0.join("manuscript/fresh-start-2.md")
    );
}

#[test]
fn a_new_section_in_a_single_file_is_text_undo_takes_back() {
    let folder = Folder::new("new-in-file");
    let text = "# One\n\nText.\n\n# Two\n\nMore.\n";
    let path = folder.write("whole.md", text);
    let mut session = Session::open(&path).expect("opens");
    let one = session.book()[0];
    session
        .new_section(template("chapter"), Some("Middle"), one)
        .expect("adds");
    assert_eq!(titles(&session.contents().book), ["One", "Middle", "Two"]);
    assert_eq!(
        session.joined().text,
        "# One\n\nText.\n\n# Middle\n\nStart writing here.\n# Two\n\nMore.\n"
    );
    assert_eq!(session.active(), session.book()[1]);

    // Text with no level 1 heading stays in its section, and undo takes it back.
    let two = session.book()[2];
    let id = session.new_section(template("imprint"), None, two).expect("adds");
    assert_eq!(id, two);
    assert!(session.buffer(two).text().contains("::: bottom"));
    session.set_active(two);
    assert!(session.active_buffer_mut().undo().is_some());
    assert!(!session.joined().text.contains("::: bottom"));
}

fn template(slug: &str) -> &'static Template {
    TEMPLATES
        .iter()
        .find(|template| template.slug == slug)
        .expect("a template")
}

/// A project with a title and an author, in the bundled theme, whose only chapter is `text`.
fn minimal_project(name: &str, text: &str) -> (Folder, Session) {
    let folder = Folder::new(name);
    folder.write(
        "druck.yaml",
        "title: T\nsubtitle: S\nauthor: A\nchapters:\n  - manuscript/a.md\n",
    );
    folder.write("manuscript/a.md", text);
    let session = Session::open(&folder.0).expect("opens");
    (folder, session)
}

#[test]
fn every_template_renders_in_the_bundled_theme() {
    for template in TEMPLATES {
        let text = format!(
            "{}\n# After\n\nA paragraph with a mark{{^term}}.\n",
            template.markdown(None)
        );
        let (_folder, session) = minimal_project(&format!("template-{}", template.slug), &text);
        let laid = layout(&session);
        assert!(!laid.pages().is_empty(), "{}", template.slug);
        assert!(page_text(&laid).contains("After"), "{}", template.slug);
    }
}

#[test]
fn a_named_template_takes_the_name_as_its_heading() {
    assert_eq!(
        template("part").markdown(Some("Teil I")),
        "# Teil I {-}\n\n::: page-break\n"
    );
    assert_eq!(template("imprint").markdown(Some("Ignored")), template("imprint").text);
}

#[test]
fn a_section_written_out_has_no_placeholder_the_document_lacks() {
    let folder = Folder::new("placeholders");
    folder.write("druck.yaml", "title: Set\nchapters:\n  - a.md\n");
    folder.write("a.md", "Text\n");
    let mut session = Session::open(&folder.0).expect("opens");
    let first = session.book()[0];
    let id = session.new_section(template("title-page"), None, first).expect("adds");
    assert_eq!(
        session.buffer(id).text(),
        "**{title}**\n\nSubtitle\n\nAuthor\n\n::: page-break\n"
    );
    assert!(page_text(&layout(&session)).contains("Set"));
}

#[test]
fn every_starter_opens_renders_and_keeps_its_manifest_readable() {
    for starter in STARTERS {
        let folder = Folder::new(&format!("starter-{}", starter.slug));
        let dir = folder.0.join("My Project");
        starter.create(&dir).expect("creates");
        let session = Session::open(&dir).expect("opens");
        assert_eq!(session.details().title, "My Project");
        assert!(session.problem().is_none(), "{}", starter.slug);
        assert!(session.contents().not_in_book.is_empty(), "{}", starter.slug);
        assert!(
            session.theme_file().is_some_and(|theme| theme.starts_with(&dir)),
            "{}",
            starter.slug
        );
        let laid = layout(&session);
        assert!(!laid.pages().is_empty(), "{}", starter.slug);
        assert!(
            laid.warnings()
                .iter()
                .all(|warning| !warning.message.contains("unknown"))
        );
        assert!(starter.create(&dir).is_err(), "a starter does not write over a project");
    }
}

#[test]
fn every_template_renders_in_every_starters_theme() {
    for starter in STARTERS {
        for template in TEMPLATES {
            // A starter that places its own contents, bibliography, or index would have two of them.
            let folder = Folder::new(&format!("mix-{}-{}", starter.slug, template.slug));
            let dir = folder.0.join("project");
            starter.create(&dir).expect("creates");
            let mut session = Session::open(&dir).expect("opens");
            let has_own = session.joined().text.contains(template.text.trim());
            if has_own {
                continue;
            }
            let last = *session.book().last().expect("a chapter");
            session.new_section(template, None, last).expect("adds");
            let mut typesetter = Typesetter::new(session.kind().clone(), None);
            if let Err(failure) = typesetter.run(session.joined(), &Cancel::default()) {
                panic!("{} in {}: {failure}", template.slug, starter.slug);
            }
        }
    }
}

#[test]
fn the_manifest_details_write_as_splices_and_refresh_the_pages() {
    let (folder, mut session) = project("details");
    let before = session.revision();
    session.set_field(Field::Title, "New: a title").expect("sets");
    assert!(session.revision() > before, "the pages refresh");
    session.set_field(Field::Subtitle, "A subtitle").expect("sets");
    session
        .set_field(Field::Authors, "Ada Example, Bo Sample")
        .expect("sets");
    session.set_field(Field::Date, "2026").expect("sets");
    session.set_meta("isbn", "978-3-16-148410-0").expect("sets");
    session.set_meta("publisher", "Harbour Press").expect("sets");
    assert_eq!(
        folder.read("druck.yaml"),
        "# The book\ntitle: \"New: a title\" # working title\nsubtitle: A subtitle\nauthor: [Ada Example, Bo Sample]\ndate: \"2026\"\nlang: en\nmeta:\n  isbn: 978-3-16-148410-0\n  publisher: Harbour Press\nchapters:\n  # opening\n  - manuscript/one.md\n  - manuscript/two.md\n  - manuscript/three.md\nexports:\n  print:\n    final: true\n"
    );
    let details = session.details();
    assert_eq!(details.title, "New: a title");
    assert_eq!(details.authors, "Ada Example, Bo Sample");
    assert_eq!(details.date, "2026");
    assert_eq!(details.meta.len(), 2);

    session.remove_meta("isbn").expect("removes");
    session.set_field(Field::Subtitle, "").expect("removes");
    session.remove_meta("publisher").expect("removes");
    session.set_field(Field::Date, "").expect("removes");
    session.set_field(Field::Authors, "").expect("removes");
    session.set_field(Field::Title, "A small book").expect("sets");
    assert_eq!(folder.read("druck.yaml"), MANIFEST_TEXT);
}

#[test]
fn a_change_the_engine_rejects_changes_nothing() {
    let (folder, mut session) = project("rejected");
    assert!(session.set_field(Field::Language, "xx").is_err());
    assert!(session.set_meta("bad key!", "x").is_err());
    assert_eq!(folder.read("druck.yaml"), MANIFEST_TEXT);
}

#[test]
fn a_manifest_edited_on_disk_is_not_overwritten_by_the_next_change() {
    let (folder, mut session) = project("outside");
    let edited = MANIFEST_TEXT.replace("# The book", "# Edited elsewhere");
    folder.write("druck.yaml", &edited);
    session.set_field(Field::Date, "Soon").expect("sets");
    assert!(folder.read("druck.yaml").starts_with("# Edited elsewhere"));
}

#[test]
fn front_matter_details_change_only_the_front_matter() {
    let folder = Folder::new("front-matter");
    let body = "# One\n\nText   with  odd   spacing.\n\n# Two\n\nMore.\n";
    let path = folder.write(
        "whole.md",
        &format!("---\n# a comment\ntitle: Old\nlang: de\n---\n\n{body}"),
    );
    let mut session = Session::open(&path).expect("opens");
    session.set_field(Field::Title, "New").expect("sets");
    session.set_meta("publisher", "Harbour").expect("sets");
    assert_eq!(
        session.file_text(&path).expect("read"),
        format!("---\n# a comment\ntitle: New\nlang: de\nmeta:\n  publisher: Harbour\n---\n\n{body}")
    );
    assert_eq!(session.details().title, "New");
    let first = session.book()[0];
    session.set_active(first);
    assert!(
        session.active_buffer_mut().undo().is_some(),
        "an edit undo can take back"
    );
}

#[test]
fn a_file_without_front_matter_gets_one_and_loses_it_when_emptied() {
    let folder = Folder::new("no-front-matter");
    let text = "# One\n\nText.\n";
    let path = folder.write("plain.md", text);
    let mut session = Session::open(&path).expect("opens");
    assert_eq!(
        session.details().title,
        "plain",
        "the file's name stands in for a title"
    );
    session.set_field(Field::Authors, "Ada").expect("sets");
    assert_eq!(
        session.file_text(&path).expect("read"),
        format!("---\nauthor: Ada\n---\n\n{text}")
    );
    session.set_field(Field::Authors, "").expect("removes");
    assert_eq!(session.file_text(&path).expect("read"), text);
}

#[test]
fn a_cover_outside_a_project_is_copied_in_and_inside_stays_put() {
    let (folder, mut session) = project("cover");
    let outside = Folder::new("cover-outside");
    let image = outside.write("my cover.png", "image bytes");
    session.set_cover(&image).expect("sets");
    assert_eq!(session.details().cover, "assets/my cover.png");
    assert_eq!(folder.read("assets/my cover.png"), "image bytes");
    assert_eq!(session.cover_file(), Some(folder.0.join("assets/my cover.png")));

    let inside = folder.write("art/front.png", "image bytes");
    session.set_cover(&inside).expect("sets");
    assert_eq!(session.details().cover, "art/front.png");
}

#[test]
fn a_document_theme_is_found_as_the_engine_finds_it() {
    let folder = Folder::new("theme");
    let path = folder.write("book.md", "---\ntheme: themes/print.json\n---\n\nText\n");
    let session = Session::open(&path).expect("opens");
    assert_eq!(session.theme_file(), Some(folder.0.join("themes/print.json")));
    let plain = folder.write("plain.md", "Text\n");
    assert_eq!(Session::open(&plain).expect("opens").theme_file(), None);
}

#[test]
fn a_file_that_is_not_text_stays_as_it_is() {
    let folder = Folder::new("binary");
    folder.write("druck.yaml", "chapters:\n  - manuscript/a.md\n  - manuscript/bad.md\n");
    folder.write("manuscript/a.md", "Text\n");
    fs::write(folder.0.join("manuscript/bad.md"), [0xff, 0xfe, 0x00]).expect("file");
    let mut session = Session::open(&folder.0).expect("opens");
    let bad = session.book()[1];
    assert!(session.contents().book[1].broken);
    assert!(session.chapter(bad).expect("known").problem().is_some());
    typing(&mut session, bad, 0, "X");
    assert_eq!(session.save(), Ok(0));
    assert_eq!(
        fs::read(folder.0.join("manuscript/bad.md")).expect("file"),
        [0xff, 0xfe, 0x00]
    );
}

#[test]
fn engine_goes_from_markdown_to_pages_and_pdf() {
    let path = druck::resource::normalize(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/report.md"));
    let session = Session::open(&path).expect("opens");
    let laid = layout(&session);
    let pdf = laid
        .pdf(&druck::PdfOptions::default(), &Cancel::default())
        .unwrap_or_else(|_| panic!("the sample writes"));
    assert!(pdf.parts > 0);
    assert!(pdf.bytes.starts_with(b"%PDF"));
}

#[test]
fn every_starter_leaves_its_exports_out_of_git() {
    for starter in STARTERS {
        let dir = std::env::temp_dir().join(format!("druck-starter-ignore-{}-{}", starter.slug, std::process::id()));
        fs::remove_dir_all(&dir).ok();
        starter.create(&dir).expect("made");
        assert_eq!(fs::read_to_string(dir.join(".gitignore")).expect("written"), "build/\n");
        fs::remove_dir_all(&dir).ok();
    }
}
