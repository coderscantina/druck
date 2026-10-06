//! Scalar configuration values: lengths, colors, font names, and token references.
//!
//! Values are written as strings in themes and front matter. Parsing happens during
//! deserialization so syntax errors carry the offending property path.

use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

const PT_PER_IN: f64 = 72.0;

/// A non-negative length after unit conversion. Absolute units become points; `em`
/// stays relative until the context that defines its font-size basis resolves it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Pt(f64),
    Em(f64),
}

impl Length {
    pub fn parse(input: &str) -> Result<Self, String> {
        let split = input
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .ok_or_else(|| format!("length \"{input}\" needs a unit (pt, mm, cm, in, or em)"))?;
        let (number, unit) = input.split_at(split);
        let value: f64 = number
            .parse()
            .ok()
            .filter(|v: &f64| v.is_finite())
            .ok_or_else(|| format!("length \"{input}\" has an invalid number"))?;
        if value < 0.0 {
            return Err(format!("length \"{input}\" must not be negative"));
        }
        match unit {
            "pt" => Ok(Self::Pt(value)),
            "mm" => Ok(Self::Pt(value * PT_PER_IN / 25.4)),
            "cm" => Ok(Self::Pt(value * PT_PER_IN / 2.54)),
            "in" => Ok(Self::Pt(value * PT_PER_IN)),
            "em" => Ok(Self::Em(value)),
            _ => Err(format!(
                "length \"{input}\" has unsupported unit \"{unit}\" (use pt, mm, cm, in, or em)"
            )),
        }
    }

    /// Resolves `em` against the font size of the context using the length.
    pub fn to_pt(self, em_basis: Pt) -> Pt {
        match self {
            Self::Pt(pt) => Pt(pt),
            Self::Em(em) => Pt(em * em_basis.0),
        }
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pt(v) => write!(f, "{v}pt"),
            Self::Em(v) => write!(f, "{v}em"),
        }
    }
}

impl Serialize for Length {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A resolved absolute length in PostScript points.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
pub struct Pt(pub f64);

/// An sRGB color written as `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub [u8; 3]);

impl Color {
    pub fn parse(input: &str) -> Result<Self, String> {
        let hex = input
            .strip_prefix('#')
            .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| format!("color \"{input}\" must be written as #rrggbb"))?;
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("validated hex");
        Ok(Self([channel(0), channel(2), channel(4)]))
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A token group and the literal value type it holds.
///
/// Each value field accepts either a literal or a `$group.name` reference to a token of
/// the matching group, so a reference can never cross value kinds.
pub trait TokenKind: Sized {
    const GROUP: &'static str;
    fn parse_literal(input: &str) -> Result<Self, String>;
    fn write_literal(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// A font size. `em` refers to the body font size, or to the surrounding text for inline styles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size(pub Length);

/// A spacing length. `em` refers to the font size of the element using it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing(pub Length);

/// A font family name, defined in the resolved `fonts` map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontName(pub String);

impl TokenKind for Size {
    const GROUP: &'static str = "sizes";
    fn parse_literal(input: &str) -> Result<Self, String> {
        Length::parse(input).map(Self)
    }
    fn write_literal(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl TokenKind for Spacing {
    const GROUP: &'static str = "spacing";
    fn parse_literal(input: &str) -> Result<Self, String> {
        Length::parse(input).map(Self)
    }
    fn write_literal(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl TokenKind for Color {
    const GROUP: &'static str = "colors";
    fn parse_literal(input: &str) -> Result<Self, String> {
        Self::parse(input)
    }
    fn write_literal(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl TokenKind for FontName {
    const GROUP: &'static str = "fonts";
    fn parse_literal(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Err("font family name must not be empty".into());
        }
        Ok(Self(input.to_owned()))
    }
    fn write_literal(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A token name: ASCII letters, digits, and hyphens.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TokenName(String);

impl TokenName {
    fn check(name: &str) -> Result<(), String> {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(format!(
                "token name \"{name}\" may contain only ASCII letters, digits, and hyphens"
            ));
        }
        Ok(())
    }
}

impl std::borrow::Borrow<str> for TokenName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TokenName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TokenName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::check(&name).map_err(de::Error::custom)?;
        Ok(Self(name))
    }
}

/// A configured value: a literal or a reference to a token of the same kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Spec<T> {
    Literal(T),
    Token(TokenName),
}

impl<T: TokenKind> Spec<T> {
    fn parse(input: &str) -> Result<Self, String> {
        let Some(reference) = input.strip_prefix('$') else {
            return T::parse_literal(input).map(Self::Literal);
        };
        let name = reference
            .strip_prefix(T::GROUP)
            .and_then(|rest| rest.strip_prefix('.'))
            .ok_or_else(|| {
                format!(
                    "token reference \"{input}\" must refer to the {0} group, as in \"${0}.name\"",
                    T::GROUP
                )
            })?;
        TokenName::check(name).map_err(|e| format!("in \"{input}\": {e}"))?;
        Ok(Self::Token(TokenName(name.to_owned())))
    }
}

impl<T: TokenKind> fmt::Display for Spec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(value) => value.write_literal(f),
            Self::Token(name) => write!(f, "${}.{name}", T::GROUP),
        }
    }
}

impl<'de, T: TokenKind> Deserialize<'de> for Spec<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SpecVisitor<T>(PhantomData<T>);

        impl<T: TokenKind> Visitor<'_> for SpecVisitor<T> {
            type Value = Spec<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string value or a \"${}.name\" token reference", T::GROUP)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Spec<T>, E> {
                Spec::parse(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(SpecVisitor(PhantomData))
    }
}

impl<T: TokenKind> Serialize for Spec<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A unitless line-height multiplier of the element's font size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LineHeight(f64);

impl LineHeight {
    pub const MIN: f64 = 0.8;
    pub const MAX: f64 = 3.0;

    pub fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for LineHeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(de::Error::custom(format!(
                "line height {value} must be a unitless number from {} to {}",
                Self::MIN,
                Self::MAX
            )));
        }
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_convert_absolute_units_and_keep_em_relative() {
        assert_eq!(Length::parse("12pt"), Ok(Length::Pt(12.0)));
        assert_eq!(Length::parse("1in"), Ok(Length::Pt(72.0)));
        assert_eq!(Length::parse("2.54cm"), Ok(Length::Pt(72.0)));
        assert_eq!(Length::parse("25.4mm"), Ok(Length::Pt(72.0)));
        assert_eq!(Length::parse("1.5em"), Ok(Length::Em(1.5)));
        assert_eq!(Length::Em(1.5).to_pt(Pt(10.0)), Pt(15.0));
    }

    #[test]
    fn lengths_reject_missing_unknown_and_negative_units() {
        for input in ["12", "12px", "50%", "1vw", "-1pt", "pt", "1e400pt"] {
            assert!(Length::parse(input).is_err(), "{input} should be rejected");
        }
    }

    #[test]
    fn colors_require_six_hex_digits() {
        assert_eq!(Color::parse("#1a2B3c"), Ok(Color([0x1a, 0x2b, 0x3c])));
        for input in ["1a2b3c", "#fff", "#gggggg", "red"] {
            assert!(Color::parse(input).is_err(), "{input} should be rejected");
        }
    }

    #[test]
    fn token_references_must_match_the_field_group() {
        let parse = |s: &str| serde_json::from_value::<Spec<Size>>(serde_json::json!(s));
        assert_eq!(parse("$sizes.body").unwrap(), Spec::Token(TokenName("body".into())));
        assert_eq!(parse("10pt").unwrap(), Spec::Literal(Size(Length::Pt(10.0))));
        let error = parse("$spacing.body").unwrap_err().to_string();
        assert!(error.contains("sizes group"), "{error}");
        assert!(parse("$sizes.").is_err());
        assert!(serde_json::from_value::<Spec<Size>>(serde_json::json!(12)).is_err());
    }
}
