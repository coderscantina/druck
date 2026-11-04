//! Edits block-style YAML text in place, for `druck.yaml` and front matter. The engine reads the file; this only finds
//! where a value is written and changes those bytes, so comments, order, and every other value stay as written.
//!
//! The scan reads top-level `key:` lines and what is indented or dashed below them, and rejects a shape it cannot
//! read. Callers check the result with the engine's parser, see `details.rs`, so a misread never reaches disk.

use std::ops::Range;

use crate::source::Edit;

/// One `key: value` of a mapping: the key's line and whatever belongs to its value below it.
pub(crate) struct Entry {
    pub(crate) key: String,
    /// The start of the key's line.
    pub(crate) start: usize,
    /// Just after the colon.
    pub(crate) colon_end: usize,
    /// The end of the key's own line, with its line ending.
    pub(crate) line_end: usize,
    /// The end of the entry's last line, with its line ending. Comments and blank lines after it are not part of it.
    pub(crate) end: usize,
}

/// The entries of the mapping in `range`, whose keys sit at `indent`.
pub(crate) fn entries(text: &str, range: Range<usize>, indent: usize) -> Result<Vec<Entry>, String> {
    let mut found: Vec<Entry> = Vec::new();
    let mut at = range.start;
    for line in text[range].split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" || trimmed == "..." {
            continue;
        }
        let width = line.len() - line.trim_start_matches(' ').len();
        let continues = width > indent
            || (width == indent && (trimmed == "-" || trimmed.starts_with("- ")))
            || trimmed.starts_with(']')
            || trimmed.starts_with('}');
        if continues {
            match found.last_mut() {
                Some(entry) => entry.end = at,
                None => return Err(format!("cannot read this YAML line: {trimmed}")),
            }
            continue;
        }
        if width < indent {
            return Err(format!("cannot read this YAML line: {trimmed}"));
        }
        let Some((key, colon)) = split_key(&line[width..]) else {
            return Err(format!("cannot read this YAML line: {trimmed}"));
        };
        found.push(Entry {
            key,
            start,
            colon_end: start + width + colon,
            line_end: at,
            end: at,
        });
    }
    Ok(found)
}

/// A line's key, without quotes, and the byte after its colon.
fn split_key(line: &str) -> Option<(String, usize)> {
    let quote = line.chars().next().filter(|first| *first == '"' || *first == '\'');
    if let Some(quote) = quote {
        let close = line[1..].find(quote)? + 1;
        let after = line[close + 1..].trim_start_matches(' ');
        let colon = line.len() - after.len();
        return after.starts_with(':').then(|| (line[1..close].to_owned(), colon + 1));
    }
    let bytes = line.as_bytes();
    let colon = (0..bytes.len())
        .find(|&at| bytes[at] == b':' && matches!(bytes.get(at + 1), None | Some(b' ' | b'\t' | b'\r' | b'\n')))?;
    let key = line[..colon].trim_end();
    (!key.starts_with(['{', '['])).then(|| (key.to_owned(), colon + 1))
}

/// The line ending the text uses.
pub(crate) fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Where a one-line value ends: before a trailing comment and spaces.
pub(crate) fn value_end(text: &str, from: usize, line_end: usize) -> usize {
    let line = &text[from..line_end];
    let bytes = line.as_bytes();
    let mut quote = None;
    let mut end = line.len();
    for (at, &byte) in bytes.iter().enumerate() {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'#' && at > 0 && bytes[at - 1] == b' ' => {
                end = at;
                break;
            }
            None => {}
        }
    }
    from + line[..end].trim_end().len()
}

/// Text as a YAML value: plain when YAML reads it back as the same text, else double-quoted.
pub fn scalar(value: &str) -> String {
    let first = value.chars().next();
    let starts_plain = first.is_some_and(|first| first.is_alphanumeric());
    let plain = starts_plain
        && value.trim() == value
        && !value.contains(": ")
        && !value.contains(" #")
        && !value.ends_with(':')
        && !value.contains(['\n', '\r', '\t', '"'])
        && value.parse::<f64>().is_err()
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "null" | "true" | "false" | "yes" | "no" | "on" | "off"
        );
    if plain {
        value.to_owned()
    } else {
        serde_json::Value::String(value.to_owned()).to_string()
    }
}

/// A list of texts as a one-line YAML value.
pub fn flow_list(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|value| flow_scalar(value)).collect();
    format!("[{}]", items.join(", "))
}

fn flow_scalar(value: &str) -> String {
    let scalar = scalar(value);
    if !scalar.starts_with('"') && scalar.contains([',', '[', ']', '{', '}']) {
        serde_json::Value::String(value.to_owned()).to_string()
    } else {
        scalar
    }
}

/// Sets a top-level key to a value already written as YAML, keeping the comment on its line. A new key goes after the
/// nearest key before it in `order`, or above the first key.
pub fn set(text: &str, key: &str, value: &str, order: &[&str]) -> Result<Edit, String> {
    let found = entries(text, 0..text.len(), 0)?;
    match found.iter().find(|entry| entry.key == key) {
        Some(entry) => Ok(replace_value(text, entry, value)),
        None => Ok(insert(text, &found, key, &format!("{key}: {value}"), order)),
    }
}

pub(crate) fn replace_value(text: &str, entry: &Entry, value: &str) -> Edit {
    if entry.end == entry.line_end {
        let end = value_end(text, entry.colon_end, entry.line_end);
        return Edit::new(entry.colon_end..end, format!(" {value}"));
    }
    Edit::new(entry.colon_end..entry.end, format!(" {value}{}", newline(text)))
}

/// Inserts whole lines for a new key, without their final line ending.
pub(crate) fn insert(text: &str, found: &[Entry], key: &str, lines: &str, order: &[&str]) -> Edit {
    let newline = newline(text);
    let wanted = order.iter().position(|name| *name == key).unwrap_or(order.len());
    let before = found
        .iter()
        .filter(|entry| order[..wanted].contains(&entry.key.as_str()))
        .max_by_key(|entry| entry.end);
    let at = match (before, found.first()) {
        (Some(entry), _) => entry.end,
        (None, Some(first)) => first.start,
        (None, None) => text.len(),
    };
    let lead = if at > 0 && !text[..at].ends_with('\n') {
        newline
    } else {
        ""
    };
    Edit::new(at..at, format!("{lead}{lines}{newline}"))
}

/// Removes a top-level key and its value. A key that is not there leaves nothing to do.
pub fn remove(text: &str, key: &str) -> Result<Option<Edit>, String> {
    let found = entries(text, 0..text.len(), 0)?;
    Ok(found
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| Edit::new(entry.start..entry.end, "")))
}

/// The children of a nested mapping entry and their indent. An inline value is an error: it would need rewriting.
pub(crate) fn children(text: &str, entry: &Entry) -> Result<(Vec<Entry>, usize), String> {
    let inline = text[entry.colon_end..value_end(text, entry.colon_end, entry.line_end)].trim();
    if !inline.is_empty() {
        return Err(format!(
            "{} is written on one line, so it cannot be edited here. Edit the file directly.",
            entry.key
        ));
    }
    let body = entry.line_end..entry.end;
    let indent = text[body.clone()]
        .split_inclusive('\n')
        .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map_or(2, |line| line.len() - line.trim_start_matches(' ').len());
    Ok((entries(text, body, indent)?, indent))
}

/// Sets `parent.key` to a value written as YAML, creating `parent` if needed. New keys go at the end of the mapping.
pub fn set_nested(text: &str, parent: &str, key: &str, value: &str, order: &[&str]) -> Result<Edit, String> {
    let found = entries(text, 0..text.len(), 0)?;
    let Some(entry) = found.iter().find(|entry| entry.key == parent) else {
        let newline = newline(text);
        let lines = format!("{parent}:{newline}  {key}: {value}");
        return Ok(insert(text, &found, parent, &lines, order));
    };
    let (list, indent) = children(text, entry)?;
    if let Some(child) = list.iter().find(|child| child.key == key) {
        return Ok(replace_value(text, child, value));
    }
    let newline = newline(text);
    let lead = if text[..entry.end].ends_with('\n') { "" } else { newline };
    let line = format!("{lead}{:indent$}{key}: {value}{newline}", "");
    Ok(Edit::new(entry.end..entry.end, line))
}

/// Removes `parent.key`, and `parent` with it when nothing else is left in it.
pub fn remove_nested(text: &str, parent: &str, key: &str) -> Result<Option<Edit>, String> {
    let found = entries(text, 0..text.len(), 0)?;
    let Some(entry) = found.iter().find(|entry| entry.key == parent) else {
        return Ok(None);
    };
    let (list, _) = children(text, entry)?;
    let Some(child) = list.iter().find(|child| child.key == key) else {
        return Ok(None);
    };
    if list.len() == 1 {
        return Ok(Some(Edit::new(entry.start..entry.end, "")));
    }
    Ok(Some(Edit::new(child.start..child.end, "")))
}

/// One `- item` of a block list, with the comment lines above it and its continuation lines.
struct Item {
    value: String,
    range: Range<usize>,
}

/// The items of a block list, or `None` if the value is written on the key's line.
fn list_items(text: &str, entry: &Entry) -> Option<Vec<Item>> {
    if !text[entry.colon_end..value_end(text, entry.colon_end, entry.line_end)]
        .trim()
        .is_empty()
    {
        return None;
    }
    let mut items: Vec<Item> = Vec::new();
    let mut at = entry.line_end;
    let mut chunk_start = at;
    for line in text[entry.line_end..entry.end].split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| (trimmed == "-").then_some("")) {
            let from = start + line.find('-').unwrap_or(0) + 1;
            let end = value_end(text, from, at);
            let written = text[from..end].trim();
            items.push(Item {
                value: unquote(if written.is_empty() { rest } else { written }),
                range: chunk_start..at,
            });
            chunk_start = at;
        } else if let Some(item) = items.last_mut() {
            item.range.end = at;
            chunk_start = at;
        }
    }
    Some(items)
}

pub(crate) fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')));
    match inner {
        Some(_) if value.starts_with('"') => serde_json::from_str(value).unwrap_or_else(|_| value.to_owned()),
        Some(inner) => inner.replace("''", "'"),
        None => value.to_owned(),
    }
}

/// Makes the block list at a top-level key hold exactly `wanted`, in that order. Items already there keep their exact
/// lines and the comments above them. A list written on the key's line becomes a block list.
pub fn set_list(text: &str, key: &str, wanted: &[String], order: &[&str]) -> Result<Edit, String> {
    let found = entries(text, 0..text.len(), 0)?;
    let newline = newline(text);
    let new_line = |indent: usize, value: &str| format!("{:indent$}- {}{newline}", "", scalar(value));
    let Some(entry) = found.iter().find(|entry| entry.key == key) else {
        let items: String = wanted.iter().map(|value| new_line(2, value)).collect();
        let lines = format!("{key}:{newline}{items}");
        let lines = lines.strip_suffix(newline).unwrap_or(&lines);
        return Ok(insert(text, &found, key, lines, order));
    };
    let Some(items) = list_items(text, entry) else {
        let body: String = wanted.iter().map(|value| new_line(2, value)).collect();
        return Ok(Edit::new(entry.colon_end..entry.end, format!("{newline}{body}")));
    };
    let (Some(first), Some(last)) = (items.first(), items.last()) else {
        let body: String = wanted.iter().map(|value| new_line(2, value)).collect();
        let lead = if text[..entry.line_end].ends_with('\n') {
            ""
        } else {
            newline
        };
        return Ok(Edit::new(entry.line_end..entry.end, format!("{lead}{body}")));
    };
    let item_line = text[first.range.clone()]
        .split_inclusive('\n')
        .find(|line| line.trim_start().starts_with('-'))
        .unwrap_or_default();
    let indent = item_line.len() - item_line.trim_start_matches(' ').len();
    let mut used = vec![false; items.len()];
    let mut body = String::new();
    for value in wanted {
        let existing = items
            .iter()
            .enumerate()
            .position(|(at, item)| !used[at] && item.value == *value);
        match existing {
            Some(at) => {
                used[at] = true;
                let chunk = &text[items[at].range.clone()];
                body.push_str(chunk);
                if !chunk.ends_with('\n') {
                    body.push_str(newline);
                }
            }
            None => body.push_str(&new_line(indent, value)),
        }
    }
    Ok(Edit::new(first.range.start..last.range.end, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: &[&str] = &[
        "title", "subtitle", "author", "date", "lang", "cover", "meta", "chapters",
    ];

    fn apply(text: &str, edit: Edit) -> String {
        format!("{}{}{}", &text[..edit.range.start], edit.text, &text[edit.range.end..])
    }

    #[test]
    fn a_value_changes_and_the_comment_on_its_line_stays() {
        let text = "# My book\ntitle: Old  # working title\nlang: de\n";
        let edit = set(text, "title", "New", ORDER).expect("edits");
        assert_eq!(apply(text, edit), "# My book\ntitle: New  # working title\nlang: de\n");
    }

    #[test]
    fn a_new_key_goes_after_its_neighbour_in_the_order() {
        let text = "# Comment\ntitle: A\n\n# chapters follow\nchapters:\n  - a.md\n";
        let edit = set(text, "subtitle", "B", ORDER).expect("edits");
        assert_eq!(
            apply(text, edit),
            "# Comment\ntitle: A\nsubtitle: B\n\n# chapters follow\nchapters:\n  - a.md\n"
        );
    }

    #[test]
    fn a_new_key_with_no_earlier_neighbour_goes_above_the_first_key() {
        let text = "# Comment\nchapters:\n  - a.md\n";
        let edit = set(text, "title", "A", ORDER).expect("edits");
        assert_eq!(apply(text, edit), "# Comment\ntitle: A\nchapters:\n  - a.md\n");
    }

    #[test]
    fn a_text_without_a_final_newline_stays_valid() {
        let text = "title: A";
        let edit = set(text, "lang", "de", ORDER).expect("edits");
        assert_eq!(apply(text, edit), "title: A\nlang: de\n");
    }

    #[test]
    fn a_list_value_turns_into_one_line() {
        let text = "author:\n  - A\n  - B\nlang: de\n";
        let edit = set(text, "author", "C", ORDER).expect("edits");
        assert_eq!(apply(text, edit), "author: C\nlang: de\n");
    }

    #[test]
    fn a_key_is_removed_with_its_lines_only() {
        let text = "title: A\n# stays\nauthor:\n  - A\nlang: de\n";
        let edit = remove(text, "author").expect("reads").expect("present");
        assert_eq!(apply(text, edit), "title: A\n# stays\nlang: de\n");
        assert!(remove(text, "date").expect("reads").is_none());
    }

    #[test]
    fn nested_values_change_and_new_ones_join_the_mapping() {
        let text = "meta:\n  # who\n  publisher: Harbour\n  isbn: 1\nlang: de\n";
        let edit = set_nested(text, "meta", "isbn", "\"2\"", ORDER).expect("edits");
        assert_eq!(
            apply(text, edit),
            "meta:\n  # who\n  publisher: Harbour\n  isbn: \"2\"\nlang: de\n"
        );
        let edit = set_nested(text, "meta", "offer", "X", ORDER).expect("edits");
        assert_eq!(
            apply(text, edit),
            "meta:\n  # who\n  publisher: Harbour\n  isbn: 1\n  offer: X\nlang: de\n"
        );
    }

    #[test]
    fn a_missing_parent_is_created_in_order() {
        let text = "title: A\nchapters:\n  - a.md\n";
        let edit = set_nested(text, "meta", "isbn", "1", ORDER).expect("edits");
        assert_eq!(apply(text, edit), "title: A\nmeta:\n  isbn: 1\nchapters:\n  - a.md\n");
    }

    #[test]
    fn an_inline_mapping_is_refused_rather_than_rewritten() {
        assert!(set_nested("meta: {isbn: 1}\n", "meta", "isbn", "2", ORDER).is_err());
    }

    #[test]
    fn removing_the_last_nested_value_removes_its_parent() {
        let text = "title: A\nmeta:\n  isbn: 1\nlang: de\n";
        let edit = remove_nested(text, "meta", "isbn").expect("reads").expect("present");
        assert_eq!(apply(text, edit), "title: A\nlang: de\n");
    }

    #[test]
    fn a_nested_list_value_is_removed_whole() {
        let text = "meta:\n  client:\n    - A\n    - B\n  offer: X\n";
        let edit = remove_nested(text, "meta", "client").expect("reads").expect("present");
        assert_eq!(apply(text, edit), "meta:\n  offer: X\n");
    }

    #[test]
    fn reordering_moves_items_with_their_comments() {
        let text = "chapters:\n  # the opening\n  - a.md # first\n  - b.md\n  - \"c d.md\"\n# after\nlang: de\n";
        let wanted = ["c d.md", "a.md", "b.md"].map(str::to_owned);
        let edit = set_list(text, "chapters", &wanted, ORDER).expect("edits");
        assert_eq!(
            apply(text, edit),
            "chapters:\n  - \"c d.md\"\n  # the opening\n  - a.md # first\n  - b.md\n# after\nlang: de\n"
        );
    }

    #[test]
    fn a_list_at_the_keys_indent_keeps_that_indent() {
        let text = "chapters:\n- a.md\n- b.md\n";
        let wanted = ["a.md", "b.md", "c.md"].map(str::to_owned);
        let edit = set_list(text, "chapters", &wanted, ORDER).expect("edits");
        assert_eq!(apply(text, edit), "chapters:\n- a.md\n- b.md\n- c.md\n");
    }

    #[test]
    fn removing_a_list_item_leaves_the_others_byte_for_byte() {
        let text = "chapters:\n  - a.md   # keep me\n  - b.md\n";
        let wanted = ["a.md"].map(str::to_owned);
        let edit = set_list(text, "chapters", &wanted, ORDER).expect("edits");
        assert_eq!(apply(text, edit), "chapters:\n  - a.md   # keep me\n");
    }

    #[test]
    fn a_list_on_one_line_becomes_a_block_list() {
        let text = "chapters: [a.md, b.md]\nlang: de\n";
        let wanted = ["b.md", "a.md"].map(str::to_owned);
        let edit = set_list(text, "chapters", &wanted, ORDER).expect("edits");
        assert_eq!(apply(text, edit), "chapters:\n  - b.md\n  - a.md\nlang: de\n");
    }

    #[test]
    fn scalars_quote_what_yaml_would_read_differently() {
        assert_eq!(scalar("The Time Machine"), "The Time Machine");
        assert_eq!(scalar("404: Gott nicht gefunden"), "\"404: Gott nicht gefunden\"");
        assert_eq!(scalar("1.50"), "\"1.50\"");
        assert_eq!(scalar("null"), "\"null\"");
        assert_eq!(scalar("#1"), "\"#1\"");
        assert_eq!(scalar("Für alle"), "Für alle");
        assert_eq!(scalar("Ünder"), "Ünder");
        assert_eq!(flow_list(&["A, B".to_owned(), "C".to_owned()]), "[\"A, B\", C]");
    }

    #[test]
    fn a_shape_it_cannot_read_is_an_error() {
        assert!(set("{title: A}\n", "title", "B", ORDER).is_err());
    }

    #[test]
    fn crlf_lines_stay_crlf() {
        let text = "title: A\r\nlang: de\r\n";
        let edit = set(text, "subtitle", "B", ORDER).expect("edits");
        assert_eq!(apply(text, edit), "title: A\r\nsubtitle: B\r\nlang: de\r\n");
        let edit = set(text, "title", "C", ORDER).expect("edits");
        assert_eq!(apply(text, edit), "title: C\r\nlang: de\r\n");
    }
}
