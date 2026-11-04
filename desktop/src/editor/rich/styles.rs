//! A block's named style as text, `{.name}`, where the engine reads it for that kind of block, and which of the theme's
//! custom styles each kind may use. There are no per-block attributes: a one-off look is a named style.

use std::ops::Range;

use serde_json::Value;

use crate::editor::commands::{body_range, line_ending};
use crate::source::{BlockKind, Blocks, Edit};

/// The kinds of block a style is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Heading,
    Paragraph,
    List,
}

/// The class of a block, if it takes a style.
pub fn class_of(kind: BlockKind, text: &str) -> Option<Class> {
    match kind {
        BlockKind::Heading { .. } => Some(Class::Heading),
        BlockKind::List => Some(Class::List),
        BlockKind::Paragraph if !text.starts_with("![") && !text.starts_with(": ") => Some(Class::Paragraph),
        _ => None,
    }
}

/// The built-in style a block starts from.
pub fn base_style(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Heading { level: 1 } => "heading-1",
        BlockKind::Heading { level: 2 } => "heading-2",
        BlockKind::Heading { level: 3 } => "heading-3",
        BlockKind::Heading { level: 4 } => "heading-4",
        BlockKind::Heading { level: 5 } => "heading-5",
        BlockKind::Heading { .. } => "heading-6",
        BlockKind::List => "list",
        BlockKind::Quote => "quote",
        BlockKind::Code => "code-block",
        _ => "body",
    }
}

/// The group of `{...}` closing `line`, as a range in it, if it holds a style word or could.
pub fn closing_group(line: &str) -> Option<Range<usize>> {
    let trimmed = line.trim_end();
    let open = trimmed.rfind('{')?;
    if !trimmed.ends_with('}') || (open > 0 && trimmed.as_bytes()[open - 1] == b'\\') {
        return None;
    }
    let inner = &trimmed[open + 1..trimmed.len() - 1];
    inner
        .split_whitespace()
        .all(|word| word.starts_with('.') || word.starts_with('#') || word == "-")
        .then_some(open..trimmed.len())
}

fn style_word(group: &str) -> Option<&str> {
    group
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split_whitespace()
        .find_map(|word| word.strip_prefix('.'))
}

fn last_line(body: &str) -> Range<usize> {
    let start = body.rfind('\n').map_or(0, |at| at + 1);
    start..body.len()
}

/// The `{.name}` line directly before a list, in the block before it.
fn list_style_line(text: &str, blocks: &Blocks, index: usize) -> Option<Range<usize>> {
    let previous = blocks.as_slice().get(index.checked_sub(1)?)?;
    if previous.kind != BlockKind::Paragraph {
        return None;
    }
    let body = body_range(text, &previous.range);
    let line = last_line(&text[body.clone()]);
    let range = body.start + line.start..body.start + line.end;
    let written = text[range.clone()].trim();
    (written.starts_with("{.") && written.ends_with('}') && !written.contains(' ')).then_some(range)
}

/// The style block `index` names.
pub fn style_of(text: &str, blocks: &Blocks, index: usize) -> Option<String> {
    let block = &blocks.as_slice()[index];
    if block.kind == BlockKind::List {
        let line = list_style_line(text, blocks, index)?;
        return style_word(&text[line]).map(str::to_owned);
    }
    class_of(block.kind, &text[block.range.clone()])?;
    let body = body_range(text, &block.range);
    let line = last_line(&text[body.clone()]);
    let line_text = &text[body.start + line.start..body.start + line.end];
    let group = closing_group(line_text)?;
    style_word(&line_text[group]).map(str::to_owned)
}

/// The edit that makes block `index` use style `name`, or none with `None`. Whatever else is in its braces stays.
pub fn set_style(text: &str, blocks: &Blocks, index: usize, name: Option<&str>) -> Option<Edit> {
    let block = &blocks.as_slice()[index];
    let eol = line_ending(text);
    if block.kind == BlockKind::List {
        return match (list_style_line(text, blocks, index), name) {
            (Some(line), Some(name)) => Some(Edit::new(line, format!("{{.{name}}}"))),
            (Some(line), None) => {
                // The line goes with its line ending. Text in the same paragraph above it stays.
                let end = line.end + eol.len();
                Some(Edit::new(line.start..end, ""))
            }
            (None, Some(name)) => Some(Edit::new(
                block.range.start..block.range.start,
                format!("{{.{name}}}{eol}"),
            )),
            (None, None) => None,
        };
    }
    class_of(block.kind, &text[block.range.clone()])?;
    let body = body_range(text, &block.range);
    let line = last_line(&text[body.clone()]);
    let line_start = body.start + line.start;
    let line_text = &text[line_start..body.end];
    match closing_group(line_text) {
        Some(group) => {
            let written = &line_text[group.clone()];
            let others: Vec<&str> = written
                .trim_start_matches('{')
                .trim_end_matches('}')
                .split_whitespace()
                .filter(|word| !word.starts_with('.'))
                .collect();
            let mut words: Vec<String> = others.iter().map(|word| (*word).to_owned()).collect();
            if let Some(name) = name {
                words.push(format!(".{name}"));
            }
            if words.is_empty() {
                // The braces go with the space before them.
                let before = line_text[..group.start].trim_end_matches(' ').len();
                return Some(Edit::new(line_start + before..line_start + group.end, ""));
            }
            let new = format!("{{{}}}", words.join(" "));
            (written != new).then(|| Edit::new(line_start + group.start..line_start + group.end, new))
        }
        None => name.map(|name| {
            let end = line_start + line_text.trim_end().len();
            Edit::new(end..end, format!(" {{.{name}}}"))
        }),
    }
}

/// The built-in style a custom style comes down to.
fn root_style<'a>(custom: &'a Value, name: &'a str) -> &'a str {
    let mut current = name;
    for _ in 0..16 {
        match custom
            .get(current)
            .and_then(|style| style.get("based-on"))
            .and_then(Value::as_str)
        {
            Some(base) => current = base,
            None => return current,
        }
    }
    current
}

/// The class a built-in style is for, by the engine's rule.
fn class_of_style(built_in: &str) -> Class {
    if built_in.starts_with("heading-") {
        Class::Heading
    } else if built_in == "list" {
        Class::List
    } else {
        Class::Paragraph
    }
}

/// The custom styles of a theme, sorted, that a block of this class may use.
pub fn styles_for(class: Class, theme: &Value) -> Vec<String> {
    let Some(custom) = theme.get("custom-styles") else {
        return Vec::new();
    };
    let Some(map) = custom.as_object() else {
        return Vec::new();
    };
    let mut names: Vec<String> = map
        .keys()
        .filter(|name| class_of_style(root_style(custom, name)) == class)
        .cloned()
        .collect();
    names.sort();
    names
}

/// How a style sets its text, following its bases, from the merged theme.
pub fn alignment(theme: &Value, style: &str) -> Option<String> {
    let custom = theme.get("custom-styles");
    let styles = theme.get("styles");
    let mut current = style;
    for _ in 0..16 {
        if let Some(found) = custom.and_then(|custom| custom.get(current)) {
            if let Some(align) = found.get("align").and_then(Value::as_str) {
                return Some(align.to_owned());
            }
            current = found.get("based-on")?.as_str()?;
        } else {
            return styles?.get(current)?.get("align")?.as_str().map(str::to_owned);
        }
    }
    None
}

/// The style that shows `base` set this way: `body-center`.
pub fn aligned_name(base: &str, align: &str) -> String {
    format!("{base}-{align}")
}

/// The style a name like `body-center` was made from, when it is one of those.
pub fn unaligned(name: &str) -> Option<&str> {
    ["left", "center", "right", "justify"]
        .into_iter()
        .find_map(|align| name.strip_suffix(align)?.strip_suffix('-'))
}

/// Whether a theme's names accept this as a custom style's name.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '-' || character == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands::applied;
    use serde_json::json;

    fn run(text: &str, index: usize, name: Option<&str>) -> String {
        let blocks = Blocks::parse(text);
        match set_style(text, &blocks, index, name) {
            Some(edit) => applied(text, &[edit]),
            None => text.to_owned(),
        }
    }

    #[test]
    fn a_paragraph_takes_its_style_at_the_end_of_its_last_line() {
        assert_eq!(
            run("One\ntwo.\n\nNext\n", 0, Some("lead")),
            "One\ntwo. {.lead}\n\nNext\n"
        );
        assert_eq!(run("One {.lead}\n", 0, Some("wide")), "One {.wide}\n");
        assert_eq!(run("One {.lead}\n", 0, None), "One\n");
        assert_eq!(run("One\n", 0, None), "One\n");
        let blocks = Blocks::parse("One {.lead}\n");
        assert_eq!(style_of("One {.lead}\n", &blocks, 0).as_deref(), Some("lead"));
    }

    #[test]
    fn a_heading_keeps_its_label_beside_its_style() {
        assert_eq!(run("# A {#sec:a}\n", 0, Some("wide")), "# A {#sec:a .wide}\n");
        assert_eq!(run("# A {#sec:a .wide}\n", 0, None), "# A {#sec:a}\n");
        assert_eq!(run("# A {#sec:a .wide}\n", 0, Some("x")), "# A {#sec:a .x}\n");
        assert_eq!(run("# A\n", 0, Some("x")), "# A {.x}\n");
    }

    #[test]
    fn a_list_takes_its_style_on_a_line_before_it() {
        assert_eq!(run("- a\n- b\n", 0, Some("checks")), "{.checks}\n- a\n- b\n");
        let styled = "{.checks}\n- a\n";
        assert_eq!(Blocks::parse(styled).as_slice()[1].kind, BlockKind::List);
        assert_eq!(run(styled, 1, Some("x")), "{.x}\n- a\n");
        assert_eq!(run(styled, 1, None), "- a\n");
        let blocks = Blocks::parse(styled);
        assert_eq!(style_of(styled, &blocks, 1).as_deref(), Some("checks"));
    }

    #[test]
    fn other_blocks_take_no_style() {
        assert_eq!(run("```\nx\n```\n", 0, Some("x")), "```\nx\n```\n");
        assert_eq!(run("![a](b.png)\n", 0, Some("x")), "![a](b.png)\n");
    }

    #[test]
    fn custom_styles_list_by_the_kind_of_block_their_chain_ends_in() {
        let theme = json!({"custom-styles": {
            "eyebrow": {"based-on": "body"},
            "chain": {"based-on": "eyebrow"},
            "checks": {"based-on": "list"},
            "step": {"based-on": "heading-3"},
        }});
        assert_eq!(styles_for(Class::Paragraph, &theme), ["chain", "eyebrow"]);
        assert_eq!(styles_for(Class::List, &theme), ["checks"]);
        assert_eq!(styles_for(Class::Heading, &theme), ["step"]);
    }

    #[test]
    fn alignment_follows_the_chain_and_the_built_in_style() {
        let theme = json!({
            "styles": {"body": {"align": "justify"}},
            "custom-styles": {"a": {"based-on": "body"}, "b": {"based-on": "a", "align": "center"}, "c": {"based-on": "b"}},
        });
        assert_eq!(alignment(&theme, "body").as_deref(), Some("justify"));
        assert_eq!(alignment(&theme, "a").as_deref(), Some("justify"));
        assert_eq!(alignment(&theme, "c").as_deref(), Some("center"));
    }

    #[test]
    fn an_aligned_style_names_where_it_came_from() {
        assert_eq!(aligned_name("body", "center"), "body-center");
        assert_eq!(unaligned("body-center"), Some("body"));
        assert_eq!(unaligned("lead"), None);
    }
}
