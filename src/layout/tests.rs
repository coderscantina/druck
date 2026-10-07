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
    render_with_images(blocks, footnotes, &[])
}

fn render_with_images(
    blocks: Vec<Block>,
    footnotes: Vec<Footnote>,
    images: &[Image],
) -> Result<Vec<Page>, Vec<Diagnostic>> {
    let config = config();
    let fonts = Fonts::load(&config).expect("bundled fonts");
    let document = Document {
        blocks,
        footnotes,
        images: Vec::new(),
    };
    layout(&document, images, &config, &fonts, &source())
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

/// The text lines of a page in placement order. Raised markers join the line they are in.
fn placed(page: &Page) -> Vec<Placed> {
    let mut lines: Vec<Placed> = Vec::new();
    for item in &page.items {
        let Item::Text { x, y, run, .. } = item else { continue };
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
