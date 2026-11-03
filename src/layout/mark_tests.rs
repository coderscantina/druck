//! Inline marks and notes in layout: superscripts, subscripts, lines through and under text, small capitals,
//! todos, and comments between paragraphs.

use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use crate::config::theme::Lang;

fn source() -> Source {
    Source::Document("/fake/doc.md".into())
}

fn config(theme: serde_json::Value) -> Config {
    let settings = |source| SettingsInput {
        source,
        settings: FrontMatter::default(),
    };
    resolve(Inputs {
        theme: Some(ThemeInput {
            source: Source::Theme("/fake/theme.json".into()),
            value: theme,
        }),
        document: settings(source()),
        overrides: settings(Source::Cli {
            working_dir: "/fake".into(),
        }),
    })
    .expect("configuration resolves")
}

fn render(config: &Config, body: &str) -> Output {
    let document = crate::markdown::parse(body, 1, &source(), Lang::En).expect("document parses");
    let fonts = Fonts::load(config, &BTreeMap::new()).expect("bundled fonts");
    layout(
        &document,
        &Cited::default(),
        &[],
        &HashMap::new(),
        config,
        &fonts,
        &source(),
        crate::date::Date::from_unix(0),
    )
    .expect("layout succeeds")
}

/// Every text run on the first page as (x, baseline, color, run).
fn runs(output: &Output) -> Vec<(f64, f64, Color, &ShapedRun)> {
    output.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text { x, y, run, color } => Some((x.0, y.0, *color, run)),
            _ => None,
        })
        .collect()
}

fn rects(output: &Output) -> Vec<(Rect, Color)> {
    output.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Rect { rect, color } => Some((*rect, *color)),
            _ => None,
        })
        .collect()
}

#[test]
fn raises_superscripts_and_lowers_subscripts_by_the_theme_settings() {
    let theme = json!({"version": 1, "inline": {"superscript": {"size": "0.5em", "raise": "0.4em"},
        "subscript": {"size": "6pt", "lower": "0.2em"}}});
    let config = config(theme);
    let output = render(&config, "H~2~O and m^3^ stay.\n");
    let runs = runs(&output);
    let find = |text: &str| runs.iter().find(|(.., run)| run.text == text).expect("run");
    let body = config.styles.body.size.0;
    let (_, base, _, plain) = find("O");
    assert_eq!(plain.size.0, body);

    let (_, sub_base, _, sub) = find("2");
    assert_eq!(sub.size.0, 6.0);
    assert!((sub_base - base - 0.2 * body).abs() < 1e-9);
    let (_, sup_base, _, sup) = find("3");
    assert_eq!(sup.size.0, 0.5 * body);
    assert!((base - sup_base - 0.4 * body).abs() < 1e-9);
    // Unmarked text keeps its baseline and size.
    assert_eq!(find("H").1, *base);
    assert_eq!(find("m").3.size.0, body);
}

#[test]
fn draws_lines_through_and_under_marked_words_across_the_spaces_between_them() {
    let config = config(json!({"version": 1}));
    let output = render(&config, "~~two words~~ and [two more]{.underline} end.\n");
    let (runs, rects) = (runs(&output), rects(&output));
    assert_eq!(rects.len(), 2, "one line per mark, not one per word");
    let find = |text: &str| runs.iter().find(|(.., run)| run.text == text).expect("run");
    let (first, base, _, _) = find("two");
    let (last_x, _, _, last) = find("words");
    let (strike, _) = rects[0];
    assert_eq!(strike.x.0, *first);
    assert!((strike.x.0 + strike.width.0 - (last_x + last.width.0)).abs() < 1e-9);
    // The strike runs through the x-height, the underline below the baseline.
    let (underline, _) = rects[1];
    assert!(strike.y.0 < *base && strike.y.0 > base - 0.6 * config.styles.body.size.0);
    assert!(underline.y.0 > *base);
    assert_eq!(strike.height, underline.height);
}

#[test]
fn breaks_and_hyphenates_marked_text_and_keeps_its_line_on_every_line() {
    let config = config(json!({"version": 1}));
    let words = ["extraordinarily incomprehensible characteristically"; 8].join(" ");
    let output = render(&config, &format!("~~{words}~~\n"));
    let lines: std::collections::BTreeSet<u64> = rects(&output).iter().map(|(rect, _)| rect.y.0.to_bits()).collect();
    assert!(lines.len() >= 3, "one strike per line, got {}", lines.len());
    assert!(
        runs(&output).iter().any(|(.., run)| run.text.ends_with('-')),
        "a word is hyphenated"
    );
}

#[test]
fn sets_small_capitals_with_the_font_feature_or_synthetically() {
    let serif = config(json!({"version": 1}));
    let output = render(&serif, "[Small]{.smallcaps}\n");
    let runs_serif = runs(&output);
    let fonts = Fonts::load(&serif, &BTreeMap::new()).expect("bundled fonts");
    let (.., run) = &runs_serif[0];
    assert_eq!(run.text, "Small", "the feature substitutes glyphs and keeps the text");
    assert!(fonts.has_small_caps(run.face));

    let mono = config(json!({"version": 1, "styles": {"body": {"font": "$fonts.mono"}}}));
    let output = render(&mono, "[Small]{.smallcaps}\n");
    let pieces: Vec<_> = runs(&output)
        .into_iter()
        .take(2)
        .map(|(.., run)| (run.text.clone(), run.size.0))
        .collect();
    let body = mono.styles.body.size.0;
    assert_eq!(pieces, [("S".to_owned(), body), ("MALL".to_owned(), 0.8 * body)]);
}

#[test]
fn sets_todos_in_the_theme_style() {
    let theme = json!({"version": 1, "inline": {"todo": {"color": "#aa0000", "background": "#ffee00"}}});
    let config = config(theme);
    let output = render(&config, "Done. {todo: fix the date}\n");
    let todo = Color([0xaa, 0, 0]);
    let colored: Vec<_> = runs(&output)
        .into_iter()
        .filter(|(_, _, color, _)| *color == todo)
        .map(|(.., run)| run.text.clone())
        .collect();
    assert_eq!(colored, ["TODO:", "fix", "the", "date"]);
    let boxes: Vec<_> = rects(&output)
        .into_iter()
        .filter(|(_, color)| *color == Color([0xff, 0xee, 0]))
        .collect();
    assert_eq!(boxes.len(), 1, "one box behind the whole note");
}

#[test]
fn a_comment_between_paragraphs_leaves_the_indent_as_if_it_were_removed() {
    let config = config(json!({"version": 1}));
    let positions = |body: &str| -> Vec<(f64, f64, String)> {
        runs(&render(&config, body))
            .into_iter()
            .map(|(x, y, _, run)| (x, y, run.text.clone()))
            .collect()
    };
    let removed = positions("One.\n\n\n\nTwo.\n\nThree.\n");
    assert_eq!(
        removed[1].0,
        config.page.margin_inner.0 + config.styles.body.first_line_indent.0
    );
    assert_eq!(
        positions("One.\n\n<!-- note\n\nmore -->\n\nTwo.\n\n<!-- again -->\nThree.\n"),
        removed
    );
}
