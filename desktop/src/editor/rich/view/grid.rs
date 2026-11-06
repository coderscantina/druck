//! A table's grid, floating exactly over the table at rest. One field moves between the cells, which keeps a large table
//! light. Every change is written and the grid read again from the text, so what shows is always what is written.

use gpui::Focusable;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, MouseDownEvent, Pixels, Point, Subscription, Window, div, point, relative, svg,
};

use super::{CONTEXT, NextField, PreviousField, RichFloat, bounds_probe, card, field_label, small_button};
use crate::a11y::Control as _;
use crate::editor::commands::marks::Mark;
use crate::editor::float::Float;
use crate::editor::rich::table::{Align, Caption, Grid};
use crate::editor::rich::{AtomKind, Key};
use crate::editor::{EditorView, ToggleBold, ToggleItalic, ToggleStrike, ToggleUnderline};
use crate::inspector::input::{self, Cancelled, Submitted, TextInput};
use crate::source::Edit;
use crate::tokens::{self, Tokens};

pub struct GridFloat {
    /// Where the table's first block starts.
    start: usize,
    row: usize,
    cell: usize,
    input: Entity<TextInput>,
    caption: Entity<TextInput>,
    label: Entity<TextInput>,
    placement: String,
    refused: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl GridFloat {
    pub fn first_byte(&self) -> usize {
        self.start
    }
}

impl EditorView {
    /// The grid at this byte of the active chapter, as the text says now.
    fn grid_at(&self, start: usize) -> Option<std::rc::Rc<Grid>> {
        let atom = self.rich_atom(self.blocks.index_at(start))?;
        match &atom.kind {
            AtomKind::Table(Ok(grid)) => Some(grid.clone()),
            _ => None,
        }
    }

    /// Opens the grid of the table standing at `at`, with its first cell in the field.
    pub(in crate::editor) fn open_grid_at(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.blocks.index_at(at);
        let Some(atom) = self.rich_atom(index).cloned() else {
            return;
        };
        let start = self.blocks.as_slice()[atom.first].range.start;
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        self.caret_out_of(atom.first, atom.last, window, cx);
        let (row, cell) = (0, 0);
        let multiline = grid.is_list();
        let input = cx.new(|cx| {
            let mut input = if multiline {
                TextInput::multiline(window, cx).in_prose()
            } else {
                TextInput::new(window, cx).keep_focus_on_enter()
            };
            input.set_value(grid.rows[row][cell].text.clone(), true, cx);
            input.select_all(cx);
            input
        });
        let make = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut field = TextInput::new(window, cx)
                    .keep_focus_on_enter()
                    .with_placeholder(placeholder);
                field.set_value(value, true, cx);
                field
            })
        };
        let caption = make(
            grid.caption.text.clone(),
            crate::t!(Key::CaptionPlaceholder),
            window,
            cx,
        );
        let label = make(grid.caption.label.clone(), crate::t!(Key::LabelPlaceholder), window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, _: &Submitted, window, cx| {
                this.grid_step_row(true, window, cx)
            }),
            cx.subscribe_in(&input, window, |this, _, _: &Cancelled, window, cx| {
                this.commit_rich_float(window, cx)
            }),
            cx.subscribe_in(&caption, window, |this, _, _: &Submitted, window, cx| {
                this.grid_write_caption(window, cx)
            }),
            cx.subscribe_in(&caption, window, |this, _, _: &Cancelled, window, cx| {
                this.commit_rich_float(window, cx)
            }),
            cx.subscribe_in(&label, window, |this, _, _: &Submitted, window, cx| {
                this.grid_write_caption(window, cx)
            }),
            cx.subscribe_in(&label, window, |this, _, _: &Cancelled, window, cx| {
                this.commit_rich_float(window, cx)
            }),
        ];
        input.focus_handle(cx).focus(window, cx);
        self.float = Float::Rich(RichFloat::Grid(GridFloat {
            start,
            row,
            cell,
            input,
            caption,
            label,
            placement: grid.caption.placement.clone(),
            refused: None,
            _subscriptions: subscriptions,
        }));
        self.start_revealing_float(true);
        cx.notify();
    }

    fn grid_float(&self) -> Option<&GridFloat> {
        match &self.float {
            Float::Rich(RichFloat::Grid(grid)) => Some(grid),
            _ => None,
        }
    }

    fn grid_float_mut(&mut self) -> Option<&mut GridFloat> {
        match &mut self.float {
            Float::Rich(RichFloat::Grid(grid)) => Some(grid),
            _ => None,
        }
    }

    /// Writes the cell in the field. Returns whether it could be.
    fn grid_write_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(float) = self.grid_float() else {
            return false;
        };
        let (start, row, cell) = (float.start, float.row, float.cell);
        let new = float.input.read(cx).text().to_owned();
        let Some(grid) = self.grid_at(start) else {
            return false;
        };
        if grid
            .rows
            .get(row)
            .and_then(|cells| cells.get(cell))
            .is_some_and(|old| old.text == new)
        {
            return true;
        }
        let text = self.session.active_buffer().text();
        let edits = match grid.set_cell(text, row, cell, &new) {
            Ok(edits) => edits,
            Err(why) => {
                self.grid_refused(crate::t!(why).to_owned(), cx);
                return false;
            }
        };
        self.grid_write(edits, window, cx)
    }

    /// Writes edits as one undo step. What is refused stays out, and the grid says why.
    fn grid_write(&mut self, edits: Vec<Edit>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let active = self.session.active();
        match self.write_rich(active, edits, None, window, cx) {
            Ok(()) => {
                if let Some(float) = self.grid_float_mut() {
                    float.refused = None;
                }
                cx.notify();
                true
            }
            Err(why) => {
                self.grid_refused(why, cx);
                false
            }
        }
    }

    fn grid_refused(&mut self, why: String, cx: &mut Context<Self>) {
        if let Some(float) = self.grid_float_mut() {
            float.refused = Some(why);
        }
        cx.notify();
    }

    /// Runs a change the grid works out, keeping the cell in the field when it can.
    fn grid_change(
        &mut self,
        change: impl FnOnce(&Grid, &str, usize, usize) -> Result<Vec<Edit>, Key>,
        to: impl FnOnce(usize, usize) -> (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some(float) = self.grid_float() else {
            return;
        };
        let (start, row, cell) = (float.start, float.row, float.cell);
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        let text = self.session.active_buffer().text();
        match change(&grid, text, row, cell) {
            Ok(edits) => {
                if self.grid_write(edits, window, cx) {
                    let (row, cell) = to(row, cell);
                    self.grid_show(row, cell, window, cx);
                }
            }
            Err(why) => self.grid_refused(crate::t!(why).to_owned(), cx),
        }
    }

    /// Puts cell (`row`, `cell`), or the nearest one that exists, in the field.
    fn grid_show(&mut self, row: usize, cell: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = self.grid_float().map(GridFloat::first_byte) else {
            return;
        };
        let Some(grid) = self.grid_at(start) else {
            self.float = Float::None;
            window.focus(&self.focus, cx);
            return;
        };
        let row = row.min(grid.rows.len() - 1);
        let cell = cell.min(grid.rows[row].len().saturating_sub(1));
        let value = grid.rows[row].get(cell).map_or(String::new(), |cell| cell.text.clone());
        let Some(float) = self.grid_float_mut() else {
            return;
        };
        float.row = row;
        float.cell = cell;
        let input = float.input.clone();
        input.update(cx, |input, cx| {
            input.set_value(value, true, cx);
            input.select_all(cx);
        });
        input.focus_handle(cx).focus(window, cx);
        self.start_revealing_float(false);
        cx.notify();
    }

    /// Tab and Shift+Tab: the next or previous cell. Tab past the last cell adds a row.
    fn grid_step_cell(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some(float) = self.grid_float() else {
            return;
        };
        let (start, row, cell) = (float.start, float.row, float.cell);
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        if forward {
            if cell + 1 < grid.rows[row].len() {
                self.grid_show(row, cell + 1, window, cx);
            } else if row + 1 < grid.rows.len() {
                self.grid_show(row + 1, 0, window, cx);
            } else {
                self.grid_change(
                    |grid, text, row, _| grid.add_row(text, row),
                    |row, _| (row + 1, 0),
                    window,
                    cx,
                );
            }
        } else if cell > 0 {
            self.grid_show(row, cell - 1, window, cx);
        } else if row > 0 {
            let last = grid.rows[row - 1].len().saturating_sub(1);
            self.grid_show(row - 1, last, window, cx);
        }
    }

    /// Enter, up, and down in a one-line cell: the same column in the next or previous row.
    fn grid_step_row(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_write_cell(window, cx) {
            return;
        }
        let Some(float) = self.grid_float() else {
            return;
        };
        let (row, cell) = (float.row, float.cell);
        if down {
            self.grid_show(row + 1, cell, window, cx);
        } else {
            self.grid_show(row.saturating_sub(1), cell, window, cx);
        }
    }

    fn grid_write_caption(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(float) = self.grid_float() else {
            return;
        };
        let caption = Caption {
            text: float.caption.read(cx).text().to_owned(),
            label: float.label.read(cx).text().trim().to_owned(),
            placement: float.placement.clone(),
        };
        let Some(grid) = self.grid_at(float.start) else {
            return;
        };
        let text = self.session.active_buffer().text();
        if let Some(edit) = grid.set_caption(text, &caption) {
            self.grid_write(vec![edit], window, cx);
        }
    }

    /// Writes what is unsaved. Something refused keeps the grid open with the reason.
    pub(super) fn commit_grid(&mut self, float: GridFloat, window: &mut Window, cx: &mut Context<Self>) {
        self.float = Float::Rich(RichFloat::Grid(float));
        let written = self.grid_write_cell(window, cx);
        self.grid_write_caption(window, cx);
        let refused = self.grid_float().is_some_and(|float| float.refused.is_some());
        if written && !refused {
            self.float = Float::None;
        }
    }

    fn grid_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = self.grid_float().map(GridFloat::first_byte) else {
            return;
        };
        let Some(grid) = self.grid_at(start) else {
            return;
        };
        let Some(atom) = self.rich_atom(self.blocks.index_at(start)).cloned() else {
            return;
        };
        let blocks = self.blocks.as_slice();
        let end = blocks[atom.last].range.start
            + self.session.active_buffer().text()[blocks[atom.last].range.clone()]
                .trim_end_matches(['\n', '\r'])
                .len();
        let edit = grid.removal(self.session.active_buffer().text(), start..end);
        self.float = Float::None;
        window.focus(&self.focus, cx);
        let active = self.session.active();
        if let Err(why) = self.write_rich(active, vec![edit], None, window, cx) {
            self.notify_rich(start, why, false, cx);
        }
    }

    fn grid_mark(&mut self, mark: Mark, cx: &mut Context<Self>) {
        if let Some(field) = self.grid_float().map(|float| float.input.clone()) {
            self.mark_in_field(&field, mark, cx);
        }
    }

    /// Cmd+Z in the field undoes its own typing first, and then, with nothing typed, the last change to the table.
    fn grid_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.grid_cell_focused(window, cx) {
            return;
        }
        let Some(float) = self.grid_float() else {
            return;
        };
        let (start, row, cell) = (float.start, float.row, float.cell);
        let unchanged = self
            .grid_at(start)
            .and_then(|grid| {
                grid.rows
                    .get(row)
                    .and_then(|cells| cells.get(cell).map(|old| old.text.clone()))
            })
            .is_some_and(|old| old == float.input.read(cx).text());
        if unchanged {
            let restored = self.session.active_buffer_mut().undo();
            self.restore(restored, window, cx);
            self.autoscroll = false;
            self.grid_show(row, cell, window, cx);
            cx.stop_propagation();
        }
    }

    pub(super) fn grid_element(&mut self, start: usize, cx: &mut Context<Self>) -> Option<(AnyElement, Point<Pixels>)> {
        let index = self.blocks.index_at(start);
        let top = self.block_top(index)?;
        let grid = self.grid_at(start)?;
        let tokens = Tokens::get(cx).clone();
        let float = self.grid_float()?;
        let (row, cell) = (float.row, float.cell);
        let input = float.input.clone();
        let is_list = grid.is_list();
        let refused = float.refused.clone();
        let (caption, label) = (float.caption.clone(), float.label.clone());
        let placement = float.placement.clone();
        let column_of =
            |row: usize, cell: usize| -> usize { grid.rows[row].iter().take(cell).map(|cell| cell.span).sum() };
        let column = column_of(row, cell);
        let align = grid.aligns.get(column).copied().unwrap_or_default();

        let button = |id: &'static str, label: &'static str| small_button(&tokens, id, label);
        let row_tools = div()
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .child(field_label(&tokens, crate::t!(Key::RowLabel)))
            .child(button("row-add", "+").on_click(cx.listener(|this, _, window, cx| {
                this.grid_change(
                    |grid, text, row, _| grid.add_row(text, row),
                    |row, cell| (row + 1, cell),
                    window,
                    cx,
                );
            })))
            .child(button("row-remove", "−").on_click(cx.listener(|this, _, window, cx| {
                this.grid_change(
                    |grid, text, row, _| grid.remove_row(text, row),
                    |row, cell| (row, cell),
                    window,
                    cx,
                );
            })))
            .child(button("row-up", "↑").on_click(cx.listener(|this, _, window, cx| {
                this.grid_change(
                    |grid, text, row, _| {
                        if row == 0 {
                            Err(Key::TableNoRowThere)
                        } else {
                            grid.move_row_down(text, row - 1)
                        }
                    },
                    |row, cell| (row.saturating_sub(1), cell),
                    window,
                    cx,
                );
            })))
            .child(button("row-down", "↓").on_click(cx.listener(|this, _, window, cx| {
                this.grid_change(
                    |grid, text, row, _| grid.move_row_down(text, row),
                    |row, cell| (row + 1, cell),
                    window,
                    cx,
                );
            })));
        let column_tools = div()
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .child(field_label(&tokens, crate::t!(Key::ColumnLabel)))
            .child(button("column-add", "+").on_click(cx.listener(|this, _, window, cx| {
                this.grid_change(
                    |grid, text, row, cell| {
                        let column = grid.rows[row]
                            .iter()
                            .take(cell + 1)
                            .map(|cell| cell.span)
                            .sum::<usize>()
                            - 1;
                        grid.add_column(text, Some(column))
                    },
                    |row, cell| (row, cell + 1),
                    window,
                    cx,
                );
            })))
            .child(
                button("column-remove", "−").on_click(cx.listener(|this, _, window, cx| {
                    this.grid_change(
                        |grid, text, row, cell| {
                            let column = grid.rows[row].iter().take(cell).map(|cell| cell.span).sum();
                            grid.remove_column(text, column)
                        },
                        |row, cell| (row, cell),
                        window,
                        cx,
                    );
                })),
            )
            .child(button("column-left", "←").on_click(cx.listener(|this, _, window, cx| {
                this.grid_change(
                    |grid, text, row, cell| {
                        let column: usize = grid.rows[row].iter().take(cell).map(|cell| cell.span).sum();
                        if column == 0 {
                            Err(Key::TableNoColumnThere)
                        } else {
                            grid.move_column_right(text, column - 1)
                        }
                    },
                    |row, cell| (row, cell.saturating_sub(1)),
                    window,
                    cx,
                );
            })))
            .child(button("column-right", "→").on_click(cx.listener(|this, _, window, cx| {
                this.grid_change(
                    |grid, text, row, cell| {
                        let column = grid.rows[row].iter().take(cell).map(|cell| cell.span).sum();
                        grid.move_column_right(text, column)
                    },
                    |row, cell| (row, cell + 1),
                    window,
                    cx,
                );
            })));
        let align_button =
            |id: &'static str, icon: &'static str, tip: &'static str, choice: Align, cx: &mut Context<Self>| {
                let chosen = align == choice;
                div()
                    .id(id)
                    .toggle(&tokens, tip, chosen)
                    .flex_none()
                    .size(tokens::SMALL_BUTTON_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::CHIP_RADIUS)
                    .cursor_pointer()
                    .when(chosen, |button| button.bg(tokens.text))
                    .when(!chosen, |button| button.hover(|button| button.bg(tokens.raised)))
                    .tooltip(crate::editor::float::tooltip(tip.to_owned()))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.grid_change(
                            move |grid, text, row, cell| {
                                let column = grid.rows[row].iter().take(cell).map(|cell| cell.span).sum();
                                grid.set_align(text, column, choice)
                            },
                            |row, cell| (row, cell),
                            window,
                            cx,
                        );
                    }))
                    .child(svg().path(icon).size(tokens::CHECK_SIZE).text_color(if chosen {
                        tokens.panel
                    } else {
                        tokens.text
                    }))
            };
        let aligns = div()
            .flex()
            .items_center()
            .child(align_button(
                "align-default",
                "icons/align-default.svg",
                crate::t!(Key::AlignDefault),
                Align::Default,
                cx,
            ))
            .child(align_button(
                "align-left",
                "icons/align-left.svg",
                crate::t!(Key::AlignLeft),
                Align::Left,
                cx,
            ))
            .child(align_button(
                "align-center",
                "icons/align-center.svg",
                crate::t!(Key::AlignCenter),
                Align::Center,
                cx,
            ))
            .child(align_button(
                "align-right",
                "icons/align-right.svg",
                crate::t!(Key::AlignRight),
                Align::Right,
                cx,
            ));
        let span_tools = is_list.then(|| {
            div()
                .flex()
                .items_center()
                .gap(tokens::SPACE_1)
                .child(
                    button("span-more", "⇥")
                        .tooltip(crate::editor::float::tooltip(crate::t!(Key::MergeRight).to_owned()))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.grid_change(
                                |grid, text, row, cell| grid.merge_right(text, row, cell),
                                |row, cell| (row, cell),
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    button("span-less", "⇤")
                        .tooltip(crate::editor::float::tooltip(crate::t!(Key::SplitCell).to_owned()))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.grid_change(
                                |grid, text, row, cell| grid.split(text, row, cell),
                                |row, cell| (row, cell),
                                window,
                                cx,
                            );
                        })),
                )
        });
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(tokens::SPACE_2)
            .child(row_tools)
            .child(column_tools)
            .child(aligns)
            .children(span_tools);
        let columns = grid.columns.max(1) as f32;
        let rows: Vec<_> = grid
            .rows
            .iter()
            .enumerate()
            .map(|(row_index, cells)| {
                let shown: Vec<_> = cells
                    .iter()
                    .enumerate()
                    .map(|(cell_index, shown)| {
                        let active = row_index == row && cell_index == cell;
                        let share = shown.span as f32 / columns;
                        div()
                            .id(("grid-cell", row_index * 1024 + cell_index))
                            .w(relative(share))
                            .min_w_0()
                            .min_h(tokens::FIELD_HEIGHT)
                            .border_r_1()
                            .border_color(tokens.divider)
                            .when(active, |cell| cell.child(input.clone()))
                            .when(!active, |cell| {
                                cell.px(tokens::SPACE_2)
                                    .py(tokens::GRID_ROW_Y)
                                    .cursor_text()
                                    .text_color(if row_index == 0 { tokens.text } else { tokens.text_quiet })
                                    .when(row_index == 0, |cell| cell.font_weight(gpui::FontWeight::SEMIBOLD))
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if this.grid_write_cell(window, cx) {
                                            this.grid_show(row_index, cell_index, window, cx);
                                        }
                                    }))
                                    .child(shown.text.replace('\n', " "))
                            })
                    })
                    .collect();
                div().flex().border_b_1().border_color(tokens.divider).children(shown)
            })
            .collect();
        let placement_button = |id: &'static str, word: &'static str, value: &'static str, cx: &mut Context<Self>| {
            small_button(&tokens, id, word)
                .h(tokens::FIELD_HEIGHT)
                .when(placement == value, |button| {
                    button.bg(tokens.raised).text_color(tokens.text)
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(float) = this.grid_float_mut() {
                        float.placement = value.to_owned();
                    }
                    this.grid_write_caption(window, cx);
                }))
        };
        let caption_row = div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(tokens::SPACE_2)
            .child(
                div()
                    .flex_1()
                    .min_w(tokens::GRID_FIELD_MIN)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(field_label(&tokens, crate::t!(Key::CaptionLabel)))
                    .child(caption),
            )
            .child(
                div()
                    .w(tokens::GRID_LABEL_WIDTH)
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(field_label(&tokens, format!("{} (tbl:)", crate::t!(Key::LabelLabel))))
                    .child(label),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(field_label(&tokens, crate::t!(Key::PlacementLabel)))
                    .child(
                        div()
                            .flex()
                            .gap(tokens::SPACE_1)
                            .child(placement_button("grid-here", crate::t!(Key::PlacementHere), "", cx))
                            .child(placement_button("grid-top", crate::t!(Key::PlacementTop), "top", cx))
                            .child(placement_button(
                                "grid-bottom",
                                crate::t!(Key::PlacementBottom),
                                "bottom",
                                cx,
                            )),
                    ),
            );
        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(tokens::SPACE_2)
            .child(
                small_button(&tokens, "grid-source", crate::t!(Key::ShowSource)).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.commit_rich_float(window, cx);
                        this.toggle_source(window, cx);
                    },
                )),
            )
            .child(
                div()
                    .flex()
                    .gap(tokens::SPACE_1)
                    .child(
                        small_button(&tokens, "grid-delete", crate::t!(Key::DeleteTable))
                            .on_click(cx.listener(|this, _, window, cx| this.grid_delete(window, cx))),
                    )
                    .child(
                        small_button(&tokens, "grid-done", crate::t!(Key::CloseLabel))
                            .text_color(tokens.accent)
                            .on_click(cx.listener(|this, _, window, cx| this.commit_rich_float(window, cx))),
                    ),
            );
        let max_height = self.column.viewport.size.height - tokens::SPACE_4;
        let element = card(&tokens)
            .id("grid-float")
            .key_context(CONTEXT)
            .w(self.column.text_width + tokens::SPACE_4)
            .max_h(max_height)
            .overflow_y_scroll()
            .p(tokens::SPACE_2)
            .gap(tokens::SPACE_2)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.commit_rich_float(window, cx)))
            .capture_action(cx.listener(move |this, _: &input::Up, window, cx| {
                if !is_list && this.grid_cell_focused(window, cx) {
                    this.grid_step_row(false, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(move |this, _: &input::Down, window, cx| {
                if !is_list && this.grid_cell_focused(window, cx) {
                    this.grid_step_row(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Escape, window, cx| {
                this.commit_rich_float(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Undo, window, cx| this.grid_undo(window, cx)))
            .on_action(cx.listener(|this, _: &NextField, window, cx| this.grid_step_cell(true, window, cx)))
            .on_action(cx.listener(|this, _: &PreviousField, window, cx| this.grid_step_cell(false, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleBold, _, cx| this.grid_mark(Mark::Bold, cx)))
            .on_action(cx.listener(|this, _: &ToggleItalic, _, cx| this.grid_mark(Mark::Italic, cx)))
            .on_action(cx.listener(|this, _: &ToggleUnderline, _, cx| this.grid_mark(Mark::Underline, cx)))
            .on_action(cx.listener(|this, _: &ToggleStrike, _, cx| this.grid_mark(Mark::Strike, cx)))
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(tokens.divider)
                    .rounded(tokens::CHIP_RADIUS)
                    .children(rows),
            )
            .children(refused.map(|why| div().text_size(tokens::TEXT_SMALL).text_color(tokens.accent).child(why)))
            .child(caption_row)
            .child(footer)
            .child(bounds_probe(self.float_bounds.clone()))
            .into_any_element();
        let x = self.column.text_left - tokens::SPACE_2;
        let viewport = self.column.viewport;
        let y = top.max(viewport.top()).min(viewport.bottom() - tokens::GRID_MIN_ROOM);
        Some((element, point(x, y)))
    }

    fn grid_cell_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.grid_float()
            .is_some_and(|float| float.input.focus_handle(cx).is_focused(window))
    }
}
