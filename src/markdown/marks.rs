//! The inline marks that the source closes with its own text, and placeholder names, found from the written text
//! alone. The parser and the desktop editor's source layer both use these, so they read the same characters.

/// Whether the character at `offset` has an odd number of backslashes before it.
pub fn escaped(body: &str, offset: usize) -> bool {
    let before = &body[..offset];
    (before.len() - before.trim_end_matches('\\').len()) % 2 == 1
}

/// A superscript or subscript: the closing delimiter is at `close`, one byte long.
pub struct Script {
    pub close: usize,
    pub superscript: bool,
}

/// The superscript or subscript that the `^` or `~` at `at` opens, if the same delimiter closes it before any
/// white space. A `^` after `{` or `[` belongs to an index mark or a footnote.
pub fn script(body: &str, at: usize) -> Option<Script> {
    let delimiter = body[at..].chars().next()?;
    let after = &body[at + 1..];
    if after.starts_with(delimiter) || body[..at].ends_with([delimiter, '{', '[']) || escaped(body, at) {
        return None;
    }
    let length = after.find(|c: char| !(c.is_alphanumeric() || "+-=(),.%°±−".contains(c)))?;
    let close = at + 1 + length;
    let closes = length > 0 && body[close..].starts_with(delimiter) && !body[close + 1..].starts_with(delimiter);
    closes.then_some(Script {
        close,
        superscript: delimiter == '^',
    })
}

/// A `[text]{.underline}` or `[text]{.smallcaps}`: the `]` is at `close` and the attribute ends at `end`.
pub struct Attribute {
    pub close: usize,
    pub end: usize,
    pub small_caps: bool,
}

/// The `[text]{.underline}` or `[text]{.smallcaps}` whose `[` is at `offset`, if one starts there. Any other
/// `{.name}` stays text. The text may not hold brackets or a blank line.
pub fn attribute(body: &str, offset: usize) -> Option<Attribute> {
    let close = offset + 1 + body[offset + 1..].find(['[', ']'])?;
    let rest = body[close..].strip_prefix(']')?;
    let (name, small_caps) = [("{.underline}", false), ("{.smallcaps}", true)]
        .into_iter()
        .find(|(name, _)| rest.starts_with(name))?;
    if close == offset + 1 || body[offset..close].contains("\n\n") {
        return None;
    }
    Some(Attribute {
        close,
        end: close + "]".len() + name.len(),
        small_caps,
    })
}

/// The `}` that ends the `{todo: text}` whose brace is at `offset`. It may span lines but not a blank line.
pub fn todo_close(body: &str, offset: usize) -> Option<usize> {
    let note = offset + "{todo:".len();
    let close = body[note..].find('}')?;
    (!body[note..note + close].contains("\n\n")).then_some(note + close)
}

/// The length of the placeholder name in `{name}` when `text` starts after the brace: a letter, then
/// letters, digits, `-`, `_`, and `.`. Other braces, such as `{.name}` and `{span=2}`, are text.
pub fn field_name(text: &str) -> Option<usize> {
    let length = text.find('}')?;
    let name = &text[..length];
    let valid = name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    valid.then_some(length)
}
