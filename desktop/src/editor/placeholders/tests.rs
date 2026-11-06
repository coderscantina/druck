use druck::editor::placeholders::Statistics;
use druck::editor::resolved_config::FrontMatter;

use super::*;
use crate::source::inline::inline_spans;

fn settings(yaml: &str) -> FrontMatter {
    druck::editor::resolved_config::parse_settings(
        yaml,
        1,
        &druck::editor::resolved_config::InputSource::Overrides {
            working_dir: std::path::PathBuf::new(),
        },
    )
    .expect("valid settings")
}

fn values(yaml: &str, words: usize, lang: &str) -> Values {
    let statistics = Statistics {
        words,
        ..Statistics::default()
    };
    Values::new(Some(&settings(yaml)), Some(statistics), lang)
}

#[test]
fn values_come_from_the_settings_and_the_engines_counts() {
    let book = values(
        "title: The Salt Road\nmeta:\n  place:\n    - Hallstatt\n    - Venice\n",
        37_508,
        "en",
    );
    assert_eq!(book.lookup("title"), Lookup::Value("The Salt Road".into()));
    assert_eq!(book.lookup("meta.place"), Lookup::Value("Hallstatt, Venice".into()));
    assert_eq!(book.lookup("words"), Lookup::Value("37,508".into()));
    assert_eq!(book.lookup("reading-time"), Lookup::Value("165".into()));
    assert_eq!(values("", 37_508, "de").lookup("words"), Lookup::Value("37.508".into()));
    assert_eq!(
        values("", 37_508, "de").lookup("reading-time"),
        Lookup::Value("210".into())
    );
}

#[test]
fn a_name_without_a_value_unknown_or_for_the_page_says_so() {
    let book = values("title: T\n", 1, "en");
    assert_eq!(book.lookup("subtitle"), Lookup::NoValue);
    assert_eq!(book.lookup("draft"), Lookup::NoValue);
    assert_eq!(
        values("draft: true\n", 1, "de").lookup("draft"),
        Lookup::Value("Entwurf".into())
    );
    assert_eq!(book.lookup("wrods"), Lookup::Unknown);
    assert_eq!(book.lookup("page"), Lookup::PageDependent);
    assert_eq!(Values::new(None, None, "en").lookup("words"), Lookup::Counting);
}

#[test]
fn a_placeholder_shows_its_value_and_the_caret_inside_shows_its_source() {
    let book = values("title: The Salt Road\n", 37_508, "en");
    let text = "It runs to {words} words, {subtitle}.";
    let spans = inline_spans(BlockKind::Paragraph, text);
    let shown = chips(BlockKind::Paragraph, text, &spans, None, &book, Display::Chip);
    let shown: Vec<_> = shown
        .iter()
        .map(|chip| (chip.text.as_str(), chip.look, chip.atomic))
        .collect();
    assert_eq!(
        shown,
        [("37,508", ChipLook::Value, true), ("{subtitle}", ChipLook::Open, true)]
    );
    let inside = chips(BlockKind::Paragraph, text, &spans, Some(13), &book, Display::Chip);
    assert_eq!(inside.len(), 1, "the caret's placeholder shows its source");
}

#[test]
fn a_placeholder_in_a_heading_shows_its_source_in_the_accent() {
    let book = values("title: T\n", 1, "en");
    let text = "# About {title}\n";
    let kind = BlockKind::Heading { level: 1 };
    let spans = inline_spans(kind, text);
    assert_eq!(
        chips(kind, text, &spans, None, &book, Display::Chip)[0].look,
        ChipLook::Open
    );
    assert_eq!(hint(kind, "title", &book), crate::t!(Key::NotInHeading));
}

#[test]
fn a_todo_has_its_label_only_as_a_chip() {
    let book = Values::default();
    let text = "Text {todo: thank them} here.";
    let spans = inline_spans(BlockKind::Paragraph, text);
    let as_chip = chips(BlockKind::Paragraph, text, &spans, None, &book, Display::Chip);
    assert_eq!(as_chip.len(), 1);
    assert_eq!(
        (as_chip[0].range.clone(), as_chip[0].look),
        (12..12, ChipLook::NoteLabel)
    );
    assert!(chips(BlockKind::Paragraph, text, &spans, None, &book, Display::Underline).is_empty());
}

#[test]
fn completion_fires_after_a_brace_and_letters_where_placeholders_are_read() {
    let query = |text: &str| query_at(text, &Blocks::parse(text), text.len());
    assert_eq!(
        query("Runs to {wo"),
        Some(Query {
            start: 8,
            typed: "wo".into()
        })
    );
    assert_eq!(query("Runs to {"), None, "a letter first");
    assert_eq!(query("Runs to \\{wo"), None, "escaped");
    assert_eq!(query("# Title {wo"), None, "a heading");
    assert_eq!(query("```\n{wo"), None, "code");
    assert_eq!(query("Use `{wo"), None, "a code span");
    let closed = "Runs to {words} words";
    assert_eq!(
        query_at(closed, &Blocks::parse(closed), 11),
        None,
        "inside a closed name"
    );
}

#[test]
fn the_best_name_completes_as_a_ghost_and_inserts_whole() {
    let names = Values::default().names();
    let chosen = best(&names, "wo").expect("a match");
    assert_eq!(names[chosen].0, "words");
    assert_eq!(ghost("words", "wo").as_deref(), Some("rds}"));
    assert_eq!(names[best(&names, "time").expect("a match")].0, "reading-time");
    assert_eq!(ghost("reading-time", "time"), None);
    assert_eq!(best(&names, "zzz"), None);
    let text = "Runs to {wo} words";
    let query = Query {
        start: 8,
        typed: "wo".into(),
    };
    let command = insert(text, &query, 11, "words");
    assert_eq!(command.edits[0].range, 8..12);
    assert_eq!(command.edits[0].text, "{words}");
    assert_eq!(command.selection, Selection::caret(15));
}

#[test]
fn names_are_listed_by_group_with_the_books_meta_keys() {
    let book = values("meta:\n  offer: \"2026-117\"\n", 1, "en");
    let names = book.names();
    let groups: Vec<Group> = names.iter().map(|(_, group)| *group).collect();
    let mut sorted = groups.clone();
    sorted.sort_by_key(|group| *group as u8);
    assert_eq!(groups, sorted);
    assert!(
        names
            .iter()
            .any(|(name, group)| name == "meta.offer" && *group == Group::Book)
    );
    assert!(!names.iter().any(|(name, _)| name == "page"));
}

#[test]
fn a_blocks_style_name_is_its_custom_style_or_its_heading_level() {
    let text = "# Tolls {#sec:tolls}\n\nSalt is a tax. {.epigraph}\n\nPlain.\n";
    let blocks = Blocks::parse(text);
    let name = |at: usize| style_name(text, &blocks, blocks.index_at(at));
    assert_eq!(
        name(2).as_deref(),
        Some(crate::inspector::style_property::style_label("heading-1"))
    );
    assert_eq!(name(25).as_deref(), Some("Epigraph"));
    assert_eq!(name(text.len() - 3), None);
    assert_eq!(sentence_case("wide-heading"), "Wide heading");
}
