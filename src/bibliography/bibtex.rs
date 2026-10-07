//! The BibTeX syntax: entries with a type, a key, and fields, before any interpretation of types or LaTeX.

/// A 1-based line and column.
pub type Position = (u64, u64);

/// A problem with a position, reported by the reader and by entry interpretation.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub message: String,
    pub at: Position,
}

impl Problem {
    pub fn new(message: impl Into<String>, at: Position) -> Self {
        Self {
            message: message.into(),
            at,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawEntry {
    /// The entry type in lowercase, without the `@`.
    pub kind: String,
    pub key: String,
    pub at: Position,
    pub fields: Vec<RawField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawField {
    /// The field name in lowercase.
    pub name: String,
    /// The value with its outer delimiters removed. Nested braces stay for the LaTeX step.
    pub value: String,
    pub at: Position,
}

impl RawEntry {
    /// The first field with this name, as BibTeX itself does when a name repeats.
    pub fn field(&self, name: &str) -> Option<&RawField> {
        self.fields.iter().find(|field| field.name == name)
    }
}

/// The standard month macros, which `month = jan` uses without defining them.
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Reads all entries. Problems that leave the rest readable (`@string`) are collected and reading continues; a
/// syntax error stops reading, since the rest of the file cannot be trusted.
pub fn read(text: &str) -> (Vec<RawEntry>, Vec<Problem>) {
    let mut reader = Reader::new(text);
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    loop {
        match reader.next_entry(&mut problems) {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => break,
            Err(problem) => {
                problems.push(problem);
                break;
            }
        }
    }
    (entries, problems)
}

struct Reader<'a> {
    text: &'a str,
    pos: usize,
    line_starts: Vec<usize>,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        let newlines = text.match_indices('\n').map(|(index, _)| index + 1);
        Self {
            text,
            pos: 0,
            line_starts: std::iter::once(0).chain(newlines).collect(),
        }
    }

    fn position(&self, offset: usize) -> Position {
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let column = self.text[self.line_starts[line]..offset].chars().count();
        (line as u64 + 1, column as u64 + 1)
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn take_while(&mut self, accept: impl Fn(u8) -> bool) -> &'a str {
        let start = self.pos;
        while self.peek().is_some_and(&accept) {
            self.pos += 1;
        }
        &self.text[start..self.pos]
    }

    fn next_entry(&mut self, problems: &mut Vec<Problem>) -> Result<Option<RawEntry>, Problem> {
        loop {
            let Some(offset) = self.text[self.pos..].find('@') else {
                return Ok(None);
            };
            let at_offset = self.pos + offset;
            let at = self.position(at_offset);
            self.pos = at_offset + 1;
            self.skip_whitespace();
            let kind = self
                .take_while(|b| b.is_ascii_alphanumeric() || b == b'_')
                .to_ascii_lowercase();
            self.skip_whitespace();
            let close = match self.peek() {
                Some(b'{') => b'}',
                Some(b'(') => b')',
                _ if kind == "comment" => {
                    self.take_while(|b| b != b'\n');
                    continue;
                }
                _ => {
                    return Err(Problem::new(
                        "expected `{` after the entry type",
                        self.position(self.pos),
                    ));
                }
            };
            match kind.as_str() {
                "comment" | "preamble" => self.skip_block()?,
                "string" => {
                    problems.push(Problem::new(
                        "`@string` macros are not supported; write the text in place",
                        at,
                    ));
                    self.skip_block()?;
                }
                _ => return self.entry(kind, at, close).map(Some),
            }
        }
    }

    /// Skips a delimited block by matching braces or parentheses.
    fn skip_block(&mut self) -> Result<(), Problem> {
        let open_at = self.position(self.pos);
        let (open, close) = if self.peek() == Some(b'{') {
            (b'{', b'}')
        } else {
            (b'(', b')')
        };
        let mut depth = 0usize;
        while let Some(byte) = self.peek() {
            self.pos += 1;
            if byte == open {
                depth += 1;
            } else if byte == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
        }
        Err(Problem::new("this block is never closed", open_at))
    }

    fn entry(&mut self, kind: String, at: Position, close: u8) -> Result<RawEntry, Problem> {
        self.pos += 1;
        let key_start = self.pos;
        let key = self.take_while(|b| b != b',' && b != close);
        let key = key.trim().to_owned();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err(Problem::new(
                format!("expected a citation key after `@{kind}{{`"),
                self.position(key_start),
            ));
        }
        let mut fields = Vec::new();
        loop {
            self.skip_whitespace();
            while self.peek() == Some(b',') {
                self.pos += 1;
                self.skip_whitespace();
            }
            match self.peek() {
                Some(byte) if byte == close => {
                    self.pos += 1;
                    return Ok(RawEntry { kind, key, at, fields });
                }
                None => return Err(Problem::new(format!("the entry `{key}` is never closed"), at)),
                Some(_) => fields.push(self.field(close)?),
            }
        }
    }

    fn field(&mut self, close: u8) -> Result<RawField, Problem> {
        let at = self.position(self.pos);
        let name = self.take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':' | b'.'));
        if name.is_empty() {
            return Err(Problem::new("expected a field name", at));
        }
        self.skip_whitespace();
        if self.peek() != Some(b'=') {
            return Err(Problem::new(
                format!("expected `=` after the field name `{name}`"),
                self.position(self.pos),
            ));
        }
        self.pos += 1;
        let value = self.value()?;
        self.skip_whitespace();
        match self.peek() {
            Some(b',') => {}
            Some(byte) if byte == close => {}
            _ => {
                return Err(Problem::new(
                    "expected `,` or the end of the entry",
                    self.position(self.pos),
                ));
            }
        }
        Ok(RawField {
            name: name.to_ascii_lowercase(),
            value,
            at,
        })
    }

    /// A value is one or more parts joined with `#`.
    fn value(&mut self) -> Result<String, Problem> {
        let mut value = String::new();
        loop {
            self.skip_whitespace();
            let at = self.position(self.pos);
            match self.peek() {
                Some(b'{') => value.push_str(self.braced()?),
                Some(b'"') => value.push_str(self.quoted()?),
                Some(byte) if byte.is_ascii_digit() => value.push_str(self.take_while(|b| b.is_ascii_digit())),
                Some(byte) if byte.is_ascii_alphabetic() => {
                    let name = self.take_while(|b| b.is_ascii_alphanumeric() || b == b'_');
                    if !MONTHS.contains(&name.to_ascii_lowercase().as_str()) {
                        let message =
                            format!("unknown macro `{name}`; `@string` is not supported, write the text in place");
                        return Err(Problem::new(message, at));
                    }
                    value.push_str(name);
                }
                _ => return Err(Problem::new("expected a field value", at)),
            }
            self.skip_whitespace();
            if self.peek() != Some(b'#') {
                return Ok(value);
            }
            self.pos += 1;
        }
    }

    /// The text between matching braces, which may nest.
    fn braced(&mut self) -> Result<&'a str, Problem> {
        let open_at = self.position(self.pos);
        self.pos += 1;
        let start = self.pos;
        let mut depth = 1usize;
        while let Some(byte) = self.peek() {
            match byte {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                let inner = &self.text[start..self.pos];
                self.pos += 1;
                return Ok(inner);
            }
            self.pos += 1;
        }
        Err(Problem::new("this `{` is never closed", open_at))
    }

    /// The text between double quotes. A quote inside braces does not end the value.
    fn quoted(&mut self) -> Result<&'a str, Problem> {
        let open_at = self.position(self.pos);
        self.pos += 1;
        let start = self.pos;
        let mut depth = 0usize;
        while let Some(byte) = self.peek() {
            match byte {
                b'{' => depth += 1,
                b'}' => depth = depth.saturating_sub(1),
                b'"' if depth == 0 => {
                    let inner = &self.text[start..self.pos];
                    self.pos += 1;
                    return Ok(inner);
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err(Problem::new("this `\"` is never closed", open_at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(entry: &RawEntry) -> Vec<(&str, &str)> {
        entry
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.value.as_str()))
            .collect()
    }

    #[test]
    fn reads_braced_quoted_and_numeric_values_case_insensitively() {
        let text = "@BOOK{k1,\n  Title = {A {Nested} title},\n  AUTHOR = \"Ada {\\\"A}\",\n  Year = 2020,\n}";
        let (entries, problems) = read(text);
        assert!(problems.is_empty());
        assert_eq!(entries[0].kind, "book");
        assert_eq!(entries[0].key, "k1");
        assert_eq!(
            fields(&entries[0]),
            [
                ("title", "A {Nested} title"),
                ("author", "Ada {\\\"A}"),
                ("year", "2020")
            ]
        );
    }

    #[test]
    fn concatenates_parts_and_accepts_month_macros() {
        let (entries, problems) = read("@misc{k, title = {A} # \" \" # {B}, month = jan}");
        assert!(problems.is_empty());
        assert_eq!(fields(&entries[0]), [("title", "A B"), ("month", "jan")]);
    }

    #[test]
    fn skips_comments_preambles_and_text_between_entries() {
        let text =
            "free text\n@comment{ @book{x, title={Y} } }\n@preamble{\"\\x\"}\n@comment line\n@misc(k, url = {u})";
        let (entries, problems) = read(text);
        assert!(problems.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "k");
    }

    #[test]
    fn reports_string_macros_and_continues() {
        let (entries, problems) = read("@string{x = {y}}\n@misc{k, title = x}");
        assert!(entries.is_empty());
        assert_eq!(problems[0].at, (1, 1));
        assert!(problems[0].message.contains("@string"));
        assert_eq!(problems[1].at, (2, 18));
        assert!(problems[1].message.contains("unknown macro `x`"));
    }

    #[test]
    fn reports_syntax_errors_with_line_and_column() {
        let (_, problems) = read("@book{k,\n  title = {open\n");
        assert_eq!(problems, [Problem::new("this `{` is never closed", (2, 11))]);
        let (_, problems) = read("@book{k,\n  title {x}}");
        assert_eq!(problems[0].at, (2, 9));
    }
}
