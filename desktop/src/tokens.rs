//! The one table of colors, spacing, and type for app chrome. Views read values from here and never hardcode
//! them. Document content on the page sheet uses the theme's own values instead.

use clap::ValueEnum;
use gpui::{App, Global, Pixels, Rgba, WindowAppearance, px, rgb};

pub const FONT_UI: &str = ".SystemUIFont";

pub const TEXT_SMALL: Pixels = px(11.);
pub const TEXT_BODY: Pixels = px(13.);

pub const SPACE_2: Pixels = px(8.);
pub const SPACE_3: Pixels = px(12.);
pub const SPACE_4: Pixels = px(16.);

pub const TITLEBAR_HEIGHT: Pixels = px(28.);
pub const SECTION_HEIGHT: Pixels = px(30.);
pub const SIDEBAR_WIDTH: Pixels = px(232.);
pub const INSPECTOR_WIDTH: Pixels = px(296.);
pub const RAIL_WIDTH: Pixels = px(40.);
pub const ICON_SIZE: Pixels = px(16.);

/// A page sheet's size before the preview has real pages: A4 at half scale.
pub const SHEET_WIDTH: Pixels = px(298.);
pub const SHEET_HEIGHT: Pixels = px(421.);

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
            },
        }
    }

    pub fn get(cx: &App) -> &Self {
        cx.global()
    }
}

impl Global for Tokens {}
