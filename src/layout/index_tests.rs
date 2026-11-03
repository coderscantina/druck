//! The index in layout: final page numbers merged into ranges, links to the marks, roman numbers in front
//! matter, columns, the words of the document language, and settling.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};

fn source() -> InputSource {
    InputSource::Document("/fake/doc.md".into())
}

fn config(yaml: &str, theme: Value) -> Config {
    let settings: FrontMatter = serde_saphyr::from_str(yaml).expect("front matter");
    resolve(Inputs {
        theme: Some(ThemeInput {
            source: InputSource::Theme("/fake/theme.json".into()),
            value: theme,
        }),
        document: SettingsInput {
            source: source(),
            settings,
        },
        overrides: SettingsInput {
            source: InputSource::Overrides {
                working_dir: "/fake".into(),
            },
            settings: FrontMatter::default(),
        },
    })
    .expect("configuration resolves")
}

/// A theme with a one-column index, so each entry is a line of its own in [`texts`].
fn one_column() -> Value {
    json!({"version": 1, "index": {"columns": 1}})
}

fn try_render(config: &Config, body: &str) -> Result<Output, Vec<Diagnostic>> {
    let document = crate::markdown::parse(body, 1, &source(), config.document.lang).expect("document parses");
    let fonts = Fonts::load(config, &Default::default()).expect("bundled fonts");
    let date = crate::date::Date::from_unix(0);
    uncancelled(
        &document,
        &Cited::default(),
        &[],
        &HashMap::new(),
        config,
        &fonts,
        &source(),
        date,
    )
}

fn render(config: &Config, body: &str) -> Output {
    try_render(config, body).expect("layout succeeds")
}

/// The text runs in the text area of `page` joined into lines by baseline, top to bottom.
fn texts(page: &Page, config: &Config) -> Vec<String> {
    let geometry = &config.page;
    let area = geometry.margin_top.0..geometry.margin_top.0 + geometry.text_height().0;
    let mut lines: Vec<(f64, f64, String)> = Vec::new();
    for item in &page.items {
        let Item::Text { x, y, run, .. } = item else { continue };
        if !area.contains(&y.0) {
            continue;
        }
        match lines.iter_mut().find(|line| line.0 == y.0) {
            Some(line) => {
                let space = if (x.0 - line.1).abs() < 0.01 { "" } else { " " };
                line.2 = format!("{}{space}{}", line.2, run.text);
                line.1 = x.0 + run.width.0;
            }
            None => lines.push((y.0, x.0 + run.width.0, run.text.clone())),
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines.into_iter().map(|line| line.2).collect()
}

/// The lines of the pages from the one with the index heading on.
fn index_lines(output: &Output, config: &Config, heading: &str) -> Vec<String> {
    let lines: Vec<String> = output.pages.iter().flat_map(|page| texts(page, config)).collect();
    let start = lines.iter().position(|line| line == heading).expect("an index heading");
    lines[start..].to_vec()
}

/// Each link to a page number on the pages from `first` on, with its text and the page number its anchor
/// shows.
fn links(output: &Output, first: usize, numbers: &[&str]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for page in &output.pages[first..] {
        for item in &page.items {
            let Item::Link {
                rect,
                link: Link::Anchor(id),
            } = item
            else {
                continue;
            };
            let text: String = (page.items.iter())
                .filter_map(|item| match item {
                    Item::Text { x, y, run, .. }
                        if x.0 >= rect.x.0 - 0.01
                            && x.0 + 0.01 < rect.x.0 + rect.width.0
                            && y.0 >= rect.y.0
                            && y.0 <= rect.y.0 + rect.height.0 =>
                    {
                        Some(run.text.as_str())
                    }
                    _ => None,
                })
                .collect();
            if text.chars().all(char::is_alphanumeric) {
                found.push((text, numbers[output.anchors[*id].page].to_owned()));
            }
        }
    }
    found
}

const PROSE: &str = "The design of a page begins with its proportions. A text block that is too wide tires the \
    eye, which must travel far to return to the start of the next line.";

#[test]
fn the_index_lists_final_pages_as_ranges_with_sub_entries_and_see_references_linked_to_the_marks() {
    let body = "# One\n\nAlpha{^alpha} and the first [beta]{^!first}.\n\n::: page-break\n\n\
        Alpha{^alpha} on two.[^n]\n\n[^n]: A note on delta{^delta}.\n\n::: page-break\n\n\
        - Alpha{^alpha} on three\n- and Gamma{^Gamma|see alpha}.\n\n::: page-break\n\n\
        > Epsilon{^epsilon} on four.{^alpha}\n";
    let config = config("{}", one_column());
    let output = render(&config, body);
    assert_eq!(
        index_lines(&output, &config, "Index"),
        [
            "Index",
            "A",
            "alpha, 1–4",
            "B",
            "beta",
            "first, 1",
            "D",
            "delta, 2",
            "E",
            "epsilon, 4",
            "G",
            "Gamma, see alpha"
        ]
    );
    let links = links(&output, 3, &["1", "2", "3", "4"]);
    let shown: Vec<&str> = links.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(shown, ["1", "4", "1", "2", "4"]);
    assert!(links.iter().all(|(text, page)| text == page), "{links:?}");
    let outline: Vec<&str> = output.outline.iter().map(|bookmark| bookmark.title.as_str()).collect();
    assert_eq!(outline, ["1 One", "Index"]);
}

#[test]
fn index_pages_in_front_matter_show_their_roman_numbers_and_unnumbered_pages_are_errors() {
    let body = "::: front-matter\n\n# Preface {-}\n\nWords on type{^type}.\n\n::: main-matter\n\n\
        # One\n\nThe body on type{^type} and more{^more}.\n";
    let config = config("{}", one_column());
    let output = render(&config, body);
    let lines = index_lines(&output, &config, "Index");
    assert_eq!(lines[1..5], ["M", "more, 1", "T", "type, i, 1"]);
    let links = links(&output, 1, &["i", "1"]);
    assert!(links.iter().all(|(text, page)| text == page), "{links:?}");

    let errors = try_render(&config, &format!("Before{{^early}}.\n\n{body}")).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location, Some((1, 7).into()));
    assert_eq!(
        errors[0].message,
        "the index mark \"early\" is on a page without a number, so its page in the index shows nothing"
    );
}

#[test]
fn page_numbers_settle_when_the_index_comes_before_its_marks() {
    let sections: String = (1..=30)
        .map(|n| format!("# Part {n}\n\n{PROSE} Term {n}{{^term {n:02}}} and the common{{^common}}.\n\n"))
        .collect();
    let body = format!("::: index\n\n::: page-break\n\n{sections}");
    let config = config("toc: true", one_column());
    let output = render(&config, &body);
    let numbers: Vec<String> = (1..=output.pages.len()).map(|page| page.to_string()).collect();
    let numbers: Vec<&str> = numbers.iter().map(String::as_str).collect();
    let links = links(&output, 0, &numbers);
    assert!(links.len() > 30, "{links:?}");
    assert!(links.iter().all(|(text, page)| text == page), "{links:?}");
    let lines = index_lines(&output, &config, "Index");
    assert_eq!(lines[1..5], ["C", "common, 3–6", "T", "term 01, 3"]);
}

#[test]
fn the_index_takes_two_columns_from_the_theme_or_those_of_its_column_section() {
    let marks: String = ('a'..='p').map(|c| format!("Word {c}{{^{c}{c}}}. ")).collect();
    let columns = |theme: Value, body: &str| {
        let output = render(&config("{}", theme), body);
        let page = output.pages.last().expect("a page");
        let mut edges: Vec<i64> = (page.items.iter())
            .filter_map(|item| match item {
                Item::Text { x, run, .. } if run.text.len() == 2 && run.text[..1] == run.text[1..] => Some(x.0 as i64),
                _ => None,
            })
            .collect();
        edges.sort_unstable();
        edges.dedup();
        edges.len()
    };
    assert_eq!(columns(json!({"version": 1}), &marks), 2);
    assert_eq!(columns(one_column(), &marks), 1);
    let in_columns = format!("{marks}\n\n::: columns\n::: index\n:::\n");
    assert_eq!(columns(one_column(), &in_columns), 2);
}

#[test]
fn a_german_index_sorts_umlauts_as_their_base_letter_and_writes_siehe() {
    let body = "Öl{^Öl}, Ofen{^Ofen}, Ähre{^Ähre|siehe auch Ofen}, Abend{^Abend}, 1{^1900} und ß{^Bad|siehe Ofen}.";
    let config = config("lang: de", one_column());
    let output = render(&config, body);
    assert_eq!(
        index_lines(&output, &config, "Register")[1..10],
        [
            "Symbole",
            "1900, 1",
            "A",
            "Abend, 1",
            "Ähre, siehe auch Ofen",
            "B",
            "Bad, siehe Ofen",
            "O",
            "Ofen, 1"
        ]
    );
}
