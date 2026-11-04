use super::*;
use crate::editor::commands::{applied, normalized};

fn grid(text: &str) -> Grid {
    let blocks = Blocks::parse(text);
    let regions = regions(text, &blocks);
    regions.into_iter().next().expect("a table").grid.expect("a grid")
}

fn run(text: &str, edits: Result<Vec<Edit>, Refused>) -> String {
    applied(text, &normalized(edits.expect("allowed"), text))
}

const PADDED: &str = "Before.\n\n| Year | Event            | Pages |\n|-----:|:-----------------|------:|\n| 1455 | The *Bible* ß    | 1 286 |\n| 1978 | TeX              |   700 |\n\n: Two events. {#tbl:events placement=top}\n\nAfter.\n";

#[test]
fn a_pipe_table_reads_its_cells_aligns_and_caption() {
    let table = grid(PADDED);
    assert_eq!(table.columns, 3);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[1][1].text, "The *Bible* ß");
    assert_eq!(table.aligns, [Align::Right, Align::Left, Align::Right]);
    assert_eq!(table.caption.text, "Two events.");
    assert_eq!(table.caption.label, "events");
    assert_eq!(table.caption.placement, "top");
}

#[test]
fn changing_a_cell_keeps_its_width_and_every_other_row() {
    let table = grid(PADDED);
    let result = run(PADDED, table.set_cell(PADDED, 2, 1, "»TeX«"));
    assert_eq!(
        result,
        PADDED.replace("| 1978 | TeX              |", "| 1978 | »TeX«            |")
    );
    let result = run(PADDED, table.set_cell(PADDED, 2, 2, "9"));
    assert_eq!(result, PADDED.replace("|   700 |", "|     9 |"));
    let long = "a very long text that cannot fit";
    let result = run(PADDED, table.set_cell(PADDED, 2, 1, long));
    assert_eq!(result, PADDED.replace("| TeX              |", &format!("| {long} |")));
    assert!(table.set_cell(PADDED, 2, 1, "TeX").expect("allowed").is_empty());
}

#[test]
fn a_pipe_in_a_cell_is_escaped_and_comes_back() {
    let text = "| a | b |\n|---|---|\n| x\\|y | 2 |\n";
    let table = grid(text);
    assert_eq!(table.rows[1][0].text, "x|y");
    assert!(table.set_cell(text, 1, 0, "x|y").expect("allowed").is_empty());
    assert_eq!(
        run(text, table.set_cell(text, 1, 1, "p|q")),
        "| a | b |\n|---|---|\n| x\\|y | p\\|q |\n"
    );
}

#[test]
fn rows_add_remove_and_move_and_leave_the_others_alone() {
    let text = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
    let table = grid(text);
    assert_eq!(
        run(text, table.add_row(text, 1)),
        "| a | b |\n|---|---|\n| 1 | 2 |\n|   |   |\n| 3 | 4 |\n"
    );
    assert_eq!(
        run(text, table.add_row(text, 0)),
        "| a | b |\n|---|---|\n|   |   |\n| 1 | 2 |\n| 3 | 4 |\n"
    );
    assert_eq!(
        run(text, table.remove_row(text, 1)),
        "| a | b |\n|---|---|\n| 3 | 4 |\n"
    );
    assert_eq!(
        run(text, table.remove_row(text, 2)),
        "| a | b |\n|---|---|\n| 1 | 2 |\n"
    );
    assert_eq!(
        run(text, table.move_row_down(text, 1)),
        "| a | b |\n|---|---|\n| 3 | 4 |\n| 1 | 2 |\n"
    );
    assert_eq!(table.remove_row(text, 0), Err(Key::TableHeaderStays));
    assert_eq!(table.move_row_down(text, 2), Err(Key::TableNoRowThere));
}

#[test]
fn columns_add_remove_and_move_on_every_row() {
    let text = "| a | b |\n|:--|--:|\n| 1 | 2 |\n";
    let table = grid(text);
    assert_eq!(
        run(text, table.add_column(text, Some(0))),
        "| a |  | b |\n|:--| --- |--:|\n| 1 |  | 2 |\n"
    );
    assert_eq!(
        run(text, table.add_column(text, None)),
        "|  | a | b |\n| --- |:--|--:|\n|  | 1 | 2 |\n"
    );
    assert_eq!(
        run(text, table.add_column(text, Some(1))),
        "| a | b |  |\n|:--|--:| --- |\n| 1 | 2 |  |\n"
    );
    assert_eq!(run(text, table.remove_column(text, 0)), "| b |\n|--:|\n| 2 |\n");
    assert_eq!(run(text, table.remove_column(text, 1)), "| a |\n|:--|\n| 1 |\n");
    assert_eq!(
        run(text, table.move_column_right(text, 0)),
        "| b | a |\n|--:|:--|\n| 2 | 1 |\n"
    );
    let single = "| a |\n|---|\n";
    assert_eq!(grid(single).remove_column(single, 0), Err(Key::TableOneColumnStays));
}

#[test]
fn alignment_changes_only_the_delimiter_cell_and_keeps_its_width() {
    let table = grid(PADDED);
    let result = run(PADDED, table.set_align(PADDED, 1, Align::Center));
    assert_eq!(result, PADDED.replace("|:-----------------|", "|:----------------:|"));
    let result = run(PADDED, table.set_align(PADDED, 0, Align::Default));
    assert_eq!(result, PADDED.replace("|-----:|", "|------|"));
}

#[test]
fn a_caption_is_added_changed_and_taken_out_with_its_blank_line() {
    let text = "| a |\n|---|\n\nNext.\n";
    let table = grid(text);
    let caption = Caption {
        text: "Größe »x«".to_owned(),
        label: "maß".to_owned(),
        placement: String::new(),
    };
    let edit = table.set_caption(text, &caption).expect("an edit");
    let result = applied(text, &[edit]);
    assert_eq!(result, "| a |\n|---|\n\n: Größe »x« {#tbl:maß}\n\nNext.\n");
    let table = grid(&result);
    assert_eq!(table.caption, caption);
    assert_eq!(table.set_caption(&result, &caption), None);
    let edit = table.set_caption(&result, &Caption::default()).expect("an edit");
    assert_eq!(applied(&result, &[edit]), text);
}

const LIST: &str = "::: table {align=\"left right\" widths=\"* auto\"}\n- - Item\n  - Amount\n- {.group}\n  - {span=2} Theme »ß«\n- - Theme in design\n\n    {.detail}\n    - Tokens\n    - Cover\n  - 2 280.00\n- {.total}\n  - {.sum-label} Total\n  - 2 280.00\n:::\n\n: Estimated costs {#tbl:costs}\n";

#[test]
fn a_list_table_reads_rows_cells_and_spans() {
    let table = grid(LIST);
    assert!(table.is_list());
    assert_eq!(table.columns, 2);
    assert_eq!(table.rows.len(), 4);
    assert_eq!(table.rows[1][0].text, "Theme »ß«");
    assert_eq!(table.rows[1][0].span, 2);
    assert_eq!(table.rows[2][0].text, "Theme in design\n\n{.detail}\n- Tokens\n- Cover");
    assert_eq!(table.rows[3][1].text, "2 280.00");
    assert_eq!(table.aligns, [Align::Left, Align::Right]);
    assert_eq!(table.caption.label, "costs");
}

#[test]
fn changing_a_list_cell_touches_only_its_text_with_its_indent() {
    let table = grid(LIST);
    let result = run(LIST, table.set_cell(LIST, 2, 0, "Neu\n\nZweite Zeile"));
    assert_eq!(
        result,
        LIST.replace(
            "- - Theme in design\n\n    {.detail}\n    - Tokens\n    - Cover\n",
            "- - Neu\n\n    Zweite Zeile\n"
        )
    );
    assert!(
        table
            .set_cell(LIST, 2, 0, "Theme in design\n\n{.detail}\n- Tokens\n- Cover")
            .expect("allowed")
            .is_empty()
    );
    let result = run(LIST, table.set_cell(LIST, 1, 0, "Thema"));
    assert_eq!(result, LIST.replace("{span=2} Theme »ß«", "{span=2} Thema"));
}

#[test]
fn list_rows_add_remove_and_move() {
    let text = "::: table\n- - a\n  - b\n- - 1\n  - 2\n:::\n";
    let table = grid(text);
    assert_eq!(
        run(text, table.add_row(text, 0)),
        "::: table\n- - a\n  - b\n- - \n  - \n- - 1\n  - 2\n:::\n"
    );
    assert_eq!(run(text, table.remove_row(text, 1)), "::: table\n- - a\n  - b\n:::\n");
    assert_eq!(
        run(text, table.move_row_down(text, 0)),
        "::: table\n- - 1\n  - 2\n- - a\n  - b\n:::\n"
    );
}

#[test]
fn list_columns_keep_the_directive_lists_in_step() {
    let text = "::: table {align=\"left right\" widths=\"* auto\"}\n- - a\n  - b\n- - 1\n  - 2\n:::\n";
    let table = grid(text);
    assert_eq!(
        run(text, table.add_column(text, Some(0))),
        "::: table {align=\"left left right\" widths=\"* auto auto\"}\n- - a\n  - \n  - b\n- - 1\n  - \n  - 2\n:::\n"
    );
    assert_eq!(
        run(text, table.remove_column(text, 0)),
        "::: table {align=\"right\" widths=\"auto\"}\n- - b\n- - 2\n:::\n"
    );
    assert_eq!(
        run(text, table.move_column_right(text, 0)),
        "::: table {align=\"right left\" widths=\"auto *\"}\n- - b\n  - a\n- - 2\n  - 1\n:::\n"
    );
    assert_eq!(
        run(text, table.set_align(text, 0, Align::Center)),
        "::: table {align=\"center right\" widths=\"* auto\"}\n- - a\n  - b\n- - 1\n  - 2\n:::\n"
    );
}

#[test]
fn spans_merge_an_empty_neighbour_and_give_it_back() {
    let text = "::: table\n- - a\n  - b\n- - 1\n  - \n:::\n";
    let table = grid(text);
    let merged = run(text, table.merge_right(text, 1, 0));
    assert_eq!(merged, "::: table\n- - a\n  - b\n- - {span=2} 1\n:::\n");
    assert_eq!(table.merge_right(text, 0, 0), Err(Key::TableMergeNotEmpty));
    let again = grid(&merged);
    assert_eq!(again.rows[1][0].span, 2);
    assert_eq!(
        run(&merged, again.split(&merged, 1, 0)),
        "::: table\n- - a\n  - b\n- - 1\n  - \n:::\n"
    );
    assert_eq!(table.split(text, 0, 0), Err(Key::TableNoSpan));
    let piped = "| a |\n|---|\n";
    assert_eq!(grid(piped).merge_right(piped, 0, 0), Err(Key::TablePipeNoSpans));
}

#[test]
fn a_new_column_inside_a_span_widens_it() {
    let text = "::: table\n- - a\n  - b\n- - {span=2} 1\n:::\n";
    let table = grid(text);
    assert_eq!(
        run(text, table.add_column(text, Some(0))),
        "::: table\n- - a\n  - \n  - b\n- - {span=3} 1\n:::\n"
    );
    assert_eq!(table.move_column_right(text, 0), Err(Key::TableSpanInTheWay));
}

#[test]
fn what_the_grid_cannot_show_says_why() {
    for text in [
        "::: table\nA paragraph.\n:::\n",
        "::: table\n- text outside\n:::\n",
        "::: table\n- - a\n  - {bad} b\n:::\n",
    ] {
        let blocks = Blocks::parse(text);
        let regions = regions(text, &blocks);
        assert!(regions[0].grid.is_err(), "{text}");
    }
}

#[test]
fn a_pipe_table_in_a_directive_keeps_it_and_its_caption_follows_the_close() {
    let text = "::: table {placement=top}\n| a |\n|---|\n:::\n\nNext.\n";
    let table = grid(text);
    assert!(!table.is_list());
    let caption = Caption {
        text: "C".to_owned(),
        ..Caption::default()
    };
    let edit = table.set_caption(text, &caption).expect("an edit");
    assert_eq!(
        applied(text, &[edit]),
        "::: table {placement=top}\n| a |\n|---|\n:::\n\n: C\n\nNext.\n"
    );
}

#[test]
fn line_endings_stay_as_written() {
    let text = "| a |\r\n|---|\r\n| 1 |\r\n";
    let table = grid(text);
    assert_eq!(
        run(text, table.add_row(text, 1)),
        "| a |\r\n|---|\r\n| 1 |\r\n|   |\r\n"
    );
}
