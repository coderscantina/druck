//! The one table of colors, spacing, and type for app chrome. Views read values from here and never hardcode
//! them. Document content on the page sheet uses the theme's own values instead.

use clap::ValueEnum;
use gpui::{App, Global, Pixels, Rgba, WindowAppearance, px, rgb};

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
pub const SIDEBAR_WIDTH: Pixels = px(256.);
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
/// The block gutter inside the sheet, from the sheet's edge to the text.
pub const GUTTER_WIDTH: Pixels = px(52.);

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

/// The colors of one appearance. The accent is an oxide red, for the ink of a proof mark.
#[derive(Clone)]
pub struct Tokens {
    /// Behind the sheets in the centre pane.
    pub canvas: Rgba,
    /// Sidebar, inspector, and title bar.
    pub panel: Rgba,
    /// A raised block in a panel, such as the document identity or the active rail tab.
    pub raised: Rgba,
    pub divider: Rgba,
    /// An empty sheet until real pages paint their own colors.
    pub sheet: Rgba,
    pub text: Rgba,
    pub text_quiet: Rgba,
    pub text_faint: Rgba,
    pub accent: Rgba,
    /// Text an edit added, and the tint behind it.
    pub added: Rgba,
    /// Text an edit removed.
    pub removed: Rgba,
}

impl Tokens {
    pub fn new(scheme: Scheme) -> Self {
        match scheme {
            Scheme::Dark => Self {
                canvas: rgb(0x15171a),
                panel: rgb(0x1d1f23),
                raised: rgb(0x2a2d32),
                divider: rgb(0x31343a),
                sheet: rgb(0xf4f4f2),
                text: rgb(0xe8e9ea),
                text_quiet: rgb(0xa2a6ad),
                text_faint: rgb(0x71767e),
                accent: rgb(0xe06c4f),
                added: rgb(0x6fcf8a),
                removed: rgb(0xe5766b),
            },
            Scheme::Light => Self {
                canvas: rgb(0xd9dce0),
                panel: rgb(0xf2f3f5),
                raised: rgb(0xe2e4e8),
                divider: rgb(0xcdd0d6),
                sheet: rgb(0xffffff),
                text: rgb(0x1b1d21),
                text_quiet: rgb(0x555a62),
                text_faint: rgb(0x7f848c),
                accent: rgb(0xb54226),
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
