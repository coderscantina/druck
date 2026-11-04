//! The `exports` of `druck.yaml`: reading them as written and changing one value at a time as a text splice, so the
//! manifest's comments, order, and every other byte stay. Holds no gpui types.

use druck::editor::resolved_config::FrontMatter;

use crate::session::details::ORDER;
use crate::session::yaml::{self, Entry};
use crate::source::Edit;

/// An export as written. Empty text and `false` mean the key is not there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Def {
    pub name: String,
    pub theme: String,
    pub output: String,
    /// As written, such as `3mm`.
    pub bleed: String,
    pub crop_marks: bool,
    pub draft: bool,
    pub is_final: bool,
}

/// The fields of an export's block, as written. `{}` is an export with no fields.
fn fields(text: &str, export: &Entry) -> Result<(Vec<Entry>, usize), String> {
    let inline = text[export.colon_end..yaml::value_end(text, export.colon_end, export.line_end)].trim();
    if inline == "{}" || (inline.is_empty() && export.end == export.line_end) {
        return Ok((Vec::new(), 4));
    }
    yaml::children(text, export)
}

fn exports_of(text: &str) -> Result<Option<(Entry, Vec<Entry>, usize)>, String> {
    let top = yaml::entries(text, 0..text.len(), 0)?;
    let Some(found) = top.into_iter().find(|entry| entry.key == "exports") else {
        return Ok(None);
    };
    let (list, indent) = yaml::children(text, &found)?;
    Ok(Some((found, list, indent)))
}

/// The exports a manifest names. Text that cannot be read as a block of exports is an error.
pub fn read(text: &str) -> Result<Vec<Def>, String> {
    let Some((_, list, _)) = exports_of(text)? else {
        return Ok(Vec::new());
    };
    list.iter()
        .map(|export| {
            let (items, _) = fields(text, export)?;
            let mut def = Def {
                name: export.key.clone(),
                ..Def::default()
            };
            for item in &items {
                let end = yaml::value_end(text, item.colon_end, item.line_end);
                let value = yaml::unquote(text[item.colon_end..end].trim());
                let on = value.eq_ignore_ascii_case("true");
                match item.key.as_str() {
                    "theme" => def.theme = value,
                    "output" => def.output = value,
                    "bleed" => def.bleed = value,
                    "crop-marks" => def.crop_marks = on,
                    "draft" => def.draft = on,
                    "final" => def.is_final = on,
                    _ => {}
                }
            }
            Ok(def)
        })
        .collect()
}

/// Whether a name can be an export's key without quoting.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Sets `exports.<name>.<key>` to `value`, already written as YAML, or removes the key for `None`. Creates the export,
/// and `exports`, as needed. `None` when there is nothing to change.
pub fn set_field(text: &str, name: &str, key: &str, value: Option<&str>) -> Result<Option<Edit>, String> {
    let newline = yaml::newline(text);
    let Some((exports, list, indent)) = exports_of(text)? else {
        let Some(value) = value else { return Ok(None) };
        let top = yaml::entries(text, 0..text.len(), 0)?;
        let lines = format!("exports:{newline}  {name}:{newline}    {key}: {value}");
        return Ok(Some(yaml::insert(text, &top, "exports", &lines, ORDER)));
    };
    let lead = if text[..exports.end].ends_with('\n') {
        ""
    } else {
        newline
    };
    let Some(export) = list.iter().find(|export| export.key == name) else {
        let Some(value) = value else { return Ok(None) };
        let pad = " ".repeat(indent);
        let lines = format!("{lead}{pad}{name}:{newline}{pad}{pad}{key}: {value}{newline}");
        return Ok(Some(Edit::new(exports.end..exports.end, lines)));
    };
    let (items, field_indent) = fields(text, export)?;
    let pad = " ".repeat(field_indent);
    let Some(item) = items.iter().find(|item| item.key == key) else {
        let Some(value) = value else { return Ok(None) };
        if items.is_empty() {
            // The export had no block, as in `name: {}`.
            let start = export.colon_end;
            let lines = format!(":{newline}{pad}{key}: {value}{newline}");
            let keep = &text[export.line_end..export.end];
            return Ok(Some(Edit::new(start - 1..export.end, format!("{lines}{keep}"))));
        }
        let lead = if text[..export.end].ends_with('\n') {
            ""
        } else {
            newline
        };
        return Ok(Some(Edit::new(
            export.end..export.end,
            format!("{lead}{pad}{key}: {value}{newline}"),
        )));
    };
    Ok(Some(match value {
        Some(value) => yaml::replace_value(text, item, value),
        None if items.len() == 1 => {
            let pad = " ".repeat(indent);
            Edit::new(export.start..export.end, format!("{pad}{name}: {{}}{newline}"))
        }
        None => Edit::new(item.start..item.end, ""),
    }))
}

/// Writes an export's value as YAML: a bleed, theme, or output as text, a switch as `true`.
pub fn written(key: &str, value: &str) -> String {
    match key {
        "crop-marks" | "draft" | "final" => value.to_owned(),
        _ => yaml::scalar(value),
    }
}

/// Whether the engine reads `exports.<name>.<key>` as the value that was set. A splice that did not take must not be
/// saved.
pub fn agrees(settings: &FrontMatter, name: &str, key: &str, value: Option<&str>) -> bool {
    let Some(export) = settings.exports.as_ref().and_then(|exports| exports.get(name)) else {
        return false;
    };
    let export = &export.value;
    match key {
        "theme" => export.theme.as_deref() == value,
        "output" => export.output.as_deref() == value,
        "bleed" => export.bleed.is_some() == value.is_some(),
        "crop-marks" => export.crop_marks == (value == Some("true")),
        "draft" => export.draft == value.map(|value| value == "true"),
        "final" => export.is_final == (value == Some("true")),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, edit: Option<Edit>) -> String {
        let edit = edit.expect("a change");
        format!("{}{}{}", &text[..edit.range.start], edit.text, &text[edit.range.end..])
    }

    const MANIFEST: &str = "# Book\ntitle: A\nchapters:\n  - a.md\nexports:\n  # for the printer\n  print:\n    theme: memoir # print theme\n    bleed: 3mm\n    final: true\n  review:\n    draft: true\n";

    #[test]
    fn exports_are_read_as_written() {
        let defs = read(MANIFEST).expect("reads");
        assert_eq!(defs.len(), 2);
        assert_eq!(
            defs[0],
            Def {
                name: "print".into(),
                theme: "memoir".into(),
                bleed: "3mm".into(),
                is_final: true,
                ..Def::default()
            }
        );
        assert!(defs[1].draft && !defs[1].is_final);
        assert!(read("title: A\n").expect("reads").is_empty());
    }

    #[test]
    fn a_changed_value_keeps_the_comment_on_its_line() {
        let edit = set_field(MANIFEST, "print", "theme", Some("book")).expect("edits");
        let after = apply(MANIFEST, edit);
        assert!(after.contains("    theme: book # print theme\n"), "{after}");
        assert_eq!(after.replace("book # print", "memoir # print"), MANIFEST);
    }

    #[test]
    fn a_new_field_joins_the_end_of_its_export_and_a_removed_one_leaves_the_rest() {
        let after = apply(
            MANIFEST,
            set_field(MANIFEST, "print", "crop-marks", Some("true")).expect("edits"),
        );
        assert!(
            after.contains("    final: true\n    crop-marks: true\n  review:"),
            "{after}"
        );
        let after = apply(MANIFEST, set_field(MANIFEST, "print", "bleed", None).expect("edits"));
        assert_eq!(after, MANIFEST.replace("    bleed: 3mm\n", ""));
    }

    #[test]
    fn a_new_export_goes_at_the_end_and_creates_exports_if_missing() {
        let after = apply(
            MANIFEST,
            set_field(MANIFEST, "web", "output", Some("build/web.epub")).expect("edits"),
        );
        assert!(
            after.ends_with("    draft: true\n  web:\n    output: build/web.epub\n"),
            "{after}"
        );
        let bare = "title: A\nchapters:\n  - a.md\n";
        let after = apply(bare, set_field(bare, "web", "draft", Some("true")).expect("edits"));
        assert_eq!(
            after,
            "title: A\nchapters:\n  - a.md\nexports:\n  web:\n    draft: true\n"
        );
        assert!(set_field(bare, "web", "draft", None).expect("fine").is_none());
    }

    #[test]
    fn removing_the_last_field_keeps_the_export() {
        let text = "exports:\n  review:\n    draft: true\n";
        let after = apply(text, set_field(text, "review", "draft", None).expect("edits"));
        assert_eq!(after, "exports:\n  review: {}\n");
        let again = apply(
            &after,
            set_field(&after, "review", "final", Some("true")).expect("edits"),
        );
        assert_eq!(again, "exports:\n  review:\n    final: true\n");
    }

    #[test]
    fn names_are_plain_words() {
        assert!(is_valid_name("print-2"));
        assert!(!is_valid_name("my export"));
        assert!(!is_valid_name(""));
    }
}
