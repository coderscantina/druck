//! The bibliography: the `references` label as an unnumbered level 1 heading, then one entry per cited
//! work in the `bibliography` style.
//!
//! An author-date entry is set like a paragraph with a hanging indent of `bibliography.hanging-indent`: its
//! first line starts at the left edge and later lines are indented. A numeric entry has its label in a
//! column as wide as the widest label plus half an em, with the text after it on every line, as in LaTeX's
//! `thebibliography`. Entries are at least `bibliography.entry-spacing` apart and break across pages and columns
//! like paragraphs. The first line of each entry is where citations of it link to.

use super::{Flow, Frame, Role, anchor_item, heading_style, pages, text_item, translate_line};
use crate::diagnostic::Diagnostic;
use crate::document::{Inline, InlineStyle, Location};

impl<'a> Flow<'a> {
    /// Sets the bibliography section for the block at `at`. A document without citations has none.
    pub(super) fn bibliography(&mut self, at: Location, frame: Frame<'a>) {
        let (config, structure, cited) = (self.config, self.structure, self.cited);
        let Some(section) = &cited.section else {
            return;
        };
        let heading = &structure.headings[self.headings];
        self.headings += 1;
        let title = [Inline::Text {
            text: heading.text.clone(),
            style: InlineStyle::default(),
        }];
        let role = Role::Heading(Some(heading.anchor));
        self.text_block(at, &title, heading_style(config, 1), frame, 0.0, role);

        let style = &config.styles.bibliography;
        let face = self.fonts.face(&style.font, style.weight, style.style);
        let lang = config.document.lang;
        let labels: Vec<_> = section
            .references
            .iter()
            .filter_map(|reference| reference.label.as_deref())
            .map(|label| self.fonts.shape(label, face, style.size, lang))
            .collect();
        // Text starts after the label column of a numeric bibliography, which has no hanging indent.
        let column = (labels.iter().map(|run| run.width.0).reduce(f64::max)).map(|widest| widest + 0.5 * style.size.0);
        let hang = column.unwrap_or(config.bibliography.hanging_indent.0);
        let first_indent = if column.is_some() { 0.0 } else { -hang };
        let left = frame.left + style.indent.0;
        let width = self.width(frame) - 2.0 * style.indent.0 - hang;
        let mut labels = labels.into_iter();
        for (index, reference) in section.references.iter().enumerate() {
            if index > 0 {
                self.space(config.bibliography.entry_spacing.0);
            }
            let label = labels.next();
            self.space(style.space_before.0);
            match self.set(&reference.content, style, width, first_indent) {
                Ok(lines) => {
                    let count = lines.len();
                    let mut label = label;
                    for (line_index, line) in lines.into_iter().enumerate() {
                        let mut line = translate_line(line, left + hang);
                        if line_index == 0 {
                            line.items.push(anchor_item(structure.entries[index], left));
                            if let Some(run) = label.take() {
                                line.items.insert(0, text_item(left, line.baseline, run, style.color));
                            }
                        }
                        self.push(line, at, pages::line_break(line_index, count));
                    }
                }
                Err(problem) => {
                    let (line, column) = reference.at;
                    let message = format!("entry `{}`: {problem}", reference.key);
                    let diagnostic = Diagnostic::new(Some(section.source.clone()), message).at(line, column);
                    self.errors.push(diagnostic);
                }
            }
            self.space(style.space_after.0);
        }
        self.after_paragraph = false;
    }
}
