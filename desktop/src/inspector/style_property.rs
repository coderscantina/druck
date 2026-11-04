//! The properties of a style as the schema has them, how each is edited, and how typed text becomes a theme
//! value. No gpui.

use serde_json::{Number as JsonNumber, Value};

use super::strings::{self, STYLES, Text};
use crate::theme_file::tokens::Group;

pub fn style_label(key: &str) -> &str {
    STYLES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(key, |(_, label)| label.get())
}

/// How a property is edited.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Control {
    /// Text that may be a token of this group.
    Reference(Group),
    /// A choice of fixed values, written as the first part.
    Choice(&'static [(&'static str, Text)]),
    Number,
    /// `regular`, `bold`, or a step from 100 to 900.
    Weight,
    Toggle,
    Align,
    /// A list of strings, typed with commas between.
    List,
    /// The name of a style.
    StyleName,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Property {
    /// Where the value is in the style: one key, or `figures` and one below it.
    pub path: &'static [&'static str],
    pub label: Text,
    pub control: Control,
}

const STYLE_CHOICE: &[(&str, Text)] = &[("normal", strings::NORMAL), ("italic", strings::ITALIC)];
const FIGURE_STYLE: &[(&str, Text)] = &[
    ("default", strings::DEFAULT_WORD),
    ("lining", strings::LINING),
    ("oldstyle", strings::OLDSTYLE),
];
const FIGURE_SPACING: &[(&str, Text)] = &[
    ("default", strings::DEFAULT_WORD),
    ("proportional", strings::PROPORTIONAL),
    ("tabular", strings::TABULAR),
];
const RULE_BELOW: &[(&str, Text)] = &[
    ("none", strings::RULE_NONE),
    ("header", strings::RULE_HEADER),
    ("row", strings::RULE_ROW),
];

const fn property(key: &'static [&'static str], label: Text, control: Control) -> Property {
    Property {
        path: key,
        label,
        control,
    }
}

use Control::{Align, Choice, List, Number, Reference, StyleName, Toggle, Weight};

pub const FONT: Property = property(&["font"], strings::FONT, Reference(Group::Fonts));
pub const SIZE: Property = property(&["size"], strings::SIZE, Reference(Group::Sizes));
pub const COLOR: Property = property(&["color"], strings::COLOR, Reference(Group::Colors));

/// The properties of every block style, top to bottom.
pub const BLOCK: &[Property] = &[
    FONT,
    SIZE,
    property(&["weight"], strings::WEIGHT, Weight),
    property(&["style"], strings::FONT_STYLE, Choice(STYLE_CHOICE)),
    COLOR,
    property(&["line-height"], strings::LINE_HEIGHT, Number),
    property(&["align"], strings::ALIGN_LABEL, Align),
    property(&["hyphenate"], strings::HYPHENATE, Toggle),
    property(&["space-before"], strings::SPACE_BEFORE, Reference(Group::Spacing)),
    property(&["space-after"], strings::SPACE_AFTER, Reference(Group::Spacing)),
    property(&["indent"], strings::INDENT, Reference(Group::Spacing)),
    property(&["first-line-indent"], strings::FIRST_LINE_INDENT, Reference(Group::Spacing)),
    property(&["tracking"], strings::TRACKING, Number),
    property(&["uppercase"], strings::UPPERCASE, Toggle),
    property(&["keep-with-next"], strings::KEEP_WITH_NEXT, Toggle),
    property(&["figures", "style"], strings::FIGURES_STYLE, Choice(FIGURE_STYLE)),
    property(&["figures", "spacing"], strings::FIGURES_SPACING, Choice(FIGURE_SPACING)),
];

pub const NUMBER_GAP: Property = property(&["number-gap"], strings::NUMBER_GAP, Reference(Group::Spacing));

/// What only a custom style has. `based-on` comes first in its detail.
pub const BASED_ON: Property = property(&["based-on"], strings::BASED_ON, StyleName);
pub const BULLETS: Property = property(&["bullets"], strings::BULLETS, List);
pub const MARKER: Property = property(&["marker"], strings::MARKER, StyleName);
pub const RULE: Property = property(&["rule-below"], strings::RULE_BELOW, Choice(RULE_BELOW));

/// The properties the detail view of a built-in style shows.
pub fn of_builtin(style: &str) -> Vec<Property> {
    let mut properties = BLOCK.to_vec();
    if style.starts_with("heading-") {
        properties.push(NUMBER_GAP);
    }
    properties
}

/// The properties of a custom style.
pub fn of_custom() -> Vec<Property> {
    let mut properties = vec![BASED_ON];
    properties.extend_from_slice(BLOCK);
    properties.push(NUMBER_GAP);
    properties.push(BULLETS);
    properties.push(MARKER);
    properties.push(RULE);
    properties
}

impl Property {
    /// The field name, joined with dots for a nested one. Also how fields are found.
    pub fn key(self) -> String {
        self.path.join(".")
    }

    /// The path in the theme, for a style at `prefix`.
    pub fn path_in(self, prefix: &[&str]) -> Vec<String> {
        prefix
            .iter()
            .chain(self.path)
            .map(|key| (*key).to_owned())
            .collect()
    }

    /// Typed text as a theme value. Units, tokens, and ranges are the engine's to judge, so only the JSON type
    /// is decided here.
    pub fn parse(self, input: &str) -> Result<Value, &'static str> {
        let input = input.trim();
        match self.control {
            Control::Number => input
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .and_then(JsonNumber::from_f64)
                .map(Value::Number)
                .ok_or(strings::NOT_A_NUMBER.get()),
            Control::Weight => Ok(input.parse::<u64>().map_or_else(
                |_| Value::String(input.to_owned()),
                |weight| Value::Number(weight.into()),
            )),
            Control::List => Ok(Value::Array(
                input
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| Value::String(item.to_owned()))
                    .collect(),
            )),
            Control::Toggle => match input {
                "true" | "on" => Ok(Value::Bool(true)),
                _ => Ok(Value::Bool(false)),
            },
            Control::Reference(_) if input == "none" && self == NUMBER_GAP => Ok(Value::Null),
            _ => Ok(Value::String(input.to_owned())),
        }
    }

    /// A theme value as text, as the author would write it.
    pub fn display(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            Value::Null => "none".to_owned(),
            Value::Array(items) => items.iter().map(Self::display).collect::<Vec<_>>().join(", "),
            other => other.to_string(),
        }
    }
}

pub const ALIGNMENTS: [(&str, Text); 4] = [
    ("left", strings::ALIGN_LEFT),
    ("center", strings::ALIGN_CENTER),
    ("right", strings::ALIGN_RIGHT),
    ("justify", strings::ALIGN_JUSTIFY),
];

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn named(key: &str) -> Property {
        BLOCK
            .iter()
            .chain(&[NUMBER_GAP, BULLETS, MARKER])
            .find(|property| property.key() == key)
            .copied()
            .unwrap()
    }

    #[test]
    fn typed_text_becomes_the_json_type_the_field_takes() {
        assert_eq!(named("size").parse(" $sizes.body "), Ok(json!("$sizes.body")));
        assert_eq!(named("line-height").parse("1.4"), Ok(json!(1.4)));
        assert_eq!(named("line-height").parse("1,4"), Ok(json!(1.4)));
        assert!(named("line-height").parse("tall").is_err());
        assert_eq!(named("weight").parse("700"), Ok(json!(700)));
        assert_eq!(named("weight").parse("bold"), Ok(json!("bold")));
        assert_eq!(named("bullets").parse("•, –"), Ok(json!(["•", "–"])));
        assert_eq!(named("number-gap").parse("none"), Ok(Value::Null));
    }

    #[test]
    fn values_display_as_written() {
        assert_eq!(Property::display(&json!("$sizes.body")), "$sizes.body");
        assert_eq!(Property::display(&json!(1.4)), "1.4");
        assert_eq!(Property::display(&json!(["•", "–"])), "•, –");
    }

    #[test]
    fn every_block_property_is_in_the_schema() {
        let schema: Value = serde_json::from_str(include_str!("../../../schema/theme.v1.schema.json")).unwrap();
        let heading = &schema["$defs"]["headingStyle"]["properties"];
        let custom = &schema["$defs"]["customStyle"]["properties"];
        for property in of_builtin("heading-1") {
            assert!(
                heading.get(property.path[0]).is_some(),
                "{} is not a heading property",
                property.key()
            );
        }
        for property in of_custom() {
            assert!(custom.get(property.path[0]).is_some(), "{} is not a custom property", property.key());
        }
        let block = schema["$defs"]["blockStyle"]["properties"].as_object().unwrap();
        for key in block.keys() {
            assert!(BLOCK.iter().any(|property| property.path[0] == key), "{key} has no field");
        }
        for key in custom.as_object().unwrap().keys() {
            assert!(of_custom().iter().any(|property| property.path[0] == key), "{key} has no field");
        }
    }
}
