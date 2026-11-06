//! What the text gives assistive technology: its lines on screen, one text run each, with the caret or selection.

use std::rc::Rc;

use gpui::px;

use super::layout::BlockLayout;
use crate::source::Selection;

/// A shown line of the galley, with where its block starts.
pub(super) type ShownBlock = (usize, Rc<BlockLayout>);

/// The galley's lines on screen for assistive technology: one text run per line, with the caret or selection where a run
/// holds it. What is off screen is not in the tree, as the toolkit's own lists leave it out.
pub(super) fn a11y_text(shown: &[ShownBlock], selection: Selection, builder: &mut gpui::A11ySubtreeBuilder) {
    let mut anchor = None;
    let mut focus = None;
    let mut key = 0usize;
    for (start, layout) in shown {
        for line in &layout.lines {
            if line.height == px(0.) || line.skipped {
                continue;
            }
            let id = builder.synthetic_node_id(("run", key));
            key += 1;
            let value = format!("{}\n", line.display.text);
            let mut run = gpui::accesskit::Node::new(gpui::Role::TextRun);
            let lengths: Vec<u8> = value.chars().map(|character| character.len_utf8() as u8).collect();
            run.set_character_lengths(lengths);
            run.set_value(value);
            let source = start + line.display.source.start..=start + line.display.source.end;
            let position = |offset: usize| gpui::accesskit::TextPosition {
                node: id,
                character_index: line.display.text[..line.display.to_display(offset - start)]
                    .chars()
                    .count(),
            };
            if source.contains(&selection.anchor) {
                anchor.get_or_insert(position(selection.anchor));
            }
            if source.contains(&selection.head) {
                focus.get_or_insert(position(selection.head));
            }
            builder.push_child(id, run);
        }
    }
    if let (Some(anchor), Some(focus)) = (anchor.or(focus), focus.or(anchor)) {
        builder
            .parent_node()
            .set_text_selection(gpui::accesskit::TextSelection { anchor, focus });
    }
}
