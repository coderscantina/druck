//! The style properties the inspector edits and how typed text becomes a theme value. No gpui.

use serde_json::{Number, Value};

use super::strings;

pub fn style_label(key: &str) -> &str {
    strings::STYLES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(key, |(_, label)| label)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Property {
    Font,
    Size,
    Weight,
    LineHeight,
    Align,
    SpaceBefore,
    SpaceAfter,
}

impl Property {
    /// The properties of the detail view, top to bottom.
    pub const ALL: [Self; 7] = [
        Self::Font,
        Self::Size,
        Self::LineHeight,
        Self::Weight,
        Self::Align,
        Self::SpaceBefore,
        Self::SpaceAfter,
    ];

    /// The field name in the theme.
    pub fn key(self) -> &'static str {
        match self {
            Self::Font => "font",
            Self::Size => "size",
            Self::Weight => "weight",
            Self::LineHeight => "line-height",
            Self::Align => "align",
            Self::SpaceBefore => "space-before",
            Self::SpaceAfter => "space-after",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Font => strings::FONT,
            Self::Size => strings::SIZE,
            Self::Weight => strings::WEIGHT,
            Self::LineHeight => strings::LINE_HEIGHT,
            Self::Align => strings::ALIGN_LABEL,
            Self::SpaceBefore => strings::SPACE_BEFORE,
            Self::SpaceAfter => strings::SPACE_AFTER,
        }
    }

    /// Typed text as a theme value. Units, tokens, and ranges are the engine's to judge, so only the JSON type
    /// is decided here.
    pub fn parse(self, input: &str) -> Result<Value, &'static str> {
        let input = input.trim();
        match self {
            Self::LineHeight => input
                .parse::<f64>()
                .ok()
                .and_then(Number::from_f64)
                .map(Value::Number)
                .ok_or(strings::NOT_A_NUMBER),
            Self::Weight => Ok(input.parse::<u64>().map_or_else(
                |_| Value::String(input.to_owned()),
                |weight| Value::Number(weight.into()),
            )),
            _ => Ok(Value::String(input.to_owned())),
        }
    }

    /// A theme value as text, as the author would write it.
    pub fn display(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }
    }
}

pub const ALIGNMENTS: [(&str, &str); 4] = [
    ("left", strings::ALIGN_LEFT),
    ("center", strings::ALIGN_CENTER),
    ("right", strings::ALIGN_RIGHT),
    ("justify", strings::ALIGN_JUSTIFY),
];

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn typed_text_becomes_the_json_type_the_field_takes() {
        assert_eq!(Property::Size.parse(" $sizes.body "), Ok(json!("$sizes.body")));
        assert_eq!(Property::LineHeight.parse("1.4"), Ok(json!(1.4)));
        assert!(Property::LineHeight.parse("tall").is_err());
        assert_eq!(Property::Weight.parse("700"), Ok(json!(700)));
        assert_eq!(Property::Weight.parse("bold"), Ok(json!("bold")));
    }

    #[test]
    fn values_display_as_written() {
        assert_eq!(Property::display(&json!("$sizes.body")), "$sizes.body");
        assert_eq!(Property::display(&json!(1.4)), "1.4");
        assert_eq!(Property::display(&json!(700)), "700");
    }
}
