//! Document structures: title blocks and pages, page variants, headers and footers, numbering,
//! the table of contents, cross-references, and settling page numbers.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};

const PROSE: &str = "The design of a page begins with its proportions. A text block that is too wide tires the \
    eye, which must travel far to return to the start of the next line; a block that is too narrow breaks the \
    text into fragments and forces frequent hyphenation.";

fn source() -> Source {
    Source::Document("/fake/doc.md".into())
}

/// The configuration of a document with front matter `yaml` and the partial `theme`.
fn config(yaml: &str, theme: Value) -> Config {
    let settings: FrontMatter = serde_saphyr::from_str(yaml).expect("front matter");
    resolve(Inputs {
        theme: Some(ThemeInput {
            source: Source::Theme("/fake/theme.json".into()),
            value: theme,
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
    .expect("configuration resolves")
}

fn theme() -> Value {
    json!({"version": 1})
}

fn render_images(config: &Config, body: &str, images: &[Image]) -> Result<Output, Vec<Diagnostic>> {
    let document = crate::markdown::parse(body, 1, &source()).expect("document parses");
    let fonts = Fonts::load(config).expect("bundled fonts");
    layout(&document, images, &HashMap::new(), config, &fonts, &source())
}

fn render(config: &Config, body: &str) -> Output {
    render_images(config, body, &[]).expect("layout succeeds")
}

/// The text lines of a page as (baseline, left edge, text), top to bottom, including headers and
/// footers. Runs that touch are joined without a space.
fn lines(page: &Page) -> Vec<(f64, f64, String)> {
    let mut lines: Vec<(f64, f64, f64, String)> = Vec::new();
    for item in &page.items {
        let Item::Text { x, y, run, .. } = item else { continue };
        let end = x.0 + run.width.0;
        match lines.iter_mut().find(|line| line.0 == y.0) {
            Some(line) => {
                let space = if (x.0 - line.2).abs() < 0.01 { "" } else { " " };
                line.3 = format!("{}{space}{}", line.3, run.text);
                line.2 = end;
            }
            None => lines.push((y.0, x.0, end, run.text.clone())),
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines.into_iter().map(|(y, x, _, text)| (y, x, text)).collect()
}

fn texts(page: &Page) -> Vec<String> {
    lines(page).into_iter().map(|line| line.2).collect()
}

/// The baseline of the line that reads `text` on `page`.
fn baseline(page: &Page, text: &str) -> f64 {
    lines(page)
        .into_iter()
        .find(|line| line.2 == text)
        .unwrap_or_else(|| panic!("no line {text:?} in {:?}", texts(page)))
        .0
}

/// The texts on the header baseline of a page, left to right.
fn header(page: &Page, config: &Config) -> Vec<String> {
    let y = config.page.margin_top.0 - config.page.header_offset.0;
    band(page, y)
}

fn footer(page: &Page, config: &Config) -> Vec<String> {
    let geometry = &config.page;
    band(
        page,
        geometry.margin_top.0 + geometry.text_height().0 + geometry.footer_offset.0,
    )
}

fn band(page: &Page, y: f64) -> Vec<String> {
    let mut runs: Vec<(f64, String)> = page
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text { x, y: at, run, .. } if at.0 == y => Some((x.0, run.text.clone())),
            _ => None,
        })
        .collect();
    runs.sort_by(|a, b| a.0.total_cmp(&b.0));
    runs.into_iter().map(|run| run.1).collect()
}

/// The physical page number of anchor `id`.
fn page_of(output: &Output, id: usize) -> usize {
    output.anchors[id].page + 1
}

#[test]
fn a_title_block_omits_empty_optional_slots_with_their_spacing_and_keeps_metadata_as_text() {
    let front = "title: \"*Plain* text\"\ndate: Today";
    let full = render(&config(front, theme()), "Body.");
    let page = &full.pages[0];
    assert_eq!(texts(page)[..3], ["*Plain* text", "Today", "Body."]);

    // Only the slots that are set count: the gap is the same as with just these two slots.
    let slots = json!({"version": 1, "title-block": {"slots": [
        {"text": "{title}", "style": "title"},
        {"text": "{date}", "style": "date", "space-before": "0.3em"},
    ]}});
    let two = render(&config(front, slots), "Body.");
    let gap = |page: &Page| baseline(page, "Today") - baseline(page, "*Plain* text");
    assert!((gap(page) - gap(&two.pages[0])).abs() < 1e-9);

    let with_author = render(&config(&format!("{front}\nauthor: Ada"), theme()), "Body.");
    assert!(
        gap(&with_author.pages[0]) > gap(page) + 10.0,
        "the author slot and its spacing appear"
    );
}

#[test]
fn a_title_page_stands_alone_and_body_pages_follow_its_parity() {
    let margins = json!({"version": 1, "page": {"margins": {"inner": "40mm", "outer": "20mm"}}});
    let config = config(
        "title: Report\nauthor: Ada\ntitle-page: true\nabstract: In short.",
        margins,
    );
    let output = render(&config, "# One\n\nText.");

    assert_eq!(texts(&output.pages[0]), ["Report", "Ada", "Abstract", "In short."]);
    assert_eq!(texts(&output.pages[1])[..2], ["1 One", "Text."]);
    assert_eq!(
        footer(&output.pages[1], &config),
        ["2"],
        "pages count from the title page"
    );
    // The first body page is the second physical page, an even page with the inner margin on the right.
    let left = lines(&output.pages[1])[0].1;
    assert!((left - config.page.margin_outer.0).abs() < 1.0, "{left}");
}

#[test]
fn a_required_title_slot_without_a_value_is_an_error() {
    let error = check_title(&config("title-page: true\nauthor: Ada", theme()), &source()).unwrap_err();
    assert_eq!(error.len(), 1);
    assert_eq!(error[0].property.as_deref(), Some("title-page.slots.0.text"));
    assert_eq!(
        error[0].message,
        "this required slot needs {title}; set title in the front matter"
    );
    assert_eq!(error[0].source, Some(source()));

    // The title block is set only when the document has metadata for it.
    assert!(check_title(&config("author: Ada", theme()), &source()).is_err());
    assert!(check_title(&config("{}", theme()), &source()).is_ok());
    let errors = render_images(&config("author: Ada", theme()), "Text.", &[]).unwrap_err();
    assert_eq!(errors[0].property.as_deref(), Some("title-block.slots.0.text"));
}

#[test]
fn page_variants_follow_the_fallback_order_and_physical_parity() {
    let variant =
        |name: &str| json!({"header": {"left": null, "center": {"text": name}, "right": null}, "footer": null});
    let pages = json!({"version": 1, "pages": {
        "title": variant("title"), "first": variant("first"), "odd": variant("odd"), "even": null,
        "body": variant("body"),
    }});
    let config = config("title: T\ntitle-page: true", pages);
    let output = render(
        &config,
        "A\n\n::: page-break\nB\n\n::: page-break\nC\n\n::: page-break\nD",
    );

    let headers: Vec<_> = output
        .pages
        .iter()
        .map(|page| header(page, &config).join(" "))
        .collect();
    assert_eq!(headers, ["title", "first", "odd", "body", "odd"]);
}

#[test]
fn headers_show_the_section_and_subsection_of_each_page_and_footers_the_page() {
    let theme = json!({"version": 1, "pages": {"first": null, "body": {
        "header": {"left": {"text": "{section}"}, "center": null, "right": {"text": "{subsection}"}},
        "footer": {"left": null, "center": {"text": "{page}"}, "right": null},
    }}});
    let config = config("{}", theme);
    let body = "Lead.\n\n# One\n\n## One A\n\nText.\n\n::: page-break\nText.\n\n## One B\n\nText.\n\n\
        ::: page-break\nText.\n\n# Two\n\nText.\n\n::: page-break\nText.";
    let output = render(&config, body);

    let headers: Vec<_> = output.pages.iter().map(|page| header(page, &config)).collect();
    assert_eq!(
        headers,
        [
            vec!["1 One", "1.1 One A"],
            vec!["1 One", "1.2 One B"],
            vec!["2 Two"],
            vec!["2 Two"],
        ]
    );
    let footers: Vec<_> = output.pages.iter().map(|page| footer(page, &config).join("")).collect();
    assert_eq!(footers, ["1", "2", "3", "4"]);

    // A required slot without a value names the slot and the page.
    let required = json!({"version": 1, "pages": {"first": null, "body": {
        "header": {"left": {"text": "{section}", "required": true}, "center": null, "right": null},
    }}});
    let errors = render_images(&self::config("{}", required), "Lead.\n\n::: page-break\n# One", &[]).unwrap_err();
    assert_eq!(errors[0].property.as_deref(), Some("pages.body.header.left"));
    assert_eq!(
        errors[0].message,
        "this required slot has no value for {section} on page 1"
    );
}

#[test]
fn numbers_headings_to_the_numbering_depth_and_bookmarks_them() {
    let body = "# A\n\n## B\n\n### C\n\n# D\n\n### E\n\nText.";
    let output = render(&config("numbering-depth: 2", theme()), body);
    assert_eq!(texts(&output.pages[0])[..6], ["1 A", "1.1 B", "C", "2 D", "E", "Text."]);
    let outline: Vec<_> = output.outline.iter().map(|b| (b.level, b.title.as_str())).collect();
    assert_eq!(outline, [(1, "1 A"), (2, "1.1 B"), (3, "C"), (1, "2 D"), (3, "E")]);
    // Each bookmark goes to the top of its heading's first line.
    for (bookmark, title) in output.outline.iter().zip(["1 A", "1.1 B", "C", "2 D", "E"]) {
        let anchor = output.anchors[bookmark.anchor];
        let line = baseline(&output.pages[anchor.page], title);
        assert!(
            anchor.y.0 < line && line - anchor.y.0 < 30.0,
            "{title}: {anchor:?} above {line}"
        );
    }

    let plain = render(&config("numbered-headings: false", theme()), body);
    assert_eq!(texts(&plain.pages[0])[..2], ["A", "B"]);
}

#[test]
fn the_table_of_contents_lists_headings_with_their_final_pages_and_links() {
    // Enough entries that the contents take two pages, which moves every heading.
    let sections: String = (1..=40)
        .map(|n| format!("# Part {n}\n\n## Detail {n}\n\n### Hidden {n}\n\n{PROSE}\n\n"))
        .collect();
    let config = config("toc: true\ntoc-depth: 2", theme());
    let output = render(&config, &sections);

    let entries: Vec<(String, usize)> = output
        .pages
        .iter()
        .flat_map(texts)
        .filter_map(|line| {
            let (title, page) = line.split_once(" ...")?;
            Some((title.to_owned(), page.trim_start_matches('.').trim().parse().ok()?))
        })
        .collect();
    assert_eq!(entries.len(), 80, "{entries:?}");
    assert_eq!(entries[0].1, 2, "the contents fill the first page");
    let headings = output.outline.iter().filter(|bookmark| bookmark.level <= 2);
    for ((title, page), bookmark) in entries.iter().zip(headings) {
        assert_eq!(*title, bookmark.title);
        assert_eq!(*page, page_of(&output, bookmark.anchor), "{title}");
    }
    let links = output.pages[0]
        .items
        .iter()
        .filter(|item| {
            matches!(
                item,
                Item::Link {
                    link: Link::Anchor(_),
                    ..
                }
            )
        })
        .count();
    assert!(links > 20, "contents entries link to their headings");
}

/// A square SVG image.
fn svg() -> Image {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100pt" height="100pt"><rect width="100%" height="100%"/></svg>"#;
    Image::decode(svg.as_bytes().to_vec(), "svg").expect("valid SVG")
}

#[test]
fn references_show_the_number_and_final_page_of_each_kind() {
    let filler = format!("{PROSE}\n\n").repeat(12);
    let body = format!(
        "# One\n\nSee @sec:two on [@sec:two, page], @fig:square, and [@tbl:data].\n\n{filler}\
         # Two {{#sec:two}}\n\n![Square](square.svg){{#fig:square}}\n\n| a |\n|---|\n| b |\n\n: Data {{#tbl:data}}\n"
    );
    let output = render_images(&config("{}", theme()), &body, &[svg()]).expect("layout succeeds");

    let two = output.outline[1].anchor;
    let page = page_of(&output, two);
    assert!(page > 1, "the section starts on a later page");
    let first = texts(&output.pages[0]);
    let expected = format!("See Section\u{a0}2 on page\u{a0}{page}, Figure\u{a0}1, and Table\u{a0}1.");
    assert!(first.contains(&expected), "{first:?}");
    let targets: Vec<usize> = output.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Link {
                link: Link::Anchor(id), ..
            } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(targets.len(), 4);
    assert!(targets[..2].iter().all(|&id| id == two));
    assert!(targets[2..].iter().all(|&id| page_of(&output, id) == page));
}

#[test]
fn settling_repeats_layout_until_shown_pages_agree_and_reports_pages_that_keep_moving() {
    let anchors = [(Location { line: 7, column: 1 }, "the heading \"Two\"".to_owned())];
    let position = |page: usize| Position {
        page: page - 1,
        x: Pt(0.0),
        y: Pt(0.0),
    };
    let run = |shown: &[usize], next: fn(usize) -> usize| {
        let mut passes = 0;
        let result = settle(&anchors, shown, &source(), |assumed| {
            passes += 1;
            Ok(((), vec![position(next(assumed[0]))]))
        });
        (passes, result.map(|_| ()))
    };

    assert_eq!(run(&[0], |_| 3), (2, Ok(())), "a guess, then a pass that confirms it");
    assert_eq!(run(&[], |_| 3), (1, Ok(())), "nothing shown, nothing to settle");
    let (passes, result) = run(&[0], |page| if page == 2 { 3 } else { 2 });
    assert_eq!(passes, PASSES);
    let errors = result.unwrap_err();
    assert_eq!(errors[0].location, Some((7, 1)));
    assert!(
        errors[0]
            .message
            .starts_with("page numbers did not settle after 5 layout passes: the heading \"Two\" moves"),
        "{}",
        errors[0].message
    );
}

#[test]
fn repeated_layout_with_generated_content_is_identical() {
    let config = config("title: T\ntoc: true", theme());
    let body =
        format!("# One {{#sec:one}}\n\n{PROSE} See [@sec:two, page].\n\n# Two {{#sec:two}}\n\nBack to @sec:one.");
    let first = render(&config, &body);
    let second = render(&config, &body);
    assert_eq!(format!("{:?}", first.pages), format!("{:?}", second.pages));
    assert_eq!(first.anchors, second.anchors);
}
