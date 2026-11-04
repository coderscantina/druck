//! The style properties the inspector edits and how typed text becomes a theme value. No gpui.

use serde_json::{Number, Value};

use super::strings::{self, Key};

pub fn style_label(key: &str) -> &str {
    strings::STYLES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(key, |&(_, label)| crate::t!(label))
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
        crate::t!(match self {
            Self::Font => Key::Font,
            Self::Size => Key::Size,
            Self::Weight => Key::Weight,
            Self::LineHeight => Key::LineHeight,
            Self::Align => Key::AlignLabel,
            Self::SpaceBefore => Key::SpaceBefore,
            Self::SpaceAfter => Key::SpaceAfter,
        })
    }

    /// Typed text as a theme value. Units, tokens, and ranges are the engine's to judge, so only the JSON type
    /// is decided here.
    pub fn parse(self, input: &str) -> Result<Value, Key> {
        let input = input.trim();
        match self {
            Self::LineHeight => input
                .parse::<f64>()
                .ok()
                .and_then(Number::from_f64)
                .map(Value::Number)
                .ok_or(Key::NotANumber),
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

pub const ALIGNMENTS: [(&str, Key); 4] = [
    ("left", Key::AlignLeft),
    ("center", Key::AlignCenter),
    ("right", Key::AlignRight),
    ("justify", Key::AlignJustify),
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
