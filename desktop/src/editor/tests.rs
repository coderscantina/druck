//! The editor's own pure parts: where a chapter opens, where Command+Up and Down go, and which blocks stay on screen.

use super::*;

fn indices(visible: &[(usize, Pixels)]) -> Vec<usize> {
    visible.iter().map(|(index, _)| *index).collect()
}

#[test]
fn a_chapter_opens_where_the_session_put_its_caret() {
    let view = |head: usize| ChapterView {
        selection: Selection::caret(head),
        scroll: None,
    };
    // A new project's caret after its title, which `{#sec:intro}` closes.
    let text = "# Einleitung {#sec:intro}\n\nText.\n";
    let title = "# Einleitung {#sec:intro}".len();
    let blocks = Blocks::parse(text);
    assert_eq!(
        opening_selection(text, &blocks, view(title), false),
        Selection::caret(title)
    );
    // In the galley a caret inside the notes above the first text moves after them and the title's hanging marker, and
    // the source view keeps it.
    let text = "<!-- note -->\n\n# T\n";
    let blocks = Blocks::parse(text);
    assert_eq!(opening_selection(text, &blocks, view(0), false), Selection::caret(17));
    assert_eq!(opening_selection(text, &blocks, view(0), true), Selection::caret(0));
    // A kept place past the end of a shorter text is clamped.
    assert_eq!(
        opening_selection(text, &blocks, view(99), false),
        Selection::caret(text.len())
    );
}

#[test]
fn a_chapter_opens_with_the_caret_in_its_title_text() {
    let view = ChapterView {
        selection: Selection::caret(0),
        scroll: None,
    };
    let text = "## Leben\n\nText.\n";
    let blocks = Blocks::parse(text);
    let caret = opening_selection(text, &blocks, view, false).head;
    assert_eq!(caret, "## ".len());
    // A letter typed there goes into the title, which stays a heading.
    let typed = format!("{}X{}", &text[..caret], &text[caret..]);
    let blocks = Blocks::parse(&typed);
    assert_eq!(blocks.as_slice()[0].kind, BlockKind::Heading { level: 2 });
    assert!(typed.starts_with("## XLeben"));
}

#[test]
fn command_up_and_down_stop_at_the_text_and_leave_folded_notes_alone() {
    let text = "<!-- opening note -->\n\n# Title\n\nLast words.\n\n<!-- closing note -->\n";
    let blocks = Blocks::parse(text);
    let edge = |end: bool| selection::chapter_edge(text, &blocks, end, |_| false);
    assert_eq!(edge(false), text.find("# Title").unwrap());
    assert_eq!(edge(true), text.find("Last words.").unwrap() + "Last words.".len());
    // With no closing note the end stays the text's end.
    let text = "# Title\n\nLast words.\n";
    let blocks = Blocks::parse(text);
    assert_eq!(selection::chapter_edge(text, &blocks, true, |_| false), text.len());
    assert_eq!(selection::chapter_edge(text, &blocks, false, |_| false), 0);
}

#[test]
fn blocks_on_screen_never_point_past_the_end_after_an_edit() {
    let on_screen =
        |from: usize, to: usize| -> Vec<(usize, Pixels)> { (from..to).map(|index| (index, px(0.))).collect() };
    // Backspace at the end of a chapter merges the last two blocks of 134.
    let mut visible = on_screen(125, 134);
    follow_blocks(
        &mut visible,
        BlockChange {
            start: 132,
            removed: 2,
            inserted: 1,
        },
    );
    assert_eq!(indices(&visible), (125..132).collect::<Vec<_>>());
    // Select all and a key: everything after the opening note became one block, leaving 3.
    let mut visible = on_screen(140, 150);
    follow_blocks(
        &mut visible,
        BlockChange {
            start: 2,
            removed: 169,
            inserted: 1,
        },
    );
    assert!(visible.is_empty());
    // Blocks after the change keep pointing at the same blocks.
    let mut visible = on_screen(4, 8);
    follow_blocks(
        &mut visible,
        BlockChange {
            start: 1,
            removed: 3,
            inserted: 1,
        },
    );
    assert_eq!(indices(&visible), [2, 3, 4, 5]);
    let mut visible = on_screen(4, 8);
    follow_blocks(
        &mut visible,
        BlockChange {
            start: 5,
            removed: 1,
            inserted: 3,
        },
    );
    assert_eq!(indices(&visible), [4, 8, 9]);
}
