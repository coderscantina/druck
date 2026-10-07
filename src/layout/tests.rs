use super::paragraph::protrusion;
use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, resolve};
use crate::config::theme::Lang;
use crate::document::{Footnote, InlineStyle};

const PROSE: &str = "The design of a page begins with its proportions. A text block that is too wide tires the \
    eye, which must travel far to return to the start of the next line; a block that is too narrow breaks the \
    text into fragments and forces frequent hyphenation.";

fn source() -> Source {
    Source::Document("/fake/doc.md".into())
}

fn config() -> Config {
    let settings = |source| SettingsInput {
        source,
        settings: FrontMatter::default(),
    };
    resolve(Inputs {
        theme: None,
        document: settings(source()),
        overrides: settings(Source::Cli {
            working_dir: "/fake".into(),
        }),
    })
    .expect("default configuration")
}

fn paragraph(line: u64, text: &str) -> Block {
    Block::Paragraph {
        at: Location { line, column: 1 },
        content: vec![Inline::Text {
            text: text.to_owned(),
            style: InlineStyle::default(),
        }],
    }
}

fn render(blocks: Vec<Block>) -> Result<Vec<Page>, Vec<Diagnostic>> {
    render_with_notes(blocks, Vec::new())
}

fn render_with_notes(blocks: Vec<Block>, footnotes: Vec<Footnote>) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let config = config();
    let fonts = Fonts::load(&config).expect("bundled fonts");
    layout(&Document { blocks, footnotes }, &config, &fonts, &source())
}

fn note(line: u64, text: &str) -> Footnote {
    Footnote {
        at: Location { line, column: 1 },
        blocks: vec![paragraph(line, text)],
    }
}

/// Text lines of a page as (left edge, right edge, text), top to bottom.
fn lines(page: &Page) -> Vec<(f64, f64, String)> {
    let mut lines: Vec<(f64, f64, f64, String)> = Vec::new();
    for item in &page.items {
        let Item::Text { x, y, run, .. } = item else { continue };
        match lines.iter_mut().find(|line| line.0 == y.0) {
            Some(line) => {
                line.2 = x.0 + run.width.0;
                line.3 = format!("{} {}", line.3, run.text);
            }
            None => lines.push((y.0, x.0, x.0 + run.width.0, run.text.clone())),
        }
    }
    lines
        .into_iter()
        .map(|(_, left, right, text)| (left, right, text))
        .collect()
}

#[test]
fn justified_lines_fill_the_measure_with_punctuation_hanging_into_the_margins() {
    let config = config();
    let style = &config.styles.body;
    let mut hanging = 0;
    for width in [150.0, 200.0, 250.0, 300.0] {
        let lines = set(&[text(PROSE, InlineStyle::default())], style, Lang::En, width);
        let (last, full) = lines.split_last().unwrap();
        for line in full {
            let (first, end) = (&line[0], &line[line.len() - 1]);
            let (left, right) = (
                protrusion::leading(&first.1, 0),
                protrusion::trailing(&end.1, end.1.text.len()),
            );
            assert!((first.0 + left).abs() < 0.01, "starts at {} with hang {left}", first.0);
            let edge = end.0 + end.1.width.0 - right;
            assert!((edge - width).abs() < 0.01, "ends at {edge}, not {width}");
            hanging += usize::from(left > 0.0) + usize::from(right > 0.0);
        }
        let end = &last[last.len() - 1];
        assert!(end.0 + end.1.width.0 < width - 1.0);
    }
    assert!(hanging > 0, "no line edge protrudes");
}

#[test]
fn only_a_continuing_paragraph_gets_a_first_line_indent() {
    let config = config();
    let pages = render(vec![paragraph(1, "First."), paragraph(3, "Second.")]).unwrap();
    let lines = lines(&pages[0]);
    let left = config.page.margin_inner.0;

    assert_eq!(lines[0].0, left);
    assert!((lines[1].0 - left - config.styles.body.first_line_indent.0).abs() < 1e-9);
}

#[test]
fn text_continues_on_new_pages_inside_the_text_area() {
    let config = config();
    let blocks = (0..30).map(|i| paragraph(i * 2 + 1, PROSE)).collect();
    let pages = render(blocks).unwrap();
    let bottom = config.page.height.0 - config.page.margin_bottom.0;

    assert!(pages.len() > 1);
    let words: usize = pages
        .iter()
        .flat_map(lines)
        .map(|(_, _, text)| text.split_whitespace().count())
        .sum();
    assert_eq!(words, 30 * PROSE.split_whitespace().count());
    for page in &pages {
        for item in &page.items {
            let Item::Text { y, .. } = item else { continue };
            assert!(y.0 < bottom && y.0 > config.page.margin_top.0);
        }
    }
}

#[test]
fn reports_a_word_wider_than_the_line_at_its_paragraph() {
    let errors = render(vec![paragraph(4, &"x".repeat(400))]).unwrap_err();

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location, Some((4, 1)));
    assert!(
        errors[0].message.contains("wider than the line"),
        "{}",
        errors[0].message
    );
}

#[test]
fn hyphenates_english_and_german_and_keeps_the_text() {
    let config = config();
    let english = "Typographical considerations notwithstanding, comprehensive hyphenation dramatically improves \
        justification.";
    let german = "Die Donaudampfschifffahrtsgesellschaft veröffentlicht Fahrplanänderungen für Schifffahrtswege.";
    for (lang, source) in [(Lang::En, english), (Lang::De, german)] {
        let lines = set(
            &[text(source, InlineStyle::default())],
            &config.styles.body,
            lang,
            120.0,
        );
        let ends: Vec<&str> = lines.iter().map(|line| line[line.len() - 1].1.text.as_str()).collect();
        assert!(ends.iter().any(|end| end.ends_with('-')), "{lang:?}: {ends:?}");
        assert_eq!(joined(&lines), source);
    }
}

#[test]
fn hyphenates_neither_unhyphenated_styles_nor_code() {
    let config = config();
    let style = Style {
        hyphenate: false,
        ..config.styles.body.clone()
    };
    let lines = set(&[text(PROSE, InlineStyle::default())], &style, Lang::En, 120.0);
    assert!(lines.iter().flatten().all(|(_, run)| !run.text.ends_with('-')));

    let code = InlineStyle {
        code: true,
        ..InlineStyle::default()
    };
    let words = "typographically ".repeat(6);
    let lines = set(&[text(&words, code)], &config.styles.body, Lang::En, 150.0);
    assert!(lines.iter().flatten().all(|(_, run)| !run.text.contains('-')));
}

#[test]
fn hyphenation_across_style_changes_keeps_each_style() {
    let config = config();
    let strong = InlineStyle {
        strong: true,
        ..InlineStyle::default()
    };
    let mut content = Vec::new();
    for _ in 0..12 {
        content.push(text("typo", strong.clone()));
        content.push(text("graphical ", InlineStyle::default()));
    }
    let lines = set(&content, &config.styles.body, Lang::En, 110.0);
    let fonts = Fonts::load(&config).unwrap();
    let bold = fonts.face(
        &config.styles.body.font,
        crate::config::theme::Weight::Bold,
        config.styles.body.style,
    );

    assert!(lines.iter().any(|line| line[line.len() - 1].1.text.ends_with('-')));
    assert_eq!(joined(&lines), "typographical ".repeat(12).trim_end());
    for (_, run) in lines.iter().flatten() {
        let in_bold = "typo".contains(run.text.trim_end_matches('-'));
        assert_eq!(run.face == bold, in_bold, "{}", run.text);
    }
}

#[test]
fn sets_raised_markers_and_notes_below_the_text_with_a_continuation_marker() {
    let config = config();
    let plain = |text: &str| text_inline(text);
    let first = Block::Paragraph {
        at: Location { line: 1, column: 1 },
        content: vec![
            plain("Short."),
            Inline::FootnoteRef(0),
            plain(" Long."),
            Inline::FootnoteRef(1),
        ],
    };
    let mut blocks = vec![first];
    blocks.extend((0..30).map(|i| paragraph(i + 3, PROSE)));
    let long = format!("Lengthy. {}", vec![PROSE; 30].join(" "));
    let pages = render_with_notes(blocks, vec![note(70, "First note."), note(72, &long)]).unwrap();

    let texts = |page: &Page| -> Vec<(f64, f64, String)> {
        page.items
            .iter()
            .filter_map(|item| match item {
                Item::Text { y, run, .. } => Some((y.0, run.size.0, run.text.clone())),
                _ => None,
            })
            .collect()
    };
    let first_page = texts(&pages[0]);
    let find = |texts: &[(f64, f64, String)], text: &str| texts.iter().find(|t| t.2 == text).cloned().unwrap();
    let (body_y, _, _) = find(&first_page, "Short.");
    let raise = config.inline.footnote_marker_raise.to_pt(config.styles.body.size).0;
    for marker in ["1", "2"] {
        assert!((find(&first_page, marker).0 - (body_y - raise)).abs() < 1e-9);
    }
    let (note_y, _, _) = find(&first_page, "First");
    let (long_y, _, _) = find(&first_page, "Lengthy.");
    assert!(body_y < note_y && note_y < long_y);
    assert!(texts(&pages[1]).iter().any(|t| t.2 == "(continued)"));

    let bottom = config.page.height.0 - config.page.margin_bottom.0;
    let body_sizes = [
        config.styles.body.size.0,
        config.inline.footnote_marker_size.to_pt(config.styles.body.size).0,
    ];
    for page in &pages {
        let rule = page.items.iter().find_map(|item| match item {
            Item::Rect { rect, .. } => Some(rect.y.0),
            _ => None,
        });
        for (y, size, _) in texts(page) {
            assert!(y < bottom);
            if let Some(rule) = rule {
                assert_eq!(
                    body_sizes.contains(&size),
                    y < rule,
                    "text at {y} of size {size}, rule at {rule}"
                );
            }
        }
    }
}

#[test]
fn reports_a_keep_group_taller_than_the_page_at_its_directive() {
    let keep = Block::Keep {
        at: Location { line: 5, column: 1 },
        blocks: (0..40).map(|i| paragraph(i * 2 + 6, PROSE)).collect(),
    };
    let errors = render(vec![paragraph(1, "Intro."), keep]).unwrap_err();

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location, Some((5, 1)));
    assert!(
        errors[0].message.starts_with("this keep group is"),
        "{}",
        errors[0].message
    );
}

#[test]
fn repeated_layout_is_identical() {
    let blocks = || (0..5).map(|i| paragraph(i * 2 + 1, PROSE)).collect();
    let first = format!("{:?}", render(blocks()).unwrap());
    assert_eq!(first, format!("{:?}", render(blocks()).unwrap()));
}

fn text_inline(value: &str) -> Inline {
    text(value, InlineStyle::default())
}

fn text(text: &str, style: InlineStyle) -> Inline {
    Inline::Text {
        text: text.to_owned(),
        style,
    }
}

/// Sets a paragraph of `width` and returns each line's text runs with their x positions.
fn set(content: &[Inline], style: &Style, lang: Lang, width: f64) -> Vec<Vec<(f64, ShapedRun)>> {
    let config = config();
    let fonts = Fonts::load(&config).expect("bundled fonts");
    let lines = paragraph::lines(content, style, &config.inline, &fonts, lang, width, 0.0).unwrap();
    lines
        .into_iter()
        .map(|line| {
            line.items
                .into_iter()
                .filter_map(|item| match item {
                    Item::Text { x, run, .. } => Some((x.0, run)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// The text of set lines, with added hyphens removed and lines joined by spaces otherwise.
fn joined(lines: &[Vec<(f64, ShapedRun)>]) -> String {
    let mut text = String::new();
    for line in lines {
        let mut previous_end = None;
        for (x, run) in line {
            if previous_end.is_some_and(|end: f64| x - end > 0.5) {
                text.push(' ');
            }
            text.push_str(&run.text);
            previous_end = Some(x + run.width.0);
        }
        match text.strip_suffix('-') {
            Some(hyphenated) => text = hyphenated.to_owned(),
            None => text.push(' '),
        }
    }
    text.trim_end().to_owned()
}
