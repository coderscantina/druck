use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use super::paragraph::protrusion;
use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, resolve};
use crate::config::resolved::PageGeometry;
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
        class: None,
    }
}

fn render(blocks: Vec<Block>) -> Result<Vec<Page>, Vec<Diagnostic>> {
    render_with_notes(blocks, Vec::new())
}

fn render_with_notes(blocks: Vec<Block>, footnotes: Vec<Footnote>) -> Result<Vec<Page>, Vec<Diagnostic>> {
    render_with_images(blocks, footnotes, &[])
}

fn render_with_images(
    blocks: Vec<Block>,
    footnotes: Vec<Footnote>,
    images: &[Image],
) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).expect("bundled fonts");
    let document = Document {
        blocks,
        footnotes,
        images: Vec::new(),
        citations: Vec::new(),
    };
    let cited = Cited::default();
    layout(&document, &cited, images, &HashMap::new(), &config, &fonts, &source()).map(|output| output.pages)
}

fn note(line: u64, text: &str) -> Footnote {
    Footnote {
        at: Location { line, column: 1 },
        blocks: vec![paragraph(line, text)],
    }
}

/// Whether a baseline lies in the text area rather than in a header or footer.
fn in_text_area(y: Pt) -> bool {
    let page = geometry();
    y.0 > page.margin_top.0 && y.0 < page.margin_top.0 + page.text_height().0
}

/// The default page geometry, resolved once.
fn geometry() -> &'static PageGeometry {
    static PAGE: LazyLock<PageGeometry> = LazyLock::new(|| config().page);
    &PAGE
}

/// Whether a baseline is that of a header or footer.
fn is_band(y: Pt) -> bool {
    let page = geometry();
    let header = page.margin_top.0 - page.header_offset.0;
    let footer = page.margin_top.0 + page.text_height().0 + page.footer_offset.0;
    y.0 == header || y.0 == footer
}

/// Text lines of a page as (left edge, right edge, text), top to bottom. Headers and footers are left out.
fn lines(page: &Page) -> Vec<(f64, f64, String)> {
    let mut lines: Vec<(f64, f64, f64, String)> = Vec::new();
    for item in &page.items {
        let Item::Text { x, y, run, .. } = item else { continue };
        if !in_text_area(*y) {
            continue;
        }
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
            assert!(y.0 < bottom && y.0 > config.page.margin_top.0 || is_band(*y));
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
    let fonts = Fonts::load(&config, &BTreeMap::new()).unwrap();
    let bold = fonts.face(
        &config.styles.body.font,
        crate::config::theme::Weight::BOLD,
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
        class: None,
    };
    let mut blocks = vec![first];
    blocks.extend((0..30).map(|i| paragraph(i + 3, PROSE)));
    let long = format!("Lengthy. {}", vec![PROSE; 30].join(" "));
    let pages = render_with_notes(blocks, vec![note(70, "First note."), note(72, &long)]).unwrap();

    let texts = |page: &Page| -> Vec<(f64, f64, String)> {
        page.items
            .iter()
            .filter_map(|item| match item {
                Item::Text { y, run, .. } if !is_band(*y) => Some((y.0, run.size.0, run.text.clone())),
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

fn columns(line: u64, blocks: Vec<Block>) -> Block {
    Block::Columns {
        at: Location { line, column: 1 },
        blocks,
    }
}

/// A paragraph that starts with the word `P{number}`, so reading order can be checked.
fn numbered(number: u64) -> Block {
    paragraph(number * 2 + 1, &format!("P{number} {PROSE}"))
}

/// A placed line: its left and right edges, baseline, and text.
#[derive(Debug)]
struct Placed {
    left: f64,
    right: f64,
    y: f64,
    text: String,
}

impl Placed {
    fn in_second_column(&self, page: &Page) -> bool {
        self.left > page.width.0 / 2.0
    }

    fn is_full_width(&self, page: &Page) -> bool {
        self.left < page.width.0 / 2.0 && self.right > page.width.0 / 2.0
    }
}

/// The text lines of a page in placement order. Raised markers join the line they are in. Headers and
/// footers are left out.
fn placed(page: &Page) -> Vec<Placed> {
    let mut lines: Vec<Placed> = Vec::new();
    for item in &page.items {
        let Item::Text { x, y, run, .. } = item else { continue };
        if !in_text_area(*y) {
            continue;
        }
        let right = x.0 + run.width.0;
        match lines.last_mut() {
            Some(line) if (line.y - y.0).abs() < 6.0 => {
                line.y = line.y.max(y.0);
                line.right = line.right.max(right);
                line.text = format!("{} {}", line.text, run.text);
            }
            _ => lines.push(Placed {
                left: x.0,
                right,
                y: y.0,
                text: run.text.clone(),
            }),
        }
    }
    lines
}

/// The numbers of the `P{number}` words over all pages, in placement order.
fn reading_order(pages: &[Page]) -> Vec<u64> {
    pages
        .iter()
        .flat_map(placed)
        .flat_map(|line| {
            let words: Vec<u64> = line
                .text
                .split_whitespace()
                .filter_map(|word| word.strip_prefix('P')?.parse().ok())
                .collect();
            words
        })
        .collect()
}

/// The lowest baseline in each column among `lines`.
fn column_bottoms(page: &Page, lines: &[Placed]) -> (f64, f64) {
    let bottom = |second: bool| {
        lines
            .iter()
            .filter(|line| line.in_second_column(page) == second)
            .map(|line| line.y)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    (bottom(false), bottom(true))
}

fn body_line() -> f64 {
    let config = config();
    config.styles.body.size.0 * config.styles.body.line_height
}

#[test]
fn text_flows_down_the_first_column_then_the_second() {
    let pages = render(vec![columns(1, (0..3).map(numbered).collect())]).unwrap();
    let lines = placed(&pages[0]);
    let second = lines.iter().position(|line| line.in_second_column(&pages[0])).unwrap();

    assert_eq!(reading_order(&pages), [0, 1, 2]);
    assert!(lines[..second].iter().all(|line| !line.in_second_column(&pages[0])));
    assert!(lines[second..].iter().all(|line| line.in_second_column(&pages[0])));
    assert_eq!(lines[0].y, lines[second].y, "both columns start at the top");
    let width = super::pages::column_width(&config().page);
    assert!(lines.iter().all(|line| line.right - line.left < width + 3.0));
}

#[test]
fn the_final_columns_of_a_section_are_balanced() {
    for count in [2, 3, 5] {
        let pages = render(vec![columns(1, (0..count).map(numbered).collect())]).unwrap();
        let (first, second) = column_bottoms(&pages[0], &placed(&pages[0]));
        assert!(
            (first - second).abs() <= body_line() + 1e-6,
            "{count}: {first} and {second}"
        );
    }
}

#[test]
fn a_column_section_continues_across_pages_with_full_columns() {
    let config = config();
    let pages = render(vec![columns(1, (0..40).map(numbered).collect())]).unwrap();
    let bottom = config.page.height.0 - config.page.margin_bottom.0;

    assert!(pages.len() > 2);
    assert_eq!(reading_order(&pages), (0..40).collect::<Vec<_>>());
    for page in &pages[..pages.len() - 1] {
        let (first, second) = column_bottoms(page, &placed(page));
        assert!(first > bottom - 2.0 * body_line() && second > bottom - 2.0 * body_line());
    }
}

#[test]
fn a_full_width_block_balances_the_columns_before_it_and_columns_resume_below() {
    let full = Block::FullWidth {
        at: Location { line: 50, column: 1 },
        blocks: vec![numbered(2)],
    };
    let section = columns(1, vec![numbered(0), numbered(1), full, numbered(3), numbered(4)]);
    let pages = render(vec![section]).unwrap();
    let page = &pages[0];
    let lines = placed(page);
    let start = lines.iter().position(|line| line.text.starts_with("P2")).unwrap();
    let end = start
        + lines[start..]
            .iter()
            .take_while(|line| line.is_full_width(page))
            .count();
    let (before, block, after) = (&lines[..start], &lines[start..end], &lines[end..]);

    assert_eq!(pages.len(), 1);
    assert_eq!(reading_order(&pages), [0, 1, 2, 3, 4]);
    assert!(block.len() > 1);
    let (first, second) = column_bottoms(page, before);
    assert!((first - second).abs() <= body_line() + 1e-6);
    assert!(block[0].y > first.max(second));
    assert!(after.iter().any(|line| line.in_second_column(page)));
    assert!(after.iter().all(|line| line.y > block[block.len() - 1].y));
}

#[test]
fn changing_from_one_to_two_columns_and_back_does_not_start_a_new_page() {
    let blocks = vec![numbered(0), columns(3, vec![numbered(1), numbered(2)]), numbered(3)];
    let pages = render(blocks).unwrap();
    let page = &pages[0];
    let lines = placed(page);
    let last = lines.iter().position(|line| line.text.starts_with("P3")).unwrap();
    let (first, second) = column_bottoms(page, &lines[..last]);

    assert_eq!(pages.len(), 1);
    assert_eq!(reading_order(&pages), [0, 1, 2, 3]);
    assert!(lines[0].is_full_width(page) && lines[last].is_full_width(page));
    assert!(
        lines[last].y > first.max(second),
        "the next layout starts below the taller column"
    );
}

#[test]
fn notes_from_both_columns_share_one_area_in_reference_order() {
    let referring = |number: u64, note: usize| Block::Paragraph {
        at: Location {
            line: number,
            column: 1,
        },
        content: vec![text_inline(&format!("P{number} {PROSE}")), Inline::FootnoteRef(note)],
        class: None,
    };
    let section = columns(1, vec![referring(0, 0), referring(1, 1)]);
    let notes = vec![note(10, "First note."), note(12, "Second note.")];
    let pages = render_with_notes(vec![section], notes).unwrap();
    let page = &pages[0];
    let lines = placed(page);
    let find = |text: &str| lines.iter().position(|line| line.text.contains(text)).unwrap();
    let (first, second) = (find("First note."), find("Second note."));

    assert_eq!(pages.len(), 1);
    assert!(
        lines
            .iter()
            .any(|line| line.in_second_column(page) && line.text.split_whitespace().any(|word| word == "2"))
    );
    assert!(first < second);
    assert_eq!(lines[first].left, lines[0].left);
    let (left, right) = column_bottoms(page, &lines[..first]);
    assert!(lines[first].y > left.max(right));
}

#[test]
fn a_keep_group_inside_columns_stays_in_one_column() {
    let keep = Block::Keep {
        at: Location { line: 20, column: 1 },
        blocks: vec![numbered(3), numbered(4)],
    };
    let mut blocks: Vec<Block> = (0..3).map(numbered).collect();
    blocks.push(keep);
    blocks.extend((5..7).map(numbered));
    let pages = render(vec![columns(1, blocks)]).unwrap();
    let page = &pages[0];
    let lines = placed(page);
    let start = lines.iter().position(|line| line.text.starts_with("P3")).unwrap();
    let end = lines.iter().position(|line| line.text.starts_with("P5")).unwrap();

    assert_eq!(pages.len(), 1);
    assert_eq!(reading_order(&pages), (0..7).collect::<Vec<_>>());
    let column = lines[start].in_second_column(page);
    assert!(
        lines[start..end]
            .iter()
            .all(|line| line.in_second_column(page) == column)
    );
}

#[test]
fn reports_a_keep_group_taller_than_a_column_at_its_directive() {
    let keep = || Block::Keep {
        at: Location { line: 3, column: 1 },
        blocks: (0..11).map(numbered).collect(),
    };
    assert!(render(vec![keep()]).is_ok(), "the group fits at full width");

    let errors = render(vec![columns(1, vec![keep()])]).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location, Some((3, 1)));
    assert!(
        errors[0].message.starts_with("this keep group is"),
        "{}",
        errors[0].message
    );
}

#[test]
fn repeated_column_layout_is_identical() {
    let blocks = || {
        let full = Block::FullWidth {
            at: Location { line: 40, column: 1 },
            blocks: vec![numbered(12)],
        };
        let mut section: Vec<Block> = (0..12).map(numbered).collect();
        section.push(full);
        section.extend((13..20).map(numbered));
        vec![numbered(30), columns(1, section), numbered(31)]
    };
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
    let fonts = Fonts::load(&config, &BTreeMap::new()).expect("bundled fonts");
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

/// A plain SVG image of the given natural size in points.
fn svg(width: f64, height: f64) -> Image {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}pt" height="{height}pt"><rect width="100%" height="100%"/></svg>"#
    );
    Image::decode(svg.into_bytes(), "svg").expect("valid SVG")
}

/// Image `image` at source line `line` with the caption `caption`, or none if empty.
fn figure(line: u64, image: usize, caption: Vec<Inline>) -> Block {
    Block::Image {
        at: Location { line, column: 1 },
        image,
        caption,
        label: None,
    }
}

/// The page index and rectangle of every image, in placement order.
fn image_rects(pages: &[Page]) -> Vec<(usize, Rect)> {
    let mut rects = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        for item in &page.items {
            if let Item::Image { rect, .. } = item {
                rects.push((index, *rect));
            }
        }
    }
    rects
}

/// The page index and placed line whose text contains `text`.
fn find_line(pages: &[Page], text: &str) -> (usize, Placed) {
    pages
        .iter()
        .enumerate()
        .find_map(|(index, page)| {
            let line = placed(page).into_iter().find(|line| line.text.contains(text))?;
            Some((index, line))
        })
        .expect("line is placed")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn images_shrink_proportionally_to_the_frame_and_never_grow() {
    let config = config();
    let images = [svg(1000.0, 500.0), svg(100.0, 50.0), svg(200.0, 2000.0)];
    let blocks = vec![
        figure(1, 0, Vec::new()),
        figure(3, 1, Vec::new()),
        figure(5, 2, vec![text_inline("Tall.")]),
    ];
    let pages = render_with_images(blocks, Vec::new(), &images).unwrap();
    let rects = image_rects(&pages);
    let width = config.page.text_width().0;

    assert_eq!(rects.len(), 3);
    let wide = rects[0].1;
    assert!(
        close(wide.width.0, width) && close(wide.height.0, width / 2.0),
        "{wide:?}"
    );
    let small = rects[1].1;
    assert!(close(small.width.0, 100.0) && close(small.height.0, 50.0), "{small:?}");
    assert!(
        close(small.x.0 - config.page.margin_inner.0, (width - 100.0) / 2.0),
        "centered"
    );
    let (page, tall) = rects[2];
    let (caption_page, caption) = find_line(&pages, "Tall.");
    let bottom = config.page.height.0 - config.page.margin_bottom.0;
    assert!(close(tall.width.0 * 10.0, tall.height.0), "{tall:?}");
    assert!(
        tall.height.0 > config.page.text_height().0 - 3.0 * body_line(),
        "{tall:?}"
    );
    assert_eq!(caption_page, page);
    assert!(caption.y > tall.y.0 + tall.height.0 && caption.y < bottom);
}

#[test]
fn captions_are_numbered_with_the_theme_label() {
    let images = [svg(50.0, 20.0)];
    let blocks = vec![
        figure(1, 0, vec![text_inline("First.")]),
        figure(3, 0, Vec::new()),
        figure(5, 0, vec![text_inline("Second.")]),
    ];
    let pages = render_with_images(blocks, Vec::new(), &images).unwrap();
    let captions: Vec<String> = placed(&pages[0]).into_iter().map(|line| line.text).collect();

    assert_eq!(captions, ["Figure 1: First.", "Figure 2: Second."]);
}

#[test]
fn an_image_and_its_caption_move_together_to_the_next_page() {
    let images = [svg(300.0, 300.0)];
    let mut moved = false;
    for count in 0..12 {
        let mut blocks: Vec<Block> = (0..count).map(numbered).collect();
        blocks.push(figure(100, 0, vec![text_inline("Kept caption.")]));
        blocks.push(numbered(50));
        let pages = render_with_images(blocks, Vec::new(), &images).unwrap();
        let (page, rect) = image_rects(&pages)[0];
        let (caption_page, caption) = find_line(&pages, "Kept caption.");

        assert_eq!(page, caption_page, "{count} paragraphs before");
        assert!(caption.y > rect.y.0 + rect.height.0);
        assert_eq!(reading_order(&pages), (0..count).chain([50]).collect::<Vec<_>>());
        moved |= page > 0 && reading_order(&pages[..page]).len() == count as usize;
    }
    assert!(moved, "some image moved to the next page");
}

#[test]
fn an_image_in_columns_has_the_column_width_and_moves_with_its_caption_to_the_next_column() {
    let width = super::pages::column_width(&config().page);
    let images = [svg(1000.0, 800.0)];
    let mut second = false;
    for count in 0..6 {
        let mut section: Vec<Block> = (0..count).map(numbered).collect();
        section.push(figure(100, 0, vec![text_inline("Column caption.")]));
        section.extend((10..14).map(numbered));
        let pages = render_with_images(vec![columns(1, section)], Vec::new(), &images).unwrap();
        let (page, rect) = image_rects(&pages)[0];
        let (caption_page, caption) = find_line(&pages, "Column caption.");
        let in_second = rect.x.0 > pages[page].width.0 / 2.0;

        assert!(close(rect.width.0, width), "{rect:?}");
        assert_eq!(page, caption_page);
        assert_eq!(caption.in_second_column(&pages[page]), in_second);
        assert!(caption.y > rect.y.0 + rect.height.0);
        second |= in_second;
    }
    assert!(second, "some image moved to the second column");
}

#[test]
fn a_full_width_image_sits_between_balanced_column_regions() {
    let config = config();
    let images = [svg(1000.0, 300.0)];
    let full = Block::FullWidth {
        at: Location { line: 50, column: 1 },
        blocks: vec![figure(51, 0, vec![text_inline("Wide caption.")])],
    };
    let section = columns(1, vec![numbered(0), numbered(1), full, numbered(2), numbered(3)]);
    let pages = render_with_images(vec![section], Vec::new(), &images).unwrap();
    let page = &pages[0];
    let lines = placed(page);
    let (_, rect) = image_rects(&pages)[0];
    let caption = lines
        .iter()
        .position(|line| line.text.contains("Wide caption."))
        .unwrap();
    let (first, second) = column_bottoms(page, &lines[..caption]);

    assert_eq!(pages.len(), 1);
    assert!(close(rect.width.0, config.page.text_width().0), "{rect:?}");
    assert!(rect.y.0 > first.max(second));
    assert!(lines[caption].y > rect.y.0 + rect.height.0);
    let after = &lines[caption + 1..];
    assert!(after.iter().any(|line| line.in_second_column(page)));
    assert!(after.iter().all(|line| line.y > lines[caption].y));
}

#[test]
fn a_note_stays_on_the_page_of_its_reference_beside_an_image() {
    let images = [svg(300.0, 250.0)];
    for count in 0..10 {
        let mut blocks: Vec<Block> = (0..count).map(numbered).collect();
        blocks.push(Block::Paragraph {
            at: Location { line: 90, column: 1 },
            content: vec![text_inline("Noted text."), Inline::FootnoteRef(0)],
            class: None,
        });
        blocks.push(figure(100, 0, vec![text_inline("Noted caption.")]));
        let pages = render_with_images(blocks, vec![note(110, "Text note.")], &images).unwrap();
        let (page, rect) = image_rects(&pages)[0];
        let (reference, _) = find_line(&pages, "Noted text.");
        let (note_page, note) = find_line(&pages, "Text note.");

        assert_eq!(note_page, reference, "{count} paragraphs before");
        assert_eq!(find_line(&pages, "Noted caption.").0, page);
        if page == note_page {
            assert!(note.y > rect.y.0 + rect.height.0, "the note area is below the image");
        }
    }
}

#[test]
fn a_tall_image_shrinks_to_fit_below_its_heading() {
    let heading = Block::Heading {
        at: Location { line: 1, column: 1 },
        level: 2,
        content: vec![text_inline("Results")],
        label: None,
        class: None,
    };
    let blocks = vec![numbered(0), heading, figure(3, 0, vec![text_inline("Tall.")])];
    let pages = render_with_images(blocks, Vec::new(), &[svg(300.0, 3000.0)]).unwrap();

    let (heading_page, _) = find_line(&pages, "Results");
    assert_eq!(image_rects(&pages)[0].0, heading_page);
    assert_eq!(find_line(&pages, "Tall.").0, heading_page);
}

#[test]
fn reports_an_image_whose_caption_leaves_no_room() {
    let caption = vec![text_inline(&PROSE.repeat(30))];
    let errors = render_with_images(vec![figure(7, 0, caption)], Vec::new(), &[svg(10.0, 10.0)]).unwrap_err();

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location, Some((7, 1)));
    assert!(
        errors[0].message.starts_with("this image has no room"),
        "{}",
        errors[0].message
    );
}

/// A pipe table cell: one paragraph.
fn cell(at: Location, content: Vec<Inline>) -> crate::document::Cell {
    crate::document::Cell {
        at,
        blocks: vec![Block::Paragraph {
            at,
            content,
            class: None,
        }],
        span: 1,
        class: None,
    }
}

/// A table at source line `line` with a header row and body rows of plain text cells. Row `i` is at
/// line `line + 2 + i`.
fn table(line: u64, header: &[&str], rows: &[Vec<String>], caption: &str) -> Block {
    let row = |line: u64, cells: Vec<String>| crate::document::Row {
        at: Location { line, column: 1 },
        cells: cells
            .into_iter()
            .map(|text| cell(Location { line, column: 3 }, vec![text_inline(&text)]))
            .collect(),
        class: None,
    };
    let column = crate::document::Column {
        align: None,
        width: crate::document::ColumnWidth::Auto,
    };
    Block::Table {
        at: Location { line, column: 1 },
        columns: vec![column; header.len()],
        header: row(line, header.iter().map(|text| text.to_string()).collect()),
        rows: rows
            .iter()
            .enumerate()
            .map(|(index, cells)| row(line + 2 + index as u64, cells.clone()))
            .collect(),
        caption: if caption.is_empty() {
            Vec::new()
        } else {
            vec![text_inline(caption)]
        },
        label: None,
    }
}

/// `count` rows whose first cell is `R{index}` and whose second cell wraps to a few lines.
fn long_rows(count: usize) -> Vec<Vec<String>> {
    (0..count)
        .map(|index| {
            let words = &PROSE[..40 + (index * 37) % 160];
            vec![format!("R{index}"), words.to_owned()]
        })
        .collect()
}

/// The page index of every placed line containing `text`.
fn pages_with(pages: &[Page], text: &str) -> Vec<usize> {
    pages
        .iter()
        .enumerate()
        .flat_map(|(index, page)| {
            placed(page)
                .into_iter()
                .filter(|line| line.text.contains(text))
                .map(move |_| index)
        })
        .collect()
}

#[test]
fn natural_widths_fit_and_wide_columns_share_the_rest() {
    let config = config();
    let padding = config.tables.cell_padding.0;
    let short = vec![vec!["a".to_owned(), "b".to_owned()]];
    let pages = render(vec![table(1, &["Key", "Value"], &short, "")]).unwrap();
    let rules: Vec<Rect> = pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    let width = config.page.text_width().0;
    assert!(rules[0].width.0 < width / 4.0, "a short table keeps its natural width");
    assert!(close(
        rules[0].x.0 - config.page.margin_inner.0,
        (width - rules[0].width.0) / 2.0
    ));

    let rows = vec![vec!["Short".to_owned(), PROSE.to_owned(), PROSE.to_owned()]];
    let pages = render(vec![table(1, &["K", "A", "B"], &rows, "")]).unwrap();
    let lines = placed(&pages[0]);
    let starts: Vec<f64> = pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text { x, run, .. } if run.text == "Short" || run.text == "The" => Some(x.0),
            _ => None,
        })
        .collect();
    let left = config.page.margin_inner.0 + padding;
    assert!(close(starts[0], left), "the narrow column starts the table");
    let (a, b) = (starts[1] - starts[0], starts[2] - starts[1]);
    assert!(
        a < b && b > width / 3.0,
        "the wide columns share the rest equally: {a} {b}"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.right <= config.page.margin_inner.0 + width + 0.01)
    );
}

#[test]
fn reports_a_word_too_wide_for_the_narrowest_columns_at_its_cell() {
    let rows = vec![vec!["x".repeat(60), "y".repeat(60), "z".repeat(60)]];
    let errors = render(vec![table(4, &["A", "B", "C"], &rows, "")]).unwrap_err();

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location, Some((6, 3)));
    assert!(
        errors[0].message.contains("with every column at its narrowest"),
        "{}",
        errors[0].message
    );
}

#[test]
fn a_row_is_as_tall_as_its_tallest_cell_with_padding_and_rule() {
    let config = config();
    let style = &config.styles.table_cell;
    let line = style.size.0 * style.line_height;
    let rows = vec![
        vec!["One".to_owned(), PROSE.to_owned()],
        vec!["Two".to_owned(), "Short.".to_owned()],
    ];
    let pages = render(vec![table(1, &["A", "B"], &rows, "")]).unwrap();
    let rules: Vec<f64> = pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Rect { rect, .. } => Some(rect.y.0),
            _ => None,
        })
        .collect();
    let wrapped = placed(&pages[0])
        .iter()
        .filter(|placed| placed.y > rules[1] && placed.y < rules[2])
        .count();
    let tables = &config.tables;
    let rule = tables.row_rule.unwrap().thickness.0;

    assert_eq!(rules.len(), 4, "above and below the header, below each row");
    assert!(wrapped > 1);
    let expected = wrapped as f64 * line + 2.0 * tables.cell_padding.0 + rule;
    assert!(
        close(rules[2] - rules[1], expected),
        "{} against {expected}",
        rules[2] - rules[1]
    );
    assert!(close(rules[3] - rules[2], line + 2.0 * tables.cell_padding.0 + rule));
}

#[test]
fn a_long_table_breaks_between_rows_and_repeats_its_header() {
    let rows = long_rows(80);
    let blocks = vec![numbered(0), table(10, &["Head", "Text"], &rows, "Long."), numbered(1)];
    let pages = render(blocks).unwrap();
    let headers = pages_with(&pages, "Head Text");

    let mut row_pages: Vec<usize> = pages
        .iter()
        .enumerate()
        .filter(|(_, page)| placed(page).iter().any(|line| line.text.starts_with('R')))
        .map(|(index, _)| index)
        .collect();
    row_pages.dedup();

    assert!(row_pages.len() > 2);
    assert_eq!(headers, row_pages, "the header is on every page with rows");
    for index in 0..80 {
        let marker = format!("R{index} ");
        let found: Vec<usize> = pages
            .iter()
            .enumerate()
            .flat_map(|(page, content)| {
                placed(content)
                    .into_iter()
                    .filter(|line| line.text.starts_with(&marker))
                    .map(move |line| (page, line))
            })
            .map(|(page, _)| page)
            .collect();
        assert_eq!(found.len(), 1, "row {index} starts once");
    }
    for (page, content) in pages.iter().enumerate().skip(1) {
        if headers.contains(&page) {
            let lines = placed(content);
            assert_eq!(lines[0].text, "Head Text", "page {page} starts with the header");
            assert!(
                lines[1].text.starts_with('R'),
                "a row follows the header on page {page}"
            );
        }
    }
    assert_eq!(pages_with(&pages, "Table 1: Long.").len(), 1);
    assert_eq!(pages_with(&pages, "Table 1: Long.")[0], headers[0]);
}

#[test]
fn a_table_in_columns_takes_the_column_width_and_repeats_its_header_in_the_next_column() {
    let width = super::pages::column_width(&config().page);
    let rows = long_rows(30);
    let pages = render(vec![columns(1, vec![table(2, &["Head", "Text"], &rows, "")])]).unwrap();
    let page = &pages[0];
    let lines = placed(page);
    let headers: Vec<&Placed> = lines.iter().filter(|line| line.text == "Head Text").collect();

    assert_eq!(headers.len(), 2, "one header per column");
    assert!(!headers[0].in_second_column(page) && headers[1].in_second_column(page));
    assert_eq!(headers[0].y, headers[1].y, "both columns start with the header");
    let rules = page.items.iter().filter_map(|item| match item {
        Item::Rect { rect, .. } => Some(rect.width.0),
        _ => None,
    });
    assert!(rules.into_iter().all(|rule| rule <= width + 0.01));
}

#[test]
fn a_full_width_table_sits_between_balanced_columns_that_resume_below() {
    let config = config();
    let rows = vec![vec!["Wide".to_owned(), PROSE.to_owned()]];
    let full = Block::FullWidth {
        at: Location { line: 50, column: 1 },
        blocks: vec![table(51, &["Head", "Text"], &rows, "Across.")],
    };
    let section = columns(1, vec![numbered(0), numbered(1), full, numbered(2), numbered(3)]);
    let pages = render(vec![section]).unwrap();
    let page = &pages[0];
    let lines = placed(page);
    let caption = lines.iter().position(|line| line.text == "Table 1: Across.").unwrap();
    let row = lines.iter().position(|line| line.text.starts_with("Wide")).unwrap();
    let (first, second) = column_bottoms(page, &lines[..caption]);

    assert_eq!(pages.len(), 1);
    assert_eq!(reading_order(&pages), [0, 1, 2, 3]);
    assert!((first - second).abs() <= body_line() + 1e-6);
    assert!(lines[caption].y > first.max(second));
    assert!(lines[row].right - lines[row].left > config.page.text_width().0 / 2.0);
    let after = &lines[row + 1..];
    assert!(after.iter().any(|line| line.in_second_column(page)));
    assert!(
        after
            .iter()
            .filter(|line| line.text.contains('P'))
            .all(|line| line.y > lines[row].y)
    );
}

#[test]
fn a_note_referenced_in_a_cell_goes_on_the_page_of_its_row() {
    for count in [40, 45, 50] {
        let mut rows = long_rows(count);
        let noted = count - 5;
        let row = |index: usize, cells: Vec<String>| {
            let at = Location {
                line: 20 + index as u64,
                column: 1,
            };
            let mut content: Vec<Vec<Inline>> = cells.into_iter().map(|text| vec![text_inline(&text)]).collect();
            if index == noted {
                content[1].push(Inline::FootnoteRef(0));
            }
            crate::document::Row {
                at,
                cells: content.into_iter().map(|content| cell(at, content)).collect(),
                class: None,
            }
        };
        let Block::Table {
            at, columns, header, ..
        } = table(18, &["Head", "Text"], &[], "")
        else {
            unreachable!()
        };
        let rows = rows
            .drain(..)
            .enumerate()
            .map(|(index, cells)| row(index, cells))
            .collect();
        let block = Block::Table {
            at,
            columns,
            header,
            rows,
            caption: Vec::new(),
            label: None,
        };
        let pages = render_with_notes(vec![block], vec![note(90, "Cell note.")]).unwrap();
        let reference = pages_with(&pages, &format!("R{noted} "));
        assert_eq!(pages_with(&pages, "Cell note."), reference, "{count} rows");
    }
}

#[test]
fn reports_a_row_taller_than_the_page_with_its_header() {
    let tall = vec![vec!["Tall".to_owned(), PROSE.repeat(40)]];
    let mut rows = long_rows(2);
    rows.extend(tall);
    let errors = render(vec![table(3, &["A", "B"], &rows, "")]).unwrap_err();

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].location, Some((7, 1)));
    assert!(
        errors[0]
            .message
            .starts_with("this table row with the repeated header is"),
        "{}",
        errors[0].message
    );
}

#[test]
fn tables_are_numbered_apart_from_figures() {
    let images = [svg(50.0, 20.0)];
    let rows = vec![vec!["a".to_owned()]];
    let blocks = vec![
        table(1, &["H"], &rows, "First."),
        figure(5, 0, vec![text_inline("Image.")]),
        table(7, &["H"], &rows, ""),
        table(11, &["H"], &rows, "Second."),
    ];
    let pages = render_with_images(blocks, Vec::new(), &images).unwrap();
    let captions: Vec<String> = placed(&pages[0])
        .into_iter()
        .map(|line| line.text)
        .filter(|text| text.contains(": "))
        .collect();

    assert_eq!(captions, ["Table 1: First.", "Figure 1: Image.", "Table 2: Second."]);
}

#[test]
fn a_table_leaves_no_single_row_at_a_page_end_or_top() {
    let mut split = 0;
    for count in 4..24 {
        let rows: Vec<Vec<String>> = (0..count)
            .map(|index| vec![format!("R{index}"), PROSE.to_owned()])
            .collect();
        let blocks = vec![numbered(0), table(10, &["Head", "Text"], &rows, "")];
        let pages = render(blocks).unwrap();
        let rows: Vec<usize> = (pages.iter())
            .map(|page| placed(page).iter().filter(|line| line.text.starts_with('R')).count())
            .filter(|&rows| rows > 0)
            .collect();
        split += usize::from(rows.len() > 1);
        assert!(rows.iter().all(|&rows| rows > 1), "{count} rows: {rows:?}");
    }
    assert!(split > 5, "{split} tables split");
}

#[test]
fn repeated_table_layout_is_identical() {
    let blocks = || {
        vec![
            numbered(0),
            columns(2, vec![table(3, &["Head", "Text"], &long_rows(40), "Cols.")]),
        ]
    };
    let first = format!("{:?}", render(blocks()).unwrap());
    assert_eq!(first, format!("{:?}", render(blocks()).unwrap()));
}

/// The body lines of a Markdown document with their break rules, as layout hands them to the composer.
fn flow(markdown: &str) -> Vec<FlowLine> {
    let config = config();
    let fonts = Fonts::load(&config, &BTreeMap::new()).expect("bundled fonts");
    let document = crate::markdown::parse(markdown, 1, &source()).expect("document parses");
    let cited = Cited::default();
    let structure = Structure::new(&document, &config, cited.references());
    let pass = Pass {
        document: &document,
        cited: &cited,
        images: &[],
        theme_images: &HashMap::new(),
        config: &config,
        fonts: &fonts,
        source: &source(),
        structure: &structure,
        first: 0,
    };
    let assumed = vec![1; structure.anchors.len()];
    pass.content(&assumed).expect("content is set").body
}

/// The rules after the lines of the block that starts on source line `line`.
fn breaks_of(lines: &[FlowLine], line: u64) -> Vec<Break> {
    lines.iter().filter(|l| l.at.line == line).map(|l| l.after).collect()
}

#[test]
fn a_heading_keeps_two_lines_of_its_paragraph_and_pages_strand_no_line() {
    let lines = flow(&format!("# Title\n\n{PROSE} {PROSE}\n\n{PROSE} {PROSE}\n"));
    let (kept, plain) = (breaks_of(&lines, 3), breaks_of(&lines, 5));

    assert!(kept.len() > 3 && kept.len() == plain.len(), "{kept:?}");
    assert_eq!(kept[0], Break::Never);
    assert!(matches!(plain[0], Break::Avoid(_)), "{plain:?}");
    assert!(matches!(plain[plain.len() - 2], Break::Avoid(_)), "{plain:?}");
    assert_eq!(kept[1..], plain[1..]);
}

#[test]
fn a_page_end_after_a_hyphenated_line_costs_more() {
    let lines = flow(&"incomprehensibilities ".repeat(60));
    let cost = |after: Break| match after {
        Break::Allowed(cost) | Break::Avoid(cost) => cost,
        Break::Never | Break::Forced => unreachable!("a plain paragraph"),
    };
    let ends_in_hyphen = |line: &FlowLine| {
        line.line.items.iter().rev().find_map(|item| match item {
            Item::Text { run, .. } => Some(run.text.ends_with('-')),
            _ => None,
        })
    };

    assert!(lines.iter().any(|line| line.line.hyphenated));
    for line in &lines[..lines.len() - 1] {
        assert_eq!(ends_in_hyphen(line), Some(line.line.hyphenated));
        assert_eq!(cost(line.after) >= pages::HYPHENATED, line.line.hyphenated);
    }
}

#[test]
fn a_page_end_after_a_colon_before_a_list_or_code_costs_a_little() {
    let lines = flow("Steps:\n\n- one\n\nSteps.\n\n- two\n\nCode:\n\n```\nx\n```\n\nText:\n\nMore.\n");

    assert_eq!(breaks_of(&lines, 1), [Break::Allowed(pages::INTRODUCTION)]);
    assert_eq!(breaks_of(&lines, 5), [Break::Allowed(0.0)]);
    assert_eq!(breaks_of(&lines, 9), [Break::Allowed(pages::INTRODUCTION)]);
    assert_eq!(breaks_of(&lines, 15), [Break::Allowed(0.0)]);
}
