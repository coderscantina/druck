//! Every rich edit against the engine: on a project of several chapters, on the samples, and on the book when it is on this
//! machine. An edit changes only what it says, its undo gives the exact bytes back, and its result renders.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use druck::cancel::Cancel;

use super::table::{Align, Caption, Grid};
use super::*;
use crate::editor::commands::normalized;
use crate::preview::job::Typesetter;
use crate::session::Session;
use crate::source::{Buffer, EditKind};

const CORPUS: &str = "Development/books/ttp404-gott-nicht-gefunden/buch-1-ttp404/manuscript";
const CORPUS_BIB: &str = "Development/books/ttp404-gott-nicht-gefunden/literatur.bib";

struct Project(PathBuf);

const BIB: &str = "@book{müller2020,\n  author = {Müller, Anna and Groß, Jörg},\n  title = {Die Straße},\n  publisher = {Verlag},\n  year = {2020},\n}\n\n@article{bostrom2003,\n  author = {Bostrom, Nick},\n  title = {Are You Living in a Computer Simulation?},\n  journal = {Philosophical Quarterly},\n  year = {2003},\n}\n";

const THEME: &str = r#"{"version": 1, "custom-styles": {"lead": {"based-on": "body", "align": "center"}, "checks": {"based-on": "list"}, "wide": {"based-on": "heading-1"}}}"#;

impl Project {
    fn new(name: &str, chapters: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("druck-rich-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("manuscript")).expect("folder");
        let list: Vec<String> = chapters
            .iter()
            .map(|(file, _)| format!("  - manuscript/{file}"))
            .collect();
        fs::write(
            dir.join("druck.yaml"),
            format!(
                "title: Größe\nlang: de\nbibliography: references.bib\nchapters:\n{}\n",
                list.join("\n")
            ),
        )
        .expect("manifest");
        fs::write(dir.join("references.bib"), BIB).expect("bibliography");
        for (file, text) in chapters {
            fs::write(dir.join("manuscript").join(file), text).expect("chapter");
        }
        Self(dir.canonicalize().expect("canonical"))
    }

    fn session(&self) -> Session {
        Session::open(&self.0).expect("the project opens")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn renders(session: &Session) -> Result<(), String> {
    let mut typesetter = Typesetter::new(session.kind().clone(), None);
    typesetter
        .run(session.joined(), &Cancel::default())
        .map(|_| ())
        .map_err(|failure| failure.to_string())
}

/// Applies edits to a buffer as one undo step and undoes them, giving back the result.
fn round_trip(text: &str, edits: &[Edit]) -> String {
    let mut buffer = Buffer::new(text);
    let edits = normalized(edits.to_vec(), text);
    assert!(!edits.is_empty(), "the edit changes something");
    let caret = Selection::caret(0);
    buffer
        .transact(EditKind::Discrete, edits, caret, caret, Instant::now())
        .expect("a valid edit");
    let result = buffer.text().to_owned();
    buffer.undo().expect("one undo step");
    assert_eq!(buffer.text(), text, "undo gives the exact bytes back");
    result
}

fn book_rich(session: &Session) -> Rich {
    let mut rich = Rich::default();
    rich.refresh(session);
    rich
}

fn chapter_named(session: &Session, name: &str) -> ChapterId {
    session
        .book()
        .iter()
        .copied()
        .find(|id| {
            session
                .chapter(*id)
                .is_some_and(|chapter| chapter.path().ends_with(name))
        })
        .expect("the chapter")
}

const ONE: &str = "# Eins {#sec:eins}\n\nText mit Notiz.[^k01-1] Und »noch« etwas.\n\nWeiter @sec:zwei und [@tbl:mass, page] [@müller2020, S. 12].\n\n| Größe | Maß |\n|:------|----:|\n| ß »x« | 1 |\n\n: Größe und Maß {#tbl:mass}\n\n![Bild ü](bild.svg){#fig:bild}\n\n[^k01-1]: Text der Notiz.\n\n    Zweiter Absatz mit Ä.\n";
const TWO: &str = "# Zwei {#sec:zwei}\n\nDie Notiz steht anderswo.[^k02-1]\n\n[^k03-fern]: Text in Kapitel zwei, von Kapitel drei genutzt.\n\n[^k02-1]: Hier.\n";
const THREE: &str = "# Drei\n\nReferenz auf fernen Text.[^k03-fern] Ohne eigene Texte.\n";

fn project(name: &str) -> Project {
    let project = Project::new(name, &[("11-k01.md", ONE), ("13-k02.md", TWO), ("14-k03.md", THREE)]);
    fs::write(
        project.0.join("bild.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="48"/>"#,
    )
    .expect("image");
    project
}

#[test]
fn the_project_renders_as_written() {
    let project = project("base");
    renders(&project.session()).expect("the base project renders");
}

#[test]
fn notes_number_across_chapters_and_labels_are_book_wide() {
    let project = project("numbers");
    let session = project.session();
    let rich = book_rich(&session);
    assert_eq!(rich.book.numbers["k01-1"], 1);
    assert_eq!(rich.book.numbers["k02-1"], 2);
    assert_eq!(rich.book.numbers["k03-fern"], 3);
    let (chapter, _) = &rich.book.definitions["k03-fern"];
    assert_eq!(*chapter, chapter_named(&session, "13-k02.md"));
    assert!(rich.book.label_set.contains("sec:zwei"));
    assert!(rich.book.label_set.contains("tbl:mass"));
    assert!(rich.book.label_set.contains("fig:bild"));
    assert_eq!(rich.book.keeping, Some(footnotes::Keeping::AtEnd));
}

#[test]
fn a_new_note_follows_the_pattern_and_renders_and_undoes() {
    let project = project("insert");
    let mut session = project.session();
    let id = chapter_named(&session, "14-k03.md");
    session.set_active(id);
    let rich = book_rich(&session);
    let text = session.active_buffer().text().to_owned();
    let blocks = Blocks::parse(&text);
    let facts = facts::scan(&text);
    let label = footnotes::next_label("14-k03", &facts, &rich.book.note_labels);
    assert_eq!(
        label, "k03-1",
        "the chapter's own stem, and `k03-fern` has no number to continue"
    );
    assert!(!rich.book.numbers.contains_key(&label));
    let at = text.find("Ohne").expect("found");
    let (command, body) = footnotes::insert(&text, &blocks, &facts, at..at, &label, "Größe »ß«.", rich.book.keeping);
    let result = round_trip(&text, &command.edits);
    assert_eq!(&result[body], "Größe »ß«.");
    assert_eq!(
        result,
        "# Drei\n\nReferenz auf fernen Text.[^k03-fern] [^k03-1]Ohne eigene Texte.\n\n[^k03-1]: Größe »ß«.\n"
    );
    session
        .edit_chapter(id, command.edits, EditKind::Discrete)
        .expect("valid");
    renders(&session).expect("the new note renders");
}

#[test]
fn a_note_kept_in_another_chapter_edits_there_and_only_there() {
    let project = project("cross");
    let mut session = project.session();
    let rich = book_rich(&session);
    let (chapter, _) = rich.book.definitions["k03-fern"].clone();
    let text = session.buffer(chapter).text().to_owned();
    let definition = facts::scan(&text)
        .definitions
        .into_iter()
        .find(|definition| definition.label == "k03-fern")
        .expect("defined");
    assert_eq!(
        footnotes::read_body(&text, &definition.range),
        "Text in Kapitel zwei, von Kapitel drei genutzt."
    );
    let edit = footnotes::write_body(&text, &definition.range, "Neu »ß«.\n\nZweiter Absatz.").expect("an edit");
    let result = round_trip(&text, std::slice::from_ref(&edit));
    assert_eq!(
        result,
        "# Zwei {#sec:zwei}\n\nDie Notiz steht anderswo.[^k02-1]\n\n[^k03-fern]: Neu »ß«.\n\n    Zweiter Absatz.\n\n[^k02-1]: Hier.\n"
    );
    let others: Vec<String> = session
        .book()
        .iter()
        .filter(|id| **id != chapter)
        .map(|id| session.buffer(*id).text().to_owned())
        .collect();
    session
        .edit_chapter(chapter, vec![edit], EditKind::Discrete)
        .expect("valid");
    renders(&session).expect("the note renders");
    let after: Vec<String> = session
        .book()
        .iter()
        .filter(|id| **id != chapter)
        .map(|id| session.buffer(*id).text().to_owned())
        .collect();
    assert_eq!(others, after, "other chapters stay byte-identical");
}

#[test]
fn a_definition_that_loses_its_reference_is_found_and_stays_visible() {
    let project = project("orphan");
    let mut session = project.session();
    let mut rich = book_rich(&session);
    let id = chapter_named(&session, "14-k03.md");
    let text = session.buffer(id).text().to_owned();
    let at = text.find("[^k03-fern]").expect("found");
    let edit = Edit::new(at..at + "[^k03-fern]".len(), "");
    session.edit_chapter(id, vec![edit], EditKind::Discrete).expect("valid");
    let orphaned = rich.refresh(&session);
    assert_eq!(orphaned, ["k03-fern"]);
    let (chapter, _) = rich.book.definitions["k03-fern"].clone();
    let blocks = Blocks::parse(session.buffer(chapter).text());
    let block = blocks
        .as_slice()
        .iter()
        .find(|block| block.kind == BlockKind::FootnoteDefinition)
        .expect("a definition");
    assert!(
        !rich.is_referenced(&session.buffer(chapter).text()[block.range.clone()]),
        "it shows as a raw block"
    );
    assert!(
        renders(&session).is_err(),
        "the engine reports the note with no reference"
    );
    // Removing it as the offer does leaves a book that renders.
    let text = session.buffer(chapter).text().to_owned();
    let facts = facts::scan(&text);
    let definition = facts
        .definitions
        .iter()
        .find(|definition| definition.label == "k03-fern")
        .expect("defined");
    let edit = footnotes::remove_definition(&text, &blocks, definition);
    round_trip(&text, std::slice::from_ref(&edit));
    session
        .edit_chapter(chapter, vec![edit], EditKind::Discrete)
        .expect("valid");
    renders(&session).expect("no orphan left");
}

#[test]
fn removing_a_note_and_amending_its_undo_step_gives_the_exact_bytes_back() {
    let text = "A[^n] b.\n\n[^n]: Note.\n\nNext.\n";
    let mut buffer = Buffer::new(text);
    let caret = Selection::caret(0);
    let now = Instant::now();
    buffer
        .transact(EditKind::Deleting, vec![Edit::new(1..5, "")], caret, caret, now)
        .expect("valid");
    let blocks = Blocks::parse(buffer.text());
    let facts = facts::scan(buffer.text());
    let edit = footnotes::remove_definition(buffer.text(), &blocks, &facts.definitions[0]);
    buffer.amend(vec![edit], caret, now).expect("valid");
    assert_eq!(buffer.text(), "A b.\n\nNext.\n");
    buffer.undo().expect("one step");
    assert_eq!(buffer.text(), text);
}

#[test]
fn an_edit_that_would_not_parse_is_refused_with_the_engines_reason() {
    let project = project("refuse");
    let session = project.session();
    let id = chapter_named(&session, "11-k01.md");
    let text = session.buffer(id).text();
    let before = check::problems_now(&session);
    assert!(before.is_empty(), "the project parses: {before:?}");
    // A heading inside a note.
    let facts = facts::scan(text);
    let definition = &facts.definitions[0];
    let edit = footnotes::write_body(text, &definition.range, "Text\n\n# Titel").expect("an edit");
    let why = check::refusal(&session, id, &[edit], &before).expect("refused");
    assert!(why.contains("heading"), "{why}");
    // A second reference to a note.
    let at = text.find("Und").expect("found");
    let why = check::refusal(&session, id, &[Edit::new(at..at, "[^k01-1]")], &before).expect("refused");
    assert!(why.contains("k01-1") || why.contains("note"), "{why}");
    // A label already taken, across chapters.
    let at = text.find("{#sec:eins}").expect("found");
    let why = check::refusal(&session, id, &[Edit::new(at..at + 11, "{#sec:zwei}")], &before).expect("refused");
    assert!(why.contains("sec:zwei"), "{why}");
    // A label with a letter its names cannot hold, so `ß` is told apart from a typo in the engine's words.
    let why = check::refusal(&session, id, &[Edit::new(at..at + 11, "{#sec:groß}")], &before).expect("refused");
    assert!(why.contains("sec:groß") || why.contains("letters"), "{why}");
    // Nothing wrong with a plain edit.
    assert_eq!(check::refusal(&session, id, &[Edit::new(at..at, " ")], &before), None);
}

#[test]
fn picker_results_resolve_at_render_with_the_engines_text() {
    let project = project("citations");
    let mut session = project.session();
    let id = chapter_named(&session, "14-k03.md");
    let text = session.buffer(id).text().to_owned();
    let at = text.find("Ohne").expect("found");
    let written = format!(
        "{} {} {} ",
        citations::new_citation("bostrom2003", "S. 243", false),
        citations::reference_text("sec:zwei", false, false),
        citations::reference_text("tbl:mass", true, false)
    );
    session
        .edit_chapter(id, vec![Edit::new(at..at, written.as_str())], EditKind::Discrete)
        .expect("valid");
    renders(&session).expect("the picks render");
    let mut rich = book_rich(&session);
    assert!(
        rich.works
            .iter()
            .any(|work| work.key == "müller2020" && work.authors.contains("Müller"))
    );
    assert_eq!(citations::search_works(&rich.works, "straße").len(), 1);
    assert_eq!(citations::search_works(&rich.works, "groß").len(), 1);
    let labels = Typesetter::new(session.kind().clone(), None)
        .labels(session.joined())
        .expect("labels");
    assert!(rich.set_labels(Some(labels)));
    let block_text = "Sieh [@bostrom2003, S. 243] und @sec:zwei und [@tbl:mass, page] und [^k01-1].";
    let spans = crate::source::inline::inline_spans(BlockKind::Paragraph, block_text);
    let chips = rich.chips(block_text, &spans, None);
    let shown: Vec<(String, ChipLook)> = chips
        .iter()
        .map(|chip| (chip.text.replace('\u{a0}', " "), chip.look))
        .collect();
    assert_eq!(shown[0], ("(Bostrom 2003, S. 243)".to_owned(), ChipLook::Label));
    assert_eq!(shown[1].1, ChipLook::Label);
    assert!(
        shown[1].0.contains('2') || shown[1].0.contains("Zwei"),
        "{:?}",
        shown[1]
    );
    assert!(shown[2].0.contains("Tabelle 1"), "{:?}", shown[2]);
    assert_eq!(shown[3], ("¹".to_owned(), ChipLook::Mark));
    assert!(chips[0].atomic);
    assert!(
        !chips[1].atomic,
        "a short form is plain to the caret, so it can be typed"
    );
}

#[test]
fn an_unknown_key_or_label_is_an_error_chip_with_its_reason() {
    let project = project("unknown");
    let session = project.session();
    let rich = book_rich(&session);
    let text = "Sieh [@nichtda] und @sec:nichtda und [^nichtda].";
    let spans = crate::source::inline::inline_spans(BlockKind::Paragraph, text);
    let chips = rich.chips(text, &spans, None);
    assert_eq!(chips.len(), 3);
    assert!(chips.iter().all(|chip| chip.look == ChipLook::Error));
    assert!(rich.problem_of("[@nichtda]").is_some_and(|why| why.contains("nichtda")));
    assert!(
        rich.problem_of("@sec:nichtda")
            .is_some_and(|why| why.contains("sec:nichtda"))
    );
    assert!(rich.problem_of("[^nichtda]").is_some_and(|why| why.contains("nichtda")));
    assert_eq!(rich.problem_of("[@bostrom2003]"), None);
    assert_eq!(rich.problem_of("@sec:zwei"), None);
}

#[test]
fn a_caret_inside_a_chip_shows_its_source_and_typing_a_short_form_is_left_alone() {
    let project = project("caret");
    let session = project.session();
    let rich = book_rich(&session);
    let text = "a [@bostrom2003] @sec:zwei b";
    let spans = crate::source::inline::inline_spans(BlockKind::Paragraph, text);
    assert_eq!(rich.chips(text, &spans, None).len(), 2);
    assert_eq!(rich.chips(text, &spans, Some(5)).len(), 1, "inside the brackets");
    assert_eq!(
        rich.chips(text, &spans, Some(2)).len(),
        2,
        "at a chip's edge it stays one"
    );
    assert_eq!(
        rich.chips(text, &spans, Some(26)).len(),
        1,
        "at the end of a short form"
    );
}

/// Runs an edit of a grid on a project's chapter: its undo gives the bytes back and its result renders.
fn check_grid(project: &Project, text: &str, edits: Vec<Edit>, what: &str) -> String {
    let result = round_trip(text, &edits);
    let mut session = project.session();
    let id = session.active();
    let edits = normalized(edits, text);
    session
        .edit_chapter(id, edits, EditKind::Discrete)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    if let Err(why) = renders(&session) {
        panic!("{what} does not render: {why}\n{result}");
    }
    result
}

fn only_grid(text: &str) -> Grid {
    let blocks = Blocks::parse(text);
    let regions = table::regions(text, &blocks);
    regions.into_iter().next().expect("a table").grid.expect("a grid")
}

/// Every change a grid offers, on a table with a body row, a second column, and text with `ß`, umlauts, and quotes.
fn every_grid_change(table: &Grid, text: &str) -> Vec<(&'static str, Vec<Edit>)> {
    let set = |edits: Result<Vec<Edit>, Key>| edits.expect("allowed");
    let mut changes = vec![
        ("cell", set(table.set_cell(text, 1, 0, "Füße »ß« \"q\" *fett*"))),
        ("cell in header", set(table.set_cell(text, 0, 1, "Überschrift ß"))),
        ("add row", set(table.add_row(text, 1))),
        ("add row after the header", set(table.add_row(text, 0))),
        ("add column", set(table.add_column(text, Some(0)))),
        ("add first column", set(table.add_column(text, None))),
        ("remove column", set(table.remove_column(text, 1))),
        ("move column", set(table.move_column_right(text, 0))),
        ("center", set(table.set_align(text, 1, Align::Center))),
        ("right", set(table.set_align(text, 0, Align::Right))),
    ];
    if table.rows.len() > 2 {
        changes.push(("remove row", set(table.remove_row(text, 1))));
        changes.push(("move row", set(table.move_row_down(text, 1))));
    }
    let caption = Caption {
        text: "Größe »ß«".to_owned(),
        label: "tabelle-1".to_owned(),
        placement: "top".to_owned(),
    };
    changes.push(("caption", vec![table.set_caption(text, &caption).expect("a change")]));
    changes
}

#[test]
fn every_change_to_a_pipe_table_renders_and_undoes() {
    let text = "# T\n\n| Größe | Maß |\n|:------|----:|\n| ß »x« | 1 |\n| Äpfel | 2 |\n\nNach.\n";
    let project = Project::new("pipe", &[("a.md", text)]);
    let table = only_grid(text);
    for (what, edits) in every_grid_change(&table, text) {
        let result = check_grid(&project, text, edits, what);
        assert!(result.starts_with("# T\n\n"), "{what}");
        assert!(result.ends_with("Nach.\n"), "{what}");
        assert!(only_grid(&result).columns >= 1, "{what}: reads back");
    }
}

#[test]
fn every_change_to_a_list_table_renders_and_undoes() {
    let text = "# T\n\n::: table {align=\"left right\"}\n- - Größe\n  - Maß\n- - ß »x«\n\n    Zweiter Absatz.\n  - 1\n- - Äpfel\n  - 2\n:::\n\nNach.\n";
    let project = Project::new("list", &[("a.md", text)]);
    let table = only_grid(text);
    for (what, edits) in every_grid_change(&table, text) {
        let result = check_grid(&project, text, edits, what);
        assert!(result.starts_with("# T\n\n"), "{what}");
        assert!(result.ends_with("Nach.\n"), "{what}");
        assert!(only_grid(&result).columns >= 1, "{what}: reads back");
    }
    // Spans on a row with an empty cell.
    let text = "# T\n\n::: table\n- - a\n  - b\n- - 1\n  - \n:::\n";
    let project = Project::new("span", &[("a.md", text)]);
    let table = only_grid(text);
    let merged = check_grid(&project, text, table.merge_right(text, 1, 0).expect("allowed"), "merge");
    assert!(merged.contains("{span=2} 1"), "{merged}");
    let project = Project::new("spanned", &[("a.md", merged.as_str())]);
    let again = only_grid(&merged);
    assert_eq!(
        check_grid(&project, &merged, again.split(&merged, 1, 0).expect("allowed"), "split"),
        text
    );
}

#[test]
fn every_change_keeps_every_table_of_the_samples_readable() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples");
    for name in ["tables.md", "report.md", "offer.md"] {
        let text = fs::read_to_string(samples.join(name)).expect("a sample");
        let blocks = Blocks::parse(&text);
        let mut grids = 0;
        for region in table::regions(&text, &blocks) {
            let Ok(table) = region.grid else {
                continue;
            };
            grids += 1;
            for (row, cells) in table.rows.iter().enumerate() {
                for (cell, shown) in cells.iter().enumerate() {
                    assert!(
                        table
                            .set_cell(&text, row, cell, &shown.text)
                            .expect("allowed")
                            .is_empty(),
                        "writing back the same text changes nothing, {name} row {row} cell {cell}"
                    );
                }
            }
            for (what, edits) in every_grid_change_tolerant(&table, &text) {
                let result = round_trip(&text, &edits);
                let blocks = Blocks::parse(&result);
                let read = table::regions(&result, &blocks);
                assert!(read.iter().any(|region| region.grid.is_ok()), "{name}: {what}");
            }
        }
        assert!(grids > 0, "{name} has tables");
    }
}

/// The changes a table allows, skipping those its spans refuse.
fn every_grid_change_tolerant(table: &Grid, text: &str) -> Vec<(&'static str, Vec<Edit>)> {
    let mut changes = Vec::new();
    let mut push = |what, edits: Result<Vec<Edit>, Key>| {
        if let Ok(edits) = edits
            && !normalized(edits.clone(), text).is_empty()
        {
            changes.push((what, edits));
        }
    };
    push("cell", table.set_cell(text, 1, 0, "Füße »ß«"));
    push("add row", table.add_row(text, 1));
    push("add column", table.add_column(text, Some(0)));
    push("remove column", table.remove_column(text, 0));
    push("move column", table.move_column_right(text, 0));
    push("align", table.set_align(text, 0, Align::Center));
    if table.rows.len() > 2 {
        push("remove row", table.remove_row(text, 1));
        push("move row", table.move_row_down(text, 1));
    }
    changes
}

#[test]
fn an_image_comes_in_shows_and_renders_with_its_caption_label_and_placement() {
    let project = project("image");
    let mut session = project.session();
    let id = chapter_named(&session, "14-k03.md");
    session.set_active(id);
    let outside = project.0.join("../druck-rich-outside.svg");
    fs::write(
        &outside,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="48"/>"#,
    )
    .expect("image");
    let written = images::bring_in(&project.0, true, &outside).expect("comes in");
    assert_eq!(written, "assets/druck-rich-outside.svg");
    assert!(outside.is_file(), "the original stays");
    fs::remove_file(&outside).ok();
    assert!(project.0.join(&written).is_file());
    let text = session.active_buffer().text().to_owned();
    let blocks = Blocks::parse(&text);
    let at = text.find("Ohne").expect("found");
    let figure = images::write(&written, "Füße »ß«", "fuesse", images::Placement::Top);
    let command = image_command(&text, &blocks, at..at, &figure);
    let result = round_trip(&text, &command.edits);
    assert_eq!(&result[command.selection.head..command.selection.head + 2], "![");
    assert!(
        result.ends_with("![Füße »ß«](assets/druck-rich-outside.svg){#fig:fuesse placement=top}\n"),
        "{result}"
    );
    session
        .edit_chapter(id, command.edits, EditKind::Discrete)
        .expect("valid");
    let shown = session.active_buffer().text().to_owned();
    let blocks = Blocks::parse(&shown);
    let atoms = atoms(&shown, &blocks);
    let AtomKind::Figure(parsed) = &atoms[0].kind else {
        panic!("a figure");
    };
    let block = &blocks.as_slice()[atoms[0].first];
    let body = &shown[block.range.clone()];
    assert_eq!(parsed.caption(body), "Füße »ß«");
    assert_eq!(parsed.placement, images::Placement::Top);
    let edit = parsed
        .edit(body, block.range.start, "Neu", "fuesse", images::Placement::Here)
        .expect("an edit");
    round_trip(&shown, std::slice::from_ref(&edit));
    session.edit_chapter(id, vec![edit], EditKind::Discrete).expect("valid");
    renders(&session).expect("the figure renders");
}

#[test]
fn a_named_style_written_on_each_kind_of_block_renders() {
    let text = "# Eins\n\nEin Absatz.\n\n- a\n- b\n";
    let project = Project::new("style", &[("a.md", text)]);
    fs::create_dir_all(project.0.join("themes/mine")).expect("folder");
    fs::write(project.0.join("themes/mine/theme.json"), THEME).expect("theme");
    let manifest = fs::read_to_string(project.0.join("druck.yaml")).expect("manifest");
    fs::write(project.0.join("druck.yaml"), format!("{manifest}theme: mine\n")).expect("manifest");
    let mut session = project.session();
    let id = session.active();
    for (block, name) in [(0, "wide"), (2, "lead"), (4, "checks")] {
        let text = session.buffer(id).text().to_owned();
        let blocks = Blocks::parse(&text);
        let edit = styles::set_style(&text, &blocks, block, Some(name)).expect("an edit");
        round_trip(&text, std::slice::from_ref(&edit));
        session.edit_chapter(id, vec![edit], EditKind::Discrete).expect("valid");
    }
    assert_eq!(
        session.buffer(id).text(),
        "# Eins {.wide}\n\nEin Absatz. {.lead}\n\n{.checks}\n- a\n- b\n"
    );
    let mut typesetter = Typesetter::new(session.kind().clone(), Some(project.0.join("themes/mine/theme.json")));
    if let Err(failure) = typesetter.run(session.joined(), &Cancel::default()) {
        panic!("the styles do not render: {failure}");
    }
}

/// The book's chapters, if it is on this machine.
fn corpus() -> Vec<(String, String)> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = fs::read_dir(Path::new(&home).join(CORPUS))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((name, fs::read_to_string(&path).ok()?))
        })
        .collect()
}

#[test]
fn on_every_chapter_of_the_book_each_note_reads_writes_and_comes_back() {
    let chapters = corpus();
    if chapters.is_empty() {
        println!("note: the book is not on this machine");
        return;
    }
    let book: Vec<String> = chapters
        .iter()
        .flat_map(|(_, text)| facts::scan(text).refs.into_iter().map(|note| note.label))
        .collect();
    let mut inserted = 0;
    for (name, text) in &chapters {
        let blocks = Blocks::parse(text);
        let facts = facts::scan(text);
        for definition in &facts.definitions {
            let body = footnotes::read_body(text, &definition.range);
            assert_eq!(
                footnotes::write_body(text, &definition.range, &body),
                None,
                "{name}: unchanged"
            );
            let new = format!("{body} ß");
            let edit = footnotes::write_body(text, &definition.range, &new).expect("an edit");
            let result = round_trip(text, std::slice::from_ref(&edit));
            assert_eq!(result.len(), text.len() + " ß".len(), "{name}: only the note changed");
        }
        let stem = name.trim_end_matches(".md");
        let label = footnotes::next_label(stem, &facts, &book);
        assert!(
            !book.iter().any(|known| known.eq_ignore_ascii_case(&label)),
            "{name}: {label} is free"
        );
        if let Some(part) = stem.split('-').nth(1).filter(|part| part.starts_with('k')) {
            assert!(label.starts_with(&format!("{part}-")), "{name}: {label}");
        }
        let Some(paragraph) = blocks
            .as_slice()
            .iter()
            .find(|block| block.kind == BlockKind::Paragraph)
        else {
            continue;
        };
        let at = paragraph.range.end - 1;
        let at = text.floor_char_boundary(at);
        let keeping = footnotes::keeping(&blocks).or(Some(footnotes::Keeping::AtEnd));
        let (command, body) = footnotes::insert(text, &blocks, &facts, at..at, &label, "Größe.", keeping);
        let result = round_trip(text, &command.edits);
        assert_eq!(&result[body], "Größe.", "{name}");
        // Every block but the ones written stays as it was.
        let reference = format!("[^{label}]");
        let definition = format!("[^{label}]: Größe.");
        let stripped = result.replacen(&reference, "", 1).replacen(&definition, "", 1);
        let squeeze = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(squeeze(&stripped), squeeze(text), "{name}");
        inserted += 1;
    }
    assert!(inserted > 15, "{inserted} chapters had text to insert into");
}

#[test]
fn the_books_project_renders_with_each_kind_of_rich_edit() {
    let chapters = corpus();
    if chapters.is_empty() {
        println!("note: the book is not on this machine");
        return;
    }
    let dir = std::env::temp_dir().join(format!("druck-rich-book-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(dir.join("manuscript")).expect("folder");
    let list: Vec<String> = chapters
        .iter()
        .map(|(name, _)| format!("  - manuscript/{name}"))
        .collect();
    // A project keeps the book's front matter in its manifest.
    let settings = chapters
        .iter()
        .find_map(|(_, text)| druck::editor::resolved_config::split_front_matter(text).ok().flatten())
        .map_or(String::new(), |split| {
            let lines: Vec<&str> = split
                .yaml
                .lines()
                .filter(|line| !line.starts_with("bibliography:"))
                .collect();
            lines.join("\n")
        });
    fs::write(
        dir.join("druck.yaml"),
        format!(
            "{settings}\nbibliography: literatur.bib\nchapters:\n{}\n",
            list.join("\n")
        ),
    )
    .expect("manifest");
    let bib = Path::new(&std::env::var_os("HOME").expect("a home")).join(CORPUS_BIB);
    fs::copy(&bib, dir.join("literatur.bib")).expect("bibliography");
    for (name, text) in &chapters {
        let body = druck::editor::resolved_config::split_front_matter(text)
            .ok()
            .flatten()
            .map_or(text.as_str(), |split| split.body);
        fs::write(dir.join("manuscript").join(name), body).expect("chapter");
    }
    let mut session = Session::open(&dir).expect("opens");
    let id = session
        .book()
        .iter()
        .copied()
        .find(|id| {
            session
                .chapter(*id)
                .is_some_and(|chapter| chapter.path().ends_with("13-k02.md"))
        })
        .expect("the chapter");
    session.set_active(id);
    let mut rich = book_rich(&session);
    assert!(rich.book.numbers.len() > 100, "{} notes", rich.book.numbers.len());
    assert_eq!(rich.book.keeping, Some(footnotes::Keeping::AtEnd));
    let works = citations::search_works(&rich.works, "bostrom");
    assert!(!works.is_empty());
    let key = works[0].key.clone();
    let text = session.active_buffer().text().to_owned();
    let blocks = Blocks::parse(&text);
    let facts_now = facts::scan(&text);
    let block = blocks
        .as_slice()
        .iter()
        .find(|block| block.kind == BlockKind::Paragraph && text[block.range.clone()].len() > 80)
        .expect("a paragraph");
    let at = text.floor_char_boundary(block.range.start + 40);
    let at = at + text[at..].find(' ').expect("a space");
    let label = footnotes::next_label("13-k02", &facts_now, &rich.book.note_labels);
    assert!(label.starts_with("k02-"), "{label}");
    let (command, _) = footnotes::insert(
        &text,
        &blocks,
        &facts_now,
        at..at,
        &label,
        "Größe »ß«.",
        rich.book.keeping,
    );
    let before = check::problems_now(&session);
    assert_eq!(check::refusal(&session, id, &command.edits, &before), None);
    round_trip(&text, &command.edits);
    session
        .edit_chapter(id, command.edits, EditKind::Discrete)
        .expect("valid");
    let citation = citations::new_citation(&key, "S. 243", false);
    let text = session.active_buffer().text().to_owned();
    let at = text.find(&format!("[^{label}]")).expect("the reference");
    let edit = Edit::new(at..at, format!(" {citation}"));
    round_trip(&text, std::slice::from_ref(&edit));
    session.edit_chapter(id, vec![edit], EditKind::Discrete).expect("valid");
    let renders_now = renders(&session);
    renders_now.expect("the book renders with the edits");
    let labels = Typesetter::new(session.kind().clone(), None)
        .labels(session.joined())
        .expect("labels");
    assert!(rich.refresh(&session).is_empty());
    assert!(rich.set_labels(Some(labels)));
    let parsed = citations::read(&citation).expect("a citation");
    assert!(rich.labels.citations.contains_key(&citation_key(&parsed)));
    fs::remove_dir_all(&dir).ok();
}
