//! Syntax highlighting of fenced code blocks.
//!
//! Highlighting only decides which color a stretch of code gets. It never changes the font or the
//! text, so layout is the same with and without it. A language the highlighter does not know, or
//! a grammar that fails on the input, gives no spans and the block is set plain.

use std::ops::Range;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxSet};

/// The kinds of code a theme can color. Each is a group of Sublime syntax scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Comment,
    Keyword,
    String,
    Number,
    Constant,
    Type,
    Function,
    Operator,
    Punctuation,
    /// Markup tags and mapping keys in data formats.
    Tag,
    /// Attribute names, properties, and annotations.
    Attribute,
}

impl Category {
    /// The name used in themes and as the suffix of the EPUB class `hl-<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Keyword => "keyword",
            Self::String => "string",
            Self::Number => "number",
            Self::Constant => "constant",
            Self::Type => "type",
            Self::Function => "function",
            Self::Operator => "operator",
            Self::Punctuation => "punctuation",
            Self::Tag => "tag",
            Self::Attribute => "attribute",
        }
    }
}

/// A byte range of one code line and its category. Lines hold spans in order, without overlap.
pub type Span = (Range<usize>, Category);

/// Scope prefixes and their categories. The longest prefix that matches a scope wins.
const RULES: &[(&str, Category)] = &[
    ("comment", Category::Comment),
    ("punctuation.definition.comment", Category::Comment),
    ("string", Category::String),
    ("punctuation.definition.string", Category::String),
    ("constant.character.escape", Category::String),
    ("constant.other.placeholder", Category::String),
    ("constant", Category::Constant),
    ("support.constant", Category::Constant),
    ("variable.other.constant", Category::Constant),
    ("constant.numeric", Category::Number),
    ("keyword", Category::Keyword),
    ("keyword.operator", Category::Operator),
    ("storage", Category::Keyword),
    ("storage.type.numeric", Category::Type),
    ("storage.type.primitive", Category::Type),
    ("entity.name.type", Category::Type),
    ("entity.name.class", Category::Type),
    ("entity.name.struct", Category::Type),
    ("entity.name.enum", Category::Type),
    ("entity.name.union", Category::Type),
    ("entity.name.trait", Category::Type),
    ("entity.name.interface", Category::Type),
    ("entity.other.inherited-class", Category::Type),
    ("support.type", Category::Type),
    ("support.class", Category::Type),
    ("entity.name.function", Category::Function),
    ("support.function", Category::Function),
    ("support.macro", Category::Function),
    ("variable.function", Category::Function),
    ("entity.name.tag", Category::Tag),
    ("entity.other.attribute-name", Category::Attribute),
    ("support.type.property-name", Category::Attribute),
    ("variable.annotation", Category::Attribute),
    ("meta.annotation", Category::Attribute),
    ("punctuation", Category::Punctuation),
];

/// Language names that are not a name or file extension in the syntax set.
const ALIASES: &[(&str, &str)] = &[
    ("shell", "sh"),
    ("shellscript", "sh"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("jsx", "js"),
    ("golang", "go"),
    ("objc", "objective-c"),
    ("csharp", "cs"),
];

struct Grammars {
    syntaxes: SyntaxSet,
    rules: Vec<(Scope, u32, Category)>,
}

fn grammars() -> &'static Grammars {
    static GRAMMARS: OnceLock<Grammars> = OnceLock::new();
    GRAMMARS.get_or_init(|| Grammars {
        syntaxes: two_face::syntax::extra_newlines(),
        rules: RULES
            .iter()
            .map(|&(prefix, category)| {
                let scope = Scope::new(prefix).expect("rule scopes are valid");
                (scope, scope.len(), category)
            })
            .collect(),
    })
}

impl Grammars {
    /// The category of the innermost scope that has a rule, using that scope's most specific rule.
    fn category(&self, stack: &ScopeStack) -> Option<Category> {
        stack.as_slice().iter().rev().find_map(|&scope| {
            self.rules
                .iter()
                .filter(|(prefix, ..)| prefix.is_prefix_of(scope))
                .max_by_key(|(_, len, _)| *len)
                .map(|(.., category)| *category)
        })
    }
}

/// The language of a code fence: the first word of its info string, before a space, a comma, or
/// an attribute brace.
pub fn language_of(info: &str) -> Option<&str> {
    let word = info.split(|c: char| c.is_whitespace() || c == ',' || c == '{').next()?;
    (!word.is_empty()).then_some(word)
}

/// The spans of every line of a code block, or `None` for an unknown language or a failed parse.
/// Lines are the text as set, so tabs must already be expanded.
pub fn highlight(language: &str, lines: &[String]) -> Option<Vec<Vec<Span>>> {
    let grammars = grammars();
    let language = language.to_lowercase();
    let token = ALIASES
        .iter()
        .find_map(|&(alias, token)| (alias == language).then_some(token))
        .unwrap_or(&language);
    let syntax = grammars.syntaxes.find_syntax_by_token(token)?;
    if syntax.name == "Plain Text" {
        return None;
    }

    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut result = Vec::with_capacity(lines.len());
    for line in lines {
        let text = format!("{line}\n");
        let ops = state.parse_line(&text, &grammars.syntaxes).ok()?;
        let mut spans: Vec<Span> = Vec::new();
        let mut start = 0;
        let mut category = grammars.category(&stack);
        for (at, op) in ops.iter().chain([&(text.len(), ScopeStackOp::Noop)]) {
            let end = (*at).min(line.len());
            if let (Some(category), true) = (category, start < end) {
                match spans.last_mut() {
                    Some((range, last)) if *last == category && range.end == start => range.end = end,
                    _ => spans.push((start..end, category)),
                }
            }
            start = start.max(end);
            stack.apply(op).ok()?;
            category = grammars.category(&stack);
        }
        result.push(spans);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str) -> Vec<String> {
        source.lines().map(str::to_owned).collect()
    }

    /// The text of every span with its category name, for one line.
    fn tokens<'a>(line: &'a str, spans: &[Span]) -> Vec<(&'a str, &'static str)> {
        spans.iter().map(|(r, c)| (&line[r.clone()], c.name())).collect()
    }

    #[test]
    fn maps_scopes_to_categories() {
        let code = lines("// note\nfn main() { let n: Vec<u8> = 42; let s = \"hi\"; }");
        let spans = highlight("rust", &code).expect("rust is known");
        let comment = tokens(&code[0], &spans[0]);
        assert_eq!(comment, [("// note", "comment")]);
        let found = tokens(&code[1], &spans[1]);
        for expected in [
            ("fn", "keyword"),
            ("main", "function"),
            ("let", "keyword"),
            ("Vec", "type"),
            ("42", "number"),
            ("\"hi\"", "string"),
        ] {
            assert!(found.contains(&expected), "{expected:?} missing from {found:?}");
        }
    }

    #[test]
    fn follows_a_string_across_lines() {
        let code = lines("let s = \"one\ntwo\";");
        let spans = highlight("rust", &code).unwrap();
        assert_eq!(tokens(&code[1], &spans[1])[0], ("two\"", "string"));
    }

    #[test]
    fn unknown_and_plain_languages_give_no_spans() {
        let code = lines("anything");
        assert!(highlight("klingon", &code).is_none());
        assert!(highlight("text", &code).is_none());
    }

    #[test]
    fn finds_common_languages_by_name_extension_and_alias() {
        for language in [
            "rust",
            "ts",
            "typescript",
            "tsx",
            "jsx",
            "jsonc",
            "json",
            "bash",
            "sh",
            "shell",
            "python",
            "py",
            "yaml",
            "yml",
            "toml",
            "html",
            "css",
            "sql",
            "js",
            "javascript",
            "c",
            "cpp",
            "c++",
            "go",
            "java",
            "php",
            "ruby",
            "xml",
            "diff",
            "markdown",
            "dockerfile",
            "swift",
            "kotlin",
            "RUST",
        ] {
            assert!(highlight(language, &lines("x")).is_some(), "{language} is unknown");
        }
    }

    #[test]
    fn reads_the_language_from_an_info_string() {
        assert_eq!(language_of("rust"), Some("rust"));
        assert_eq!(language_of("rust,ignore"), Some("rust"));
        assert_eq!(language_of("ts {.wide}"), Some("ts"));
        assert_eq!(language_of(""), None);
    }
}
