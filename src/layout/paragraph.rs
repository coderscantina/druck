//! Paragraph composition: shapes inline content and breaks it into lines.
//!
//! Lines are filled greedily, one word at a time. Milestone 03 replaces this with
//! paragraph-wide break optimization and hyphenation.

use std::collections::HashMap;

use super::{Line, inline_face, text_item};
use crate::config::resolved::{InlineStyles, Style};
use crate::config::theme::{Align, FontStyle, Lang, Weight};
use crate::config::values::{Color, Pt};
use crate::document::Inline;
use crate::page::{Item, Rect};
use crate::text::{FaceId, Fonts, Metrics, ShapedRun};

/// A piece of a word in one face and size.
struct Fragment {
    run: ShapedRun,
    color: Color,
    link: Option<String>,
    metrics: Metrics,
}

/// Text between two spaces. Style changes inside a word split it into fragments.
#[derive(Default)]
struct Word {
    fragments: Vec<Fragment>,
    width: f64,
}

enum Token {
    Word(Word),
    /// An interword space with its natural width.
    Space(f64),
    LineBreak,
}

/// The words of one line, each with the natural space before it.
#[derive(Default)]
struct Filled {
    words: Vec<(f64, Word)>,
    natural: f64,
    /// Ends the paragraph or a hard break, so it is not justified.
    last: bool,
}

/// Breaks inline content into positioned lines of `width`. The first line is indented by `first_indent`.
/// Fails for a word wider than its line or a character the font cannot show.
pub fn lines(
    content: &[Inline],
    style: &Style,
    inline: &InlineStyles,
    fonts: &Fonts,
    lang: Lang,
    width: f64,
    first_indent: f64,
) -> Result<Vec<Line>, String> {
    let tokens = tokens(content, style, inline, fonts, lang)?;
    let available = |line: usize| if line == 0 { width - first_indent } else { width };

    let mut filled: Vec<Filled> = Vec::new();
    let mut current = Filled::default();
    let mut space = 0.0;
    for token in tokens {
        match token {
            Token::Space(width) => space = width,
            Token::LineBreak => {
                current.last = true;
                filled.push(std::mem::take(&mut current));
            }
            Token::Word(word) => {
                let available = available(filled.len());
                if word.width > available {
                    let text: String = word.fragments.iter().map(|f| f.run.text.as_str()).collect();
                    return Err(format!(
                        "\"{text}\" is {:.1}pt wider than the line and cannot be broken",
                        word.width - available
                    ));
                }
                let mut before = if current.words.is_empty() { 0.0 } else { space };
                if current.natural + before + word.width > available {
                    filled.push(std::mem::take(&mut current));
                    before = 0.0;
                }
                current.natural += before + word.width;
                current.words.push((before, word));
            }
        }
    }
    if !current.words.is_empty() || filled.is_empty() {
        current.last = true;
        filled.push(current);
    }

    let block_face = fonts.face(&style.font, style.weight, style.style);
    let height = style.size.0 * style.line_height;
    let baseline = baseline(height, &fonts.metrics(block_face, style.size));
    let underline = inline.link_underline;
    Ok(filled
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let indent = if index == 0 { first_indent } else { 0.0 };
            let slack = available(index) - line.natural;
            let spaces = line.words.len().saturating_sub(1);
            let (start, stretch) = match style.align {
                Align::Justify if !line.last && spaces > 0 => (0.0, slack / spaces as f64),
                Align::Justify | Align::Left => (0.0, 0.0),
                Align::Center => (slack / 2.0, 0.0),
                Align::Right => (slack, 0.0),
            };
            let mut items = Vec::new();
            let mut open_link: Option<usize> = None;
            let mut x = indent + start;
            for (position, (before, word)) in line.words.into_iter().enumerate() {
                if position > 0 {
                    x += before + stretch;
                }
                for fragment in word.fragments {
                    let width = fragment.run.width.0;
                    if let Some(url) = fragment.link {
                        let metrics = fragment.metrics;
                        let extends = open_link.and_then(|i| match &mut items[i] {
                            Item::Link { rect, url: open } if *open == url => Some(rect),
                            _ => None,
                        });
                        match extends {
                            Some(rect) => rect.width = Pt(x + width - rect.x.0),
                            None => {
                                open_link = Some(items.len());
                                items.push(Item::Link {
                                    rect: Rect {
                                        x: Pt(x),
                                        y: Pt(baseline - metrics.ascender.0),
                                        width: Pt(width),
                                        height: Pt(metrics.ascender.0 + metrics.descender.0),
                                    },
                                    url,
                                });
                            }
                        }
                        if underline {
                            let rect = Rect {
                                x: Pt(x),
                                y: Pt(baseline + metrics.underline_position.0),
                                width: Pt(width),
                                height: metrics.underline_thickness,
                            };
                            items.push(Item::Rect {
                                rect,
                                color: fragment.color,
                            });
                        }
                    } else {
                        open_link = None;
                    }
                    items.push(text_item(x, baseline, fragment.run, fragment.color));
                    x += width;
                }
            }
            Line {
                height,
                baseline,
                items,
            }
        })
        .collect())
}

/// The baseline offset from the top of a line of `height`, centering the font's ascender and descender.
pub fn baseline(height: f64, metrics: &Metrics) -> f64 {
    (height - metrics.ascender.0 - metrics.descender.0) / 2.0 + metrics.ascender.0
}

/// Describes the first character of `run` that the font has no glyph for.
pub fn missing_glyph(run: &ShapedRun) -> Option<String> {
    let glyph = run.glyphs.iter().find(|glyph| glyph.id == 0)?;
    let character = run.text[glyph.text.clone()].chars().next()?;
    Some(format!(
        "character '{character}' (U+{:04X}) has no glyph in the selected font",
        u32::from(character)
    ))
}

fn tokens(
    content: &[Inline],
    style: &Style,
    inline: &InlineStyles,
    fonts: &Fonts,
    lang: Lang,
) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut word = Word::default();
    let mut space_widths: HashMap<(FaceId, u64), f64> = HashMap::new();
    let finish = |word: &mut Word, tokens: &mut Vec<Token>| {
        if !word.fragments.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    for piece in content {
        let (text, text_style) = match piece {
            Inline::LineBreak => {
                finish(&mut word, &mut tokens);
                tokens.push(Token::LineBreak);
                continue;
            }
            Inline::Text { text, style } => (text, style),
        };
        let (face, size) = if text_style.code {
            let face = fonts.face(&inline.code_font, Weight::Regular, FontStyle::Normal);
            (face, inline.code_size.to_pt(style.size))
        } else {
            let (weight, font_style) = inline_face(style, text_style.emphasis, text_style.strong);
            (fonts.face(&style.font, weight, font_style), style.size)
        };
        let color = match (&text_style.link, text_style.code) {
            (Some(_), _) => inline.link_color,
            (None, true) => inline.code_color,
            (None, false) => style.color,
        };
        let metrics = fonts.metrics(face, size);
        for (index, part) in text.split(' ').enumerate() {
            if index > 0 {
                finish(&mut word, &mut tokens);
                let space = *space_widths
                    .entry((face, size.0.to_bits()))
                    .or_insert_with(|| fonts.shape(" ", face, size, lang).width.0);
                match tokens.last_mut() {
                    Some(Token::Space(width)) => *width = width.max(space),
                    _ => tokens.push(Token::Space(space)),
                }
            }
            if part.is_empty() {
                continue;
            }
            let run = fonts.shape(part, face, size, lang);
            if let Some(problem) = missing_glyph(&run) {
                return Err(problem);
            }
            word.width += run.width.0;
            word.fragments.push(Fragment {
                run,
                color,
                link: text_style.link.clone(),
                metrics,
            });
        }
    }
    finish(&mut word, &mut tokens);
    Ok(tokens)
}
