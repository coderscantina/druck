//! Tokens of a theme: the four groups, who refers to them, and the text edits that keep every reference whole.
//! Holds no gpui types.

use serde_json::Value;

use super::patch;
use super::strings::Key;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Group {
    Fonts,
    Sizes,
    Spacing,
    Colors,
}

impl Group {
    pub const ALL: [Self; 4] = [Self::Fonts, Self::Sizes, Self::Spacing, Self::Colors];

    /// The member of `tokens`, and the name before the dot of a reference.
    pub fn key(self) -> &'static str {
        match self {
            Self::Fonts => "fonts",
            Self::Sizes => "sizes",
            Self::Spacing => "spacing",
            Self::Colors => "colors",
        }
    }

    pub fn reference(self, name: &str) -> String {
        format!("${}.{name}", self.key())
    }

    /// The group and name a reference such as `$sizes.body` means.
    pub fn parse(reference: &str) -> Option<(Self, &str)> {
        let (group, name) = reference.strip_prefix('$')?.split_once('.')?;
        Some((Self::ALL.into_iter().find(|candidate| candidate.key() == group)?, name))
    }
}

/// Token names are letters, digits, and hyphens, as the schema says.
pub fn is_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The tokens of a group in a theme value: name and value.
pub fn of(theme: &Value, group: Group) -> Vec<(String, Value)> {
    theme
        .get("tokens")
        .and_then(|tokens| tokens.get(group.key()))
        .and_then(Value::as_object)
        .map(|tokens| {
            tokens
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Where a theme refers to `reference`, as dotted paths of the strings that hold it.
pub fn uses(theme: &Value, reference: &str) -> Vec<String> {
    let mut found = Vec::new();
    walk(theme, &mut Vec::new(), &mut |path, text| {
        if text == reference {
            found.push(path.join("."));
        }
    });
    found
}

/// Every string value of a theme with its path. Array items are numbered.
pub fn walk(value: &Value, path: &mut Vec<String>, visit: &mut impl FnMut(&[String], &str)) {
    match value {
        Value::String(text) => visit(path, text),
        Value::Object(members) => {
            for (key, inner) in members {
                path.push(key.clone());
                walk(inner, path, visit);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                path.push(index.to_string());
                walk(inner, path, visit);
                path.pop();
            }
        }
        _ => {}
    }
}

/// The text with the token under a new name, and every reference to it following. A name taken is refused.
pub fn rename(theme: &Value, text: &str, group: Group, from: &str, to: &str) -> Result<String, String> {
    if !is_name(to) {
        return Err(crate::t!(Key::NotATokenName, name = to));
    }
    if of(theme, group).iter().any(|(name, _)| name == to) {
        return Err(crate::t!(Key::TokenExists, name = to));
    }
    let renamed = patch::rename_key(text, &["tokens", group.key(), from], to)?;
    Ok(patch::rename_values(
        &renamed,
        &group.reference(from),
        &group.reference(to),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const TEXT: &str = "{\n  \"version\": 1,\n  \"tokens\": {\n    \"sizes\": { \"big\": \"20pt\", \"huge\": \"$sizes.big\" }\n  },\n  \"styles\": {\n    \"title\": { \"size\": \"$sizes.big\" }\n  },\n  \"pages\": [ { \"size\": \"$sizes.big\" } ]\n}\n";

    #[test]
    fn a_reference_parses_into_its_group_and_name() {
        assert_eq!(Group::parse("$sizes.big"), Some((Group::Sizes, "big")));
        assert_eq!(Group::parse("$nope.big"), None);
        assert_eq!(Group::parse("20pt"), None);
    }

    #[test]
    fn uses_finds_references_in_objects_and_arrays() {
        let theme: Value = serde_json::from_str(TEXT).unwrap();
        assert_eq!(
            uses(&theme, "$sizes.big"),
            ["tokens.sizes.huge", "styles.title.size", "pages.0.size"]
        );
    }

    #[test]
    fn renaming_keeps_the_token_in_place_and_updates_every_reference() {
        let theme: Value = serde_json::from_str(TEXT).unwrap();
        let renamed = rename(&theme, TEXT, Group::Sizes, "big", "large").unwrap();
        assert_eq!(
            renamed,
            TEXT.replace("\"big\"", "\"large\"")
                .replace("$sizes.big", "$sizes.large")
        );
        assert!(rename(&theme, TEXT, Group::Sizes, "big", "huge").is_err());
        assert!(rename(&theme, TEXT, Group::Sizes, "big", "no way").is_err());
        let again: Value = serde_json::from_str(&renamed).unwrap();
        assert_eq!(of(&again, Group::Sizes)[1], ("huge".to_owned(), json!("$sizes.large")));
    }
}
