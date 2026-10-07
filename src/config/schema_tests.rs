//! Keeps `schema/theme.v1.schema.json`, the typed theme, and the bundled default in agreement.

use jsonschema::Validator;
use serde_json::{Value, json};

use super::layer::{check_theme_layer, default_theme_value, deserialize_theme};

fn schema() -> Value {
    serde_json::from_str(include_str!("../../schema/theme.v1.schema.json")).expect("schema is valid JSON")
}

fn validator() -> Validator {
    jsonschema::validator_for(&schema()).expect("schema is a valid JSON Schema")
}

#[test]
fn default_theme_deserializes_and_matches_the_schema() {
    let theme = default_theme_value();
    deserialize_theme(theme.clone()).expect("default theme deserializes");
    let errors: Vec<_> = validator().iter_errors(&theme).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => root
            .pointer(reference.trim_start_matches('#'))
            .expect("reference resolves"),
        None => node,
    }
}

/// Compares fixed properties with the default theme in both directions. Maps are entered
/// per entry; arrays are not entered.
fn compare(root: &Value, node: &Value, theme: &Value, path: &str, problems: &mut Vec<String>) {
    let node = resolve(root, node);
    if let Some(branches) = node.get("anyOf").and_then(Value::as_array) {
        for branch in branches {
            compare(root, branch, theme, path, problems);
        }
        return;
    }
    let Some(fields) = theme.as_object() else { return };
    if let Some(properties) = node.get("properties").and_then(Value::as_object) {
        for key in properties.keys().filter(|k| !fields.contains_key(*k)) {
            problems.push(format!("{path}/{key} is in the schema but not in the default theme"));
        }
        for (key, value) in fields {
            match properties.get(key) {
                Some(child) => compare(root, child, value, &format!("{path}/{key}"), problems),
                None => problems.push(format!("{path}/{key} is in the default theme but not in the schema")),
            }
        }
    } else if let Some(entry) = node.get("additionalProperties").filter(|v| v.is_object()) {
        for (key, value) in fields {
            compare(root, entry, value, &format!("{path}/{key}"), problems);
        }
    }
}

#[test]
fn schema_and_default_theme_declare_the_same_fields() {
    let root = schema();
    let mut problems = Vec::new();
    compare(&root, &root, &default_theme_value(), "", &mut problems);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn schema_and_layer_check_agree_on_partial_themes() {
    let validator = validator();
    let cases = [
        (
            "nested override",
            json!({"version": 1, "styles": {"body": {"size": "11pt"}}}),
            true,
        ),
        (
            "array replacement",
            json!({"version": 1, "lists": {"bullets": ["*"]}}),
            true,
        ),
        ("permitted null", json!({"version": 1, "pages": {"first": null}}), true),
        (
            "prose width and unmirrored margins",
            json!({"version": 1, "page": {"text-width": "12cm", "wide": ["table"], "margins": {"mirror": false}}}),
            true,
        ),
        (
            "unknown wide block",
            json!({"version": 1, "page": {"wide": ["quote"]}}),
            false,
        ),
        (
            "slot group",
            json!({"version": 1, "pages": {"body": {"footer": [{"anchor": "top-right", "offset": {"x": "1em"},
                "width": "4cm", "align": "right", "slots": [{"text": "{page}|{pages}", "space-before": "2pt"}]}]}}}),
            true,
        ),
        (
            "unknown anchor",
            json!({"version": 1, "title-page": {"groups": [{"anchor": "center", "slots": []}]}}),
            false,
        ),
        (
            "band with left, center, and right slots",
            json!({"version": 1, "pages": {"body": {"footer": {"left": null, "center": {"text": "{page}"}, "right": null}}}}),
            false,
        ),
        (
            "null font face",
            json!({"version": 1, "fonts": {"X": {"regular": "x.otf", "bold": null}}}),
            true,
        ),
        (
            "null on required field",
            json!({"version": 1, "styles": {"body": {"size": null}}}),
            false,
        ),
        (
            "unknown field",
            json!({"version": 1, "styles": {"body": {"colour": "#000000"}}}),
            false,
        ),
        ("wrong version", json!({"version": 2}), false),
        ("missing version", json!({"styles": {}}), false),
        (
            "invalid unit",
            json!({"version": 1, "styles": {"body": {"size": "12px"}}}),
            false,
        ),
        (
            "negative length",
            json!({"version": 1, "styles": {"body": {"indent": "-1pt"}}}),
            false,
        ),
        (
            "percentage",
            json!({"version": 1, "styles": {"body": {"indent": "50%"}}}),
            false,
        ),
        (
            "token of the wrong group",
            json!({"version": 1, "styles": {"body": {"size": "$spacing.x"}}}),
            false,
        ),
        (
            "bad color",
            json!({"version": 1, "tokens": {"colors": {"text": "#fff"}}}),
            false,
        ),
        (
            "line height out of range",
            json!({"version": 1, "styles": {"body": {"line-height": 3.5}}}),
            false,
        ),
        (
            "heading depth 7",
            json!({"version": 1, "document": {"toc-depth": 7}}),
            false,
        ),
        (
            "partial custom page size",
            json!({"version": 1, "page": {"size": {"width": "10cm"}}}),
            false,
        ),
        (
            "invalid token name",
            json!({"version": 1, "tokens": {"sizes": {"a.b": "1pt"}}}),
            false,
        ),
        (
            "remote resource",
            json!({"version": 1, "images": {"logo": "https://example.com/a.png"}}),
            false,
        ),
        (
            "faces of other weights and a collection face",
            json!({"version": 1, "fonts": {"X": {"regular": {"file": "x.ttc", "index": 2}, "500": "m.otf", "300-italic": null}}}),
            true,
        ),
        (
            "a numeric face key for regular",
            json!({"version": 1, "fonts": {"X": {"regular": "x.otf", "400": "y.otf"}}}),
            false,
        ),
        (
            "a collection face without index",
            json!({"version": 1, "fonts": {"X": {"regular": {"file": "x.ttc"}}}}),
            false,
        ),
        (
            "numeric weight",
            json!({"version": 1, "styles": {"body": {"weight": 300, "tracking": -0.05}}}),
            true,
        ),
        (
            "weight between steps",
            json!({"version": 1, "styles": {"body": {"weight": 350}}}),
            false,
        ),
        (
            "tracking out of range",
            json!({"version": 1, "styles": {"body": {"tracking": 2}}}),
            false,
        ),
        (
            "custom style",
            json!({"version": 1, "custom-styles": {"checks": {"based-on": "list", "bullets": ["✓"]}}}),
            true,
        ),
        (
            "custom style without base",
            json!({"version": 1, "custom-styles": {"eyebrow": {"uppercase": true}}}),
            false,
        ),
    ];
    for (name, layer, valid) in cases {
        assert_eq!(validator.is_valid(&layer), valid, "schema verdict for {name}");
        assert_eq!(
            check_theme_layer(&layer).is_ok(),
            valid,
            "layer check verdict for {name}"
        );
    }
}
