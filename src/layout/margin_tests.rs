//! Margin notes, sidenotes, and figures and tables placed at the top or bottom of a page.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use crate::output::{Aside, NoteKind};

const PROSE: &str = "The design of a page begins with its proportions. A text block that is too wide tires the \
    eye, which must travel far to return to the start of the next line; a block that is too narrow breaks the \
    text into fragments and forces frequent hyphenation.";

fn source() -> InputSource {
    InputSource::Document("/fake/doc.md".into())
}

/// The configuration with the partial `theme`.
fn config(theme: Value) -> Config {
    let settings = |source| SettingsInput {
        source,
        settings: FrontMatter::default(),
    };
    resolve(Inputs {
        theme: Some(ThemeInput {
            source: InputSource::Theme("/fake/theme.json".into()),
            value: theme,
        }),
        document: settings(source()),
        overrides: settings(InputSource::Overrides {
            working_dir: "/fake".into(),
        }),
    })
    .expect("configuration resolves")
}

fn default_config() -> Config {
    config(json!({"version": 1}))
}

/// Lays out the Markdown `body`, whose images are `images` in order of first use.
fn try_render(config: &Config, body: &str, images: &[Image]) -> Result<Output, Vec<Diagnostic>> {
    let document = crate::markdown::parse(body, 1, &source(), config.document.lang).expect("document parses");
    let fonts = Fonts::load(config, &Default::default()).expect("bundled fonts");
    uncancelled(
        &document,
        &crate::citations::Cited::default(),
        images,
        &HashMap::new(),
        config,
        &fonts,
        &source(),
        crate::date::Date::from_unix(0),
    )
}

fn render(config: &Config, body: &str, images: &[Image]) -> Output {
    try_render(config, body, images).expect("layout succeeds")
}

/// A plain SVG image of the given natural size in points.
fn svg(width: f64, height: f64) -> Image {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}pt" height="{height}pt"><rect width="100%" height="100%"/></svg>"#
    );
    Image::decode(svg.into_bytes(), "svg").expect("valid SVG")
}

/// The text lines of a page as (baseline, left edge, text), top to bottom. Runs on one baseline that
/// start within `left..right` join one line.
fn lines_in(page: &Page, left: f64, right: f64) -> Vec<(f64, f64, String)> {
    let mut lines: Vec<(f64, f64, String)> = Vec::new();
    for item in &page.items {
        let Item::Text { x, y, run, .. } = item else { continue };
        if x.0 < left || x.0 >= right {
            continue;
        }
        match lines.iter_mut().find(|line| (line.0 - y.0).abs() < 0.01) {
            Some(line) => line.2 = format!("{} {}", line.2, run.text),
            None => lines.push((y.0, x.0, run.text.clone())),
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

/// The baseline of the first line on `page` that contains `text`.
fn baseline_of(page: &Page, text: &str) -> f64 {
    lines_in(page, 0.0, f64::INFINITY)
        .into_iter()
        .find(|line| line.2.contains(text))
        .unwrap_or_else(|| panic!("no line with {text:?}"))
        .0
}

/// The first baseline inside a note's box.
fn first_baseline(page: &Page, aside: &Aside) -> f64 {
    let (left, right) = (aside.rect.x.0, aside.rect.x.0 + aside.rect.width.0);
    lines_in(page, left - 0.01, right)
        .into_iter()
        .map(|line| line.0)
        .find(|&y| y >= aside.rect.y.0 && y <= aside.rect.y.0 + aside.rect.height.0)
        .expect("a note has text")
}

/// The page index and rectangle of every image, in placement order.
fn images(output: &Output) -> Vec<(usize, Rect)> {
    let mut rects = Vec::new();
    for (index, page) in output.pages.iter().enumerate() {
        for item in &page.items {
            if let Item::Image { rect, .. } = item {
                rects.push((index, *rect));
            }
        }
    }
    rects
}

/// The index of the first page that holds a line containing `text`.
fn page_of(output: &Output, text: &str) -> usize {
    output
        .pages
        .iter()
        .position(|page| {
            lines_in(page, 0.0, f64::INFINITY)
                .iter()
                .any(|line| line.2.contains(text))
        })
        .unwrap_or_else(|| panic!("no page with {text:?}"))
}

/// Paragraphs `P1` to `P{count}` of prose, each starting with its marker.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|number| format!("P{number} {PROSE}\n\n")).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn a_margin_note_stands_beside_its_line_on_the_same_baseline() {
    let config = default_config();
    let body = format!(
        "{}Noted text.[^a] {PROSE}\n\n[^a]: {{-}} Beside the line.\n",
        paragraphs(2)
    );
    let output = render(&config, &body, &[]);
    let [aside] = output.asides.as_slice() else {
        panic!("one note: {:?}", output.asides)
    };
    let page = &output.pages[aside.page];
    assert_eq!(aside.note, NoteKind::Margin(0));
    // Page 1 is odd, so the note column is right of the prose, `gap` away.
    let geometry = &config.page;
    let column = geometry.margin_inner.0 + geometry.prose_width.0 + config.margin_notes.gap.0;
    assert!(close(aside.rect.x.0, column), "{aside:?}");
    let reference = baseline_of(page, "Noted text.");
    assert!(close(first_baseline(page, aside), reference));
    assert!(close(aside.reference.y.0, reference));
    // The note is not in the text, and footnote numbering is untouched.
    assert!(!lines_in(page, 0.0, column).iter().any(|line| line.2.contains("Beside")));
}

#[test]
fn notes_that_would_overlap_stack_downward_in_reading_order() {
    let config = default_config();
    let body = "Three notes.[^a][^b][^c] Then text.\n\n[^a]: {-} First note, long enough to take more than one \
        line in the narrow column.\n\n[^b]: {-} Second note.\n\n[^c]: {-} Third note.\n";
    let output = render(&config, body, &[]);
    let notes = &output.asides;
    let kinds: Vec<NoteKind> = notes.iter().map(|aside| aside.note).collect();
    assert_eq!(kinds, [NoteKind::Margin(0), NoteKind::Margin(1), NoteKind::Margin(2)]);
    let page = &output.pages[0];
    assert!(close(
        first_baseline(page, &notes[0]),
        baseline_of(page, "Three notes.")
    ));
    for pair in notes.windows(2) {
        let bottom = pair[0].rect.y.0 + pair[0].rect.height.0;
        assert!(
            close(pair[1].rect.y.0, bottom + config.margin_notes.spacing.0),
            "{pair:?}"
        );
    }
}

#[test]
fn notes_at_the_foot_of_a_page_move_up_to_end_at_the_bottom_of_the_text_area() {
    let config = default_config();
    let long = "A note of several lines that runs down the narrow column beside the text. ".repeat(3);
    let body = format!(
        "{}Last.[^a][^b]\n\n[^a]: {{-}} {long}\n\n[^b]: {{-}} {long}\n",
        paragraphs(15)
    );
    let output = render(&config, &body, &[]);
    let index = page_of(&output, "Last.");
    let notes: Vec<&Aside> = output.asides.iter().filter(|aside| aside.page == index).collect();
    assert_eq!(
        notes.len(),
        2,
        "both notes stay on the page of their line: {:?}",
        output.asides
    );
    let bottom = config.page.margin_top.0 + config.page.text_height().0;
    let last = notes[1];
    assert!(close(last.rect.y.0 + last.rect.height.0, bottom), "{last:?}");
    // The first note starts above its line to make room, still `spacing` above the second.
    let page = &output.pages[index];
    assert!(first_baseline(page, notes[0]) < baseline_of(page, "Last.") - 1.0);
    assert!(close(
        notes[0].rect.y.0 + notes[0].rect.height.0 + config.margin_notes.spacing.0,
        last.rect.y.0
    ));
}

#[test]
fn a_page_ends_where_the_notes_of_its_lines_still_fit() {
    let config = default_config();
    let long = "Margin notes take room beside the text, so a page holds only as many as fit. ".repeat(5);
    let mut body = String::new();
    let mut notes = String::new();
    for number in 1..=8 {
        body.push_str(&format!("P{number} {PROSE}[^n{number}]\n\n"));
        notes.push_str(&format!("[^n{number}]: {{-}} {long}\n\n"));
    }
    let output = render(&config, &format!("{body}{notes}"), &[]);
    assert_eq!(output.asides.len(), 8);
    let (top, height) = (config.page.margin_top.0, config.page.text_height().0);
    for aside in &output.asides {
        assert!(aside.rect.y.0 >= top - 0.01 && aside.rect.y.0 + aside.rect.height.0 <= top + height + 0.01);
    }
    // Each note is on the page of its paragraph.
    for (number, aside) in (1..=8).zip(&output.asides) {
        assert_eq!(aside.page, page_of(&output, &format!("P{number} ")), "note {number}");
    }
    assert!(output.pages.len() > 1, "the notes need more than one page");
}

#[test]
fn notes_too_tall_for_the_page_are_an_error_at_the_note() {
    let config = default_config();
    let long = "This note goes on and on. ".repeat(200);
    let body = format!("Text.[^a]\n\n[^a]: {{-}} {long}\n");
    let errors = try_render(&config, &body, &[]).expect_err("the note cannot fit");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].location, Some((3, 1).into()));
    assert!(
        errors[0].message.starts_with("this margin note is "),
        "{}",
        errors[0].message
    );
}

#[test]
fn sidenotes_keep_their_numbers_and_leave_the_foot_of_the_page_empty() {
    let config = config(json!({"version": 1, "margin-notes": {"sidenotes": true}}));
    let body = "One.[^a] Two.[^b] Three.[^c]\n\n[^a]: First.\n\n[^b]: {-} Unnumbered.\n\n[^c]: Second.\n";
    let output = render(&config, body, &[]);
    let kinds: Vec<NoteKind> = output.asides.iter().map(|aside| aside.note).collect();
    assert_eq!(kinds, [NoteKind::Side(0), NoteKind::Margin(0), NoteKind::Side(1)]);
    let page = &output.pages[0];
    let column = output.asides[0].rect.x.0;
    let notes: Vec<String> = lines_in(page, column - 0.01, f64::INFINITY)
        .into_iter()
        .map(|line| line.2)
        .collect();
    // The raised numbers sit above the baselines of their notes.
    assert_eq!(notes, ["1", "First.", "Unnumbered.", "2", "Second."]);
    // No separator rule: the bottom area holds no notes.
    assert!(!page.items.iter().any(|item| matches!(item, Item::Rect { .. })));
}

#[test]
fn the_note_column_is_on_the_outer_side_and_fixed_without_mirroring() {
    let body = "Odd page.[^a]\n\n::: page-break\n\nEven page.[^b]\n\n[^a]: {-} Right.\n\n[^b]: {-} Left.\n";
    let config = default_config();
    let output = render(&config, body, &[]);
    let page = &config.page;
    let gap = config.margin_notes.gap.0;
    let width = config.margin_notes.width.0;
    assert!(close(
        output.asides[0].rect.x.0,
        page.margin_inner.0 + page.prose_width.0 + gap
    ));
    assert_eq!(output.asides[1].page, 1);
    assert!(close(output.asides[1].rect.x.0, page.margin_outer.0 - gap - width));

    let config = self::config(json!({"version": 1, "page": {"margins": {"mirror": false}}}));
    let output = render(&config, body, &[]);
    let right = config.page.margin_inner.0 + config.page.prose_width.0 + gap;
    assert!(close(output.asides[1].rect.x.0, right), "{:?}", output.asides[1]);
}

#[test]
fn notes_from_both_columns_go_to_the_outer_margin_beside_their_lines() {
    let config = default_config();
    let body = format!(
        "::: columns\nLeft.[^a] {PROSE} {PROSE}\n\nRight.[^b] {PROSE} {PROSE}\n:::\n\n[^a]: {{-}} From the first.\n\n\
         [^b]: {{-}} From the second.\n"
    );
    let output = render(&config, &body, &[]);
    let page = &output.pages[0];
    let [first, second] = output.asides.as_slice() else {
        panic!("two notes")
    };
    assert!(close(first.rect.x.0, second.rect.x.0));
    assert!(close(first_baseline(page, first), baseline_of(page, "Left.")));
    // The second note is beside its line unless the first pushes it down.
    let wanted = baseline_of(page, "Right.");
    let below = first.rect.y.0 + first.rect.height.0 + config.margin_notes.spacing.0;
    let offset = first_baseline(page, first) - first.rect.y.0;
    assert!(close(first_baseline(page, second), wanted.max(below + offset)));
}

#[test]
fn a_note_column_that_does_not_fit_beside_the_prose_is_an_error() {
    let config = config(json!({"version": 1, "margin-notes": {"width": "40mm"}}));
    let errors = try_render(&config, "Text.[^a]\n\n[^a]: {-} Note.\n", &[]).expect_err("no room");
    assert!(
        errors[0].message.contains("margin-notes.width"),
        "{}",
        errors[0].message
    );
    // Without notes the theme is not constrained.
    assert!(try_render(&config, "Text.\n", &[]).is_ok());

    let narrow = json!({"version": 1, "page": {"text-width": "120mm"}, "margin-notes": {"width": "30mm"}});
    let errors = try_render(&self::config(narrow), "Text.[^a]\n\n[^a]: {-} Note.\n", &[]).expect_err("wide blocks");
    assert!(errors[0].message.contains("page.wide"), "{}", errors[0].message);
}

#[test]
fn a_top_figure_goes_to_the_top_of_the_page_its_anchor_falls_on() {
    let config = default_config();
    let body = format!(
        "{}![Chart.](a.svg){{#fig:a placement=top}}\n\n{}",
        paragraphs(2),
        paragraphs(1)
    );
    let output = render(&config, &body, &[svg(200.0, 100.0)]);
    let [(page, rect)] = images(&output)[..] else {
        panic!("one image")
    };
    assert_eq!(page, 0);
    assert!(close(rect.y.0, config.page.margin_top.0));
    // The text before the anchor follows below the figure.
    assert!(baseline_of(&output.pages[0], "P1 ") > rect.y.0 + rect.height.0);
}

#[test]
fn a_bottom_table_sits_above_the_footnotes() {
    let config = default_config();
    let body = "Text with a note.[^a]\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n: Sizes. {#tbl:a placement=bottom}\n\n\
        More text.\n\n[^a]: The note.\n";
    let output = render(&config, body, &[]);
    let page = &output.pages[0];
    let row = baseline_of(page, "1 2");
    let more = baseline_of(page, "More text.");
    let note = baseline_of(page, "The note.");
    assert!(more < row && row < note, "text {more}, table {row}, note {note}");
    let rule = page
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Rect { rect, .. } => Some(rect.y.0),
            _ => None,
        })
        .fold(0.0, f64::max);
    assert!(rule > row, "the footnote separator is below the table");
}

#[test]
fn a_placed_figure_that_does_not_fit_waits_for_the_next_page_while_text_fills_this_one() {
    let config = default_config();
    let body = format!(
        "{}![Tall.](a.svg){{placement=top}}\n\n{}",
        paragraphs(7),
        paragraphs(8).replace('P', "Q")
    );
    let output = render(&config, &body, &[svg(300.0, 500.0)]);
    let [(page, rect)] = images(&output)[..] else {
        panic!("one image")
    };
    assert_eq!(page, 1);
    assert!(close(rect.y.0, config.page.margin_top.0));
    assert_eq!(
        page_of(&output, "Q1 "),
        0,
        "the text after the anchor keeps filling the first page"
    );
}

#[test]
fn placed_blocks_keep_their_order() {
    let config = default_config();
    let body = format!(
        "{}![First.](a.svg){{placement=top}}\n\n![Second.](b.svg){{placement=top}}\n\n{}",
        paragraphs(5),
        paragraphs(6).replace('P', "Q")
    );
    let output = render(&config, &body, &[svg(300.0, 600.0), svg(200.0, 50.0)]);
    let placed = images(&output);
    assert_eq!(placed.len(), 2);
    // The small second figure would fit on the first page, but it never passes the first.
    assert!(
        (placed[0].0, placed[0].1.y.0) < (placed[1].0, placed[1].1.y.0),
        "{placed:?}"
    );
}

#[test]
fn a_waiting_block_is_set_before_the_next_page_break() {
    let config = default_config();
    let body = format!(
        "{}![Waits.](a.svg){{placement=top}}\n\n::: page-break\n\nAfter the break.\n",
        paragraphs(7)
    );
    let output = render(&config, &body, &[svg(300.0, 500.0)]);
    let [(page, _)] = images(&output)[..] else {
        panic!("one image")
    };
    assert_eq!(page, 1, "on a page of its own after the text");
    assert_eq!(page_of(&output, "After the break."), 2);
}

#[test]
fn a_placed_figure_in_a_full_width_block_spans_the_page_and_keeps_the_columns_whole() {
    let config = default_config();
    let body = format!(
        "::: columns\n{}::: full-width\n![Wide.](a.svg){{placement=bottom}}\n:::\n\n{}:::\n",
        paragraphs(2),
        paragraphs(2).replace('P', "Q")
    );
    let output = render(&config, &body, &[svg(400.0, 100.0)]);
    let [(0, rect)] = images(&output)[..] else {
        panic!("one image on the first page")
    };
    let page = &output.pages[0];
    let bottom = config.page.margin_top.0 + config.page.text_height().0;
    assert!(rect.width.0 > crate::layout::pages::column_width(&config.page));
    assert!(rect.y.0 + rect.height.0 < bottom && rect.y.0 > baseline_of(page, "Q2 "));
    // One balanced section: the second column starts at the top, beside P1.
    let second = config.page.margin_inner.0 + config.page.prose_width.0 / 2.0;
    let right = lines_in(page, second, f64::INFINITY);
    assert!(close(right[0].0, baseline_of(page, "P1 ")), "{right:?}");
}

#[test]
fn the_theme_places_figures_in_the_text_but_not_in_columns() {
    let config = config(json!({"version": 1, "placement": {"figure": "top"}}));
    let body = format!(
        "{}![Placed.](a.svg)\n\n::: columns\n{}![In a column.](b.svg)\n\n{}:::\n",
        paragraphs(1),
        paragraphs(1).replace('P', "Q"),
        paragraphs(1).replace('P', "R")
    );
    let output = render(&config, &body, &[svg(100.0, 50.0), svg(100.0, 50.0)]);
    let placed = images(&output);
    assert!(close(placed[0].1.y.0, config.page.margin_top.0), "{placed:?}");
    let page = &output.pages[placed[1].0];
    assert!(
        placed[1].1.y.0 > baseline_of(page, "Q1 "),
        "the column figure stays in place"
    );
}

#[test]
fn page_references_name_the_page_where_a_placed_figure_lands() {
    let config = default_config();
    let body = format!(
        "See [@fig:tall, page].\n\n{}![Tall.](a.svg){{#fig:tall placement=top}}\n\n{}",
        paragraphs(6),
        paragraphs(3).replace('P', "Q")
    );
    let output = render(&config, &body, &[svg(300.0, 500.0)]);
    let [(page, _)] = images(&output)[..] else {
        panic!("one image")
    };
    assert_eq!(page, 1);
    let first = lines_in(&output.pages[0], 0.0, f64::INFINITY);
    assert!(first.iter().any(|line| line.2.contains("page\u{a0}2")), "{first:?}");
}

#[test]
fn reports_a_placed_table_taller_than_a_page() {
    let config = default_config();
    let rows: String = (0..80).map(|row| format!("| {row} | x |\n")).collect();
    let body = format!("| A | B |\n|---|---|\n{rows}\n: Long. {{#tbl:a placement=top}}\n");
    let errors = try_render(&config, &body, &[]).expect_err("too tall");
    assert!(
        errors[0].message.contains("placed at the top or bottom"),
        "{}",
        errors[0].message
    );
    assert_eq!(errors[0].location, Some((1, 1).into()));
}
