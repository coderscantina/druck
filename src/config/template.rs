//! Slot text with `{placeholder}` values. Values are inserted as text, never interpreted.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A value a slot can insert. Metadata is known at configuration time; section titles and
/// page numbers are known after layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placeholder {
    Title,
    Subtitle,
    Author,
    Date,
    Abstract,
    Section,
    Subsection,
    Page,
}

impl Placeholder {
    const ALL: [(&'static str, Self); 8] = [
        ("title", Self::Title),
        ("subtitle", Self::Subtitle),
        ("author", Self::Author),
        ("date", Self::Date),
        ("abstract", Self::Abstract),
        ("section", Self::Section),
        ("subsection", Self::Subsection),
        ("page", Self::Page),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, p)| *p == self)
            .map(|(n, _)| *n)
            .expect("listed")
    }

    /// Whether the value depends on the page the slot appears on.
    pub fn is_page_dependent(self) -> bool {
        matches!(self, Self::Section | Self::Subsection | Self::Page)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Value(Placeholder),
}

/// Parsed slot text. `{{` and `}}` write literal braces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    source: String,
    segments: Vec<Segment>,
}

impl Template {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut segments = Vec::new();
        let mut text = String::new();
        let mut chars = source.chars();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.as_str().starts_with('{') => {
                    chars.next();
                    text.push('{');
                }
                '}' if chars.as_str().starts_with('}') => {
                    chars.next();
                    text.push('}');
                }
                '{' => {
                    let rest = chars.as_str();
                    let end = rest
                        .find('}')
                        .ok_or_else(|| format!("unclosed placeholder in \"{source}\""))?;
                    let name = &rest[..end];
                    let placeholder = Placeholder::ALL
                        .iter()
                        .find(|(n, _)| *n == name)
                        .map(|(_, p)| *p)
                        .ok_or_else(|| {
                            let known: Vec<_> = Placeholder::ALL.iter().map(|(n, _)| *n).collect();
                            format!("unknown placeholder {{{name}}} (use {})", known.join(", "))
                        })?;
                    if !text.is_empty() {
                        segments.push(Segment::Text(std::mem::take(&mut text)));
                    }
                    segments.push(Segment::Value(placeholder));
                    chars = rest[end + 1..].chars();
                }
                '}' => return Err(format!("unmatched \"}}\" in \"{source}\"; write \"}}}}\" for a brace")),
                c => text.push(c),
            }
        }
        if !text.is_empty() {
            segments.push(Segment::Text(text));
        }
        Ok(Self {
            source: source.to_owned(),
            segments,
        })
    }

    #[allow(dead_code, reason = "slot layout arrives in milestone 08")]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn placeholders(&self) -> impl Iterator<Item = Placeholder> + '_ {
        self.segments.iter().filter_map(|s| match s {
            Segment::Value(p) => Some(*p),
            Segment::Text(_) => None,
        })
    }
}

impl<'de> Deserialize<'de> for Template {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let source = String::deserialize(deserializer)?;
        Self::parse(&source).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Template {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_placeholders_and_escaped_braces() {
        let template = Template::parse("{{{title}}} p. {page}").unwrap();
        assert_eq!(
            template.segments(),
            [
                Segment::Text("{".into()),
                Segment::Value(Placeholder::Title),
                Segment::Text("} p. ".into()),
                Segment::Value(Placeholder::Page),
            ]
        );
    }

    #[test]
    fn rejects_unknown_and_unbalanced_placeholders() {
        for source in ["{chapter}", "{title", "title}", "{}"] {
            assert!(Template::parse(source).is_err(), "{source} should be rejected");
        }
    }
}
