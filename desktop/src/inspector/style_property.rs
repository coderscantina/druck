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

    /// The values the interface can produce for a property, and values it can also be given by typing that the engine
    /// must refuse.
    fn samples(property: Property, theme: &Value) -> Vec<Value> {
        let tokens_of = |group: Group| -> Vec<Value> {
            crate::theme_file::tokens::of(theme, group)
                .into_iter()
                .take(3)
                .map(|(name, _)| Value::String(group.reference(&name)))
                .collect()
        };
        let strings = |values: &[&str]| values.iter().map(|value| Value::String((*value).to_owned())).collect::<Vec<_>>();
        match property.control {
            Control::Reference(group) => {
                let mut values: Vec<Value> = tokens_of(group).into_iter().take(1).collect();
                values.extend(match group {
                    Group::Fonts => strings(&["Libertinus Serif", "$fonts.mono", "No Such Family"]),
                    Group::Sizes => strings(&["12pt", "huge", "$sizes.nope"]),
                    Group::Spacing => strings(&["6mm", "wide", "$spacing.nope"]),
                    Group::Colors => strings(&["#aa3366", "red", "$colors.nope"]),
                });
                values
            }
            Control::Choice(choices) => {
                let mut values: Vec<Value> = choices.iter().map(|(value, _)| Value::String((*value).to_owned())).collect();
                values.push(Value::String("sideways".to_owned()));
                values
            }
            Control::Number if property.key() == "tracking" => vec![Value::from(0.05), Value::from(3)],
            Control::Number => vec![Value::from(1.2), Value::from(9)],
            Control::Weight => vec![Value::from("bold"), Value::from(600), Value::from(450)],
            Control::Toggle => vec![Value::Bool(true), Value::Bool(false)],
            Control::Align => strings(&["center", "justify", "diagonal"]),
            Control::List => vec![serde_json::json!(["•", "–"]), serde_json::json!([])],
            // A marker in a heading font may lack the bullet glyphs, which is a font's limit and not the interface's.
            Control::StyleName => strings(&["body", "list", "no-such-style"]),
        }
    }

    /// Every theme the app starts from in the tests: the bundled one and each sample theme, as scratch copies.
    fn fixtures(scratch: &crate::theme_file::tests::Scratch) -> Vec<(std::path::PathBuf, bool)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(root.join("samples/themes")).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "json") {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                let name = name.trim_end_matches(".json");
                let copy = scratch.write(&format!("project/themes/{name}/theme.json"), &std::fs::read_to_string(&path).unwrap());
                found.push((copy, ["report", "typography-styles"].contains(&name)));
            }
        }
        // Their images and fonts sit beside the originals, so copy those next to every scratch theme.
        for (theme, _) in &found {
            let folder = theme.parent().unwrap();
            for entry in std::fs::read_dir(root.join("samples/themes")).unwrap().flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|extension| extension != "json") {
                    std::fs::copy(&path, folder.join(path.file_name().unwrap())).unwrap();
                }
            }
        }
        // Some sample themes name the bundled fonts by a path from `samples/themes`.
        let fonts = scratch.0.join("project/fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        for entry in std::fs::read_dir(root.join("fonts")).unwrap().flatten() {
            std::fs::copy(entry.path(), fonts.join(entry.file_name())).unwrap();
        }
        found.push((scratch.write("project/themes/bare/theme.json", "{\n  \"version\": 1\n}\n"), true));
        // Machine themes are tried too when the machine has any: always as copies, never in place.
        if let Some(common) = crate::theme_file::library::common_dir().filter(|dir| dir.is_dir()) {
            for entry in std::fs::read_dir(&common).unwrap().flatten().take(3) {
                let path = entry.path();
                if path.extension().is_some_and(|extension| extension == "json") {
                    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
                    let copy = scratch.write(&format!("project/themes/machine-{name}/theme.json"), &std::fs::read_to_string(&path).unwrap_or_default());
                    // Assets of a machine theme sit under the common folder, so the copy names them by absolute path.
                    let text = std::fs::read_to_string(&copy).unwrap_or_default();
                    if serde_json::from_str::<Value>(&text).is_ok() && text.contains("\"version\"") {
                        let mut value: Value = serde_json::from_str(&text).unwrap();
                        absolutize(&mut value, &common);
                        std::fs::write(&copy, serde_json::to_string_pretty(&value).unwrap()).unwrap();
                        found.push((copy, false));
                    }
                }
            }
        }
        found
    }

    /// Turns the relative resource paths of a theme into absolute ones under `base`, for a copy elsewhere.
    fn absolutize(value: &mut Value, base: &std::path::Path) {
        for (section, deep) in [("fonts", 2usize), ("images", 0)] {
            let Some(map) = value.get_mut(section).and_then(Value::as_object_mut) else { continue };
            for entry in map.values_mut() {
                if deep == 0 {
                    if let Some(path) = entry.as_str() {
                        *entry = Value::String(base.join(path).to_string_lossy().into_owned());
                    }
                } else if let Some(faces) = entry.as_object_mut() {
                    for face in faces.values_mut() {
                        match face {
                            Value::String(path) => *face = Value::String(base.join(&*path).to_string_lossy().into_owned()),
                            Value::Object(object) => {
                                if let Some(Value::String(path)) = object.get_mut("file") {
                                    *path = base.join(&*path).to_string_lossy().into_owned();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_change_the_panel_can_make_is_refused_or_gives_a_theme_the_engine_renders() {
        use crate::preview::job::Typesetter;
        use crate::session::{Joined, Kind};
        use crate::theme_file::editor::{Change, Dirs, Editor, Outcome};
        use crate::theme_file::tests::Scratch;
        use druck::cancel::Cancel;

        let scratch = Scratch::new();
        let dirs = Dirs {
            project: Some(scratch.0.join("project")),
            common: Some(scratch.0.join("common")),
        };
        let specimen = include_str!("../../assets/specimen.md");
        let (mut accepted, mut refused, mut rendered) = (0, 0, 0);
        let rendered_fixtures = ["bare", "typography-styles"];
        for (path, full) in fixtures(&scratch) {
            let mut editor = Editor::open(Some(&path), dirs.clone());
            let own_problems = editor.file().unwrap().problems().len();
            // The properties are what is under test, and they are the same for every style of a kind.
            let styles: Vec<(Vec<String>, Vec<Property>)> = vec![
                (vec!["styles".to_owned(), "body".to_owned()], of_builtin("body")),
                (vec!["styles".to_owned(), "heading-1".to_owned()], vec![NUMBER_GAP]),
                (
                    vec!["custom-styles".to_owned(), "probe".to_owned()],
                    of_custom().into_iter().filter(|property| !BLOCK.contains(property)).collect(),
                ),
            ];
            editor
                .apply(Change::set(&["custom-styles", "probe", "based-on"], Value::String("body".to_owned())))
                .unwrap();
            let styles = if full { styles } else { styles.into_iter().take(1).collect() };
            let renders = rendered_fixtures.contains(&path.parent().unwrap().file_name().unwrap().to_string_lossy().into_owned().as_str());
            let count = styles.len();
            let mut batch: Vec<Vec<String>> = Vec::new();
            for (index, (prefix, properties)) in styles.into_iter().enumerate() {
                batch.push(prefix.clone());
                for property in properties {
                    if property == BASED_ON {
                        continue;
                    }
                    let theme = editor.file().unwrap().merged().clone();
                    for value in samples(property, &theme) {
                        let before = std::fs::read_to_string(&path).unwrap();
                        let at = property.path_in(&prefix.iter().map(String::as_str).collect::<Vec<_>>());
                        match editor.apply(Change::Set(at, value.clone())) {
                            Ok(Outcome::Saved) => {
                                accepted += 1;
                                let reopened = crate::theme_file::ThemeFile::open(&path).unwrap();
                                assert!(
                                    reopened.problems().len() <= own_problems,
                                    "{} = {value} left problems in {}: {:?}",
                                    property.key(),
                                    path.display(),
                                    reopened.problems()
                                );
                            }
                            Ok(Outcome::NeedsDecision) => panic!("a project theme asked a question"),
                            Err(_) => {
                                refused += 1;
                                assert_eq!(
                                    std::fs::read_to_string(&path).unwrap(),
                                    before,
                                    "{} = {value} was refused but changed the file",
                                    property.key()
                                );
                            }
                        }
                    }
                }
                if !renders || index + 1 != count {
                    continue;
                }
                // What the author has now must typeset, since each change was accepted on its own.
                let typeset = Typesetter::new(Kind::Single { path: path.with_file_name("probe.md") }, Some(path.clone()))
                    .run(Joined::single(specimen.to_owned(), 0), &Cancel::default());
                match typeset {
                    Ok(_) => rendered += 1,
                    Err(druck::Failure::Errors(errors)) => {
                        panic!("{} after {:?} no longer typesets: {:?}", path.display(), batch, errors.first())
                    }
                    Err(other) => panic!("{} failed: {other:?}", path.display()),
                }
                batch.clear();
            }
        }
        assert!(accepted > 100 && refused > 30, "{accepted} accepted, {refused} refused");
        assert!(rendered >= 2, "{rendered} typesets");
    }
}
