//! Supported entry types, their required fields, and the typed entry built from the raw syntax.

use super::bibtex::{Problem, RawEntry, RawField};
use super::latex;
use super::names::Names;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThesisKind {
    Phd,
    Masters,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Article,
    Book,
    InProceedings,
    Thesis(ThesisKind),
    Report,
    Online,
}

impl Kind {
    /// The kind for a lowercase BibTeX entry type.
    pub fn from_type(name: &str) -> Option<Self> {
        Some(match name {
            "article" => Self::Article,
            "book" => Self::Book,
            "inproceedings" | "conference" => Self::InProceedings,
            "phdthesis" => Self::Thesis(ThesisKind::Phd),
            "mastersthesis" => Self::Thesis(ThesisKind::Masters),
            "thesis" => Self::Thesis(ThesisKind::Other),
            "techreport" | "report" => Self::Report,
            "online" | "misc" => Self::Online,
            _ => return None,
        })
    }
}

/// Who leads a reference: the authors, else the editors, else a report's institution, else the title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead<'a> {
    Authors(&'a Names),
    Editors(&'a Names),
    Institution(&'a str),
    Title,
}

/// A validated entry. Fields the supported types do not use are dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub kind: Kind,
    pub authors: Names,
    pub editors: Names,
    pub title: String,
    pub year: Option<String>,
    /// The journal or, for conference papers, the proceedings title.
    pub container: Option<String>,
    pub volume: Option<String>,
    pub number: Option<String>,
    pub pages: Option<String>,
    /// The publisher, or the organization of a web resource.
    pub publisher: Option<String>,
    pub address: Option<String>,
    pub edition: Option<String>,
    /// The institution of a report, or the school of a thesis.
    pub institution: Option<String>,
    /// The type of a thesis or report, such as "Habilitation thesis". Replaces the default wording.
    pub type_: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub urldate: Option<String>,
}

impl Entry {
    pub fn lead(&self) -> Lead<'_> {
        match (&self.authors, &self.editors, &self.institution) {
            (authors, ..) if !authors.is_empty() => Lead::Authors(authors),
            (_, editors, _) if !editors.is_empty() => Lead::Editors(editors),
            (.., Some(institution)) if self.kind == Kind::Report => Lead::Institution(institution),
            _ => Lead::Title,
        }
    }

    /// Builds an entry from its raw form. Reports every problem found: LaTeX that cannot be converted in a used field,
    /// `crossref`, and missing required fields.
    pub fn from_raw(raw: &RawEntry, kind: Kind) -> Result<Self, Vec<Problem>> {
        let mut reader = FieldReader {
            raw,
            problems: Vec::new(),
        };
        if let Some(field) = raw.field("crossref") {
            let message = format!(
                "entry `{}`: `crossref` is not supported; repeat the fields in the entry",
                raw.key
            );
            reader.problems.push(Problem::new(message, field.at));
        }
        let authors = reader.names("author");
        let editors = reader.names("editor");
        let entry = Self {
            key: raw.key.clone(),
            kind,
            authors,
            editors,
            title: reader.text("title").unwrap_or_default(),
            year: reader.text("year"),
            container: reader.text(if kind == Kind::Article { "journal" } else { "booktitle" }),
            volume: reader.text("volume"),
            number: reader.text("number"),
            pages: reader.text("pages").map(|pages| pages_with_dash(&pages)),
            publisher: reader.text("publisher").or_else(|| reader.text("organization")),
            address: reader.text("address"),
            edition: reader.text("edition"),
            institution: reader.text("institution").or_else(|| reader.text("school")),
            type_: reader.text("type"),
            doi: reader.verbatim("doi"),
            url: reader.verbatim("url"),
            urldate: reader.text("urldate"),
        };
        let missing = entry.missing_fields();
        let FieldReader { mut problems, .. } = reader;
        if problems.is_empty() && !missing.is_empty() {
            let list = missing
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let message = format!("entry `{}` is missing required fields: {list}", raw.key);
            problems.push(Problem::new(message, raw.at));
        }
        if problems.is_empty() { Ok(entry) } else { Err(problems) }
    }

    /// The required fields this entry lacks, as written in the documentation table.
    fn missing_fields(&self) -> Vec<&'static str> {
        let has = |value: &Option<String>| value.is_some();
        let mut missing = Vec::new();
        let mut require = |present: bool, name: &'static str| {
            if !present {
                missing.push(name);
            }
        };
        require(!self.title.is_empty(), "title");
        match self.kind {
            Kind::Article => {
                require(!self.authors.is_empty(), "author");
                require(has(&self.container), "journal");
                require(has(&self.year), "year");
            }
            Kind::Book => {
                require(!self.authors.is_empty() || !self.editors.is_empty(), "author or editor");
                require(has(&self.publisher), "publisher");
                require(has(&self.year), "year");
            }
            Kind::InProceedings => {
                require(!self.authors.is_empty(), "author");
                require(has(&self.container), "booktitle");
                require(has(&self.year), "year");
            }
            Kind::Thesis(_) => {
                require(!self.authors.is_empty(), "author");
                require(has(&self.institution), "school");
                require(has(&self.year), "year");
            }
            Kind::Report => {
                require(has(&self.institution), "institution");
                require(has(&self.year), "year");
            }
            Kind::Online => require(has(&self.url), "url"),
        }
        missing
    }
}

/// Reads fields by name, converting them and collecting conversion problems.
struct FieldReader<'a> {
    raw: &'a RawEntry,
    problems: Vec<Problem>,
}

impl FieldReader<'_> {
    fn get<T>(&mut self, name: &str, convert: impl Fn(&str) -> Result<T, String>) -> Option<T> {
        let RawField { value, at, .. } = self.raw.field(name)?;
        match convert(value) {
            Ok(converted) => Some(converted),
            Err(reason) => {
                let message = format!(
                    "entry `{}`, field `{name}`: unsupported text, found {reason}",
                    self.raw.key
                );
                self.problems.push(Problem::new(message, *at));
                None
            }
        }
    }

    /// Converted text, `None` when absent or empty.
    fn text(&mut self, name: &str) -> Option<String> {
        self.get(name, latex::text).filter(|text| !text.is_empty())
    }

    fn names(&mut self, name: &str) -> Names {
        self.get(name, Names::parse).unwrap_or_default()
    }

    fn verbatim(&mut self, name: &str) -> Option<String> {
        self.get(name, |value| Ok(latex::verbatim(value)))
            .filter(|text| !text.is_empty())
    }
}

/// Turns a hyphen between digits into an en dash: "45-67" becomes "45–67".
fn pages_with_dash(pages: &str) -> String {
    let chars: Vec<char> = pages.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let between_digits =
                i > 0 && chars.get(i + 1).is_some_and(char::is_ascii_digit) && chars[i - 1].is_ascii_digit();
            if c == '-' && between_digits { '–' } else { c }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::bibtex::read;
    use super::*;

    fn build(text: &str) -> Result<Entry, Vec<Problem>> {
        let (entries, problems) = read(text);
        assert!(problems.is_empty());
        let kind = Kind::from_type(&entries[0].kind).unwrap();
        Entry::from_raw(&entries[0], kind)
    }

    #[test]
    fn maps_type_aliases() {
        assert_eq!(Kind::from_type("conference"), Some(Kind::InProceedings));
        assert_eq!(
            Kind::from_type("mastersthesis"),
            Some(Kind::Thesis(ThesisKind::Masters))
        );
        assert_eq!(Kind::from_type("techreport"), Some(Kind::Report));
        assert_eq!(Kind::from_type("misc"), Some(Kind::Online));
        assert_eq!(Kind::from_type("inbook"), None);
    }

    #[test]
    fn reports_all_missing_required_fields_at_the_entry() {
        let problems = build("\n  @article{a1, title = {T}, author = {A B}, note = {ignored}}").unwrap_err();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].at, (2, 3));
        assert_eq!(
            problems[0].message,
            "entry `a1` is missing required fields: `journal`, `year`"
        );
    }

    #[test]
    fn names_the_entry_and_field_for_unsupported_latex() {
        let problems =
            build("@book{b1, title = {A \\textbf{B}}, author = {A B}, publisher = {P}, year = 2020}").unwrap_err();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.starts_with("entry `b1`, field `title`:"));
        assert!(problems[0].message.contains("\\textbf"));
        assert_eq!(problems[0].at, (1, 11));
    }

    #[test]
    fn ignores_unsupported_text_in_unused_fields_and_rejects_crossref() {
        assert!(build("@misc{m, title = {T}, url = {u}, abstract = {\\textbf{x}}}").is_ok());
        let problems = build("@misc{m, title = {T}, url = {u}, crossref = {other}}").unwrap_err();
        assert!(problems[0].message.contains("`crossref` is not supported"));
    }

    #[test]
    fn uses_editors_or_institution_when_authors_are_missing() {
        let book = build("@book{b, title = {T}, editor = {Ada Smith}, publisher = {P}, year = 2020}").unwrap();
        assert!(matches!(book.lead(), Lead::Editors(_)));
        let report = build("@techreport{r, title = {T}, institution = {Lab}, year = 2020}").unwrap();
        assert_eq!(report.lead(), Lead::Institution("Lab"));
        let web = build("@online{w, title = {T}, url = {https://a.org}}").unwrap();
        assert_eq!(web.lead(), Lead::Title);
    }

    #[test]
    fn normalizes_page_ranges() {
        assert_eq!(pages_with_dash("45-67"), "45–67");
        assert_eq!(pages_with_dash("e-12"), "e-12");
    }
}
