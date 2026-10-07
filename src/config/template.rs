//! Slot text with `{placeholder}` values. Values are inserted as text, never interpreted.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A value a slot or the body can insert. Metadata, the build date, and text statistics are known
/// before layout; section titles and page numbers after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placeholder {
    Title,
    Subtitle,
    Author,
    Date,
    Abstract,
    Section,
    Subsection,
    Page,
    Pages,
    /// The page within the pages that show the current `{section}`, and their number.
    SectionPage,
    SectionPages,
    /// Characters with and without spaces, words, sentences, and paragraphs; see [`crate::statistics`].
    Chars,
    CharsNoSpaces,
    Words,
    Sentences,
    Paragraphs,
    /// Reading time in whole minutes.
    ReadingTime,
    /// Numbered figures and tables.
    Figures,
    Tables,
    /// The day of the run, and its year.
    BuildDate,
    Year,
    /// The `draft` label, only in drafts.
    Draft,
    /// An entry of the front matter `meta` map.
    Meta(String),
}

impl Placeholder {
    const FIXED: [(&'static str, Self); 22] = [
        ("title", Self::Title),
        ("subtitle", Self::Subtitle),
        ("author", Self::Author),
        ("date", Self::Date),
        ("abstract", Self::Abstract),
        ("section", Self::Section),
        ("subsection", Self::Subsection),
        ("page", Self::Page),
        ("pages", Self::Pages),
        ("section-page", Self::SectionPage),
        ("section-pages", Self::SectionPages),
        ("chars", Self::Chars),
        ("chars-no-spaces", Self::CharsNoSpaces),
        ("words", Self::Words),
        ("sentences", Self::Sentences),
        ("paragraphs", Self::Paragraphs),
        ("reading-time", Self::ReadingTime),
        ("figures", Self::Figures),
        ("tables", Self::Tables),
        ("build-date", Self::BuildDate),
        ("year", Self::Year),
        ("draft", Self::Draft),
    ];

    /// The placeholder `name` names, as written between braces.
    pub fn parse(name: &str) -> Result<Self, String> {
        if let Some(key) = name.strip_prefix("meta.") {
            return if is_meta_key(key) {
                Ok(Self::Meta(key.to_owned()))
            } else {
                Err(format!(
                    "placeholder {{{name}}} needs a meta key of letters, digits, \"-\", and \"_\""
                ))
            };
        }
        Self::FIXED
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, p)| p.clone())
            .ok_or_else(|| {
                let known: Vec<_> = Self::FIXED.iter().map(|(n, _)| *n).collect();
                format!("unknown placeholder {{{name}}} (use {}, or meta.key)", known.join(", "))
            })
    }

    /// Whether the value depends on the page the slot appears on, or on the page count.
    pub fn is_page_dependent(&self) -> bool {
        matches!(
            self,
            Self::Section | Self::Subsection | Self::Page | Self::Pages | Self::SectionPage | Self::SectionPages
        )
    }
}

impl fmt::Display for Placeholder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Meta(key) => write!(f, "meta.{key}"),
            fixed => {
                let (name, _) = Self::FIXED.iter().find(|(_, p)| p == fixed).expect("listed");
                f.write_str(name)
            }
        }
    }
}

/// Whether `key` can name a `meta` entry: ASCII letters, digits, `-`, and `_`.
pub fn is_meta_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The value of a placeholder: text, or lines that each start a new slot line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Lines(Vec<String>),
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
                    let placeholder = Placeholder::parse(&rest[..end])?;
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

    #[cfg(test)]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The slot's lines with every placeholder replaced by its value, or the first placeholder without
    /// one. A line break in the template and each entry of a [`Value::Lines`] after the first start a
    /// new line. Blank values, and lists without a non-blank entry, count as missing; blank entries
    /// are skipped.
    pub fn fill(&self, value: impl Fn(&Placeholder) -> Option<Value>) -> Result<Vec<String>, Placeholder> {
        let mut lines = vec![String::new()];
        let mut extend = |pieces: &mut dyn Iterator<Item = &str>| {
            for (index, piece) in pieces.enumerate() {
                if index > 0 {
                    lines.push(String::new());
                }
                lines.last_mut().expect("never empty").push_str(piece);
            }
        };
        for segment in &self.segments {
            match segment {
                Segment::Text(piece) => extend(&mut piece.split('\n')),
                Segment::Value(placeholder) => match value(placeholder) {
                    Some(Value::Text(text)) if !text.trim().is_empty() => extend(&mut std::iter::once(text.as_str())),
                    Some(Value::Lines(entries)) if entries.iter().any(|e| !e.trim().is_empty()) => {
                        extend(&mut entries.iter().map(String::as_str).filter(|e| !e.trim().is_empty()));
                    }
                    _ => return Err(placeholder.clone()),
                },
            }
        }
        Ok(lines)
    }

    pub fn placeholders(&self) -> impl Iterator<Item = &Placeholder> + '_ {
        self.segments.iter().filter_map(|s| match s {
            Segment::Value(p) => Some(p),
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
        let template = Template::parse("{{{title}}} p. {page}/{pages} {meta.offer-no}").unwrap();
        assert_eq!(
            template.segments(),
            [
                Segment::Text("{".into()),
                Segment::Value(Placeholder::Title),
                Segment::Text("} p. ".into()),
                Segment::Value(Placeholder::Page),
                Segment::Text("/".into()),
                Segment::Value(Placeholder::Pages),
                Segment::Text(" ".into()),
                Segment::Value(Placeholder::Meta("offer-no".into())),
            ]
        );
    }

    #[test]
    fn fills_values_as_text_and_names_the_first_missing_one() {
        let template = Template::parse("{title} ({date})").unwrap();
        let value = |placeholder: &Placeholder| match placeholder {
            Placeholder::Title => Some(Value::Text("*Not* {markup}".to_owned())),
            Placeholder::Date => Some(Value::Text("  ".to_owned())),
            Placeholder::Meta(_) => Some(Value::Lines(vec![" ".to_owned()])),
            _ => None,
        };
        assert_eq!(template.fill(value), Err(Placeholder::Date));
        let filled = Template::parse("{title}!").unwrap().fill(value);
        assert_eq!(filled, Ok(vec!["*Not* {markup}!".to_owned()]));
        let blank = Template::parse("{meta.lines}").unwrap().fill(value);
        assert_eq!(blank, Err(Placeholder::Meta("lines".into())));
    }

    #[test]
    fn template_line_breaks_and_list_entries_start_new_lines() {
        let template = Template::parse("To: {meta.address}\nRef. {meta.ref}").unwrap();
        let lines = template.fill(|placeholder| match placeholder {
            Placeholder::Meta(key) if key == "address" => {
                Some(Value::Lines(vec!["ACME".into(), "".into(), "Main St 1".into()]))
            }
            _ => Some(Value::Text("A-7".into())),
        });
        assert_eq!(lines.unwrap(), ["To: ACME", "Main St 1", "Ref. A-7"]);
    }

    #[test]
    fn rejects_unknown_and_unbalanced_placeholders() {
        for source in ["{chapter}", "{title", "title}", "{}", "{meta.}", "{meta.a b}"] {
            assert!(Template::parse(source).is_err(), "{source} should be rejected");
        }
    }
}
