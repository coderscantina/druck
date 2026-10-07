//! The table of contents: the `contents` label in the `toc-heading` style, then one entry per heading
//! up to `document.toc-depth`, in the `toc-entry` style.
//!
//! An entry is indented by `toc.level-indent` per level below the first and shows the heading's
//! number and text. Lines after its first are indented by one more level. The page number is right
//! aligned at the edge of the text area, in a column as wide as three digits or the number if wider,
//! and with `toc.leader` a row of dots leads to it. The dots sit on a grid shared by all entries, so
//! they line up. The whole entry links to its heading.

use super::structure::Heading;
use super::titles::FRONT_MATTER;
use super::{Flow, Frame, Role, pages, text_item, translate_line};
use crate::config::values::Pt;
use crate::document::{Inline, InlineStyle, Link};
use crate::page::{Item, Rect};
use crate::text::{Glyph, ShapedRun};

/// Space between an entry's text, its dots, and its page number, in ems of the entry style.
const GAP: f64 = 0.5;
/// Distance between two leader dots, in ems of the entry style.
const DOT_STEP: f64 = 0.5;

impl<'a> Flow<'a> {
    /// Sets the table of contents with the page numbers this pass assumes.
    pub(super) fn contents(&mut self, frame: Frame<'a>) {
        let config = self.config;
        let label = Inline::Text {
            text: config.labels.contents.clone(),
            style: InlineStyle::default(),
        };
        self.text_block(
            FRONT_MATTER,
            &[label],
            &config.styles.toc_heading,
            frame,
            0.0,
            Role::Heading(None),
        );
        let depth = config.document.toc_depth.get();
        let structure = self.structure;
        for heading in structure.headings.iter().filter(|heading| heading.level <= depth) {
            self.entry(heading, frame);
        }
        self.after_paragraph = false;
    }

    fn entry(&mut self, heading: &Heading, frame: Frame<'a>) {
        let (config, fonts) = (self.config, self.fonts);
        let style = &config.styles.toc_entry;
        let lang = config.document.lang;
        let face = fonts.face(&style.font, style.weight, style.style);
        let em = style.size.0;
        let level = f64::from(heading.level - 1);
        let indent = style.indent.0 + config.toc.level_indent.0 * level;
        let hang = config.toc.level_indent.0;
        let width = self.width(frame);

        let number = fonts.shape(&self.assumed[heading.anchor].to_string(), face, style.size, lang);
        let column = number.width.0.max(fonts.shape("000", face, style.size, lang).width.0);
        let measure = width - indent - hang - column - 2.0 * GAP * em;
        let content = [Inline::Text {
            text: heading.title(),
            style: InlineStyle::default(),
        }];
        self.space(style.space_before.0);
        let lines = match self.set(&content, style, measure, -hang) {
            Ok(lines) => lines,
            Err(problem) => {
                self.errors.push(self.error(heading.at, problem));
                return;
            }
        };
        let count = lines.len();
        for (index, line) in lines.into_iter().enumerate() {
            let mut line = translate_line(line, indent + hang);
            let link = Item::Link {
                rect: Rect {
                    x: Pt(indent),
                    y: Pt(0.0),
                    width: Pt(width - indent),
                    height: Pt(line.height),
                },
                link: Link::Anchor(heading.anchor),
            };
            if index + 1 == count {
                let end = line
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Item::Text { x, run, .. } => Some(x.0 + run.width.0),
                        _ => None,
                    })
                    .fold(indent + hang, f64::max);
                if config.toc.leader {
                    let dot = fonts.shape(".", face, style.size, lang);
                    let (from, to) = (end + GAP * em, width - column - GAP * em);
                    if let Some((x, run)) = leader(&dot, DOT_STEP * em, from, to) {
                        line.items.push(text_item(x, line.baseline, run, style.color));
                    }
                }
                let x = width - number.width.0;
                line.items
                    .push(text_item(x, line.baseline, number.clone(), style.color));
            }
            line.items.push(link);
            let line = translate_line(line, frame.left);
            self.push(line, heading.at, pages::line_break(index, count));
        }
        self.space(style.space_after.0);
    }
}

/// A row of dots at multiples of `step` from the frame's left edge, within `from..to`, as one run
/// starting at the first dot. `None` if no dot fits.
fn leader(dot: &ShapedRun, step: f64, from: f64, to: f64) -> Option<(f64, ShapedRun)> {
    let glyph = dot.glyphs.first()?;
    let first = (from / step).ceil();
    let last = ((to - glyph.x_advance.0) / step).floor();
    if last < first {
        return None;
    }
    let count = (last - first) as usize + 1;
    let glyphs = (0..count)
        .map(|index| Glyph {
            x_advance: Pt(if index + 1 == count { glyph.x_advance.0 } else { step }),
            text: index..index + 1,
            ..glyph.clone()
        })
        .collect();
    let run = ShapedRun {
        text: ".".repeat(count),
        face: dot.face,
        size: dot.size,
        glyphs,
        width: Pt(step * (count - 1) as f64 + glyph.x_advance.0),
    };
    Some((first * step, run))
}
