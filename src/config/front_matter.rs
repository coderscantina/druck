//! YAML front matter: document metadata and the limited document settings surface.
//!
//! CLI `--set` overrides use the same keys. Settings map onto fixed theme paths, so a
//! document can adjust the design but never redefine templates.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value, json};

use super::non_null;
use super::template::is_meta_key;
use super::theme::{CitationStyle, FontFamily, HeadingDepth, Lang, PageSize};
use super::values::{FontName, LineHeight, Size, Spacing, Spec};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FrontMatter {
    #[serde(default, deserialize_with = "non_null")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    pub subtitle: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    pub author: Option<Authors>,
    #[serde(default, deserialize_with = "non_null")]
    pub date: Option<String>,
    #[serde(default, deserialize_with = "non_null", rename = "abstract")]
    pub abstract_: Option<String>,
    /// Free metadata for `{meta.key}` slots.
    #[serde(default, deserialize_with = "meta")]
    pub meta: BTreeMap<String, MetaValue>,

    /// Theme file, relative to the document.
    #[serde(default, deserialize_with = "non_null")]
    pub theme: Option<String>,
    /// BibTeX file, relative to the document.
    #[serde(default, deserialize_with = "non_null")]
    pub bibliography: Option<String>,
    /// Cover image of an EPUB, relative to the document.
    #[serde(default, deserialize_with = "non_null")]
    pub cover: Option<String>,

    #[serde(default, deserialize_with = "non_null")]
    pub lang: Option<Lang>,
    #[serde(default, deserialize_with = "non_null")]
    pub title_page: Option<bool>,
    #[serde(default, deserialize_with = "non_null")]
    pub toc: Option<bool>,
    #[serde(default, deserialize_with = "non_null")]
    pub duplex: Option<bool>,
    #[serde(default, deserialize_with = "non_null")]
    pub numbered_headings: Option<bool>,
    #[serde(default, deserialize_with = "non_null")]
    pub numbering_depth: Option<HeadingDepth>,
    #[serde(default, deserialize_with = "non_null")]
    pub toc_depth: Option<HeadingDepth>,
    #[serde(default, deserialize_with = "non_null")]
    pub citation_style: Option<CitationStyle>,
    #[serde(default, deserialize_with = "non_null")]
    pub draft: Option<bool>,

    #[serde(default, deserialize_with = "non_null")]
    pub page_size: Option<PageSize>,
    #[serde(default, deserialize_with = "non_null")]
    pub margins: Option<MarginsSetting>,
    #[serde(default, deserialize_with = "non_null")]
    pub column_gap: Option<Spec<Spacing>>,
    #[serde(default, deserialize_with = "non_null")]
    pub font_size: Option<Spec<Size>>,
    #[serde(default, deserialize_with = "non_null")]
    pub line_height: Option<LineHeight>,
    #[serde(default, deserialize_with = "non_null")]
    pub paragraph_spacing: Option<Spec<Spacing>>,
    #[serde(default, deserialize_with = "non_null")]
    pub fonts: Option<FontsSetting>,
    /// Local font families, relative to the document.
    #[serde(default, deserialize_with = "non_null")]
    pub font_files: Option<BTreeMap<String, FontFamily>>,
}

/// Implements the visitor methods that read a number or boolean as its text, so `date: 2024` and
/// `offer: 2026` need no quotes. A float prints as its value, so `1.50` reads as `1.5`.
macro_rules! scalar_text {
    ($value:ty) => {
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<$value, E> {
            self.visit_str(&value.to_string())
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<$value, E> {
            self.visit_str(&value.to_string())
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<$value, E> {
            self.visit_str(&value.to_string())
        }

        fn visit_bool<E: de::Error>(self, value: bool) -> Result<$value, E> {
            self.visit_str(&value.to_string())
        }
    };
}

/// One author or a list of authors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authors(pub Vec<String>);

impl<'de> Deserialize<'de> for Authors {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AuthorsVisitor;

        impl<'de> Visitor<'de> for AuthorsVisitor {
            type Value = Authors;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an author name or a list of author names")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Authors, E> {
                Ok(Authors(vec![value.to_owned()]))
            }

            scalar_text!(Authors);

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Authors, A::Error> {
                let mut names = Vec::new();
                while let Some(name) = seq.next_element::<String>()? {
                    names.push(name);
                }
                Ok(Authors(names))
            }
        }

        deserializer.deserialize_any(AuthorsVisitor)
    }
}

/// A `meta` value: text, or a list of lines.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum MetaValue {
    Text(String),
    Lines(Vec<String>),
}

impl<'de> Deserialize<'de> for MetaValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MetaVisitor;

        impl<'de> Visitor<'de> for MetaVisitor {
            type Value = MetaValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a text or a list of lines")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<MetaValue, E> {
                Ok(MetaValue::Text(value.to_owned()))
            }

            scalar_text!(MetaValue);

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<MetaValue, A::Error> {
                let mut lines = Vec::new();
                while let Some(line) = seq.next_element::<String>()? {
                    lines.push(line);
                }
                Ok(MetaValue::Lines(lines))
            }
        }

        deserializer.deserialize_any(MetaVisitor)
    }
}

/// The `meta` map, whose keys must be usable in `{meta.key}`.
fn meta<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, MetaValue>, D::Error> {
    let map = BTreeMap::<String, MetaValue>::deserialize(deserializer)?;
    match map.keys().find(|key| !is_meta_key(key)) {
        Some(key) => Err(de::Error::custom(format!(
            "meta key \"{key}\" must consist of letters, digits, \"-\", and \"_\""
        ))),
        None => Ok(map),
    }
}

/// One length for all margins, or individual sides.
#[derive(Debug, Clone)]
pub enum MarginsSetting {
    All(Spec<Spacing>),
    Sides(MarginSides),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MarginSides {
    #[serde(default, deserialize_with = "non_null")]
    pub top: Option<Spec<Spacing>>,
    #[serde(default, deserialize_with = "non_null")]
    pub bottom: Option<Spec<Spacing>>,
    #[serde(default, deserialize_with = "non_null")]
    pub inner: Option<Spec<Spacing>>,
    #[serde(default, deserialize_with = "non_null")]
    pub outer: Option<Spec<Spacing>>,
}

impl<'de> Deserialize<'de> for MarginsSetting {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MarginsVisitor;

        impl<'de> Visitor<'de> for MarginsVisitor {
            type Value = MarginsSetting;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a length for all margins, or a map of top, bottom, inner, and outer")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<MarginsSetting, E> {
                Spec::deserialize(de::value::StrDeserializer::new(value)).map(MarginsSetting::All)
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<MarginsSetting, A::Error> {
                MarginSides::deserialize(de::value::MapAccessDeserializer::new(map)).map(MarginsSetting::Sides)
            }
        }

        deserializer.deserialize_any(MarginsVisitor)
    }
}

/// Fonts for the well-known `body`, `heading`, and `mono` font tokens.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FontsSetting {
    #[serde(default, deserialize_with = "non_null")]
    pub body: Option<Spec<FontName>>,
    #[serde(default, deserialize_with = "non_null")]
    pub heading: Option<Spec<FontName>>,
    #[serde(default, deserialize_with = "non_null")]
    pub mono: Option<Spec<FontName>>,
}

/// Settings that hold text. A number or boolean given for one is read as its source text.
pub const TEXT_SETTINGS: [&str; 4] = ["title", "subtitle", "date", "abstract"];

impl FrontMatter {
    /// The settings as a partial theme value, merged after the selected theme.
    pub fn theme_layer(&self) -> Value {
        let mut layer = Map::new();
        let mut set = |pointer: &[&str], value: Value| insert(&mut layer, pointer, value);

        let document = [
            ("lang", self.lang.as_ref().map(json)),
            ("title-page", self.title_page.map(Value::Bool)),
            ("toc", self.toc.map(Value::Bool)),
            ("duplex", self.duplex.map(Value::Bool)),
            ("numbered-headings", self.numbered_headings.map(Value::Bool)),
            ("numbering-depth", self.numbering_depth.as_ref().map(json)),
            ("toc-depth", self.toc_depth.as_ref().map(json)),
            ("citation-style", self.citation_style.as_ref().map(json)),
            ("draft", self.draft.map(Value::Bool)),
        ];
        for (key, value) in document {
            if let Some(value) = value {
                set(&["document", key], value);
            }
        }

        if let Some(size) = &self.page_size {
            set(&["page", "size"], json(size));
        }
        match &self.margins {
            Some(MarginsSetting::All(length)) => {
                for side in ["top", "bottom", "inner", "outer"] {
                    set(&["page", "margins", side], json(length));
                }
            }
            Some(MarginsSetting::Sides(sides)) => {
                let sides = [
                    ("top", &sides.top),
                    ("bottom", &sides.bottom),
                    ("inner", &sides.inner),
                    ("outer", &sides.outer),
                ];
                for (side, length) in sides {
                    if let Some(length) = length {
                        set(&["page", "margins", side], json(length));
                    }
                }
            }
            None => {}
        }
        if let Some(gap) = &self.column_gap {
            set(&["page", "column-gap"], json(gap));
        }
        if let Some(size) = &self.font_size {
            set(&["styles", "body", "size"], json(size));
        }
        if let Some(line_height) = &self.line_height {
            set(&["styles", "body", "line-height"], json(line_height));
        }
        if let Some(spacing) = &self.paragraph_spacing {
            set(&["styles", "body", "space-after"], json(spacing));
        }
        if let Some(fonts) = &self.fonts {
            for (token, font) in [
                ("body", &fonts.body),
                ("heading", &fonts.heading),
                ("mono", &fonts.mono),
            ] {
                if let Some(font) = font {
                    set(&["tokens", "fonts", token], json(font));
                }
            }
        }
        for (family, files) in self.font_files.iter().flatten() {
            set(&["fonts", family], json(files));
        }
        Value::Object(layer)
    }

    /// Field-wise override: values set in `later` replace those in `self`.
    pub fn metadata_overridden_by(&self, later: &Self) -> Metadata {
        Metadata {
            title: later.title.clone().or_else(|| self.title.clone()),
            subtitle: later.subtitle.clone().or_else(|| self.subtitle.clone()),
            authors: later
                .author
                .clone()
                .or_else(|| self.author.clone())
                .map(|a| a.0)
                .unwrap_or_default(),
            date: later.date.clone().or_else(|| self.date.clone()),
            abstract_: later.abstract_.clone().or_else(|| self.abstract_.clone()),
            meta: self.meta.clone().into_iter().chain(later.meta.clone()).collect(),
        }
    }
}

/// Document metadata available to template slots. Values are plain text.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Metadata {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub date: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_: Option<String>,
    pub meta: BTreeMap<String, MetaValue>,
}

fn insert(object: &mut Map<String, Value>, pointer: &[&str], value: Value) {
    let (last, parents) = pointer.split_last().expect("non-empty pointer");
    let mut target = object;
    for key in parents {
        target = target
            .entry(*key)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("settings paths only nest objects");
    }
    target.insert((*last).to_owned(), value);
}

fn json(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("settings values serialize to JSON")
}

/// Front matter split from a Markdown source.
#[derive(Debug, PartialEq, Eq)]
pub struct Split<'a> {
    /// The YAML text between the fences.
    pub yaml: &'a str,
    /// 1-based line number of the first YAML line in the source.
    pub first_line: u64,
    /// The Markdown after the closing fence.
    pub body: &'a str,
}

/// Splits leading `---` front matter, closed by `---` or `...`. Returns `None` without front matter.
pub fn split(source: &str) -> Result<Option<Split<'_>>, String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let Some(rest) = source.strip_prefix("---\n").or_else(|| source.strip_prefix("---\r\n")) else {
        return Ok(None);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let fence = line.trim_end_matches(['\n', '\r']);
        if fence == "---" || fence == "..." {
            return Ok(Some(Split {
                yaml: &rest[..offset],
                first_line: 2,
                body: &rest[offset + line.len()..],
            }));
        }
        offset += line.len();
    }
    Err("front matter starting on line 1 has no closing \"---\" line".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Result<FrontMatter, String> {
        serde_saphyr::from_str(yaml).map_err(|e| e.to_string())
    }

    #[test]
    fn splits_front_matter_from_the_body() {
        let split = split("---\ntitle: A\n---\n# Body\n").unwrap().unwrap();
        assert_eq!(split.yaml, "title: A\n");
        assert_eq!(split.body, "# Body\n");
        assert_eq!(super::split("# No front matter\n").unwrap(), None);
        assert!(super::split("---\ntitle: A\n").is_err());
    }

    #[test]
    fn maps_settings_onto_fixed_theme_paths() {
        let front = parse(
            "margins: 2cm\nfont-size: 11pt\ntoc: true\nfonts:\n  body: My Serif\nfont-files:\n  My Serif:\n    regular: fonts/my.otf\n",
        )
        .unwrap();
        let layer = front.theme_layer();
        assert_eq!(layer["page"]["margins"]["inner"], "56.69291338582677pt");
        assert_eq!(layer["styles"]["body"]["size"], "11pt");
        assert_eq!(layer["document"]["toc"], true);
        assert_eq!(layer["tokens"]["fonts"]["body"], "My Serif");
        assert_eq!(layer["fonts"]["My Serif"]["regular"], "fonts/my.otf");
        assert!(layer.get("lists").is_none());
    }

    #[test]
    fn rejects_null_unknown_keys_and_template_fields() {
        assert!(parse("toc: ~\n").is_err());
        assert!(parse("title:\n").is_err());
        assert!(parse("headers: {}\n").is_err());
        assert!(parse("pages: {}\n").is_err());
        assert!(parse("lang: fr\n").is_err());
    }

    #[test]
    fn reads_meta_text_and_lines_and_rejects_keys_no_slot_can_name() {
        let front = parse("meta:\n  client: ACME\n  address: [Main St 1, Vienna]\n").unwrap();
        assert_eq!(front.meta["client"], MetaValue::Text("ACME".into()));
        assert_eq!(
            front.meta["address"],
            MetaValue::Lines(vec!["Main St 1".into(), "Vienna".into()])
        );
        let later = parse("meta:\n  client: Other\n").unwrap();
        let meta = front.metadata_overridden_by(&later).meta;
        assert_eq!(meta["client"], MetaValue::Text("Other".into()));
        assert_eq!(meta.len(), 2, "overrides replace single entries");
        assert!(parse("meta:\n  my client: ACME\n").is_err());
        assert!(parse("meta:\n  client: {name: ACME}\n").is_err());
        assert!(parse("client: ACME\n").is_err());
    }

    #[test]
    fn reads_numbers_and_booleans_as_text_for_authors_and_meta() {
        let front = parse("author: 42\nmeta:\n  offer: 2026\n  ratio: 1.5\n  flag: true\n").unwrap();
        assert_eq!(front.author, Some(Authors(vec!["42".into()])));
        assert_eq!(front.meta["offer"], MetaValue::Text("2026".into()));
        assert_eq!(front.meta["ratio"], MetaValue::Text("1.5".into()));
        assert_eq!(front.meta["flag"], MetaValue::Text("true".into()));
        assert_eq!(
            parse("title: 1.50\ndate: 2024\n").unwrap().date.as_deref(),
            Some("2024")
        );
    }

    #[test]
    fn accepts_one_or_many_authors_and_yaml_1_2_scalars() {
        let one = parse("author: Ada\n").unwrap();
        let many = parse("author: [Ada, Grace]\ntitle: no\n").unwrap();
        assert_eq!(one.author, Some(Authors(vec!["Ada".into()])));
        assert_eq!(many.author, Some(Authors(vec!["Ada".into(), "Grace".into()])));
        assert_eq!(many.title.as_deref(), Some("no"));
    }
}
