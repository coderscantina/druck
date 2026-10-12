//! Tagging: the elements layout adds for the document and the page content it marks as theirs.

use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};

fn source() -> Source {
    Source::Document("/fake/doc.md".into())
}

fn render(yaml: &str, body: &str) -> Output {
    let settings: FrontMatter = serde_saphyr::from_str(yaml).expect("front matter");
    let config = resolve(Inputs {
        theme: Some(ThemeInput {
            source: Source::Theme("/fake/theme.json".into()),
            value: json!({"version": 1}),
        }),
        document: SettingsInput {
            source: source(),
            settings,
        },
        overrides: SettingsInput {
            source: Source::Cli {
                working_dir: "/fake".into(),
            },
            settings: FrontMatter::default(),
        },
    })
    .expect("configuration resolves");
    let document = crate::markdown::parse(body, 1, &source(), config.document.lang).expect("document parses");
    let cited = Cited::default();
    let fonts = Fonts::load(&config, &Default::default()).expect("bundled fonts");
    let date = crate::date::Date::from_unix(0);
    layout(
        &document,
        &cited,
        &[],
        &HashMap::new(),
        &config,
        &fonts,
        &source(),
        date,
    )
    .expect("layout succeeds")
}

/// The text of each run on the pages, inside an element or not.
fn runs(output: &Output) -> (Vec<String>, Vec<String>) {
    let (mut marked, mut decoration) = (Vec::new(), Vec::new());
    for page in &output.pages {
        let mut depth = 0usize;
        for item in &page.items {
            match item {
                Item::Begin(_) => depth += 1,
                Item::End => depth -= 1,
                Item::Text { run, .. } | Item::TurnedText { run, .. } if depth > 0 => marked.push(run.text.clone()),
                Item::Text { run, .. } | Item::TurnedText { run, .. } => decoration.push(run.text.clone()),
                _ => {}
            }
        }
        assert_eq!(depth, 0, "every element ends on the page it begins");
    }
    (marked, decoration)
}

#[test]
fn adds_elements_in_document_order_and_leaves_running_heads_and_the_watermark_as_decoration() {
    let body = "# One\n\nA note[^n] and [a link](https://example.com).\n\n- first\n- second\n\n\
        | Key | Value |\n|---|---|\n| a | 1 |\n\n[^n]: The note.\n";
    let output = render("{draft: true}", body);
    let tags = &output.tags;
    let kind = |index: usize| &tags.nodes[index].kind;
    let parent = |index: usize| tags.nodes[index].parent.map(Id::index);

    let heading = Kind::Heading {
        level: 1,
        text: "1 One".to_owned(),
    };
    assert_eq!(kind(0), &heading);
    // The note follows its reference in the paragraph and holds the note's own paragraph.
    assert_eq!(
        [kind(1), kind(2), kind(3), kind(4)],
        [&Kind::Paragraph, &Kind::Reference, &Kind::Note, &Kind::Link]
    );
    assert_eq!([parent(2), parent(3), parent(4)], [Some(1), Some(1), Some(1)]);
    let note_paragraph = (0..tags.nodes.len()).find(|&index| parent(index) == Some(3));
    assert_eq!(note_paragraph.map(kind), Some(&Kind::Paragraph));
    assert_eq!(
        [kind(5), kind(6), kind(7), kind(8)],
        [
            &Kind::List { ordered: false },
            &Kind::ListItem,
            &Kind::Label,
            &Kind::ListBody
        ]
    );
    let cells: Vec<_> = (0..tags.nodes.len())
        .filter_map(|index| match kind(index) {
            Kind::TableCell { header, .. } => Some(*header),
            _ => None,
        })
        .collect();
    assert_eq!(cells, [true, true, false, false]);

    let (marked, decoration) = runs(&output);
    for text in ["One", "note", "link", "first", "•", "Key", "1", "The"] {
        assert!(marked.iter().any(|run| run == text), "{text} is content: {marked:?}");
    }
    assert_eq!(decoration, ["DRAFT", "1"], "the watermark and the page number");
}

#[test]
fn tags_callouts_the_index_and_margin_notes() {
    let body = "Text beside[^m] a note.\n\n> [!TIP]\n> Advice{^term}.\n\n[^m]: {-} In the margin.\n";
    let output = render("{}", body);
    let tags = &output.tags;
    let kind = |index: usize| &tags.nodes[index].kind;
    let children = |parent: usize| -> Vec<usize> {
        (0..tags.nodes.len())
            .filter(|&index| tags.nodes[index].parent.map(Id::index) == Some(parent))
            .collect()
    };

    // The margin note follows the paragraph that references it and holds its own paragraph.
    assert_eq!([kind(0), kind(1)], [&Kind::Paragraph, &Kind::Note]);
    assert_eq!(children(0), [1]);
    assert_eq!(
        children(1).into_iter().map(kind).collect::<Vec<_>>(),
        [&Kind::Paragraph]
    );

    // The callout holds its label and its text as paragraphs.
    let callout = (0..tags.nodes.len())
        .find(|&index| *kind(index) == Kind::Callout)
        .expect("a callout");
    let parts: Vec<_> = children(callout).into_iter().map(kind).collect();
    assert_eq!(parts, [&Kind::Paragraph, &Kind::Paragraph]);

    // The index has a heading per letter and a paragraph per entry.
    let index = (0..tags.nodes.len())
        .find(|&index| *kind(index) == Kind::Index)
        .expect("an index");
    let letter = Kind::Heading {
        level: 2,
        text: "T".to_owned(),
    };
    let entries: Vec<_> = children(index).into_iter().map(kind).collect();
    assert_eq!(entries, [&letter, &Kind::Paragraph]);

    let (marked, _) = runs(&output);
    for text in ["margin.", "TIP", "Advice", "T", "term"] {
        assert!(marked.iter().any(|run| run == text), "{text} is content: {marked:?}");
    }
}
