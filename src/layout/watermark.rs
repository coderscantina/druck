//! The watermark: one line of text in the `watermark` style, centered on the page and turned, behind
//! the content, as LaTeX's `draftwatermark` package sets "DRAFT".
//!
//! The center of the line, between its baseline and cap height, lies on the center of the page. The
//! line is set at the style's size, or smaller so that its turned box fits within 90% of the page width
//! and height. Line breaks in the text and the values become spaces.

use super::paragraph;
use crate::config::resolved::{Config, Watermark};
use crate::config::template::{Placeholder, Value};
use crate::config::values::Pt;
use crate::page::{Item, Page};
use crate::text::Fonts;

/// The share of the page width and height the turned line may fill.
const FILL: f64 = 0.9;

/// The watermark item for `page`, or `None` while a placeholder in its text has no value.
pub(super) fn item(
    watermark: &Watermark,
    value: &dyn Fn(&Placeholder) -> Option<Value>,
    page: &Page,
    config: &Config,
    fonts: &Fonts,
) -> Result<Option<Item>, String> {
    let Ok(lines) = watermark.text.fill(value) else {
        return Ok(None);
    };
    let mut text = lines.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Ok(None);
    }
    let style = &config.styles.watermark;
    if style.uppercase {
        text = text.to_uppercase();
    }
    let lang = config.document.lang;
    let face = fonts.face(&style.font, style.weight, style.style);
    let mut run = fonts.shape_tracked(&text, face, style.size, lang, style.tracking);
    if let Some(problem) = paragraph::missing_glyph(&run) {
        return Err(problem);
    }
    let (sin, cos) = watermark.angle.to_radians().sin_cos();
    let (width, height) = (run.set_width().0, fonts.cap_height(face, style.size).0);
    let turned_width = width * cos.abs() + height * sin.abs();
    let turned_height = width * sin.abs() + height * cos.abs();
    let scale = (FILL * page.width.0 / turned_width)
        .min(FILL * page.height.0 / turned_height)
        .min(1.0);
    let (width, height) = if scale < 1.0 {
        let size = Pt(style.size.0 * scale);
        run = fonts.shape_tracked(&text, face, size, lang, style.tracking);
        (run.set_width().0, fonts.cap_height(face, size).0)
    } else {
        (width, height)
    };
    // The line's center is (width / 2, -height / 2) from the start of its baseline, turned with it.
    let (dx, dy) = (width / 2.0, -height / 2.0);
    let x = page.width.0 / 2.0 - (dx * cos + dy * sin);
    let y = page.height.0 / 2.0 - (-dx * sin + dy * cos);
    Ok(Some(Item::TurnedText {
        x: Pt(x),
        y: Pt(y),
        angle: watermark.angle,
        run,
        color: style.color,
    }))
}
