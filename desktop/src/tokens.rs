//! The one table of colors, spacing, and type for app chrome and the writing surface. Views read values from here and
//! never hardcode them. Publish's pages use the theme's own values instead.

use clap::ValueEnum;
use gpui::{App, BoxShadow, Global, Pixels, Rgba, WindowAppearance, hsla, px, rgb};

use crate::preview::job::{BookType, Face};

pub const FONT_UI: &str = ".SystemUIFont";
/// For code and diffs.
pub const FONT_MONO: &str = "Menlo";

pub const TEXT_SMALL: Pixels = px(11.);
pub const TEXT_BODY: Pixels = px(13.);

pub const SPACE_1: Pixels = px(4.);
pub const SPACE_2: Pixels = px(8.);
pub const SPACE_3: Pixels = px(12.);
pub const SPACE_4: Pixels = px(16.);

/// The smallest the window gets, which every view is laid out to work at.
pub const WINDOW_MIN_SIZE: (f32, f32) = (720., 480.);

pub const TITLEBAR_HEIGHT: Pixels = px(28.);
pub const SECTION_HEIGHT: Pixels = px(30.);
/// The Binder.
pub const SIDEBAR_WIDTH: Pixels = px(216.);
pub const INSPECTOR_WIDTH: Pixels = px(296.);
pub const RAIL_WIDTH: Pixels = px(40.);
pub const ICON_SIZE: Pixels = px(16.);
pub const ROW_HEIGHT: Pixels = px(36.);
/// A row of the sidebar's contents list.
pub const LIST_ROW_HEIGHT: Pixels = px(30.);
/// The number or icon column of a contents row.
pub const LIST_LEAD_WIDTH: Pixels = px(24.);
/// The slot at the end of a contents row that holds its dot.
pub const LIST_DOT_WIDTH: Pixels = px(18.);
pub const DOT_SIZE: Pixels = px(6.);
pub const COVER_THUMB_HEIGHT: Pixels = px(56.);
pub const COVER_THUMB_WIDTH: Pixels = px(38.);
pub const COVER_PREVIEW_HEIGHT: Pixels = px(96.);
pub const COVER_PREVIEW_WIDTH: Pixels = px(64.);
pub const FOOTER_HEIGHT: Pixels = px(30.);
pub const DIALOG_WIDTH: Pixels = px(380.);
pub const DIALOG_LIST_HEIGHT: Pixels = px(440.);
pub const LAUNCH_WIDTH: Pixels = px(440.);
pub const TEXT_TITLE: Pixels = px(20.);
pub const FIELD_HEIGHT: Pixels = px(30.);
pub const RADIUS: Pixels = px(6.);
/// The card that shows the other side of a conflict, over the conflict line.
pub const CONFLICT_CARD_WIDTH: Pixels = px(420.);
/// The floating toolbar and its buttons.
pub const TOOLBAR_HEIGHT: Pixels = px(36.);
pub const TOOLBAR_BUTTON: Pixels = px(28.);
pub const TOOLBAR_SEPARATOR: Pixels = px(9.);
/// A list that floats over the galley: the slash menu and a block's convert menu.
pub const MENU_WIDTH: Pixels = px(248.);
pub const MENU_ROW_HEIGHT: Pixels = px(28.);
pub const MENU_MAX_ROWS: usize = 10;
pub const LINK_POPUP_WIDTH: Pixels = px(320.);
/// Room kept between something floating and what it belongs to, or the window's edge.
pub const FLOAT_GAP: Pixels = px(6.);
pub const SCROLLBAR_WIDTH: Pixels = px(10.);
pub const SCROLLBAR_THUMB_MIN: Pixels = px(28.);
/// The block gutter, left of the text.
pub const GUTTER_WIDTH: Pixels = px(52.);
/// Above a chapter's first line and below its last.
pub const TEXT_MARGIN_Y: Pixels = px(52.);
/// The scroll offset of a chapter's first view, with that margin above it.
pub const TEXT_START: Pixels = px(-52.);
/// The least room kept right of the text.
pub const TEXT_MARGIN_RIGHT: Pixels = px(24.);
/// The least width the text keeps, about 40 characters, before the Binder gives way.
pub const TEXT_MIN_WIDTH: Pixels = px(320.);
/// A margin note, and the gap between it and the text.
/// How long a passing note stays at the window's foot.
pub const NOTE_TIME: std::time::Duration = std::time::Duration::from_secs(6);
pub const NOTE_WIDTH: Pixels = px(220.);
pub const NOTE_GAP: Pixels = px(28.);
pub const NOTE_LINE_HEIGHT: Pixels = px(18.);
pub const NOTE_TEXT: Pixels = px(12.);
pub const NOTE_ICON: Pixels = px(12.);
pub const NOTE_RULE: Pixels = px(1.);
/// The buttons of the agent's cards and of a passage under review.
pub const SMALL_BUTTON_HEIGHT: Pixels = px(22.);
/// The room under a passage that holds its buttons, and how far below the text they stand.
pub const REVIEW_ROW_HEIGHT: Pixels = px(36.);
pub const REVIEW_ROW_GAP: Pixels = px(6.);
/// The tints behind words an agent changed.
pub const REVIEW_TINT: f32 = 0.22;
/// The one-line card under a selection, and the small chat card over the text.
pub const PROMPT_CARD_WIDTH: Pixels = px(420.);
pub const PROMPT_CARD_HEIGHT: Pixels = px(34.);
pub const CHAT_CARD_WIDTH: Pixels = px(300.);
pub const CHAT_CARD_MAX_HEIGHT: Pixels = px(520.);
/// The caret's width in the text.
pub const CARET_WIDTH: Pixels = px(1.5);
/// A line of the source view against the body size, and its line height.
pub const SOURCE_SCALE: f32 = 0.85;
pub const SOURCE_LEADING: f32 = 1.5;
/// A character's width against the body size, for estimating what is not shaped yet and for a line length in characters.
pub const CHAR_WIDTH: f32 = 0.47;
/// The room kept between paragraphs a theme neither spaces nor indents, against the body size.
pub const PARAGRAPH_GAP: f32 = 0.7;
/// Logical pixels per point of a theme's sizes at 100 percent zoom. Write is for reading on screen, larger than the page.
pub const POINT: f32 = 1.4;
/// How other paragraphs show when they are dimmed.
pub const DIM_OPACITY: f32 = 0.32;
/// How far down the pane the caret's line stands with typewriter scrolling.
pub const TYPEWRITER_AT: f32 = 0.45;
/// The zoom steps of Write. Pages have their own, see `preview`.
pub const WRITE_ZOOMS: [f32; 8] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
/// Where Write's zoom starts until the author picks another, and where `Cmd+0` returns: body text of about 18 px.
pub const WRITE_ZOOM_START: f32 = 1.25;
/// A heading's tick at the left edge: its width by level, the height, the step between two, and the strip they stand in.
pub const TICK_WIDTHS: [Pixels; 3] = [px(16.), px(12.), px(8.)];
pub const TICK_HEIGHT: Pixels = px(1.5);
pub const TICK_STEP: Pixels = px(8.);
pub const TICK_LEFT: Pixels = px(12.);
pub const TICK_ZONE_WIDTH: Pixels = px(36.);
pub const TICK_OPACITY: f32 = 0.65;
/// How far the pointer's reach is along the strip, and how much wider the tick under it grows, against its width.
pub const TICK_REACH: Pixels = px(48.);
pub const TICK_GROWTH: f32 = 2.2;
/// The outline a resting pointer unfolds from the ticks: its row, its width, how far a level moves a heading in, and
/// how much of the text shows through the veil laid over it.
pub const OUTLINE_ROW_HEIGHT: Pixels = px(20.);
pub const OUTLINE_WIDTH: Pixels = px(260.);
pub const OUTLINE_INDENT: Pixels = px(12.);
pub const OUTLINE_TEXT_OPACITY: f32 = 0.62;
/// Motion. One spring for what follows the pointer, the ticks, the outline's lit row, and the rail, so they move alike:
/// quick, settled in about 250 ms, with no visible overshoot.
pub const SPRING: gpui::SpringConfig = gpui::SpringConfig::new(420., 38., 1.);
/// How long something fades in or out as it shows or goes: the outline, a tick's title, the rail's tick.
pub const FADE: std::time::Duration = std::time::Duration::from_millis(140);
/// How far the unfolding outline slides in from the left as it fades in.
pub const OUTLINE_SLIDE: Pixels = px(6.);
/// How long the pointer may be off the outline or the rail before it folds or leaves, so a slip of the hand does not
/// close it.
pub const LEAVE_GRACE: std::time::Duration = std::time::Duration::from_millis(220);
/// A jump to a heading: where down the pane its line lands, how long the view glides one screen, and how many screens it
/// glides at most, so a long jump does not race past pages of text: beyond that it starts this far from its target.
pub const HEADING_LANDING: f32 = 0.16;
pub const GLIDE: std::time::Duration = std::time::Duration::from_millis(260);
pub const GLIDE_REACH: f32 = 8.;
/// The bookshelf. A spine is as wide as its chapter is long: this much plus [`SPINE_WORD_WIDTH`] per word, and never
/// under [`SPINE_MIN_WIDTH`], which keeps its number legible and lets 40 chapters stand in the narrowest window. A spine
/// stops growing at [`SPINE_MAX_WIDTH`]. Too many words for the window shrink every spine's share of words alike.
pub const SPINE_MIN_WIDTH: f32 = 14.;
pub const SPINE_WORD_WIDTH: f32 = 0.014;
pub const SPINE_MAX_WIDTH: f32 = 120.;
/// Between spines of one group, and between groups.
pub const SPINE_GAP: f32 = 2.;
pub const SHELF_GROUP_GAP: f32 = 16.;
/// The least width of the group of files not in the book, which its label "Not in book" needs.
pub const SHELF_LOOSE_MIN_WIDTH: f32 = 84.;
/// The room after the last chapter of the book that takes a dropped spine to the end.
pub const SHELF_END_SLOT: f32 = 12.;
pub const SPINE_HEIGHT: Pixels = px(72.);
/// How far the open chapter's spine stands above the baseline, and a hovered one.
pub const SPINE_LIFT_OPEN: Pixels = px(6.);
pub const SPINE_LIFT_HOVER: Pixels = px(3.);
/// A spine needs this much width to show its title beside its number.
pub const SPINE_TITLE_MIN_WIDTH: f32 = 64.;
/// The room under the baseline for a part's label.
pub const SHELF_LABEL_HEIGHT: Pixels = px(20.);
/// The raised shelf: its spines, labels, and lift, then a fade into the text above.
pub const SHELF_HEIGHT: Pixels = px(116.);
pub const SHELF_FADE_HEIGHT: Pixels = px(28.);
/// The strip at the window's bottom edge where the pointer raises the shelf.
pub const SHELF_EDGE_HEIGHT: Pixels = px(8.);
/// The trace at rest: a hairline per chapter, as wide as its spine, above the window's edge.
pub const TRACE_HEIGHT: Pixels = px(2.);
pub const TRACE_BOTTOM: Pixels = px(3.);
pub const TRACE_OPACITY: f32 = 0.5;
pub const TRACE_OPEN_OPACITY: f32 = 0.7;
/// The baseline the spines stand on.
pub const SHELF_RULE_OPACITY: f32 = 0.4;
/// The hover card over a spine.
pub const SPINE_CARD_WIDTH: Pixels = px(204.);
/// The command palette: one card at the top centre of the window.
pub const PALETTE_WIDTH: Pixels = px(480.);
pub const PALETTE_TOP: Pixels = px(72.);
pub const PALETTE_MAX_ROWS: usize = 12;
/// The settings window.
pub const SETTINGS_WINDOW: (f32, f32) = (860., 620.);
pub const SETTINGS_NAV_WIDTH: Pixels = px(190.);
pub const SETTINGS_SWATCH: Pixels = px(17.);
pub const SETTINGS_SWATCH_RADIUS: Pixels = px(4.);
pub const SETTINGS_SWATCH_GAP: Pixels = px(5.);
pub const SETTINGS_ROW_PADDING: Pixels = px(10.);
pub const SETTINGS_HEX_WIDTH: Pixels = px(78.);
pub const SWITCH_WIDTH: Pixels = px(30.);
pub const SWITCH_HEIGHT: Pixels = px(17.);
pub const SWITCH_KNOB: Pixels = px(13.);
pub const SWITCH_RADIUS: Pixels = px(4.);
pub const SWITCH_KNOB_RADIUS: Pixels = px(3.);
/// Where the window controls stand in a window with its own title bar.
pub const TRAFFIC_LIGHTS: (f32, f32) = (9., 7.);
/// The thin line around a page in Publish, which a white page on a light surface needs.
pub const PAGE_EDGE: Pixels = px(1.);
/// The gap at a spread's gutter, and around sheets on a contact sheet.
pub const SPREAD_GUTTER: Pixels = px(2.);
pub const CONTACT_GAP: Pixels = px(10.);
/// A dot on a page of a contact sheet that has a problem, in from the page's corner.
pub const CONTACT_DOT_INSET: Pixels = px(5.);
/// How far a mark stands off the text it marks, in points of the page.
pub const MARK_PADDING: f32 = 2.;
/// The faint tint of a marked paragraph, and of the problem being looked at.
pub const MARK_TINT: f32 = 0.08;
pub const MARK_TINT_CURRENT: f32 = 0.2;
/// A typesetting warning's mark on a page, which is white in either appearance. Not the accent: a warning is not an error.
pub const WARNING_MARK: u32 = 0x8a8a8a;
/// The proof slip: its width, its text's size, and how tall the text gets before it scrolls.
pub const SLIP_WIDTH: Pixels = px(380.);
pub const SLIP_TEXT: Pixels = px(14.);
pub const SLIP_TEXT_MAX_HEIGHT: Pixels = px(260.);
/// The least room the slip needs under its paragraph. With less, and more above, it stands above.
pub const SLIP_ROOM_BELOW: Pixels = px(240.);
/// The slip's text while its list of styles is open, so the card stays small.
pub const SLIP_TEXT_SHORT_HEIGHT: Pixels = px(60.);
pub const SLIP_STYLES_MAX_HEIGHT: Pixels = px(168.);
/// What stands in place of the pages when a document has none.
pub const EMPTY_STATE_WIDTH: Pixels = px(440.);
/// The status line's zoom buttons.
pub const ZOOM_BUTTON: Pixels = px(20.);
/// The commit card and the rows and buttons the versions views share.
pub const COMMIT_CARD_WIDTH: Pixels = px(420.);
pub const COMMIT_CARD_MAX_ROWS: usize = 12;
pub const VERSIONS_ROW_HEIGHT: Pixels = px(24.);
pub const VERSIONS_BUTTON_HEIGHT: Pixels = px(22.);
/// The angle of a fade that darkens toward the foot of a list.
pub const FADE_DOWN: f32 = 180.;
/// The timeline's strip: commits as tall marks, local history as small ticks, and the position.
pub const STRIP_HEIGHT: Pixels = px(50.);
pub const STRIP_COMMIT_WIDTH: Pixels = px(2.);
pub const STRIP_COMMIT_HEIGHT: Pixels = px(23.);
pub const STRIP_TICK_WIDTH: Pixels = px(1.5);
pub const STRIP_TICK_HEIGHT: Pixels = px(7.);
pub const STRIP_POSITION_HEIGHT: Pixels = px(27.);
/// How strongly a word difference tints the text behind it.
pub const DIFF_TINT: f32 = 0.24;
/// A gap of two pixels, between things that belong together.
pub const HAIR_GAP: Pixels = px(2.);
/// A thin line: an underline, a rule, a caret.
pub const HAIRLINE: Pixels = px(1.);
/// A chip's corners, and a tick box's.
pub const CHIP_RADIUS: Pixels = px(4.);
pub const BOX_RADIUS: Pixels = px(3.);
/// A tick box in a list, a chip's close button and its icon.
pub const CHECK_SIZE: Pixels = px(14.);
pub const CHECK_SMALL: Pixels = px(12.);
/// How far a check's explanation stands in, past its box.
pub const CHECK_INDENT: Pixels = px(22.);
pub const CHIP_CLOSE: Pixels = px(16.);
pub const CHIP_CLOSE_ICON: Pixels = px(10.);
/// A dot's place on a rail tab, and on the mode switch.
pub const RAIL_DOT_INSET: Pixels = px(10.);
pub const SWITCH_DOT_INSET: Pixels = px(1.);
/// The bar beside a row that has the keys, and a dragged block's drop line.
pub const KEY_BAR: Pixels = px(2.);
/// Export's progress segments.
pub const PROGRESS_HEIGHT: Pixels = px(3.);
pub const PROGRESS_RADIUS: Pixels = px(1.5);
/// The proof list.
pub const PROOF_HEADING_HEIGHT: Pixels = px(28.);
pub const PROOF_DOT_TOP: Pixels = px(5.);
/// The project search card, and the line number beside a hit.
pub const SEARCH_CARD_WIDTH: Pixels = px(440.);
/// The column a commit's leading symbol, such as a gitmoji, stands in on every history row.
pub const COMMIT_SYMBOL_WIDTH: Pixels = px(18.);
/// The search card narrows to this in a small window, so the text keeps its least width beside it.
pub const SEARCH_CARD_MIN_WIDTH: Pixels = px(280.);
pub const SEARCH_HIT_Y: Pixels = px(3.);
pub const SEARCH_LINE_WIDTH: Pixels = px(34.);
/// The find bar, and its count of hits.
pub const FIND_BAR_WIDTH: Pixels = px(380.);
pub const FIND_COUNT_WIDTH: Pixels = px(64.);
/// The agent's panel. The diff height is the tallest a diff in an approval grows before it scrolls.
pub const AGENT_DIFF_HEIGHT: Pixels = px(180.);
pub const AGENT_LINE_NUMBER: Pixels = px(22.);
pub const AGENT_LIST_MARK: Pixels = px(14.);
pub const AGENT_HEADING_1: Pixels = px(15.);
pub const AGENT_HEADING_2: Pixels = px(14.);
pub const AGENT_PARAGRAPH_Y: Pixels = px(4.);
pub const PROMPT_MAX_HEIGHT: Pixels = px(160.);
pub const PROMPT_MIN_HEIGHT: Pixels = px(54.);
pub const PROMPT_Y: Pixels = px(6.);
/// The block gutter's icons, and the space between a comment marker and the text.
pub const GUTTER_ICON: Pixels = px(14.);
pub const COMMENT_ICON: Pixels = px(12.);
pub const COMMENT_MARKER_INSET: Pixels = px(20.);
pub const GUTTER_ICON_INSET: Pixels = px(14.);
/// The cards over the text: a figure's, a citation's, and a footnote's, and the citation card's rows.
pub const FIGURE_CARD_WIDTH: Pixels = px(360.);
pub const PICKER_CARD_WIDTH: Pixels = px(400.);
pub const PICKER_ROW_HEIGHT: Pixels = px(40.);
pub const NOTE_CARD_WIDTH: Pixels = px(400.);
pub const IMAGE_THUMB: Pixels = px(44.);
/// A table in the text and in its editing grid.
pub const CELL_X: Pixels = px(6.);
pub const CELL_Y: Pixels = px(4.);
pub const CAPTION_GAP: Pixels = px(6.);
pub const GRID_FIELD_MIN: Pixels = px(160.);
pub const GRID_LABEL_WIDTH: Pixels = px(120.);
pub const GRID_ROW_Y: Pixels = px(6.);

/// The keyboard's focus ring.
pub const FOCUS_RING: Pixels = px(2.);
/// What a text field's caret keeps from its right edge, and the width a field reckons with before it has been drawn.
pub const INPUT_CARET_ROOM: Pixels = px(4.);
pub const INPUT_MEASURE_DEFAULT: Pixels = px(200.);
/// How close to the end a conversation counts as read to the end, so it follows what comes in.
pub const FOLLOW_ROOM: Pixels = px(32.);
/// How far a line of a mouse wheel scrolls.
pub const SCROLL_LINE: Pixels = px(20.);
/// How far a code block's background stands out from the text, and a quotation's rule.
pub const CODE_OUTDENT: Pixels = px(6.);
pub const QUOTE_RULE_X: Pixels = px(2.);
pub const QUOTE_RULE_WIDTH: Pixels = px(2.);
/// The narrowest a selected stretch in a note draws, to stay visible.
pub const NOTE_SELECTION_MIN: Pixels = px(4.);
/// A source line's width before anything is known, and the least a wrapped line is given.
pub const SOURCE_LINE_FALLBACK: Pixels = px(20.);
pub const WRAP_MIN: Pixels = px(40.);
/// The borders on both sides of a card, for a card's outer size.
pub const CARD_BORDERS: Pixels = px(2.);
/// How far around a comment marker a click still lands on it.
pub const MARKER_HIT: Pixels = px(3.);
/// How much of the view Page Up and Page Down keep.
pub const PAGE_KEY_OVERLAP: Pixels = px(48.);
/// The fastest a drag past the pane's edge scrolls in one tick, and how far the thumb stands in from its track.
pub const DRAG_SCROLL_MAX: Pixels = px(60.);
pub const SCROLLBAR_INSET: Pixels = px(2.);
/// The thin rule between a table's rows, a cell's least width, and a missing figure's box.
pub const TABLE_RULE: Pixels = px(0.5);
pub const CELL_MIN_WIDTH: Pixels = px(16.);
pub const FIGURE_MISSING_HEIGHT: Pixels = px(48.);
/// What the editing grid keeps free above the window's foot.
pub const GRID_MIN_ROOM: Pixels = px(120.);
/// How strongly the accent tints a selection, and a prompt's.
pub const SELECTION_TINT: f32 = 0.25;
/// How strongly the accent tints a find or search match that is not the selection.
pub const MATCH_TINT: f32 = 0.12;
pub const PROMPT_SELECTION_TINT: f32 = 0.3;
/// A misspelled word's line.
pub const MISSPELLED_TINT: f32 = 0.6;
/// A chip's tint, and that of a chip the book cannot resolve.
pub const CHIP_TINT: f32 = 0.12;
pub const CHIP_ERROR_TINT: f32 = 0.14;
/// Behind a placeholder's value shown as a chip, and behind a todo, in the text color.
pub const PLACEHOLDER_TINT: f32 = 0.08;
pub const NOTE_TINT: f32 = 0.05;
/// The dotted line under a placeholder's value: a dot of a hairline every this many pixels.
pub const DOT_STEP: Pixels = px(3.);
/// The card that completes a placeholder: its list of names, the chosen name's side, and its height.
pub const COMPLETION_LIST_WIDTH: Pixels = px(262.);
pub const COMPLETION_DETAIL_WIDTH: Pixels = px(240.);
pub const COMPLETION_HEIGHT: Pixels = px(300.);
/// The least it shrinks to in a short window.
pub const COMPLETION_MIN_HEIGHT: Pixels = px(160.);
/// A group's label in that list, and the widest a value at a row's end gets.
pub const COMPLETION_GROUP_HEIGHT: Pixels = px(22.);
pub const COMPLETION_VALUE_WIDTH: Pixels = px(110.);
/// The chosen name's value, large, in the book's type.
pub const COMPLETION_VALUE_SIZE: Pixels = px(22.);
/// Between a block's style name, far out in the left margin, and where the block gutter's icon stands.
pub const STYLE_NAME_GAP: Pixels = px(8.);
/// The scrollbar's thumb, at rest and under the pointer.
pub const THUMB_FAINT: f32 = 0.4;
pub const THUMB_STRONG: f32 = 0.8;
/// The gutter's other icons, a figure still loading, and the prose in a page's small drawing.
pub const GUTTER_REST: f32 = 0.55;
pub const FIGURE_LOADING_TINT: f32 = 0.25;
pub const SKETCH_OPACITY: f32 = 0.45;
/// A word's tint and a row's in an approval's diff.
pub const DIFF_WORD_TINT: f32 = 0.35;
pub const DIFF_ROW_TINT: f32 = 0.14;
/// The start of a fade.
pub const CLEAR: f32 = 0.;

/// The path bar, its columns, and the overview. The path starts after the traffic lights; a segment has room for at least
/// an ellipsis.
pub const NAV_PATH_LEFT: Pixels = px(76.);
pub const NAV_SEGMENT_X: Pixels = px(6.);
pub const NAV_SEGMENT_LEAST: Pixels = px(24.);
pub const NAV_CHEVRON: Pixels = px(10.);
/// The columns' card, its columns, and its foot. A column's rows are `MENU_ROW_HEIGHT`; a deeper heading is indented.
pub const NAV_CARD_LEFT: Pixels = px(64.);
pub const NAV_PARTS_WIDTH: Pixels = px(216.);
pub const NAV_CHAPTERS_WIDTH: Pixels = px(196.);
pub const NAV_HEADINGS_WIDTH: Pixels = px(228.);
pub const NAV_OPENING_WIDTH: Pixels = px(232.);
pub const NAV_COLUMN_Y: Pixels = px(6.);
pub const NAV_INDENT: Pixels = px(14.);
pub const NAV_MARK: Pixels = px(6.);
pub const NAV_ICON: Pixels = px(10.);
pub const NAV_FOOT_HEIGHT: Pixels = px(30.);
/// The chosen file's opening: its title, and its paragraphs in the book face, cut after a few lines.
pub const NAV_OPENING_Y: Pixels = px(14.);
pub const NAV_OPENING_TITLE: Pixels = px(16.);
pub const NAV_OPENING_LEADING: Pixels = px(19.5);
pub const NAV_OPENING_LINES: usize = 4;
/// How strongly the raised grey marks the path's choice in a column that does not have the keys.
pub const NAV_PATH_TINT: f32 = 0.55;
/// The overview: its margins, narrower below `NAV_DESK_WIDE`, the label column, and the entries, at most
/// `NAV_DESK_COLUMNS` to a line and none narrower than `NAV_DESK_CARD_MIN`.
pub const NAV_DESK_WIDE: Pixels = px(960.);
pub const NAV_DESK_MARGIN: Pixels = px(48.);
pub const NAV_DESK_TOP: Pixels = px(36.);
pub const NAV_DESK_LABEL: Pixels = px(140.);
pub const NAV_DESK_COLUMNS: usize = 4;
pub const NAV_DESK_CARD_MIN: Pixels = px(176.);
pub const NAV_DESK_CARD_HEIGHT: Pixels = px(92.);
pub const NAV_DESK_CARD_X: Pixels = px(10.);
pub const NAV_DESK_CARD_Y: Pixels = px(9.);
pub const NAV_DESK_NUMBER_GAP: Pixels = px(6.);
pub const NAV_DESK_TITLE: Pixels = px(24.);
pub const NAV_DESK_LABEL_TEXT: Pixels = px(14.);
pub const NAV_DESK_CARD_TITLE: Pixels = px(15.);
pub const NAV_DESK_FIRST_LINE: Pixels = px(12.5);
pub const NAV_DESK_TITLE_LEADING: Pixels = px(19.);
pub const NAV_DESK_FIRST_LINE_LEADING: Pixels = px(17.);
/// Layout directives in Write. Brackets stand in the gutter's left strip, left of its type icons, one step further
/// right per directive they stand in, and their names end a gap left of the outermost.
pub const BRACKET_INSET: Pixels = px(2.);
pub const BRACKET_STEP: Pixels = px(4.);
pub const BRACKET_TICK: Pixels = px(3.);
/// The deepest step a bracket takes; deeper ones share it.
pub const BRACKET_DEPTHS: usize = 3;
pub const BRACKET_NAME_GAP: Pixels = px(6.);
/// How far a bracket's ends stay inside its text's first and last lines.
pub const BRACKET_END_INSET: Pixels = px(3.);
/// Each of two columns holds at least this many characters of body text, or the text shows one column.
pub const COLUMN_MIN_CHARACTERS: u32 = 24;
/// A new page's gap between two sheet edges, in body lines, and the tint between the edges.
pub const PAGE_GAP_LINES: f32 = 2.5;
pub const SHEET_GAP_TINT: f32 = 0.04;
/// A directive's options under its name, and their small segmented choice.
pub const OPTIONS_WIDTH: Pixels = px(262.);
pub const SEGMENT_HEIGHT: Pixels = px(18.);
pub const SEGMENT_RADIUS: Pixels = px(3.);
/// A directive the slash menu would insert, drawn into the text before it is.
pub const GHOST_OPACITY: f32 = 0.42;

/// The paint order of a card that asks a question, above the proof slip (1) and the text's floats (2).
pub const LAYER_QUESTION: usize = 3;

/// The window's regions, in the order Tab walks them.
pub const TAB_TITLEBAR: isize = 0;
pub const TAB_PANEL: isize = 1;
pub const TAB_RAIL: isize = 2;
pub const TAB_CENTRE: isize = 3;
pub const TAB_RIGHT: isize = 4;
/// A control that cannot be used right now.
pub const DISABLED_OPACITY: f32 = 0.4;

/// Pins in the right margin: a rule for one, shorter rules stacked for several, at most this many, standing this far
/// below the paragraph's first line's top, and the count beside them while they are folded.
pub const PIN_RULE: Pixels = px(14.);
pub const PIN_RULE_SHORT: Pixels = px(8.);
pub const PIN_RULE_GAP: Pixels = px(3.);
pub const PIN_RULES_SHOWN: usize = 4;
pub const PIN_RULE_TOP: Pixels = px(7.);
pub const PIN_COUNT_TEXT: Pixels = px(10.);
/// The open stack of pins: its header and foot, its line, and the excerpt in the book's face, cut to a few lines.
pub const PIN_HEADER_HEIGHT: Pixels = px(28.);
pub const PIN_LINE_HEIGHT: Pixels = px(16.);
pub const PIN_EXCERPT_TEXT: Pixels = px(13.);
pub const PIN_EXCERPT_LINE: Pixels = px(19.);
pub const PIN_EXCERPT_LINES: usize = 6;
/// The material panel in Write, wider than Publish's for reading, and how narrow it gets before it gives way.
pub const MATERIAL_PANEL_WIDTH: Pixels = px(336.);
pub const MATERIAL_PANEL_MIN_WIDTH: Pixels = px(280.);
/// The faint tick at Write's right edge, the strip the pointer rests on to open the rail, and how long it rests. The
/// strip stands left of the text's scrollbar, so a drag on the scrollbar never opens the rail.
pub const MATERIAL_TICK: (Pixels, Pixels) = (px(3.), px(32.));
pub const MATERIAL_TICK_INSET: Pixels = px(4.);
pub const MATERIAL_TICK_OPACITY: f32 = 0.45;
pub const MATERIAL_EDGE: Pixels = px(24.);
/// How tall the band around the tick is that the pointer rests on, and how the tick grows while it is there.
pub const MATERIAL_EDGE_BAND: Pixels = px(120.);
pub const MATERIAL_TICK_LIT: (Pixels, f32) = (px(48.), 0.9);
pub const MATERIAL_REST: std::time::Duration = std::time::Duration::from_millis(350);
/// The trail's header, and the longest an earlier place's name stands before it is cut.
pub const MATERIAL_TRAIL_HEIGHT: Pixels = px(38.);
pub const MATERIAL_TRAIL_STEP: Pixels = px(96.);
pub const MATERIAL_TRAIL_ARROW: Pixels = px(10.);
/// An item's text in the book's face at a small size, and an image or a table at the panel's width.
pub const MATERIAL_READING_TEXT: Pixels = px(15.);
pub const MATERIAL_READING_LINE: Pixels = px(23.);
pub const MATERIAL_IMAGE_HEIGHT: Pixels = px(180.);
pub const MATERIAL_TABLE_ROWS: usize = 40;
pub const MATERIAL_ROW_HEIGHT: Pixels = px(28.);
/// The library card: its width, its results' column, its panes' height, and its preview's image.
pub const LIBRARY_WIDTH: Pixels = px(680.);
pub const LIBRARY_RESULTS_WIDTH: Pixels = px(372.);
pub const LIBRARY_PANES_HEIGHT: Pixels = px(236.);
pub const LIBRARY_IMAGE_HEIGHT: Pixels = px(112.);

/// The accent presets. The first is the default, oxide red, which is darker on a light surface.
pub const ACCENTS: [u32; 6] = [0xe06c4f, 0xd9a441, 0x4fae7b, 0x2f8fb8, 0x6b73e0, 0xb86bd6];
const OXIDE_ON_LIGHT: u32 = 0xb54226;

/// The shadow of what floats at the text: the toolbar, the link field, a menu, and a rich block's card.
pub fn float_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(4.), hsla(0., 0., 0., 0.1))
            .blur_radius(px(6.))
            .spread_radius(px(-1.)),
        BoxShadow::new(px(0.), px(2.), hsla(0., 0., 0., 0.1))
            .blur_radius(px(4.))
            .spread_radius(px(-2.)),
    ]
}

/// The shadow of a card over the window: a dialog, the palette, the search card, the slip, and the like.
pub fn card_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(10.), hsla(0., 0., 0., 0.1))
            .blur_radius(px(15.))
            .spread_radius(px(-3.)),
        BoxShadow::new(px(0.), px(4.), hsla(0., 0., 0., 0.1))
            .blur_radius(px(6.))
            .spread_radius(px(-4.)),
    ]
}

/// The text's type before a layout has read the theme, and for a theme that does not lay out.
pub fn stand_in_type() -> BookType {
    let face = |size: f32, weight: u16, line_height: f32, before: f32, after: f32| Face {
        family: "Georgia".to_owned(),
        size,
        weight,
        italic: false,
        line_height,
        space_before: before,
        space_after: after,
        first_line_indent: 0.,
    };
    let body = face(10.7, 400, 1.45, 0., 7.5);
    BookType {
        headings: [
            face(18.2, 700, 1.25, 18., 8.),
            face(14.4, 700, 1.25, 14., 6.),
            face(12.3, 700, 1.25, 12., 5.),
            face(10.7, 700, 1.25, 10., 5.),
            face(10.7, 700, 1.25, 10., 5.),
            face(10.7, 700, 1.25, 10., 5.),
        ],
        quote: body.clone(),
        list: body.clone(),
        footnote: face(9.3, 400, 1.45, 0., 3.),
        code: Face {
            family: FONT_MONO.to_owned(),
            ..face(8.6, 400, 1.4, 0., 7.5)
        },
        measure: 330.,
        column_gap: 16.,
        files: Vec::new(),
        body,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, ValueEnum)]
pub enum Scheme {
    Dark,
    Light,
}

impl From<WindowAppearance> for Scheme {
    fn from(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
        }
    }
}

/// The colors of one appearance: neutral greys with no colour cast, and one accent.
#[derive(Clone)]
pub struct Tokens {
    /// The window's surface, where the text stands in Write and the pages in Publish.
    pub canvas: Rgba,
    /// The surface behind Publish's pages, darker than a white page in the light appearance.
    pub pages_canvas: Rgba,
    /// Binder, rail, and cards.
    pub panel: Rgba,
    /// A raised block in a panel, such as the document identity or the active rail tab.
    pub raised: Rgba,
    /// A softly filled field.
    pub field: Rgba,
    pub divider: Rgba,
    /// A page in a small drawing, such as the page panel's.
    pub sheet: Rgba,
    pub text: Rgba,
    /// The author's running text in Write: softer than [`text`](Self::text), which headings and the chrome keep, so a
    /// page of it does not glare.
    pub writing: Rgba,
    pub text_quiet: Rgba,
    pub text_faint: Rgba,
    /// The author's choice. It colours only the caret, selection, marks, and the action that moves on, and says what is
    /// wrong.
    pub accent: Rgba,
    /// A switch that is on, and its knob: a raised grey with a light knob, dark enough on a light surface to show it.
    pub switch_on: Rgba,
    pub switch_knob: Rgba,
    /// Links, footnote marks, and citation chips in the text.
    pub link: Rgba,
    /// What the book cannot resolve.
    pub error: Rgba,
    /// Text an edit added, and the tint behind it.
    pub added: Rgba,
    /// Text an edit removed.
    pub removed: Rgba,
}

impl Tokens {
    /// The colors of `scheme`, with the author's accent or the default for `None`.
    pub fn new(scheme: Scheme, accent: Option<u32>) -> Self {
        match scheme {
            Scheme::Dark => Self {
                // Lifted off black and kept to about 8:1 for running text and 13:1 for headings, so a long read in the
                // dark neither glares nor blooms. Asked for by the owner on 2026-11-06.
                canvas: rgb(0x1a1a1a),
                pages_canvas: rgb(0x1a1a1a),
                panel: rgb(0x1f1f1f),
                raised: rgb(0x2d2d2d),
                field: rgb(0x262626),
                divider: rgb(0x333333),
                sheet: rgb(0xf7f7f5),
                text: rgb(0xdedede),
                writing: rgb(0xb4b4b4),
                text_quiet: rgb(0x969696),
                text_faint: rgb(0x6f6f6f),
                accent: rgb(accent.unwrap_or(ACCENTS[0])),
                switch_on: rgb(0x4a4a4a),
                switch_knob: rgb(0xededed),
                link: rgb(0x8fb4dc),
                error: rgb(0xe5766b),
                added: rgb(0x6fcf8a),
                removed: rgb(0xe5766b),
            },
            Scheme::Light => Self {
                canvas: rgb(0xfbfbfb),
                pages_canvas: rgb(0xd6d6d6),
                panel: rgb(0xf3f3f3),
                raised: rgb(0xe6e6e6),
                field: rgb(0xffffff),
                divider: rgb(0xdedede),
                sheet: rgb(0xffffff),
                text: rgb(0x1b1b1b),
                writing: rgb(0x2e2e2e),
                text_quiet: rgb(0x5e5e5e),
                text_faint: rgb(0x8f8f8f),
                accent: rgb(accent.unwrap_or(OXIDE_ON_LIGHT)),
                switch_on: rgb(0x5a5a5a),
                switch_knob: rgb(0xffffff),
                link: rgb(0x2f5d8a),
                error: rgb(0xb5442c),
                added: rgb(0x1f7a3d),
                removed: rgb(0xb42318),
            },
        }
    }

    pub fn get(cx: &App) -> &Self {
        cx.global()
    }
}

impl Global for Tokens {}
