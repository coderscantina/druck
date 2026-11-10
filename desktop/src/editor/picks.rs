//! The lists that set one thing about a block without its syntax: the theme's custom style it is set in, and a code
//! block's language. The block menu leads to them, and so do their commands and the selection toolbar.

use std::ops::Range;

use gpui::{Bounds, Context, Pixels, SharedString, Window};

use super::commands::{Command, code, normalized, selection_after};
use super::placeholders::sentence_case;
use super::rich::styles::{self, Class};
use super::rich::view::TableAct;
use super::strings::Key;
use super::{CodeLanguage, EditorView, LeaveCode, MarkIndex, SetStyle, ShowBlockMenu};
use crate::source::BlockKind;

/// A row of the block menu that opens a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Extra {
    Style,
    Language,
}

/// What choosing a row of a list writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pick {
    /// The custom style of every block in the range that takes one of its kind, or with `None` the theme's own.
    Style {
        blocks: Range<usize>,
        class: Class,
        name: Option<String>,
    },
    /// The language of the code block that starts at this byte.
    Language { block: usize, word: String },
    /// A row of a table's menu, for the cell at this row and place of the table that starts at this byte.
    Table {
        start: usize,
        row: usize,
        cell: usize,
        act: TableAct,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub label: SharedString,
    /// The choice the block has now.
    pub on: bool,
    pub icon: &'static str,
    pub pick: Pick,
}

/// The first block in `blocks` that takes a custom style, with its class.
fn styled_block(text: &str, list: &[crate::source::blocks::Block], blocks: &Range<usize>) -> Option<(usize, Class)> {
    blocks.clone().find_map(|index| {
        let block = list.get(index)?;
        styles::class_of(block.kind, &text[block.range.clone()]).map(|class| (index, class))
    })
}

impl EditorView {
    /// The blocks the selection touches, or the caret's block.
    pub(super) fn touched_blocks(&self) -> Range<usize> {
        let range = self.selection.range();
        let first = self.blocks.index_at(range.start);
        let last = self.blocks.index_at(range.end.max(range.start + 1) - 1).max(first);
        first..last + 1
    }

    /// The rows of the block menu that lead to a list, for these blocks.
    pub(super) fn extras_for(&self, blocks: &Range<usize>) -> Vec<Extra> {
        let mut extras = Vec::new();
        if !self.style_rows(blocks.clone()).is_empty() {
            extras.push(Extra::Style);
        }
        if blocks.len() == 1 && !self.language_rows(blocks.start).is_empty() {
            extras.push(Extra::Language);
        }
        extras
    }

    /// How the block menu shows such a row: its name, what the block has now, and its icon.
    pub(super) fn extra_row(
        &self,
        extra: Extra,
        blocks: &Range<usize>,
    ) -> (SharedString, Option<SharedString>, &'static str) {
        let rows = match extra {
            Extra::Style => self.style_rows(blocks.clone()),
            Extra::Language => self.language_rows(blocks.start),
        };
        let now = rows.into_iter().find(|row| row.on).map(|row| row.label);
        match extra {
            Extra::Style => (crate::t!(Key::Style).into(), now, "icons/styles.svg"),
            Extra::Language => (crate::t!(Key::Language).into(), now, "icons/block-code.svg"),
        }
    }

    pub(super) fn extra_rows(&self, extra: Extra, blocks: &Range<usize>) -> Vec<Row> {
        match extra {
            Extra::Style => self.style_rows(blocks.clone()),
            Extra::Language => self.language_rows(blocks.start),
        }
    }

    /// The styles these blocks can be set in: the theme's own for their kind, then its custom styles for that kind.
    /// Empty when the theme has none for them.
    fn style_rows(&self, blocks: Range<usize>) -> Vec<Row> {
        let text = self.session.active_buffer().text();
        let list = self.blocks.as_slice();
        let Some((first, class)) = styled_block(text, list, &blocks) else {
            return Vec::new();
        };
        let custom: Vec<&str> = (self.style.custom_styles().iter())
            .filter(|custom| custom.class == class)
            .map(|custom| custom.name.as_str())
            .collect();
        if custom.is_empty() {
            return Vec::new();
        }
        let now = styles::style_of(text, &self.blocks, first);
        let icon = "icons/styles.svg";
        let own = crate::inspector::style_property::style_label(styles::base_style(list[first].kind));
        let pick = |name: Option<&str>| Pick::Style {
            blocks: blocks.clone(),
            class,
            name: name.map(str::to_owned),
        };
        let mut rows = vec![Row {
            label: own.to_owned().into(),
            on: now.is_none(),
            icon,
            pick: pick(None),
        }];
        rows.extend(custom.into_iter().map(|name| Row {
            label: sentence_case(name).into(),
            on: now.as_deref() == Some(name),
            icon,
            pick: pick(Some(name)),
        }));
        rows
    }

    /// The languages of the fenced code block `index`: none, the list's, and the block's own when the list lacks it.
    fn language_rows(&self, index: usize) -> Vec<Row> {
        let Some(block) = self
            .blocks
            .as_slice()
            .get(index)
            .filter(|block| block.kind == BlockKind::Code)
        else {
            return Vec::new();
        };
        let text = self.session.active_buffer().text();
        let Some(now) = code::language(&text[block.range.clone()]) else {
            return Vec::new();
        };
        let known = |word: &str| {
            code::LANGUAGES
                .iter()
                .any(|(_, written)| written.eq_ignore_ascii_case(word))
        };
        let row = |label: &str, word: &str| Row {
            label: label.to_owned().into(),
            on: now.eq_ignore_ascii_case(word),
            icon: "icons/block-code.svg",
            pick: Pick::Language {
                block: block.range.start,
                word: word.to_owned(),
            },
        };
        let mut rows = vec![row(crate::t!(Key::PlainText), "")];
        if !now.is_empty() && !known(now) {
            rows.push(row(now, now));
        }
        rows.extend(code::LANGUAGES.iter().map(|(name, word)| row(name, word)));
        rows
    }

    /// Writes what a row of a list chose, as one undo step.
    pub(super) fn run_pick(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if let Pick::Table { start, row, cell, act } = pick {
            return self.run_table_act((start, row, cell), act, window, cx);
        }
        let text = self.session.active_buffer().text();
        let edits = match pick {
            Pick::Style { blocks, class, name } => blocks
                .filter(|index| {
                    let block = &self.blocks.as_slice()[*index];
                    styles::class_of(block.kind, &text[block.range.clone()]) == Some(class)
                })
                .filter_map(|index| styles::set_style(text, &self.blocks, index, name.as_deref()))
                .collect(),
            Pick::Language { block, word } => {
                let block = &self.blocks.as_slice()[self.blocks.index_at(block)];
                code::set_language(text, block, &word).into_iter().collect()
            }
            Pick::Table { .. } => Vec::new(),
        };
        let edits = normalized(edits, text);
        if edits.is_empty() {
            return;
        }
        let selection = selection_after(self.selection, &edits);
        self.run_command(Command { edits, selection }, window, cx);
    }

    /// Opens the block menu at the caret or over the selection, as a click on the gutter's icon does.
    pub(super) fn show_block_menu(&mut self, _: &ShowBlockMenu, _: &mut Window, cx: &mut Context<Self>) {
        if self.source {
            return;
        }
        let blocks = self.touched_blocks();
        let several = (blocks.len() > 1).then(|| blocks.clone());
        self.open_block_menu(blocks.start, several, None, cx);
    }

    /// Opens one of the lists for the blocks the selection touches, at `anchor` or the selection.
    pub(super) fn show_extra(&mut self, extra: Extra, anchor: Option<Bounds<Pixels>>, cx: &mut Context<Self>) {
        if self.source {
            return;
        }
        let blocks = self.touched_blocks();
        let rows = self.extra_rows(extra, &blocks);
        if !rows.is_empty() {
            self.open_pick(rows, anchor, cx);
        }
    }

    pub(super) fn set_style(&mut self, _: &SetStyle, _: &mut Window, cx: &mut Context<Self>) {
        self.show_extra(Extra::Style, None, cx);
    }

    pub(super) fn code_language(&mut self, _: &CodeLanguage, _: &mut Window, cx: &mut Context<Self>) {
        self.show_extra(Extra::Language, None, cx);
    }

    pub(super) fn mark_index(&mut self, _: &MarkIndex, window: &mut Window, cx: &mut Context<Self>) {
        self.open_index(window, cx);
    }

    /// Leaves the code block the caret is in, for an empty line after it.
    pub(super) fn leave_code(&mut self, _: &LeaveCode, window: &mut Window, cx: &mut Context<Self>) {
        if self.source {
            return;
        }
        let text = self.session.active_buffer().text();
        match code::leave(text, &self.blocks, self.selection.head) {
            Some(command) if command.edits.is_empty() => self.set_selection(command.selection, false, window, cx),
            Some(command) => self.run_command(command, window, cx),
            None => {}
        }
    }

    /// Whether the selection may take a new index mark or stands at one, which is when the toolbar offers it.
    pub(super) fn index_offered(&self) -> bool {
        super::rich::index::allowed(&self.blocks, self.selection.range().end.saturating_sub(1))
    }

    /// Whether the theme has a custom style for the blocks the selection touches.
    pub(super) fn styles_offered(&self) -> bool {
        !self.style_rows(self.touched_blocks()).is_empty()
    }
}
