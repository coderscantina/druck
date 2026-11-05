//! The one table of colors, spacing, and type for app chrome and the writing surface. Views read values from here and
//! never hardcode them. Publish's pages use the theme's own values instead.

use clap::ValueEnum;
use gpui::{App, Global, Pixels, Rgba, WindowAppearance, px, rgb};

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
pub const TEXT_MIN_WIDTH: Pixels = px(200.);
/// A margin note, and the gap between it and the text.
pub const NOTE_WIDTH: Pixels = px(220.);
pub const NOTE_GAP: Pixels = px(28.);
pub const NOTE_LINE_HEIGHT: Pixels = px(18.);
pub const NOTE_TEXT: Pixels = px(12.);
pub const NOTE_RULE: Pixels = px(1.);
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
/// A chapter's tick at the left edge, the current one longer.
pub const TICK_WIDTH: Pixels = px(10.);
pub const TICK_WIDTH_CURRENT: Pixels = px(18.);
pub const TICK_HEIGHT: Pixels = px(1.5);
pub const TICK_ROW: Pixels = px(8.5);
pub const TICK_ZONE_WIDTH: Pixels = px(36.);
pub const TICK_OPACITY: f32 = 0.65;
/// The contents card that opens from the ticks.
pub const CONTENTS_CARD_WIDTH: Pixels = px(240.);
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
/// The commit card and the rows and buttons the versions views share.
pub const COMMIT_CARD_WIDTH: Pixels = px(420.);
pub const COMMIT_CARD_MAX_ROWS: usize = 12;
pub const VERSIONS_ROW_HEIGHT: Pixels = px(24.);
pub const VERSIONS_BUTTON_HEIGHT: Pixels = px(22.);
/// The timeline's strip: commits as tall marks, local history as small ticks, and the position.
pub const STRIP_HEIGHT: Pixels = px(50.);
pub const STRIP_COMMIT_WIDTH: Pixels = px(2.);
pub const STRIP_COMMIT_HEIGHT: Pixels = px(23.);
pub const STRIP_TICK_WIDTH: Pixels = px(1.5);
pub const STRIP_TICK_HEIGHT: Pixels = px(7.);
pub const STRIP_POSITION_HEIGHT: Pixels = px(27.);
/// How strongly a word difference tints the text behind it.
pub const DIFF_TINT: f32 = 0.24;
/// The accent presets. The first is the default, oxide red, which is darker on a light surface.
pub const ACCENTS: [u32; 6] = [0xe06c4f, 0xd9a441, 0x4fae7b, 0x2f8fb8, 0x6b73e0, 0xb86bd6];
const OXIDE_ON_LIGHT: u32 = 0xb54226;

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
    pub text_quiet: Rgba,
    pub text_faint: Rgba,
    /// The author's choice. It colours only the caret, selection, marks, and the action that moves on.
    pub accent: Rgba,
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
                canvas: rgb(0x161616),
                panel: rgb(0x1c1c1c),
                raised: rgb(0x2a2a2a),
                field: rgb(0x232323),
                divider: rgb(0x303030),
                sheet: rgb(0xf7f7f5),
                text: rgb(0xededed),
                text_quiet: rgb(0xa6a6a6),
                text_faint: rgb(0x6f6f6f),
                accent: rgb(accent.unwrap_or(ACCENTS[0])),
                link: rgb(0x8fb4dc),
                error: rgb(0xe5766b),
                added: rgb(0x6fcf8a),
                removed: rgb(0xe5766b),
            },
            Scheme::Light => Self {
                canvas: rgb(0xfbfbfb),
                panel: rgb(0xf3f3f3),
                raised: rgb(0xe6e6e6),
                field: rgb(0xffffff),
                divider: rgb(0xdedede),
                sheet: rgb(0xffffff),
                text: rgb(0x1b1b1b),
                text_quiet: rgb(0x5e5e5e),
                text_faint: rgb(0x8f8f8f),
                accent: rgb(accent.unwrap_or(OXIDE_ON_LIGHT)),
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
