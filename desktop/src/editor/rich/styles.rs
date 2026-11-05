//! Where a block's named style stands in its text: `{.name}` closes its line.

use std::ops::Range;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closing_group_holds_style_words_only() {
        assert_eq!(closing_group("# Title {.wide}"), Some(8..15));
        assert_eq!(closing_group("Text {-}  "), Some(5..8));
        assert_eq!(closing_group("Text {braces here}"), None);
        assert_eq!(closing_group("Text \\{.not}"), None);
        assert_eq!(closing_group("No group"), None);
    }
}
