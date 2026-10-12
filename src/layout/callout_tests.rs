//! Callouts in layout: the label, background and bar, pages that split a callout, and column width.

use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};

const BACKGROUND: Color = Color([0xf1, 0xf1, 0xf1]);
const BAR: Color = Color([0x8c, 0x8c, 0x8c]);

fn source() -> Source {
    Source::Document("/fake/doc.md".into())
}

fn config() -> Config {
    let settings = |source| SettingsInput {
        source,
        settings: FrontMatter::default(),
    };
    resolve(Inputs {
        theme: Some(ThemeInput {
            source: Source::Theme("/fake/theme.json".into()),
            value: json!({"version": 1, "pages": {"body": {"header": null, "footer": null}}}),
        }),
        document: settings(source()),
        overrides: settings(Source::Cli {
            working_dir: "/fake".into(),
        }),
    })
    .expect("configuration resolves")
}

fn render(config: &Config, body: &str) -> Vec<Page> {
    let fonts = Fonts::load(config, &BTreeMap::new()).unwrap();
    let document = crate::markdown::parse(body, 1, &source(), config.document.lang).expect("document parses");
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
    .pages
}

/// The rectangles of `color` on a page.
fn rects(page: &Page, color: Color) -> Vec<Rect> {
    let rects = page.items.iter().filter_map(|item| match item {
        Item::Rect { rect, color: c } if *c == color => Some(*rect),
        _ => None,
    });
    rects.collect()
}

/// The baseline and left edge of the first run with `text` on a page.
fn run(page: &Page, text: &str) -> (f64, f64) {
    page.items
        .iter()
        .find_map(|item| match item {
            Item::Text { x, y, run, .. } if run.text.eq_ignore_ascii_case(text) => Some((x.0, y.0)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no run \"{text}\""))
}

/// How many runs with `text` stand between `top` and `bottom` on a page.
fn runs_within(page: &Page, text: &str, rect: Rect) -> usize {
    let inside = |y: f64| y > rect.y.0 && y < rect.y.0 + rect.height.0;
    page.items
        .iter()
        .filter(|item| matches!(item, Item::Text { y, run, .. } if run.text == text && inside(y.0)))
        .count()
}

#[test]
fn a_callout_sets_its_label_above_the_text_inside_a_box_with_a_bar() {
    let config = config();
    let pages = render(&config, "> [!NOTE]\n> Text.\n");
    let page = &pages[0];
    let [background] = rects(page, BACKGROUND)[..] else {
        panic!("one background")
    };
    let [bar] = rects(page, BAR)[..] else { panic!("one bar") };
    let (label_x, label_y) = run(page, "Note");
    let (text_x, text_y) = run(page, "Text.");
    let padding = config.callouts.padding.0;
    assert!(label_y < text_y, "the label is above the text");
    assert!(background.y.0 < label_y && text_y < background.y.0 + background.height.0);
    assert_eq!(
        (bar.x.0, bar.y.0, bar.height.0),
        (background.x.0, background.y.0, background.height.0)
    );
    assert_eq!(bar.width.0, 2.0);
    assert!((label_x - (bar.x.0 + 2.0 + padding)).abs() < 1e-6);
    assert!((text_x - label_x).abs() < 1e-6);
    assert!(rects(&render(&config, "> Plain.\n")[0], BACKGROUND).is_empty());
}

#[test]
fn a_callout_split_across_pages_continues_its_box_and_never_strands_a_line() {
    let config = config();
    let paragraphs: String = (1..=6).map(|n| format!(">\n> Para {n}.\n")).collect();
    let mut split = false;
    for filler in 20..60 {
        let lines: String = (0..filler).map(|n| format!("Line {n}.\n\n")).collect();
        let pages = render(&config, &format!("{lines}> [!NOTE]\n{paragraphs}"));
        let boxes: Vec<_> = pages
            .iter()
            .filter_map(|page| Some((page, *rects(page, BACKGROUND).first()?)))
            .collect();
        for (page, background) in &boxes {
            assert!(
                runs_within(page, "Para", *background) >= 2,
                "{filler} lines before leave a single line"
            );
        }
        if boxes.len() == 2 {
            split = true;
            for (page, _) in &boxes {
                assert_eq!(rects(page, BAR).len(), 1, "the bar continues");
            }
        }
    }
    assert!(split, "some count splits the callout");
}

#[test]
fn a_callout_in_columns_takes_the_column_width() {
    let config = config();
    let pages = render(&config, "::: columns\n\n> [!TIP]\n> Text.\n\n:::\n");
    let [background] = rects(&pages[0], BACKGROUND)[..] else {
        panic!("one background")
    };
    assert!((background.width.0 - pages::column_width(&config.page)).abs() < 1e-6);
}
