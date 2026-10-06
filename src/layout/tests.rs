use super::*;
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, resolve};
use crate::document::InlineStyle;

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
    let config = config();
    let fonts = Fonts::load(&config).expect("bundled fonts");
    layout(&Document { blocks }, &config, &fonts, &source())
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
fn justified_lines_fill_the_text_width_except_the_last() {
    let config = config();
    let pages = render(vec![paragraph(1, PROSE)]).unwrap();
    let lines = lines(&pages[0]);
    let right = config.page.margin_inner.0 + config.page.text_width().0;

    let (last, full) = lines.split_last().unwrap();
    assert!(full.len() >= 2);
    for (_, end, text) in full {
        assert!((end - right).abs() < 0.01, "{text}: ends at {end}, not {right}");
    }
    assert!(last.1 < right - 1.0);
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
