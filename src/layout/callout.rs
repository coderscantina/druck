//! Callouts: a labelled box around blocks, set like a quotation and drawn behind its lines.
//!
//! Layout gives every line of a callout the box it belongs to. The box is drawn when the page is placed,
//! because only then are the gaps between lines known. A callout that breaks across pages continues its
//! background and bar on the next page.

use super::{Flow, Frame, Line, Role, pages, translate};
use crate::config::resolved::Rule;
use crate::config::theme::CalloutKind;
use crate::config::values::{Color, Pt};
use crate::document::{Block, Inline, InlineStyle, Location};
use crate::output::{Item, Rect};
use crate::source_map::Spans;
use crate::tags::Kind;

/// The box a line of a callout is part of. `x` and `width` are in line coordinates.
#[derive(Debug, Clone, Copy)]
pub(super) struct CalloutBox {
    x: f64,
    width: f64,
    padding: f64,
    background: Option<Color>,
    rule: Option<Rule>,
    /// Whether the line is the first or last of its callout, which then has no continuation there.
    first: bool,
    pub(super) last: bool,
}

impl CalloutBox {
    /// The items of the box around lines placed from `top` to `bottom`, whose first line is at `left`.
    /// Where a callout goes on beyond the page, the box reaches `padding` into the margin.
    pub(super) fn items(first: &Self, last: &Self, left: f64, top: f64, bottom: f64) -> Vec<Item> {
        let top = if first.first { top } else { top - first.padding };
        let bottom = if last.last { bottom } else { bottom + last.padding };
        let x = left + first.x;
        let rect = |width: f64| Rect {
            x: Pt(x),
            y: Pt(top),
            width: Pt(width),
            height: Pt(bottom - top),
        };
        let mut items = Vec::with_capacity(2);
        if let Some(color) = first.background {
            items.push(Item::Rect {
                rect: rect(first.width),
                color,
            });
        }
        if let Some(rule) = first.rule.filter(|rule| rule.thickness.0 > 0.0) {
            items.push(Item::Rect {
                rect: rect(rule.thickness.0),
                color: rule.color,
            });
        }
        items
    }
}

impl<'a> Flow<'a> {
    /// Sets a callout of `kind`: its label, then `blocks`, inside the padding and the bar. A callout with
    /// fewer than four lines stays on one page, and a longer one leaves at least two lines on each.
    pub(super) fn callout(&mut self, at: Location, kind: CalloutKind, blocks: &[Block], frame: Frame<'a>) {
        let config = self.config;
        let style = &config.styles.callout;
        let look = config.callouts.get(kind);
        let padding = config.callouts.padding.0;
        let bar = look.rule.map_or(0.0, |rule| rule.thickness.0);
        let x = frame.left + style.indent.0;
        let inner = Frame {
            left: x + bar + padding,
            right: frame.right + style.indent.0 + padding,
            style,
            widen: 0.0,
            ..frame
        };
        self.space(style.space_before.0);
        self.after_paragraph = false;
        let first = self.lines.len();
        let scope = self.open(Kind::Callout, false);
        let label = config.labels.callout(kind);
        if !label.is_empty() {
            let label_scope = self.open(Kind::Paragraph, true);
            let content = [Inline::Text {
                text: label.to_owned(),
                style: InlineStyle::default(),
                source: Spans::default(),
            }];
            // Set as a heading, which never ends a page, so the label stays with the text below it.
            self.text_block(
                at,
                &content,
                &config.styles.callout_label,
                inner,
                0.0,
                Role::Heading(None),
            );
            self.close(label_scope);
        }
        let text = self.lines.len();
        self.blocks(blocks, inner);
        self.close(scope);
        if self.lines.len() == first {
            return;
        }
        if let Some(line) = self.lines.get_mut(text).filter(|_| text > first) {
            line.space_before = config.styles.callout_label.space_after.0;
        }
        pages::guard_ends(&mut self.lines[text..]);
        let template = CalloutBox {
            x,
            width: self.width(frame) - 2.0 * style.indent.0,
            padding,
            background: look.background,
            rule: look.rule,
            first: false,
            last: false,
        };
        let last = self.lines.len() - 1;
        for (index, line) in self.lines[first..].iter_mut().enumerate() {
            line.callout = Some(CalloutBox {
                first: index == 0,
                last: first + index == last,
                ..template
            });
        }
        pad(&mut self.lines[first].line, padding, 0.0);
        pad(&mut self.lines[last].line, 0.0, padding);
        self.space(style.space_after.0);
    }
}

/// Adds room above and below the content of `line`.
fn pad(line: &mut Line, top: f64, bottom: f64) {
    line.height += top + bottom;
    line.baseline += top;
    if top > 0.0 {
        line.items = std::mem::take(&mut line.items)
            .into_iter()
            .map(|item| translate(item, 0.0, top))
            .collect();
    }
}
