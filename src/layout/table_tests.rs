//! List tables in layout: column widths and alignment, spans, theme rules and their opt-out, repeated
//! headers, wide placement, and band slots in custom styles.

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
        "page": {"text-width": "10cm", "wide": ["table"], "margins": {"mirror": false}},
        "tables": {"top-rule": null, "header-rule": {"thickness": "1.5pt", "color": "#000000"}},
        "custom-styles": {
            "total": {"based-on": "table-cell", "rule-below": "none"},
            "group": {"based-on": "table-cell", "keep-with-next": true},
            "label": {"based-on": "table-cell", "align": "right"},
            "spaced": {"based-on": "footer", "uppercase": true, "tracking": 0.2},
        },
        "pages": {"first": null, "body": {"header": null, "footer": [{"anchor": "top-left", "slots": [{"text": "page {page}", "style": "spaced"}]}]}},
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

fn render(body: &str) -> (Config, Vec<Page>) {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let document = crate::markdown::parse(body, 1, &source()).expect("document parses");
    let pages = layout(
        &document,
        &Cited::default(),
        &[],
        &HashMap::new(),
        &config,
        &fonts,
        &source(),
    )
    .expect("layout succeeds")
    .pages;
    (config, pages)
}

/// Every text run on a page as (x, baseline, run).
fn runs(page: &Page) -> Vec<(f64, f64, &ShapedRun)> {
    let runs = page.items.iter().filter_map(|item| match item {
        Item::Text { x, y, run, .. } => Some((x.0, y.0, run)),
        _ => None,
    });
    runs.collect()
}

fn find<'a>(page: &'a Page, text: &str) -> (f64, f64, &'a ShapedRun) {
    runs(page)
        .into_iter()
        .find(|(.., run)| run.text == text)
        .unwrap_or_else(|| panic!("no run \"{text}\""))
}

fn rects(page: &Page) -> Vec<Rect> {
    let rects = page.items.iter().filter_map(|item| match item {
        Item::Rect { rect, .. } => Some(*rect),
        _ => None,
    });
    rects.collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

const COSTS: &str = "::: table {align=\"left right\" widths=\"* auto\"}
- - Item
  - Amount
- - Design

    - Styleguide
    - Logo
  - 2,400.00
- {.total}
  - {.label} Total
  - 2,400.00
:::
";

#[test]
fn star_columns_fill_the_frame_and_auto_columns_fit_their_content() {
    let (config, pages) = render(COSTS);
    let page = &pages[0];
    let (left, frame) = (config.page.margin_inner.0, config.page.text_width().0);
    let header_rule = rects(page)[0];
    assert!(
        close(header_rule.x.0, left) && close(header_rule.width.0, frame),
        "{header_rule:?}"
    );
    let padding = config.tables.cell_padding.0;
    let (x, _, run) = find(page, "Amount");
    assert!(
        close(x + run.width.0, left + frame - padding),
        "the amount column is right aligned"
    );
    let (x, _, run) = find(page, "2,400.00");
    let amount_column = run.width.0.max(find(page, "Amount").2.width.0);
    let (label, _, total) = find(page, "Total");
    assert!(
        close(label + total.width.0, x + run.width.0 - amount_column - 2.0 * padding),
        "the label cell takes its own right alignment"
    );
    let (detail, ..) = find(page, "Styleguide");
    assert!(detail > left + padding, "the detail list is indented in its cell");
}

#[test]
fn rows_draw_the_theme_rules_unless_their_style_opts_out() {
    let (config, pages) = render(COSTS);
    let thicknesses: Vec<f64> = rects(&pages[0]).iter().map(|rect| rect.height.0).collect();
    let row = config.tables.row_rule.unwrap().thickness.0;
    assert_eq!(
        thicknesses,
        [1.5, row],
        "no top rule, the header rule, one row rule, none below the total"
    );
}

#[test]
fn a_spanning_cell_is_set_across_its_columns() {
    let body = "::: table\n- - A\n  - B\n- - {span=2} A text wider than the first column\n- - x\n  - y\n:::\n";
    let (config, pages) = render(body);
    let (x, baseline, _) = find(&pages[0], "text");
    let (end, last, run) = find(&pages[0], "column");
    let (y, ..) = find(&pages[0], "y");
    assert!(
        last == baseline && end + run.width.0 > y,
        "one line across both columns"
    );
    assert!(x > config.page.margin_inner.0 + config.tables.cell_padding.0);
}

#[test]
fn a_list_table_repeats_its_header_on_every_page() {
    let rows: String = (0..120).map(|n| format!("- - Row {n}\n  - {n}.00\n")).collect();
    let body = format!("::: table\n- - Item\n  - Amount\n{rows}:::\n");
    let (_, pages) = render(&body);
    assert!(pages.len() >= 3, "{} pages", pages.len());
    for page in &pages {
        find(page, "Item");
    }
}

#[test]
fn a_row_in_a_style_kept_with_the_next_never_ends_a_page() {
    for count in 40..50 {
        let rows: String = (0..count).map(|n| format!("- - Row {n}\n")).collect();
        let body = format!("::: table\n- - Item\n{rows}- {{.group}}\n  - Group\n- - Last\n:::\n");
        let (_, pages) = render(&body);
        let page_of = |text: &str| {
            pages
                .iter()
                .position(|page| runs(page).iter().any(|(.., run)| run.text == text))
        };
        assert_eq!(page_of("Group"), page_of("Last"), "{count} rows before");
    }
}

#[test]
fn a_wide_table_wider_than_the_prose_starts_where_the_prose_starts() {
    let wide = "word ".repeat(30);
    let body = format!("::: table\n- - A\n  - B\n- - {wide}\n  - x\n:::\n\n| A | B |\n|---|---|\n| a | b |\n");
    let (config, pages) = render(&body);
    let rules = rects(&pages[0]);
    let (left, prose) = (config.page.margin_inner.0, config.page.prose_width.0);
    assert!(close(rules[0].x.0, left), "flush with the prose: {:?}", rules[0]);
    let narrow = rules.last().unwrap();
    assert!(narrow.width.0 < prose / 2.0);
    assert!(
        close(narrow.x.0 + narrow.width.0 / 2.0, left + prose / 2.0),
        "a narrow wide table is centered in the prose width: {narrow:?}"
    );
}

#[test]
fn a_caption_starts_at_the_table_and_is_no_wider_than_it() {
    let body = "| Column header | Another header |\n|---|---|\n| a | b |\n\n: A caption that is longer than the table is wide.\n";
    let (_, pages) = render(body);
    let page = &pages[0];
    let rule = rects(page)[0];
    let first = find(page, "Table");
    assert!(close(first.0, rule.x.0), "{} against {}", first.0, rule.x.0);
    let caption_end = runs(page)
        .into_iter()
        .filter(|(_, y, _)| *y < first.1 + 1.0)
        .map(|(x, _, run)| x + run.width.0)
        .fold(0.0, f64::max);
    assert!(
        caption_end <= rule.x.0 + rule.width.0 + 0.01,
        "{caption_end} past {rule:?}"
    );
}

#[test]
fn band_slots_take_custom_styles_with_capitals_and_tracking() {
    let (config, pages) = render("Text.\n");
    let (_, _, run) = find(&pages[0], "PAGE 1");
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let size = config.styles.footer.size;
    let plain = fonts.shape("PAGE 1", run.face, size, Lang::En);
    assert!(
        close(run.width.0, plain.width.0 + 6.0 * 0.2 * size.0),
        "{}",
        run.width.0
    );
}
