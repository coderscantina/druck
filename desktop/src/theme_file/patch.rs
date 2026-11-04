//! Edits JSON text in place. Setting or removing one object member changes only the bytes of that member,
//! so key order, indentation, inline objects, and every other value stay as the author wrote them.

use std::ops::Range;

use serde_json::Value;

/// A JSON value that is either an object, whose members can change, or anything else, replaced whole.
enum Node {
    Object(Object),
    Other(Range<usize>),
}

struct Object {
    span: Range<usize>,
    members: Vec<Member>,
}

struct Member {
    key: String,
    /// Byte offset of the opening quote of the key.
    start: usize,
    /// Byte offset after the closing quote of the key.
    key_end: usize,
    value: Node,
}

impl Node {
    fn span(&self) -> Range<usize> {
        match self {
            Self::Object(object) => object.span.clone(),
            Self::Other(span) => span.clone(),
        }
    }
}

impl Object {
    /// The last member with a key, as `serde_json` keeps the last of duplicate keys.
    fn position(&self, key: &str) -> Option<usize> {
        self.members.iter().rposition(|member| member.key == key)
    }
}

struct Edit {
    range: Range<usize>,
    text: String,
}

impl Edit {
    fn apply(self, text: &str) -> String {
        format!("{}{}{}", &text[..self.range.start], self.text, &text[self.range.end..])
    }
}

/// Sets the value at an object path, creating the missing objects on the way. The text must be valid JSON
/// with an object at the root.
pub fn set(text: &str, path: &[&str], value: &Value) -> Result<String, String> {
    let root = parse(text)?;
    plan_set(text, &root, path, value).map(|edit| edit.apply(text))
}

/// Removes the member at an object path, and any object it leaves empty except the root. A path that is
/// not there leaves the text unchanged.
pub fn remove(text: &str, path: &[&str]) -> Result<String, String> {
    let root = parse(text)?;
    Ok(plan_remove(&root, path).map_or_else(|| text.to_owned(), |edit| edit.apply(text)))
}

/// The bytes of the value at an object path, or `None` where nothing is written.
pub fn value_span(text: &str, path: &[&str]) -> Result<Option<Range<usize>>, String> {
    let root = parse(text)?;
    let mut node = &root;
    for key in path {
        let Node::Object(object) = node else { return Ok(None) };
        let Some(index) = object.position(key) else {
            return Ok(None);
        };
        node = &object.members[index].value;
    }
    Ok(Some(node.span()))
}

/// Renames the last key of an object path in place, so the member keeps its position and its value. A
/// path that is not there leaves the text unchanged.
pub fn rename_key(text: &str, path: &[&str], to: &str) -> Result<String, String> {
    let root = parse(text)?;
    let mut node = &root;
    let mut found = None;
    for key in path {
        let Node::Object(object) = node else { return Ok(text.to_owned()) };
        let Some(index) = object.position(key) else {
            return Ok(text.to_owned());
        };
        found = Some(&object.members[index]);
        node = &object.members[index].value;
    }
    Ok(found.map_or_else(
        || text.to_owned(),
        |member| {
            Edit {
                range: member.start..member.key_end,
                text: quote(to),
            }
            .apply(text)
        },
    ))
}

/// A string in JSON text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Literal {
    pub range: Range<usize>,
    pub value: String,
    /// The key of the member this string is the value of, if it is one.
    pub member: Option<String>,
    pub is_key: bool,
}

/// Every string of the text in order, found without building the tree, so strings inside arrays are seen.
pub fn literals(text: &str) -> Vec<Literal> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    let mut last_key: Option<String> = None;
    // The key a string would be the value of, once the colon after it is seen.
    let mut awaiting: Option<String> = None;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                let start = at;
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                at = (at + 1).min(bytes.len());
                let range = start..at;
                let Ok(value) = serde_json::from_str::<String>(&text[range.clone()]) else {
                    continue;
                };
                let rest = text[at..].trim_start();
                let is_key = rest.starts_with(':');
                let member = if is_key { None } else { awaiting.take() };
                if is_key {
                    last_key = Some(value.clone());
                    awaiting = None;
                }
                found.push(Literal {
                    range,
                    value,
                    member,
                    is_key,
                });
            }
            b':' => {
                awaiting = last_key.take();
                at += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => at += 1,
            _ => {
                awaiting = None;
                at += 1;
            }
        }
    }
    found
}

/// Replaces every string value equal to `from`, wherever it is, with `to`. Keys stay. With `members`, only
/// values of members with one of these keys change.
pub fn rename_values(text: &str, from: &str, to: &str, members: Option<&[&str]>) -> String {
    let mut out = text.to_owned();
    let hits: Vec<_> = literals(text)
        .into_iter()
        .filter(|lit| !lit.is_key && lit.value == from)
        .filter(|lit| members.is_none_or(|keys| lit.member.as_deref().is_some_and(|key| keys.contains(&key))))
        .collect();
    for lit in hits.into_iter().rev() {
        out.replace_range(lit.range, &quote(to));
    }
    out
}

fn plan_set(text: &str, node: &Node, path: &[&str], value: &Value) -> Result<Edit, String> {
    let Some((key, rest)) = path.split_first() else {
        return Ok(Edit {
            range: node.span(),
            text: inline(value),
        });
    };
    match node {
        Node::Object(object) => match object.position(key) {
            Some(index) => plan_set(text, &object.members[index].value, rest, value),
            None => Ok(insert(text, object, key, &nest(rest, value))),
        },
        Node::Other(span) => Ok(Edit {
            range: span.clone(),
            text: inline(&nest(path, value)),
        }),
    }
}

/// The value wrapped in one object per key, outermost first.
pub fn nest(keys: &[&str], value: &Value) -> Value {
    keys.iter().rev().fold(value.clone(), |inner, key| {
        Value::Object(serde_json::Map::from_iter([((*key).to_owned(), inner)]))
    })
}

fn insert(text: &str, object: &Object, key: &str, value: &Value) -> Edit {
    let member = format!("{}: {}", quote(key), inline(value));
    let Some(last) = object.members.last() else {
        return Edit {
            range: object.span.clone(),
            text: format!("{{ {member} }}"),
        };
    };
    let end = last.value.span().end;
    let separator = if is_multiline(text, object) {
        format!(",{}{}", newline(text), indent(text, &object.members[0]))
    } else {
        ", ".to_owned()
    };
    Edit {
        range: end..end,
        text: format!("{separator}{member}"),
    }
}

fn plan_remove(root: &Node, path: &[&str]) -> Option<Edit> {
    let mut chain: Vec<(&Object, usize)> = Vec::new();
    let mut node = root;
    for key in path {
        let Node::Object(object) = node else { return None };
        let index = object.position(key)?;
        chain.push((object, index));
        node = &object.members[index].value;
    }
    let mut level = chain.len().checked_sub(1)?;
    while level > 0 && chain[level].0.members.len() == 1 {
        level -= 1;
    }
    let (object, index) = chain[level];
    if object.members.len() == 1 {
        return Some(Edit {
            range: object.span.clone(),
            text: "{}".to_owned(),
        });
    }
    let member = &object.members[index];
    let range = if index > 0 {
        object.members[index - 1].value.span().end..member.value.span().end
    } else {
        member.start..object.members[1].start
    };
    Some(Edit {
        range,
        text: String::new(),
    })
}

fn is_multiline(text: &str, object: &Object) -> bool {
    text[object.span.start..object.members[0].start].contains('\n')
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The whitespace in front of a member on its own line.
fn indent(text: &str, member: &Member) -> String {
    let line = text[..member.start].rsplit('\n').next().unwrap_or_default();
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

fn quote(key: &str) -> String {
    Value::String(key.to_owned()).to_string()
}

/// A value on one line, with spaces inside objects as in the bundled themes: `{ "size": "20pt" }`.
fn inline(value: &Value) -> String {
    match value {
        Value::Object(map) if !map.is_empty() => {
            let members: Vec<_> = map
                .iter()
                .map(|(key, value)| format!("{}: {}", quote(key), inline(value)))
                .collect();
            format!("{{ {} }}", members.join(", "))
        }
        value => value.to_string(),
    }
}

fn parse(text: &str) -> Result<Node, String> {
    let mut parser = Parser { text, at: 0 };
    let root = parser.value()?;
    match root {
        Node::Object(_) => Ok(root),
        Node::Other(_) => Err("a theme must be a JSON object".to_owned()),
    }
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    fn skip_space(&mut self) {
        while self.bytes().get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.skip_space();
        if self.bytes().get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", byte as char, self.at))
        }
    }

    fn value(&mut self) -> Result<Node, String> {
        self.skip_space();
        match self.bytes().get(self.at) {
            Some(b'{') => self.object().map(Node::Object),
            Some(b'"') => self.string().map(|(span, _)| Node::Other(span)),
            Some(b'[') => self.array().map(Node::Other),
            Some(_) => {
                let start = self.at;
                while self
                    .bytes()
                    .get(self.at)
                    .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b',' | b'}' | b']'))
                {
                    self.at += 1;
                }
                Ok(Node::Other(start..self.at))
            }
            None => Err("unexpected end of text".to_owned()),
        }
    }

    fn string(&mut self) -> Result<(Range<usize>, String), String> {
        let start = self.at;
        self.at += 1;
        loop {
            match self.bytes().get(self.at) {
                Some(b'\\') => self.at += 2,
                Some(b'"') => break,
                Some(_) => self.at += 1,
                None => return Err("unterminated string".to_owned()),
            }
        }
        self.at += 1;
        let span = start..self.at;
        let decoded = serde_json::from_str(&self.text[span.clone()]).map_err(|e| e.to_string())?;
        Ok((span, decoded))
    }

    fn array(&mut self) -> Result<Range<usize>, String> {
        let start = self.at;
        self.at += 1;
        self.skip_space();
        if self.bytes().get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(start..self.at);
        }
        loop {
            self.value()?;
            self.skip_space();
            match self.bytes().get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(start..self.at);
                }
                _ => return Err(format!("expected `,` or `]` at byte {}", self.at)),
            }
        }
    }

    fn object(&mut self) -> Result<Object, String> {
        let start = self.at;
        self.at += 1;
        let mut members = Vec::new();
        self.skip_space();
        if self.bytes().get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Object {
                span: start..self.at,
                members,
            });
        }
        loop {
            self.skip_space();
            if self.bytes().get(self.at) != Some(&b'"') {
                return Err(format!("expected a key at byte {}", self.at));
            }
            let member_start = self.at;
            let (key_span, key) = self.string()?;
            self.expect(b':')?;
            let value = self.value()?;
            members.push(Member {
                key,
                start: member_start,
                key_end: key_span.end,
                value,
            });
            self.skip_space();
            match self.bytes().get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Object {
                        span: start..self.at,
                        members,
                    });
                }
                _ => return Err(format!("expected `,` or `}}` at byte {}", self.at)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const MULTILINE: &str = "{\n  \"version\": 1,\n  \"styles\": {\n    \"heading-1\": { \"color\": \"$colors.accent\" },\n    \"title\": {\n      \"align\": \"left\"\n    }\n  }\n}\n";

    #[test]
    fn sets_a_member_inside_an_inline_object_and_removing_it_restores_the_text() {
        let added = set(MULTILINE, &["styles", "heading-1", "size"], &json!("20pt")).unwrap();
        assert!(added.contains("\"heading-1\": { \"color\": \"$colors.accent\", \"size\": \"20pt\" },"));
        assert_eq!(remove(&added, &["styles", "heading-1", "size"]).unwrap(), MULTILINE);
    }

    #[test]
    fn adds_members_on_their_own_line_where_siblings_have_one() {
        let added = set(MULTILINE, &["styles", "title", "size"], &json!("2em")).unwrap();
        assert!(added.contains("\"align\": \"left\",\n      \"size\": \"2em\"\n    }"));
        assert_eq!(remove(&added, &["styles", "title", "size"]).unwrap(), MULTILINE);
    }

    #[test]
    fn creates_missing_objects_and_prunes_them_on_removal() {
        let added = set(MULTILINE, &["styles", "heading-3", "size"], &json!("14pt")).unwrap();
        assert!(added.contains(",\n    \"heading-3\": { \"size\": \"14pt\" }\n  }"));
        assert_eq!(remove(&added, &["styles", "heading-3", "size"]).unwrap(), MULTILINE);

        let bare = "{\n  \"version\": 1\n}\n";
        let added = set(bare, &["styles", "body", "size"], &json!("11pt")).unwrap();
        assert_eq!(
            added,
            "{\n  \"version\": 1,\n  \"styles\": { \"body\": { \"size\": \"11pt\" } }\n}\n"
        );
        assert_eq!(remove(&added, &["styles", "body", "size"]).unwrap(), bare);
    }

    #[test]
    fn replaces_one_value_and_keeps_the_rest_byte_for_byte() {
        let text = "{ \"a\": [1,  2], \"b\":   \"x\" , \"c\": {\"d\":1}}";
        assert_eq!(
            set(text, &["b"], &json!("y")).unwrap(),
            "{ \"a\": [1,  2], \"b\":   \"y\" , \"c\": {\"d\":1}}"
        );
        assert_eq!(remove(text, &["a"]).unwrap(), "{ \"b\":   \"x\" , \"c\": {\"d\":1}}");
    }

    #[test]
    fn a_value_has_its_own_span_and_a_missing_one_has_none() {
        let span = value_span(MULTILINE, &["styles", "heading-1"]).unwrap().unwrap();
        assert_eq!(&MULTILINE[span], "{ \"color\": \"$colors.accent\" }");
        assert_eq!(value_span(MULTILINE, &["nope"]).unwrap(), None);
    }

    #[test]
    fn renaming_a_key_keeps_its_place_and_value_and_references_follow_by_value() {
        let renamed = rename_key(MULTILINE, &["styles", "title"], "headline").unwrap();
        assert_eq!(renamed, MULTILINE.replace("\"title\"", "\"headline\""));
        assert_eq!(rename_key(MULTILINE, &["styles", "nope"], "x").unwrap(), MULTILINE);
        let text = "{ \"a\": { \"based-on\": \"x\", \"x\": \"x\" }, \"l\": [\"x\"] }";
        assert_eq!(
            rename_values(text, "x", "y", None),
            "{ \"a\": { \"based-on\": \"y\", \"x\": \"y\" }, \"l\": [\"y\"] }"
        );
        assert_eq!(
            rename_values(text, "x", "y", Some(&["based-on"])),
            "{ \"a\": { \"based-on\": \"y\", \"x\": \"x\" }, \"l\": [\"x\"] }"
        );
    }

    #[test]
    fn a_missing_path_is_not_a_change() {
        assert_eq!(remove(MULTILINE, &["styles", "quote", "size"]).unwrap(), MULTILINE);
    }

    #[test]
    fn keys_and_strings_with_escapes_and_brackets_do_not_confuse_the_spans() {
        let text = "{\"a\\\"b\": \"}\", \"l\": [\"]\", {\"x\": 1}], \"z\": 1}";
        let out = set(text, &["z"], &json!(2)).unwrap();
        assert_eq!(out, "{\"a\\\"b\": \"}\", \"l\": [\"]\", {\"x\": 1}], \"z\": 2}");
    }
}
