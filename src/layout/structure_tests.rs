//! Document structures: title blocks and pages, page variants, headers and footers, numbering,
//! the table of contents, cross-references, settling page numbers, and page geometry.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Value, json};

use super::*;
use crate::bibliography::Bibliography;
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
    render_cited(config, body, None, images)
}

/// Lays out `body` with its citations formatted from the BibTeX text `bib`.
fn render_cited(config: &Config, body: &str, bib: Option<&str>, images: &[Image]) -> Result<Output, Vec<Diagnostic>> {
    let document = crate::markdown::parse(body, 1, &source()).expect("document parses");
    let bibliography =
        bib.map(|bib| Bibliography::parse(bib, Path::new("/fake/refs.bib")).expect("bibliography parses"));
    let cited = crate::citations::resolve(&document, bibliography.as_ref(), config, &source())?;
    let fonts = Fonts::load(config, &Default::default()).expect("bundled fonts");
    layout(&document, &cited, images, &HashMap::new(), config, &fonts, &source())
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

/// A group across the frame with one slot, as the left, center, or right slot of a band was.
fn group(align: &str, text: &str) -> Value {
    let anchor = if align == "right" { "top-right" } else { "top-left" };
    json!({"anchor": anchor, "align": align, "slots": [{"text": text}]})
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
    assert_eq!(error[0].property.as_deref(), Some("title-page.groups.0.slots.0.text"));
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
    let variant = |name: &str| json!({"header": [group("center", name)], "footer": null});
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
    let marks = json!([group("left", "{section}"), group("right", "{subsection}")]);
    let theme = json!({"version": 1, "pages": {"first": null, "body": {
        "header": marks,
        "footer": [group("center", "{page}")],
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
        "header": [{"anchor": "top-left", "slots": [{"text": "{section}", "required": true}]}],
    }}});
    let errors = render_images(&self::config("{}", required), "Lead.\n\n::: page-break\n# One", &[]).unwrap_err();
    assert_eq!(errors[0].property.as_deref(), Some("pages.body.header.0.slots.0"));
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
    let cited = format!("{body} Cited [@lee2022; @weber2020, p. 4].");
    let first = render_cited(&config, &cited, Some(BIB), &[]).expect("layout succeeds");
    let second = render_cited(&config, &cited, Some(BIB), &[]).expect("layout succeeds");
    assert_eq!(format!("{:?}", first.pages), format!("{:?}", second.pages));
    assert_eq!(first.anchors, second.anchors);
}

const BIB: &str = include_str!("../../tests/fixtures/bib/all-types.bib");

fn render_bib(front: &str, body: &str) -> Output {
    render_cited(&config(front, theme()), body, Some(BIB), &[]).expect("layout succeeds")
}

/// The text lines of every page as (page index, baseline, left edge, text), with no-break spaces as spaces.
fn all_lines(output: &Output) -> Vec<(usize, f64, f64, String)> {
    let pages = output.pages.iter().enumerate();
    pages
        .flat_map(|(index, page)| lines(page).into_iter().map(move |(y, x, text)| (index, y, x, text)))
        .map(|(index, y, x, text)| (index, y, x, text.replace('\u{a0}', " ")))
        .collect()
}

/// The anchors that the links on all pages go to, in page order, without repeats for wrapped links.
fn link_targets(output: &Output) -> Vec<usize> {
    let mut targets: Vec<usize> = Vec::new();
    for item in output.pages.iter().flat_map(|page| &page.items) {
        if let Item::Link {
            link: Link::Anchor(id), ..
        } = item
            && targets.last() != Some(id)
        {
            targets.push(*id);
        }
    }
    targets
}

#[test]
fn citations_show_their_style_and_link_to_their_bibliography_entry() {
    let body = "# Intro {#sec:intro}\n\n[@smith2024a, p. 12; @lee2022]\n\n@weber2020 [pp. 3-5] and @sec:intro.\n";
    let cases = [
        (
            "author-date",
            [
                "(Smith and Jones 2024, p. 12; Lee et al. 2022)",
                "Weber (2020, pp. 3–5) and Section 1.",
            ],
            "Smith, Ada and Bob Jones (2024).",
            [2, 1, 3],
        ),
        (
            "numeric",
            ["[1, p. 12; 2]", "Weber [3, pp. 3–5] and Section 1."],
            "[1] Smith, Ada",
            [1, 2, 3],
        ),
    ];
    for (style, texts, smith, entries) in cases {
        let output = render_bib(&format!("citation-style: {style}"), body);
        let lines = all_lines(&output);
        let has = |text: &str| lines.iter().any(|line| line.3 == text);
        assert!(texts.iter().all(|text| has(text)), "{style}: {lines:?}");

        let intro = output.outline[0].anchor;
        let references = &output.outline[1];
        assert_eq!((references.title.as_str(), references.level), ("References", 1));
        let first = references.anchor;
        let expected = entries.map(|entry| first + entry);
        assert_eq!(link_targets(&output), [expected[0], expected[1], expected[2], intro]);
        let anchor = output.anchors[first + entries[0]];
        let line = lines
            .iter()
            .find(|line| line.3.starts_with(smith))
            .expect("Smith's entry");
        assert!(
            line.1 > anchor.y.0 && line.1 - anchor.y.0 < 15.0,
            "{style}: the anchor marks the entry"
        );
    }
}

#[test]
fn the_bibliography_lists_each_cited_entry_once_in_style_order_with_a_hanging_indent() {
    let keys = [
        "weber2020",
        "smith2024b",
        "typst",
        "knuth1984",
        "lee2022",
        "ito2021",
        "who2023",
        "lab2018",
        "pdfspec",
        "mueller2019",
        "smith2024a",
        "weber2020",
    ];
    let citations: Vec<String> = keys.iter().map(|key| format!("[@{key}]")).collect();
    let body = format!("# Text\n\n{}\n", citations.join(" "));
    let starts = |output: &Output| -> Vec<String> {
        let lines = all_lines(output);
        let heading = lines.iter().position(|line| line.3 == "References").expect("heading");
        let left = lines[heading].2;
        let entries = lines[heading + 1..].iter().filter(|line| (line.2 - left).abs() < 0.01);
        entries
            .map(|line| line.3.split(" (").next().unwrap_or_default().to_owned())
            .collect()
    };

    let author_date = render_bib("toc: true", &body);
    assert_eq!(
        starts(&author_date),
        [
            "Adobe Systems",
            "Ito, Mei",
            "Knuth, Donald E.",
            "Kyber Lab",
            "Lee, Cy et al.",
            "Müller, Hans, Eva Großmann, and Cy Lee",
            "Smith, Ada",
            "Smith, Ada and Bob Jones",
            "Typst documentation",
            "Weber, Lena",
            "World Health Organization",
        ]
    );
    let numeric = render_bib("toc: true\ncitation-style: numeric", &body);
    let numbered = starts(&numeric);
    assert_eq!(numbered.len(), 11);
    assert_eq!(numbered[0], "[1] Weber, Lena");
    assert_eq!(numbered[10], "[11] Smith, Ada and Bob Jones");

    // A wrapped author-date entry hangs by the theme's indent, and numeric entries set their text after a
    // label column as wide as "[11]" on every line, so the text after "[9]" and "[10]" starts together.
    let config = config("{}", theme());
    let lines = all_lines(&author_date);
    let heading = lines.iter().position(|line| line.3 == "References").expect("heading");
    let wrapped = lines[heading + 1..]
        .iter()
        .find(|line| line.2 > lines[heading].2 + 1.0)
        .expect("an entry that wraps");
    let hang = wrapped.2 - lines[heading].2;
    assert!((hang - config.bibliography.hanging_indent.0).abs() < 0.01, "{hang}");

    let texts: Vec<_> = numeric
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Text { x, run, .. } => Some((x.0, run.text.as_str())),
            _ => None,
        })
        .collect();
    let starts: Vec<f64> = texts
        .windows(2)
        .filter(|pair| pair[0].1 == "[9]" || pair[0].1 == "[10]")
        .map(|pair| pair[1].0)
        .collect();
    assert_eq!(starts.len(), 2);
    assert!((starts[0] - starts[1]).abs() < 0.01, "{starts:?}");
    let lines = all_lines(&numeric);
    let references = numeric.outline.last().expect("bookmarks");
    let page = page_of(&numeric, references.anchor);
    assert!(
        lines
            .iter()
            .any(|line| line.3.starts_with("References .") && line.3.ends_with(&format!(" {page}")))
    );
}

#[test]
fn the_bibliography_marker_sets_the_section_in_columns_and_entry_errors_name_the_bib_file() {
    let citations: Vec<&str> = BIB
        .lines()
        .filter_map(|line| line.strip_prefix('@')?.split_once('{')?.1.strip_suffix(','))
        .collect();
    let citations: Vec<String> = citations.iter().map(|key| format!("[@{key}]")).collect();
    let body = |citations: &[String]| {
        format!(
            "{}\n\n::: columns\n::: bibliography\n:::\n\nAfter.\n",
            citations.join(" ")
        )
    };
    let config = config("{}", theme());

    // The long URL of `pdfspec` does not fit a column, which is reported at the entry in the `.bib` file.
    let errors = render_cited(&config, &body(&citations), Some(BIB), &[]).unwrap_err();
    assert_eq!(errors[0].source, Some(Source::Bibliography("/fake/refs.bib".into())));
    assert_eq!(errors[0].location, Some((90, 1)));
    assert!(
        errors[0].message.starts_with("entry `pdfspec`: \"https://"),
        "{}",
        errors[0].message
    );

    let fitting: Vec<String> = citations
        .into_iter()
        .filter(|citation| citation != "[@pdfspec]")
        .collect();
    let output = render_cited(&config, &body(&fitting), Some(BIB), &[]).expect("layout succeeds");
    let lines = all_lines(&output);
    let middle = config.page.margin_inner.0 + config.page.text_width().0 / 2.0;
    let heading = lines.iter().position(|line| line.3 == "References").expect("heading");
    let after = lines.iter().position(|line| line.3 == "After.").expect("text after");
    assert!(lines[heading].2 < middle, "the heading starts the first column");
    assert!(
        lines[heading..after].iter().any(|line| line.2 > middle),
        "entries fill the second column"
    );
}

const MM: f64 = 72.0 / 25.4;

/// The left and right edge of the run that reads `text` on `page`, else of the line that does.
fn extent(page: &Page, text: &str) -> (f64, f64) {
    let run = page.items.iter().find_map(|item| match item {
        Item::Text { x, run, .. } if run.text == text => Some((x.0, x.0 + run.width.0)),
        _ => None,
    });
    run.unwrap_or_else(|| {
        let y = baseline(page, text);
        let runs = page.items.iter().filter_map(|item| match item {
            Item::Text { x, y: at, run, .. } if at.0 == y => Some((x.0, x.0 + run.width.0)),
            _ => None,
        });
        runs.fold((f64::MAX, f64::MIN), |(left, right), run| {
            (left.min(run.0), right.max(run.1))
        })
    })
}

/// The baseline of the run that reads `text` on `page`.
fn run_baseline(page: &Page, text: &str) -> f64 {
    let run = page.items.iter().find_map(|item| match item {
        Item::Text { y, run, .. } if run.text == text => Some(y.0),
        _ => None,
    });
    run.unwrap_or_else(|| panic!("no run {text:?} in {:?}", texts(page)))
}

fn rules(page: &Page) -> Vec<Rect> {
    let rects = page.items.iter().filter_map(|item| match item {
        Item::Rect { rect, .. } => Some(*rect),
        _ => None,
    });
    rects.collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn margins_mirror_on_even_pages_unless_mirroring_is_off() {
    for mirror in [true, false] {
        let margins = json!({"inner": "40mm", "outer": "20mm", "mirror": mirror});
        let config = config(
            "numbered-headings: false",
            json!({"version": 1, "page": {"margins": margins}}),
        );
        let output = render(&config, "# One\n\nOne.\n\n::: page-break\n# Two");
        let geometry = &config.page;
        let even = if mirror {
            geometry.margin_outer.0
        } else {
            geometry.margin_inner.0
        };
        assert!(close(extent(&output.pages[0], "One.").0, geometry.margin_inner.0));
        let heading = lines(&output.pages[1])
            .into_iter()
            .find(|line| line.0 > geometry.margin_top.0);
        assert!(close(heading.expect("heading").1, even), "mirror {mirror}");
        assert!(
            close(extent(&output.pages[1], "Two").0, even),
            "the header follows the margins"
        );
        let (left, right) = extent(&output.pages[1], "2");
        let middle = even + geometry.text_width().0 / 2.0;
        assert!(close((left + right) / 2.0, middle), "the footer follows the margins");
    }
}

#[test]
fn prose_keeps_the_text_width_from_the_inner_edge_and_wide_blocks_span_the_frame() {
    let page = json!({"margins": {"inner": "30mm", "outer": "20mm"}, "text-width": "100mm"});
    let config = config("{}", json!({"version": 1, "page": page}));
    let geometry = &config.page;
    let (frame, prose) = (geometry.text_width().0, 100.0 * MM);
    let table = format!(
        "| Key | Value |\n| --- | --- |\n| A | {} |",
        "Wide cell text. ".repeat(12)
    );
    let body = format!("{PROSE}\n\n{table}\n\n::: columns\n{PROSE}\n:::\n\n::: page-break\n{PROSE}\n\n{table}");
    let output = render(&config, &body);

    for (index, page) in output.pages.iter().enumerate() {
        // The inner edge is on the left of page 1 and on the right of page 2.
        let left = geometry.left_margin(index).0;
        let prose_left = left + geometry.prose_shift(index);
        assert!(close(prose_left - left, if index == 0 { 0.0 } else { frame - prose }));
        let first = extent(page, &texts(page)[0]);
        assert!(
            close(first.0, prose_left),
            "prose starts at the inner edge on page {}",
            index + 1
        );
        for text in texts(page).iter().filter(|text| PROSE.contains(text.as_str())) {
            let (start, end) = extent(page, text);
            assert!(start > prose_left - 3.0 && end < prose_left + prose + 3.0, "{text:?}");
        }
        let widest = rules(page).iter().map(|rule| rule.width.0).fold(0.0, f64::max);
        assert!(close(widest, frame), "the table spans the frame on page {}", index + 1);
        assert!(rules(page).iter().all(|rule| close(rule.x.0, left)));
    }

    // Wide blocks widen only where prose is set directly: a code line wider than the prose width fits
    // at the top level but not in a list.
    let code = "x".repeat(70);
    assert!(render_images(&config, &format!("```\n{code}\n```"), &[]).is_ok());
    let errors = render_images(&config, &format!("- Item\n\n  ```\n  {code}\n  ```"), &[]).unwrap_err();
    assert!(errors[0].message.starts_with("code line is"), "{}", errors[0].message);
}

#[test]
fn title_page_groups_sit_at_their_anchor_and_offset() {
    let group = |anchor: &str| {
        let align = if anchor.ends_with("right") { "right" } else { "left" };
        json!({"anchor": anchor, "offset": {"x": "10mm", "y": "20mm"}, "width": "50mm", "align": align,
            "slots": [{"text": anchor, "style": "date"}]})
    };
    let anchors = [
        "top-left",
        "top-right",
        "middle-left",
        "middle-right",
        "bottom-left",
        "bottom-right",
    ];
    let groups: Vec<_> = anchors.into_iter().map(group).collect();
    let config = config(
        "title-page: true",
        json!({"version": 1, "title-page": {"groups": groups}}),
    );
    let output = render(&config, "Text.");
    let page = &output.pages[0];

    let geometry = &config.page;
    let (left, width, height) = (
        geometry.margin_inner.0,
        geometry.text_width().0,
        geometry.text_height().0,
    );
    let line = config.styles.date.size.0 * config.styles.date.line_height;
    // Line boxes: the top one starts 20mm below the frame's top, the bottom one ends 20mm above its
    // bottom, and the middle one is centered 20mm below the frame's middle.
    let top = run_baseline(page, "top-left");
    assert!(close(run_baseline(page, "middle-left") - top, (height - line) / 2.0));
    assert!(close(
        run_baseline(page, "bottom-left") - top,
        height - 2.0 * 20.0 * MM - line
    ));
    for vertical in ["top", "middle", "bottom"] {
        let (start, _) = extent(page, &format!("{vertical}-left"));
        assert!(close(start, left + 10.0 * MM));
        let (_, end) = extent(page, &format!("{vertical}-right"));
        assert!(close(end, left + width - 10.0 * MM));
        let right = run_baseline(page, &format!("{vertical}-right"));
        assert_eq!(run_baseline(page, &format!("{vertical}-left")), right);
    }
}

#[test]
fn band_groups_stack_multi_line_slots_and_meta_lists_at_the_band_baseline() {
    let footer = json!([
        {"anchor": "top-left", "width": "50mm", "slots": [
            {"text": "{meta.company}"},
            {"text": "{meta.address}"},
            {"text": "{meta.missing}"},
        ]},
        {"anchor": "middle-left", "offset": {"x": "60mm"}, "width": "40mm", "slots": [{"text": "One\nTwo\nThree"}]},
        {"anchor": "top-right", "width": "30mm", "align": "right", "slots": [{"text": "Page {page}|{pages}"}]},
    ]);
    let theme = json!({"version": 1, "pages": {"first": null, "body": {
        "header": [{"anchor": "bottom-left", "slots": [{"text": "Upper\nLower"}]}],
        "footer": footer,
    }}});
    let front = "meta:\n  company: Example Ltd\n  address: [Main Street 1, \"\", 1010 Vienna]";
    let config = config(front, theme);
    let output = render(&config, "One.\n\n::: page-break\nTwo.\n\n::: page-break\nThree.");
    let page = &output.pages[0];

    let geometry = &config.page;
    let header = geometry.margin_top.0 - geometry.header_offset.0;
    let footer = geometry.margin_top.0 + geometry.text_height().0 + geometry.footer_offset.0;
    assert_eq!(
        run_baseline(page, "Lower"),
        header,
        "a bottom anchor puts the last baseline on the band's"
    );
    assert!(run_baseline(page, "Upper") < header);
    assert_eq!(
        run_baseline(page, "Example Ltd"),
        footer,
        "a top anchor puts the first baseline on the band's"
    );
    let address = [run_baseline(page, "Main Street 1"), run_baseline(page, "1010 Vienna")];
    assert!(
        close(address[0] - footer, address[1] - address[0]),
        "one line per entry, blank entries dropped"
    );
    let middle = (run_baseline(page, "One") + run_baseline(page, "Three")) / 2.0;
    assert!(
        close(middle, footer),
        "a middle anchor centers the lines on the band's baseline"
    );
    assert!(close(extent(page, "One").0, geometry.margin_inner.0 + 60.0 * MM));

    let totals: Vec<_> = output
        .pages
        .iter()
        .filter_map(|page| {
            page.items.iter().find_map(|item| match item {
                Item::Text { run, .. } if run.text.starts_with("Page") => Some(run.text.clone()),
                _ => None,
            })
        })
        .collect();
    assert_eq!(totals, ["Page 1|3", "Page 2|3", "Page 3|3"]);
    let (_, end) = extent(page, "Page 1|3");
    assert!(close(end, geometry.margin_inner.0 + geometry.text_width().0));
}

#[test]
fn a_missing_meta_value_omits_its_slot_unless_it_is_required() {
    let theme = json!({"version": 1, "title-page": {"groups": [{"anchor": "top-left", "slots": [
        {"text": "{meta.client}", "required": true},
        {"text": "Ref. {meta.ref}"},
        {"text": "{title}"},
    ]}]}});
    let front = "title: Offer\ntitle-page: true";
    let output = render(
        &config(&format!("{front}\nmeta: {{client: ACME}}"), theme.clone()),
        "Text.",
    );
    assert_eq!(texts(&output.pages[0]), ["ACME", "Offer"]);

    let errors = check_title(&config(front, theme), &source()).unwrap_err();
    assert_eq!(errors[0].property.as_deref(), Some("title-page.groups.0.slots.0.text"));
    assert_eq!(
        errors[0].message,
        "this required slot needs {meta.client}; set meta.client in the front matter"
    );
}
