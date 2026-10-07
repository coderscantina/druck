//! Custom styles in layout: case, letter spacing, keeping with the next block, list bullets and markers, and
//! hanging heading numbers.

use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use crate::config::theme::Lang;

fn source() -> Source {
    Source::Document("/fake/doc.md".into())
}

fn config() -> Config {
    let theme = json!({
        "version": 1,
        "custom-styles": {
            "eyebrow": {"based-on": "body", "uppercase": true, "tracking": 0.1, "keep-with-next": true},
            "checks": {"based-on": "list", "bullets": ["✔"]},
            "small-marks": {"based-on": "list", "marker": "footnote"},
            "step": {"based-on": "heading-3", "number-gap": "1em"},
        },
    });
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

fn render(config: &Config, fonts: &Fonts, body: &str) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let document = crate::markdown::parse(body, 1, &source()).expect("document parses");
    let cited = Cited::default();
    layout(&document, &cited, &[], &HashMap::new(), config, fonts, &source()).map(|output| output.pages)
}

/// Every text run as (page, x, baseline, run).
fn runs(pages: &[Page]) -> Vec<(usize, f64, f64, &ShapedRun)> {
    let mut runs = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        for item in &page.items {
            if let Item::Text { x, y, run, .. } = item {
                runs.push((index, x.0, y.0, run));
            }
        }
    }
    runs
}

fn find<'a>(runs: &[(usize, f64, f64, &'a ShapedRun)], text: &str) -> (usize, f64, f64, &'a ShapedRun) {
    *runs
        .iter()
        .find(|(.., run)| run.text == text)
        .unwrap_or_else(|| panic!("no run \"{text}\""))
}

#[test]
fn sets_a_custom_style_in_capitals_with_letter_spacing() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let pages = render(&config, &fonts, "Straße `code` {.eyebrow}\n\nText.\n").unwrap();
    let runs = runs(&pages);
    let (.., run) = find(&runs, "STRASSE");
    let body = &config.styles.body;
    let plain = fonts.shape("STRASSE", run.face, body.size, Lang::En);
    let spacing = 7.0 * 0.1 * body.size.0;
    assert!((run.width.0 - plain.width.0 - spacing).abs() < 1e-9, "{}", run.width.0);
    find(&runs, "code");
}

#[test]
fn a_kept_paragraph_never_ends_a_page_without_the_next_block() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let mut split_without_keeping = false;
    for count in 40..54 {
        let lines: String = (0..count).map(|n| format!("Line {n}.\n\n")).collect();
        let page_of = |class: &str| {
            let body = format!("{lines}Kicker{class}\n\n## Heading\n\nText.\n");
            let pages = render(&config, &fonts, &body).unwrap();
            let runs = runs(&pages);
            let kicker = runs
                .iter()
                .find(|(.., run)| run.text.eq_ignore_ascii_case("kicker"))
                .unwrap();
            (kicker.0, find(&runs, "Heading").0)
        };
        let (kicker, heading) = page_of(" {.eyebrow}");
        assert_eq!(kicker, heading, "{count} lines before");
        let (kicker, heading) = page_of("");
        split_without_keeping |= kicker != heading;
    }
    assert!(
        split_without_keeping,
        "some count puts the plain paragraph at a page end"
    );
}

#[test]
fn takes_list_bullets_from_the_list_style() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let pages = render(&config, &fonts, "{.checks}\n- One\n- Two\n\nText.\n\n- Three\n").unwrap();
    let markers: Vec<_> = runs(&pages)
        .into_iter()
        .filter(|(.., run)| ["✔", "•"].contains(&run.text.as_str()))
        .map(|(.., run)| run.text.clone())
        .collect();
    assert_eq!(markers, ["✔", "✔", "•"]);
}

#[test]
fn sets_list_markers_in_the_marker_style_on_the_first_baseline() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let pages = render(&config, &fonts, "{.small-marks}\n1. One\n2. Two\n").unwrap();
    let runs = runs(&pages);
    let find = |text: &str| runs.iter().find(|(.., run)| run.text == text).expect(text);
    let (list, footnote) = (&config.styles.list, &config.styles.footnote);

    for (number, item) in [("1.", "One"), ("2.", "Two")] {
        let (_, x, y, marker) = find(number);
        let (_, text_x, text_y, _) = find(item);
        assert_eq!(marker.size, footnote.size);
        assert_eq!(y, text_y);
        assert!((x + marker.width.0 + 0.5 * list.size.0 - text_x).abs() < 1e-9);
    }
}

#[test]
fn hangs_a_typed_heading_number_with_the_gap_of_its_style() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let body = "### 7. Install the tools and check that every one of them runs before you go on with the next \
        step {.step}\n\nText.\n";
    let pages = render(&config, &fonts, body).unwrap();
    let runs = runs(&pages);
    let (_, left, baseline, number) = find(&runs, "7.");
    let CustomStyle::Heading {
        number_gap: Some(gap), ..
    } = &config.custom_styles["step"]
    else {
        panic!("a heading style")
    };
    let text = left + number.width.0 + gap.0;
    let (.., first, _) = find(&runs, "Install");
    assert_eq!(first, baseline);
    let starts: Vec<f64> = runs
        .iter()
        .filter(|(_, x, y, _)| *y >= baseline && *x < text + 1.0 && *x > left)
        .map(|(_, x, ..)| *x)
        .collect();
    assert!(starts.len() >= 2, "the heading wraps: {starts:?}");
    assert!(starts.iter().all(|x| (x - text).abs() < 1e-9), "{starts:?}");
    assert!(
        !runs.iter().any(|(.., run)| run.text.starts_with("0.0.1")),
        "not numbered automatically"
    );
}

#[test]
fn reports_unknown_styles_and_styles_for_another_kind_of_block_at_the_attribute() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let errors = render(&config, &fonts, "Text {.nope}\n\n# Head {.checks}\n").unwrap_err();
    let found: Vec<_> = errors.iter().map(|e| (e.location, e.message.as_str())).collect();
    assert_eq!(
        found,
        [
            (
                Some((1, 6)),
                "unknown style \"nope\"; the theme's custom styles are checks, eyebrow, small-marks, step"
            ),
            (
                Some((3, 8)),
                "style \"checks\" is for a list and cannot style a heading"
            ),
        ]
    );
}

#[test]
fn sets_automatic_heading_numbers_hanging_with_the_gap_of_their_style() {
    let mut config = config();
    config.styles.heading_2.number_gap = Some(Pt(30.0));
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let pages = render(&config, &fonts, "# One\n\n## Two\n\nText.\n").unwrap();
    let runs = runs(&pages);
    let (_, left, baseline, number) = find(&runs, "1.1");
    let (_, text, text_baseline, _) = find(&runs, "Two");
    assert_eq!(text_baseline, baseline);
    assert!((text - (left + number.width.0 + 30.0)).abs() < 1e-9, "{text}");
}

#[test]
fn sets_contents_entries_in_the_style_of_their_level() {
    let mut config = config();
    config.document.toc = true;
    let style = |size| Style {
        size: Pt(size),
        ..config.styles.toc_entry.clone()
    };
    config.toc.level_styles = vec![style(20.0), style(8.0)];
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let pages = render(&config, &fonts, "# One\n\n## Two\n\n### Three\n\nText.\n").unwrap();
    let runs = runs(&pages);
    let size = |text: &str, size: f64| runs.iter().any(|(.., run)| run.text == text && run.size.0 == size);
    assert!(size("One", 20.0));
    assert!(size("Two", 8.0));
}

#[test]
fn scales_lists_in_footnotes_to_the_footnote_size() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let body = "Text.[^n]\n\n- Body item\n\n[^n]: Note.\n\n    - Note item\n";
    let pages = render(&config, &fonts, body).unwrap();
    let sizes: Vec<f64> = runs(&pages)
        .into_iter()
        .filter(|(.., run)| run.text == "•")
        .map(|(.., run)| run.size.0)
        .collect();
    let ratio = config.styles.footnote.size.0 / config.styles.body.size.0;
    assert_eq!(sizes.len(), 2, "{sizes:?}");
    assert!((sizes[0] - config.styles.list.size.0).abs() < 1e-9);
    assert!((sizes[1] - config.styles.list.size.0 * ratio).abs() < 1e-9);
}

#[test]
fn scales_custom_lists_and_quotations_in_footnotes_to_the_footnote_size() {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let body = "Text.[^n]\n\n[^n]: Note.\n\n    {.checks}\n    - Checked\n\n    > Quoted\n";
    let pages = render(&config, &fonts, body).unwrap();
    let runs = runs(&pages);
    let ratio = config.styles.footnote.size.0 / config.styles.body.size.0;
    let (list, quote) = (find(&runs, "Checked").3.size.0, find(&runs, "Quoted").3.size.0);
    assert!((list - config.styles.list.size.0 * ratio).abs() < 1e-9, "{list}");
    assert!((quote - config.styles.quote.size.0 * ratio).abs() < 1e-9, "{quote}");
}
