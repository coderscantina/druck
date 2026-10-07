//! Optical margin alignment: punctuation at the edges of justified lines hangs partly into the
//! margins, so the text edge looks straight. A run of punctuation, such as a comma after a closing
//! quote, hangs by the sum of its shares. Letters do not protrude.

use crate::text::ShapedRun;

/// The share of a glyph's advance that protrudes past the margin when its character starts or
/// ends a justified line. Quotes have one value for both sides, since „ “ ‚ ‘ open or close
/// depending on the language.
fn factor(c: char) -> f64 {
    match c {
        '.' | ',' | '-' | '\u{2010}' | '\'' | '‘' | '’' | '‚' | '‛' => 0.7,
        ':' | ';' | '"' | '“' | '”' | '„' | '‟' => 0.5,
        '–' | '«' | '»' | '‹' | '›' => 0.3,
        '!' | '?' | '—' => 0.2,
        _ => 0.0,
    }
}

/// How far the glyphs at or after byte `from` of `run` hang into the left margin: the sum over the
/// protruding characters that start there, such as „‚.
pub fn leading(run: &ShapedRun, from: usize) -> f64 {
    run.glyphs
        .iter()
        .filter(|glyph| glyph.text.start >= from && !glyph.text.is_empty())
        .map_while(|glyph| {
            let hang = factor(run.text[glyph.text.start..].chars().next()?) * glyph.x_advance.0;
            (hang > 0.0).then_some(hang)
        })
        .sum()
}

/// How far the glyphs before byte `to` of `run` hang into the right margin: the sum over the
/// protruding characters that end there, such as ”, after a word.
pub fn trailing(run: &ShapedRun, to: usize) -> f64 {
    run.glyphs
        .iter()
        .rev()
        .filter(|glyph| glyph.text.start < to && !glyph.text.is_empty())
        .map_while(|glyph| {
            let text = &run.text[glyph.text.start..glyph.text.end.min(to)];
            let hang = factor(text.chars().next_back()?) * glyph.x_advance.0;
            (hang > 0.0).then_some(hang)
        })
        .sum()
}
