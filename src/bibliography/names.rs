//! Author and editor lists.

use crate::config::theme::Lang;

use super::latex;
use super::words::words;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
    Person {
        family: String,
        given: String,
    },
    /// A name in braces, such as `{World Health Organization}`, kept whole.
    Corporate(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    pub list: Vec<Name>,
    /// The list ended with `others`.
    pub et_al: bool,
}

impl Name {
    /// The part used in text citations: the family name or the whole corporate name.
    fn family(&self) -> &str {
        match self {
            Self::Person { family, .. } | Self::Corporate(family) => family,
        }
    }

    fn inverted(&self) -> String {
        match self {
            Self::Person { family, given } if !given.is_empty() => format!("{family}, {given}"),
            _ => self.family().to_owned(),
        }
    }

    fn natural(&self) -> String {
        match self {
            Self::Person { family, given } if !given.is_empty() => format!("{given} {family}"),
            _ => self.family().to_owned(),
        }
    }
}

impl Names {
    /// Parses a BibTeX name list: names separated by `and`, in "Last, First" or "First Last" form.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut names = Self::default();
        for name in split_names(raw) {
            if name == "others" {
                names.et_al = true;
            } else {
                names.list.push(parse_name(&name)?);
            }
        }
        Ok(names)
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn count(&self) -> usize {
        self.list.len()
    }

    /// Family names for in-text citations: "Smith", "Smith and Jones", "Smith et al.".
    pub fn short(&self, lang: Lang) -> String {
        let words = words(lang);
        match self.list.as_slice() {
            [] => String::new(),
            [one] if !self.et_al => one.family().to_owned(),
            [first, second] if !self.et_al => format!("{} {} {}", first.family(), words.and, second.family()),
            [first, ..] => format!("{} {}", first.family(), words.et_al),
        }
    }

    /// The list for the bibliography: the first name inverted, "Smith, Ada, Bob Jones, and Cy Lee".
    pub fn full(&self, lang: Lang) -> String {
        let words = words(lang);
        let mut items: Vec<String> = self
            .list
            .iter()
            .enumerate()
            .map(|(i, name)| if i == 0 { name.inverted() } else { name.natural() })
            .collect();
        if self.et_al {
            return format!("{} {}", items.join(", "), words.et_al);
        }
        let Some(last) = items.pop() else { return String::new() };
        match (items.len(), lang) {
            (0, _) => last,
            (1, _) => format!("{} {} {last}", items[0], words.and),
            (_, Lang::En) => format!("{}, {} {last}", items.join(", "), words.and),
            (_, Lang::De) => format!("{} {} {last}", items.join(", "), words.and),
        }
    }

    /// The text names sort by: family and given names, lowercase, with umlauts folded.
    pub fn sort_key(&self) -> String {
        let key: Vec<String> = self.list.iter().map(|name| fold(&name.inverted())).collect();
        key.join(" ")
    }
}

/// Lowercases and folds umlauts so that "Müller" sorts with "Mueller" and not after "Z".
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(|c| c.to_lowercase())
        .flat_map(|c| match c {
            'ä' => "ae".chars().collect::<Vec<_>>(),
            'ö' => "oe".chars().collect(),
            'ü' => "ue".chars().collect(),
            'ß' => "ss".chars().collect(),
            c => vec![c],
        })
        .collect()
}

/// Splits at whitespace outside braces.
fn words_outside_braces(text: &str) -> Vec<&str> {
    split_outside_braces(text, char::is_whitespace)
}

fn split_outside_braces(text: &str, is_separator: impl Fn(char) -> bool) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (index, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            c if depth == 0 && is_separator(c) => {
                parts.push(&text[start..index]);
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

/// Splits a list at the word `and` outside braces.
fn split_names(raw: &str) -> Vec<String> {
    let mut names = vec![Vec::new()];
    for word in words_outside_braces(raw) {
        if word.eq_ignore_ascii_case("and") {
            names.push(Vec::new());
        } else if let Some(current) = names.last_mut() {
            current.push(word);
        }
    }
    names
        .into_iter()
        .filter(|words| !words.is_empty())
        .map(|words| words.join(" "))
        .collect()
}

fn parse_name(name: &str) -> Result<Name, String> {
    if is_wrapped(name) {
        return Ok(Name::Corporate(latex::text(&name[1..name.len() - 1])?));
    }
    let comma_parts = split_outside_braces(name, |c| c == ',');
    let (family, given) = if comma_parts.len() > 1 {
        let (given, family) = comma_parts.split_last().expect("more than one part");
        (family.join(", "), (*given).to_owned())
    } else {
        let words = words_outside_braces(name);
        // The family name starts at the first lowercase word, as in "Ludwig van Beethoven", else at the last word.
        let start = (1..words.len().saturating_sub(1))
            .find(|&i| words[i].chars().next().is_some_and(char::is_lowercase))
            .unwrap_or(words.len().saturating_sub(1));
        (words[start..].join(" "), words[..start].join(" "))
    };
    Ok(Name::Person {
        family: latex::text(family.trim())?,
        given: latex::text(given.trim())?,
    })
}

/// Whether one brace pair encloses the whole text.
fn is_wrapped(text: &str) -> bool {
    if !(text.starts_with('{') && text.ends_with('}')) {
        return false;
    }
    let mut depth = 0usize;
    for (index, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return index == text.len() - 1;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(family: &str, given: &str) -> Name {
        Name::Person {
            family: family.to_owned(),
            given: given.to_owned(),
        }
    }

    #[test]
    fn parses_last_first_and_first_last_forms() {
        let names = Names::parse("Smith, Ada and Bob Jones AND Ludwig van Beethoven and Ada M{\\\"u}ller").unwrap();
        assert_eq!(
            names.list,
            [
                person("Smith", "Ada"),
                person("Jones", "Bob"),
                person("van Beethoven", "Ludwig"),
                person("Müller", "Ada")
            ]
        );
    }

    #[test]
    fn keeps_corporate_names_whole_and_reads_others() {
        let names = Names::parse("{World Health Organization} and others").unwrap();
        assert_eq!(names.list, [Name::Corporate("World Health Organization".to_owned())]);
        assert!(names.et_al);
        assert_eq!(names.short(Lang::En), "World Health Organization et al.");
    }

    #[test]
    fn formats_short_lists_per_language() {
        let two = Names::parse("Ada Smith and Bob Jones").unwrap();
        assert_eq!(two.short(Lang::En), "Smith and Jones");
        assert_eq!(two.short(Lang::De), "Smith und Jones");
        let three = Names::parse("Ada Smith and Bob Jones and Cy Lee").unwrap();
        assert_eq!(three.short(Lang::En), "Smith et al.");
    }

    #[test]
    fn formats_full_lists_per_language() {
        let three = Names::parse("Ada Smith and Bob Jones and Cy Lee").unwrap();
        assert_eq!(three.full(Lang::En), "Smith, Ada, Bob Jones, and Cy Lee");
        assert_eq!(three.full(Lang::De), "Smith, Ada, Bob Jones und Cy Lee");
        assert_eq!(
            Names::parse("Ada Smith and Bob Jones").unwrap().full(Lang::En),
            "Smith, Ada and Bob Jones"
        );
    }
}
