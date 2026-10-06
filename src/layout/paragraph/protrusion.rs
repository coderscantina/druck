//! Optical margin alignment: punctuation at the edges of justified lines hangs partly into the
//! margins, so the text edge looks straight. Letters do not protrude.

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

/// How far the first glyph at or after byte `from` of `run` hangs into the left margin.
pub fn leading(run: &ShapedRun, from: usize) -> f64 {
    run.glyphs
        .iter()
        .find(|glyph| glyph.text.start >= from && !glyph.text.is_empty())
        .and_then(|glyph| Some(factor(run.text[glyph.text.start..].chars().next()?) * glyph.x_advance.0))
        .unwrap_or(0.0)
}

/// How far the last glyph before byte `to` of `run` hangs into the right margin.
pub fn trailing(run: &ShapedRun, to: usize) -> f64 {
    run.glyphs
        .iter()
        .rev()
        .find(|glyph| glyph.text.start < to && !glyph.text.is_empty())
        .and_then(|glyph| {
            let text = &run.text[glyph.text.start..glyph.text.end.min(to)];
            Some(factor(text.chars().next_back()?) * glyph.x_advance.0)
        })
        .unwrap_or(0.0)
}
