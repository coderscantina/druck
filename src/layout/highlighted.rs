//! Colored code lines. A line is shaped once, as a whole, and then cut into runs of one color, so
//! glyphs, advances, and the line width are the same as without highlighting.

use std::collections::BTreeMap;

use crate::config::values::{Color, Pt};
use crate::highlight::{Category, Span};
use crate::text::ShapedRun;

/// Cuts `run` where the color changes. Each piece has its offset from the start of the run. A glyph
/// takes the color of the span that holds the start of its text. Characters outside any span, and
/// categories the palette lacks, get `base`.
pub fn split(
    run: ShapedRun,
    spans: &[Span],
    palette: &BTreeMap<Category, Color>,
    base: Color,
) -> Vec<(Pt, ShapedRun, Color)> {
    let color_at = |offset: usize| {
        let at = spans.partition_point(|(range, _)| range.end <= offset);
        match spans.get(at) {
            Some((range, category)) if range.start <= offset => palette.get(category).copied().unwrap_or(base),
            _ => base,
        }
    };

    let mut pieces: Vec<(Pt, ShapedRun, Color)> = Vec::new();
    let mut advanced = 0.0;
    let mut first = 0;
    while first < run.glyphs.len() {
        let color = color_at(run.glyphs[first].text.start);
        let len = run.glyphs[first..]
            .iter()
            .take_while(|glyph| color_at(glyph.text.start) == color)
            .count();
        let glyphs = &run.glyphs[first..first + len];
        if len == run.glyphs.len() {
            return vec![(Pt(0.0), run, color)];
        }
        let start = glyphs.iter().map(|glyph| glyph.text.start).min().unwrap_or(0);
        let end = glyphs.iter().map(|glyph| glyph.text.end).max().unwrap_or(start);
        let width: f64 = glyphs.iter().map(|glyph| glyph.x_advance.0).sum();
        let piece = ShapedRun {
            text: run.text[start..end].to_owned(),
            face: run.face,
            size: run.size,
            glyphs: glyphs
                .iter()
                .cloned()
                .map(|mut glyph| {
                    glyph.text = glyph.text.start - start..glyph.text.end - start;
                    glyph
                })
                .collect(),
            width: Pt(width),
            spacing: run.spacing,
        };
        pieces.push((Pt(advanced), piece, color));
        advanced += width;
        first += len;
    }
    pieces
}
