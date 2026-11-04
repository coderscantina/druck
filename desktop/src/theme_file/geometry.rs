//! Page geometry of a theme in points: the page, its margins, and the text frame, read from a theme's merged value,
//! and a diagram of an odd and an even page from it. Holds no gpui types.

use serde_json::Value;

use super::tokens::Group;

/// Points in a millimetre.
const MM: f64 = 72.0 / 25.4;

/// A paper size by name, portrait, in points. The engine's table.
pub fn paper(name: &str) -> Option<(f64, f64)> {
    match name {
        "a4" => Some((210.0 * MM, 297.0 * MM)),
        "a5" => Some((148.0 * MM, 210.0 * MM)),
        "b5" => Some((176.0 * MM, 250.0 * MM)),
        "letter" => Some((612.0, 792.0)),
        "legal" => Some((612.0, 1008.0)),
        _ => None,
    }
}

pub fn to_mm(points: f64) -> f64 {
    points / MM
}

/// A number with at most one decimal, without a trailing `.0`.
pub fn rounded(value: f64) -> String {
    let text = format!("{value:.1}");
    match text.strip_suffix(".0") {
        Some(whole) => whole.to_owned(),
        None => text,
    }
}

/// A length such as `10.5pt`, `28mm`, `1cm`, `8.5in`, or `1.5em`, with `em` the given size. Negative is refused.
fn parse(text: &str, em: Option<f64>) -> Option<f64> {
    let split = text.find(|c: char| c.is_ascii_alphabetic())?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    if !number.is_finite() || number < 0.0 {
        return None;
    }
    let scale = match unit {
        "pt" => 1.0,
        "mm" => MM,
        "cm" => 10.0 * MM,
        "in" => 72.0,
        "em" => em?,
        _ => return None,
    };
    Some(number * scale)
}

/// The text a value comes to, following `$group.name` references between tokens of one group.
fn follow<'a>(theme: &'a Value, text: &'a str, group: Group) -> Option<&'a str> {
    let mut text = text;
    for _ in 0..8 {
        if !text.starts_with('$') {
            return Some(text);
        }
        let (found, name) = Group::parse(text)?;
        if found != group {
            return None;
        }
        text = theme.get("tokens")?.get(group.key())?.get(name)?.as_str()?;
    }
    None
}

/// The body font size in points, which `em` on the page refers to. It must be absolute and above zero.
pub fn body_size(theme: &Value) -> Option<f64> {
    let size = theme.get("styles")?.get("body")?.get("size")?.as_str()?;
    parse(follow(theme, size, Group::Sizes)?, None).filter(|size| *size > 0.0)
}

/// A page length, written or a `$spacing` token, in points.
pub fn length(theme: &Value, value: &Value) -> Option<f64> {
    let text = follow(theme, value.as_str()?, Group::Spacing)?;
    parse(text, body_size(theme))
}

/// `page.text-width`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextWidth {
    /// `null`: prose uses the whole frame.
    Frame,
    Points(f64),
    Unresolved,
}

/// The page geometry in points. `None` where the theme's value does not resolve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub top: Option<f64>,
    pub bottom: Option<f64>,
    pub inner: Option<f64>,
    pub outer: Option<f64>,
    pub mirror: bool,
    pub text_width: TextWidth,
    pub column_gap: Option<f64>,
}

/// The margin frame and the prose block in it, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub width: f64,
    pub height: f64,
    /// Prose width, from the inner edge, never wider than the frame.
    pub prose: f64,
}

/// A rectangle in fractions of the page's width and height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    pub frame: Rect,
    pub prose: Rect,
    /// The gap between two columns, centred in the prose block, when it fits.
    pub gap: Option<Rect>,
}

/// An odd and an even page, with the page's width over its height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diagram {
    pub aspect: f64,
    pub odd: Sheet,
    pub even: Sheet,
}

impl Geometry {
    pub fn read(theme: &Value) -> Self {
        let at = |path: &[&str]| path.iter().try_fold(theme.get("page")?, |value, key| value.get(*key));
        let measure = |path: &[&str]| at(path).and_then(|value| length(theme, value));
        let (width, height) = match at(&["size"]) {
            Some(Value::String(name)) => paper(name).unzip(),
            Some(_) => (measure(&["size", "width"]), measure(&["size", "height"])),
            None => (None, None),
        };
        let text_width = match at(&["text-width"]) {
            None | Some(Value::Null) => TextWidth::Frame,
            Some(value) => length(theme, value).map_or(TextWidth::Unresolved, TextWidth::Points),
        };
        Self {
            width,
            height,
            top: measure(&["margins", "top"]),
            bottom: measure(&["margins", "bottom"]),
            inner: measure(&["margins", "inner"]),
            outer: measure(&["margins", "outer"]),
            mirror: at(&["margins", "mirror"]).and_then(Value::as_bool).unwrap_or(true),
            text_width,
            column_gap: measure(&["column-gap"]),
        }
    }

    /// The frame the margins leave, if every part resolves and it has room.
    pub fn frame(&self) -> Option<Frame> {
        let width = self.width? - self.inner? - self.outer?;
        let height = self.height? - self.top? - self.bottom?;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let prose = match self.text_width {
            TextWidth::Frame => width,
            TextWidth::Points(points) => points.min(width),
            TextWidth::Unresolved => return None,
        };
        Some(Frame { width, height, prose })
    }

    /// Mirrored margins swap inner and outer on even pages, so prose moves to the inner edge on the right there.
    pub fn diagram(&self) -> Option<Diagram> {
        let frame = self.frame()?;
        let (page_width, page_height) = (self.width?, self.height?);
        let top = self.top? / page_height;
        let height = frame.height / page_height;
        let rect = |x: f64, width: f64| Rect {
            x: x / page_width,
            y: top,
            width: width / page_width,
            height,
        };
        let sheet = |left: f64, prose_left: f64| Sheet {
            frame: rect(left, frame.width),
            prose: rect(prose_left, frame.prose),
            gap: self
                .column_gap
                .filter(|gap| *gap < frame.prose)
                .map(|gap| rect(prose_left + (frame.prose - gap) / 2.0, gap)),
        };
        let inner = self.inner?;
        let odd = sheet(inner, inner);
        let even = if self.mirror {
            let outer = self.outer?;
            sheet(outer, outer + frame.width - frame.prose)
        } else {
            odd
        };
        Some(Diagram {
            aspect: page_width / page_height,
            odd,
            even,
        })
    }
}

#[cfg(test)]
mod tests {
    use druck::editor::theme_value::default_theme_value;
    use serde_json::json;

    use super::*;

    fn close(actual: Option<f64>, expected: f64) {
        let actual = actual.unwrap_or(f64::NAN);
        assert!((actual - expected).abs() < 1e-6, "{actual} is not {expected}");
    }

    fn theme(page: Value) -> Value {
        json!({
            "tokens": {
                "sizes": { "body": "12pt", "alias": "$sizes.body" },
                "spacing": { "side": "20mm", "wide": "$spacing.side", "gap": "1.5em" }
            },
            "styles": { "body": { "size": "$sizes.alias" } },
            "page": page,
        })
    }

    #[test]
    fn paper_names_are_in_points() {
        close(paper("a4").map(|size| size.0), 595.275_590_551);
        assert_eq!(paper("letter"), Some((612.0, 792.0)));
        assert_eq!(paper("a3"), None);
    }

    #[test]
    fn lengths_convert_every_unit_to_points() {
        let theme = theme(json!({}));
        close(length(&theme, &json!("10pt")), 10.0);
        close(length(&theme, &json!("25.4mm")), 72.0);
        close(length(&theme, &json!("2.54cm")), 72.0);
        close(length(&theme, &json!("8.5in")), 612.0);
        close(length(&theme, &json!(".5em")), 6.0);
    }

    #[test]
    fn tokens_resolve_through_other_tokens_and_em_follows_the_body_size() {
        let theme = theme(json!({}));
        close(body_size(&theme), 12.0);
        close(length(&theme, &json!("$spacing.wide")), 20.0 * MM);
        close(length(&theme, &json!("$spacing.gap")), 18.0);
    }

    #[test]
    fn what_does_not_resolve_is_none() {
        let theme = theme(json!({}));
        for value in [
            json!("$spacing.missing"),
            json!("$sizes.body"),
            json!("10px"),
            json!("-1pt"),
            json!(3),
        ] {
            assert_eq!(length(&theme, &value), None, "{value}");
        }
        let relative = json!({ "styles": { "body": { "size": "1em" } } });
        assert_eq!(body_size(&relative), None);
        assert_eq!(length(&relative, &json!("1em")), None);
    }

    #[test]
    fn the_default_theme_reads_completely() {
        let geometry = Geometry::read(&default_theme_value());
        close(geometry.width, 210.0 * MM);
        close(geometry.top, 27.0 * MM);
        close(geometry.inner, 28.0 * MM);
        assert!(geometry.mirror);
        assert_eq!(geometry.text_width, TextWidth::Frame);
        close(geometry.column_gap, 1.5 * 10.5);
        close(geometry.frame().map(|frame| frame.prose), (210.0 - 56.0) * MM);
    }

    #[test]
    fn a_custom_size_reads_its_width_and_height() {
        let geometry = Geometry::read(&theme(json!({ "size": { "width": "6in", "height": "$spacing.side" } })));
        close(geometry.width, 432.0);
        close(geometry.height, 20.0 * MM);
    }

    #[test]
    fn text_width_narrows_prose_but_never_past_the_frame() {
        let page = |text_width: Value| {
            json!({ "size": { "width": "200pt", "height": "300pt" }, "text-width": text_width,
                "margins": { "top": "10pt", "bottom": "20pt", "inner": "30pt", "outer": "40pt" } })
        };
        let frame = |text_width| Geometry::read(&theme(page(text_width))).frame();
        assert_eq!(
            frame(json!(null)),
            Some(Frame {
                width: 130.0,
                height: 270.0,
                prose: 130.0
            })
        );
        close(frame(json!("100pt")).map(|frame| frame.prose), 100.0);
        close(frame(json!("500pt")).map(|frame| frame.prose), 130.0);
        assert_eq!(frame(json!("$spacing.missing")), None);
    }

    #[test]
    fn mirrored_margins_swap_inner_and_outer_on_even_pages() {
        let page = |mirror: bool| {
            json!({ "size": { "width": "100pt", "height": "200pt" }, "text-width": "40pt", "column-gap": "10pt",
                "margins": { "top": "20pt", "bottom": "20pt", "inner": "10pt", "outer": "30pt", "mirror": mirror } })
        };
        let diagram = Geometry::read(&theme(page(true))).diagram().unwrap();
        assert_eq!(diagram.aspect, 0.5);
        assert_eq!(
            diagram.odd.frame,
            Rect {
                x: 0.1,
                y: 0.1,
                width: 0.6,
                height: 0.8
            }
        );
        assert_eq!(diagram.odd.prose.x, 0.1);
        assert_eq!(diagram.even.frame.x, 0.3);
        assert!((diagram.even.prose.x - 0.5).abs() < 1e-9);
        let flat = Geometry::read(&theme(page(false))).diagram().unwrap();
        assert_eq!(flat.even, flat.odd);
    }

    #[test]
    fn the_column_gap_is_centred_in_the_prose_and_dropped_when_it_does_not_fit() {
        let page = |gap: &str| {
            json!({ "size": { "width": "100pt", "height": "100pt" }, "column-gap": gap,
                "margins": { "top": "10pt", "bottom": "10pt", "inner": "10pt", "outer": "10pt" } })
        };
        let gap = Geometry::read(&theme(page("20pt"))).diagram().unwrap().odd.gap.unwrap();
        assert!((gap.x - 0.4).abs() < 1e-9 && (gap.width - 0.2).abs() < 1e-9);
        assert_eq!(Geometry::read(&theme(page("90pt"))).diagram().unwrap().odd.gap, None);
    }

    #[test]
    fn numbers_round_to_one_decimal() {
        assert_eq!(rounded(210.0), "210");
        assert_eq!(rounded(79.370_078), "79.4");
    }
}
