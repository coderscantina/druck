//! The index: the `index` label as an unnumbered level 1 heading, then the entries under their letters, in
//! two columns unless the theme sets one or the index stands in a `columns` section, whose columns it takes.
//! The heading opens a chapter as the theme's `chapters` section sets, with its page break, sink, and ornament.
//!
//! A letter in the `index-letter` style stays with the entry below it. An entry in the `index-entry` style
//! shows its term, `index.separator`, and the pages of its marks, each linked to the first mark on its page.
//! Runs of three or more pages become a range whose first and last number link. `see` and `see also`
//! references follow in the words of the document language. Sub-entries follow in `index-sub-entry`, indented
//! by `index.level-indent` per level. An entry stays with its first sub-entry, and its last sub-entry with the
//! one before, so no sub-entry starts a column alone. The later lines of an entry hang by
//! `index.hanging-indent`. Entries break across pages and columns like paragraphs.

use std::borrow::Cow;

use super::{Flow, Frame, Role, heading_style, pages, translate_line};
use crate::config::values::Pt;
use crate::document::{Inline, InlineStyle, Link, Location};
use crate::index::{Entry, locators};
use crate::tags::Kind;

impl<'a> Flow<'a> {
    /// Sets the index for the block at `at` with the page numbers this pass assumes. A document without
    /// index marks has none.
    pub(super) fn index(&mut self, at: Location, frame: Frame<'a>) {
        let (config, structure) = (self.config, self.structure);
        let Some(index) = &structure.index else {
            return;
        };
        let heading = &structure.headings[self.headings];
        self.headings += 1;
        let title = [plain(&heading.text)];
        let role = Role::Heading(Some(heading.anchor));
        let mut style = Cow::Borrowed(heading_style(config, 1));
        if self.open_chapter(at) {
            style.to_mut().space_before = Pt(0.0);
        }
        let scope = self.open(
            Kind::Heading {
                level: 1,
                text: heading.title(),
            },
            true,
        );
        self.text_block(at, &title, &style, frame, 0.0, role);
        self.close(scope);
        self.ornament(at, &style, frame);

        // The parser allows the index only outside other blocks or in a column section, so a frame
        // narrower than the prose is a column.
        let page = &config.page;
        let in_columns = self.width(frame) < page.prose_width.0 - 1.0;
        let two = config.index.columns == 2 && !in_columns;
        let entries = if two {
            Frame {
                right: frame.right + page.prose_width.0 - pages::column_width(page),
                widen: 0.0,
                ..frame
            }
        } else {
            frame
        };
        let start = two.then(|| self.change_layout());
        let scope = self.open(Kind::Index, false);
        for group in &index.groups {
            let letter = group.letter.as_deref().unwrap_or(&config.labels.symbols);
            let style = &config.styles.index_letter;
            let heading = self.open(
                Kind::Heading {
                    level: 2,
                    text: letter.to_owned(),
                },
                true,
            );
            self.text_block(at, &[plain(letter)], style, entries, 0.0, Role::Heading(None));
            self.close(heading);
            for entry in &group.entries {
                self.index_entry(at, entry, 0, entries);
            }
        }
        self.close(scope);
        if let Some(start) = start {
            self.end_run(start);
        }
        self.after_paragraph = false;
    }

    /// Sets `entry` at sub-entry `level`, 0 for an entry, and its sub-entries after it.
    fn index_entry(&mut self, at: Location, entry: &Entry, level: usize, frame: Frame<'a>) {
        let config = self.config;
        let style = if level == 0 {
            &config.styles.index_entry
        } else {
            &config.styles.index_sub_entry
        };
        let content = self.entry_content(entry);
        let (indent, hang) = (
            config.index.level_indent.0 * level as f64,
            config.index.hanging_indent.0,
        );
        let left = frame.left + style.indent.0 + indent;
        let width = self.width(frame) - 2.0 * style.indent.0 - indent - hang;
        self.space(style.space_before.0);
        let scope = self.open(Kind::Paragraph, true);
        match self.set(&content, style, width, -hang) {
            Ok(lines) => {
                let count = lines.len();
                for (index, line) in lines.into_iter().enumerate() {
                    self.push(translate_line(line, left + hang), at, pages::line_break(index, count));
                }
            }
            Err(problem) => self.errors.push(self.error(at, problem)),
        }
        self.close(scope);
        self.space(style.space_after.0);
        if !entry.sub_entries.is_empty() {
            self.keep_last();
        }
        for (index, sub_entry) in entry.sub_entries.iter().enumerate() {
            // The last sub-entry does not start a column on its own, away from its entry.
            if index > 0 && index + 1 == entry.sub_entries.len() {
                self.keep_last();
            }
            self.index_entry(at, sub_entry, level + 1, frame);
        }
    }

    /// The term, its linked pages, and its references, as in "Hyphenation, 3–5, 9; see also Line breaking".
    fn entry_content(&self, entry: &Entry) -> Vec<Inline> {
        let (config, structure) = (self.config, self.structure);
        let mut content = vec![plain(&entry.term)];
        let pages: Vec<(String, usize)> = (entry.marks.iter())
            .map(|&mark| {
                let anchor = structure.marks[mark];
                (self.assumed[anchor].clone(), anchor)
            })
            .collect();
        let page = |(number, anchor): &(String, usize)| Inline::Text {
            text: number.clone(),
            style: InlineStyle {
                link: Some(Link::Anchor(*anchor)),
                unbreakable: true,
                ..InlineStyle::default()
            },
        };
        for (index, locator) in locators(&pages).iter().enumerate() {
            content.push(plain(if index == 0 { &config.index.separator } else { ", " }));
            content.push(page(&locator.first));
            if let Some(last) = &locator.last {
                content.push(Inline::Text {
                    text: "–".to_owned(),
                    style: InlineStyle {
                        unbreakable: true,
                        ..InlineStyle::default()
                    },
                });
                content.push(page(last));
            }
        }
        let labels = &config.labels;
        for (label, targets) in [(&labels.see, &entry.see), (&labels.see_also, &entry.see_also)] {
            if targets.is_empty() {
                continue;
            }
            content.push(plain(if content.len() == 1 {
                &config.index.separator
            } else {
                "; "
            }));
            content.push(Inline::Text {
                text: label.clone(),
                style: InlineStyle {
                    emphasis: true,
                    ..InlineStyle::default()
                },
            });
            content.push(plain(&format!(" {}", targets.join("; "))));
        }
        content
    }
}

fn plain(text: &str) -> Inline {
    Inline::Text {
        text: text.to_owned(),
        style: InlineStyle::default(),
    }
}
