//! What the supporting material is, without gpui: the files under `research/` and `notes/`, the probes under
//! `notes/probes/`, and the bibliography's works, with where the open chapter uses each.

use std::fs;
use std::ops::Range;
use std::path::Path;

use druck::editor::rich::Work;
use druck::editor::syntax::{self, Segment};

use crate::editor::rich::images::Figure;
use crate::source::{BlockKind, Blocks};

pub const RESEARCH: &str = "research";
pub const NOTES: &str = "notes";
/// Probes are notes in their own folder, so they stay notes for everything else that reads `notes/`.
pub const PROBES: &str = "notes/probes";
/// The id of a bibliography entry starts with this, then its key. A file's id is its path, which never does.
const SOURCE_ID: &str = "@";

/// Where an item belongs: a rail tab, or the bibliography.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Research,
    Notes,
    Probes,
    Sources,
}

/// What an item is, which says how it shows and what Shift+Return inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Form {
    Text,
    Image,
    Table,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The path from the book's folder for a file, `@key` for a bibliography entry.
    pub id: String,
    pub kind: Kind,
    pub form: Form,
    pub title: String,
    /// The front matter's `date:`, for a text.
    pub date: Option<String>,
    /// The bibliography key the item stands for, see [`key_of`].
    pub key: Option<String>,
    /// A text's body without its front matter, read when the material is scanned.
    pub text: Option<String>,
}

impl Item {
    /// The path from the book's folder, for a file.
    pub fn path(&self) -> Option<&str> {
        (!self.id.starts_with(SOURCE_ID)).then_some(self.id.as_str())
    }

    /// The file's name, or the entry's key.
    pub fn name(&self) -> &str {
        self.id.rsplit('/').next().unwrap_or(&self.id)
    }

    /// How a list names the item: a file by its name, a bibliography entry by its authors, year, and title.
    pub fn label(&self) -> &str {
        if self.form == Form::Source { &self.title } else { self.name() }
    }

    /// The text's paragraphs without its headings, joined, for a short excerpt.
    pub fn prose(&self) -> String {
        let text = self.text.as_deref().unwrap_or_default();
        text.split("\n\n")
            .map(str::trim)
            .filter(|paragraph| !paragraph.is_empty() && !paragraph.starts_with('#'))
            .map(|paragraph| paragraph.replace('\n', " "))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The paragraphs of a text, trimmed, without headings' markers.
    pub fn paragraphs(&self) -> Vec<String> {
        let Some(text) = &self.text else {
            return Vec::new();
        };
        text.split("\n\n")
            .map(|paragraph| paragraph.trim().trim_start_matches('#').trim().replace('\n', " "))
            .filter(|paragraph| !paragraph.is_empty())
            .collect()
    }

    /// How many words a text has.
    pub fn words(&self) -> usize {
        self.text.as_deref().map_or(0, |text| text.split_whitespace().count())
    }
}

/// A text file's front matter: the value of `name:` between `---` lines at its start, unquoted.
fn front_value<'a>(front: &'a str, name: &str) -> Option<&'a str> {
    front.lines().find_map(|line| {
        let (field, value) = line.split_once(':')?;
        (field.trim() == name)
            .then(|| value.trim().trim_matches(['"', '\'']))
            .filter(|value| !value.is_empty())
    })
}

/// A text split into its front matter and its body.
fn split_front(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return ("", text);
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + 4..];
            (&rest[..end], after.trim_start_matches(['\r', '\n']))
        }
        None => ("", text),
    }
}

/// The bibliography key an item stands for: its front matter's `key:`, or else a key of the bibliography that is the
/// file's name without its extension. The smallest rule that needs nothing new for a file named after its source.
pub fn key_of(front: &str, stem: &str, works: &[Work]) -> Option<String> {
    front_value(front, "key").map(str::to_owned).or_else(|| {
        works
            .iter()
            .find(|work| work.key.eq_ignore_ascii_case(stem))
            .map(|work| work.key.clone())
    })
}

fn form_of(path: &Path) -> Option<Form> {
    let extension = path.extension()?.to_str()?.to_lowercase();
    Some(match extension.as_str() {
        "md" | "txt" | "markdown" => Form::Text,
        "png" | "jpg" | "jpeg" | "svg" => Form::Image,
        "csv" => Form::Table,
        _ => return None,
    })
}

/// Every file below `folder`, hidden ones left out, as paths from `base` with forward slashes.
fn files_below(base: &Path, folder: &str, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(base.join(folder)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = format!("{folder}/{name}");
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => files_below(base, &path, found),
            Ok(_) => found.push(path),
            Err(_) => {}
        }
    }
}

/// Reads the material of the book in `base`: research, notes, probes, and the bibliography's works. Texts are read
/// whole, since they are notes and short, and searching needs them.
pub fn scan(base: &Path, works: &[Work]) -> Vec<Item> {
    let mut paths = Vec::new();
    files_below(base, RESEARCH, &mut paths);
    files_below(base, NOTES, &mut paths);
    paths.sort();
    let mut items: Vec<Item> = paths
        .into_iter()
        .filter_map(|id| {
            let path = Path::new(&id);
            let form = form_of(path)?;
            if form != Form::Text && !id.starts_with(RESEARCH) {
                return None;
            }
            let kind = if id.starts_with(&format!("{PROBES}/")) {
                Kind::Probes
            } else if id.starts_with(RESEARCH) {
                Kind::Research
            } else {
                Kind::Notes
            };
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            let read = (form == Form::Text)
                .then(|| fs::read_to_string(base.join(&id)).ok())
                .flatten();
            let (front, body) = read.as_deref().map_or(("", ""), split_front);
            let title = front_value(front, "title")
                .map(str::to_owned)
                .or_else(|| {
                    body.lines()
                        .find_map(|line| line.strip_prefix("# "))
                        .map(|title| title.trim().to_owned())
                })
                .unwrap_or_else(|| path.file_name().map_or(stem.clone(), |name| name.to_string_lossy().into()));
            Some(Item {
                kind,
                form,
                title,
                date: front_value(front, "date").map(str::to_owned),
                key: key_of(front, &stem, works),
                text: read.as_ref().map(|_| body.to_owned()),
                id,
            })
        })
        .collect();
    items.extend(works.iter().map(|work| Item {
        id: format!("{SOURCE_ID}{}", work.key),
        kind: Kind::Sources,
        form: Form::Source,
        title: source_title(work),
        date: None,
        key: Some(work.key.clone()),
        text: None,
    }));
    items
}

/// "Klein 1998, Die Maut zu Golling".
fn source_title(work: &Work) -> String {
    let lead = [work.authors.as_str(), work.year.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match (lead.is_empty(), work.title.is_empty()) {
        (true, true) => work.key.clone(),
        (true, false) => work.title.clone(),
        (false, true) => lead,
        (false, false) => format!("{lead}, {}", work.title),
    }
}

/// Where the open chapter uses an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// A figure of the image, in this section.
    Figure(String),
    /// Cited once, in this section.
    Section(String),
    /// Cited this many times.
    Cited(usize),
}

/// One item's uses in the chapter: where, for its list's row, and the sections that cite it, for its header's links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uses {
    pub item: String,
    pub shown: Where,
    /// Each section that cites it once, with the offset of its first citation there.
    pub sections: Vec<(String, usize)>,
}

/// A heading's words, without its markers and a closing `{...}`.
fn heading_title(block: &str) -> String {
    let line = block.lines().next().unwrap_or_default().trim().trim_start_matches('#').trim();
    let line = match line.rfind('{') {
        Some(open) if line.ends_with('}') => line[..open].trim_end(),
        _ => line,
    };
    line.trim_end_matches('#').trim_end().to_owned()
}

/// What the chapter's `text` uses of `items`, in the order of first use: figures of an image by its path, and citations of
/// a key an item stands for. Druck has no table built from a CSV file, so a table is never used.
pub fn uses(items: &[Item], text: &str) -> Vec<Uses> {
    let blocks = Blocks::parse(text);
    let mut section = String::new();
    // In order of first use: the item, and each use's section and offset.
    let mut found: Vec<(String, bool, Vec<(String, usize)>)> = Vec::new();
    let mut note = |item: &str, figure: bool, section: &str, offset: usize| {
        let entry = match found.iter_mut().position(|(id, ..)| id == item) {
            Some(at) => &mut found[at],
            None => {
                found.push((item.to_owned(), figure, Vec::new()));
                found.last_mut().expect("just pushed")
            }
        };
        entry.2.push((section.to_owned(), offset));
    };
    for block in blocks.as_slice() {
        let body = &text[block.range.clone()];
        match block.kind {
            BlockKind::Heading { .. } => section = heading_title(body),
            BlockKind::Paragraph | BlockKind::Quote | BlockKind::List => {
                if let Some(figure) = Figure::parse(body) {
                    let path = figure.path(body).trim_start_matches("./");
                    if let Some(item) = items.iter().find(|item| item.form == Form::Image && item.id == path) {
                        note(&item.id, true, &section, block.range.start);
                    }
                }
                if !body.contains('@') {
                    continue;
                }
                for segment in syntax::find(body) {
                    let Segment::Citation(citation) = segment else {
                        continue;
                    };
                    for cited in &citation.items {
                        let offset = block.range.start + cited.range.start;
                        for item in items.iter().filter(|item| item.key.as_deref() == Some(cited.key.as_str())) {
                            note(&item.id, false, &section, offset);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    found
        .into_iter()
        .map(|(item, figure, places)| {
            let mut sections: Vec<(String, usize)> = Vec::new();
            for (title, offset) in &places {
                if !sections.iter().any(|(seen, _)| seen == title) {
                    sections.push((title.clone(), *offset));
                }
            }
            let shown = match (figure, places.len()) {
                (true, _) => Where::Figure(places[0].0.clone()),
                (false, 1) => Where::Section(places[0].0.clone()),
                (false, count) => Where::Cited(count),
            };
            Uses {
                item,
                shown,
                sections,
            }
        })
        .collect()
}

/// Where a text names another item by its file name, as an interview names a scan: the byte ranges and the item's id.
pub fn mentions(text: &str, items: &[Item], own: &str) -> Vec<(Range<usize>, String)> {
    let mut found: Vec<(Range<usize>, String)> = Vec::new();
    for item in items.iter().filter(|item| item.id != own && item.path().is_some()) {
        let name = item.name();
        let mut from = 0;
        while let Some(at) = text[from..].find(name) {
            let start = from + at;
            let range = start..start + name.len();
            if !found.iter().any(|(other, _)| other.start < range.end && range.start < other.end) {
                found.push((range, item.id.clone()));
            }
            from = start + name.len();
        }
    }
    found.sort_by_key(|(range, _)| range.start);
    found
}

/// The items whose title, path, or key hold every word of `query`, ignoring case, in the order given.
pub fn matching<'a>(items: &'a [Item], query: &str) -> Vec<&'a Item> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    items
        .iter()
        .filter(|item| {
            let haystack = format!("{} {} {}", item.title, item.id, item.key.as_deref().unwrap_or("")).to_lowercase();
            words.iter().all(|word| haystack.contains(word))
        })
        .collect()
}

/// Rows of a CSV text, as far as they read: fields split on commas outside double quotes, with `""` for a quote.
pub fn csv_rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = Vec::new();
            let mut field = String::new();
            let mut quoted = false;
            let mut chars = line.chars().peekable();
            while let Some(char) = chars.next() {
                match (char, quoted) {
                    ('"', true) if chars.peek() == Some(&'"') => {
                        field.push('"');
                        chars.next();
                    }
                    ('"', _) => quoted = !quoted,
                    (',', false) => fields.push(std::mem::take(&mut field).trim().to_owned()),
                    _ => field.push(char),
                }
            }
            fields.push(field.trim().to_owned());
            fields
        })
        .collect()
}

/// A CSV's rows as a Markdown pipe table, its first row the header.
pub fn pipe_table(rows: &[Vec<String>]) -> Option<String> {
    let columns = rows.iter().map(Vec::len).max()?;
    let line = |row: &[String]| {
        let cells: Vec<String> = (0..columns)
            .map(|column| row.get(column).map_or(String::new(), |cell| cell.replace('|', "\\|")))
            .collect();
        format!("| {} |", cells.join(" | "))
    };
    let mut lines = vec![line(&rows[0]), format!("|{}|", vec!["---"; columns].join("|"))];
    lines.extend(rows[1..].iter().map(|row| line(row)));
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(key: &str) -> Work {
        Work {
            key: key.to_owned(),
            authors: "Salzmann".to_owned(),
            title: "Interview".to_owned(),
            year: "2024".to_owned(),
        }
    }

    fn item(id: &str, form: Form, key: Option<&str>) -> Item {
        Item {
            id: id.to_owned(),
            kind: Kind::Research,
            form,
            title: id.to_owned(),
            date: None,
            key: key.map(str::to_owned),
            text: None,
        }
    }

    #[test]
    fn an_item_stands_for_its_front_matters_key_or_a_key_named_as_its_file() {
        let works = [work("salzmann2024")];
        assert_eq!(key_of("key: klein1998", "interview", &works).as_deref(), Some("klein1998"));
        assert_eq!(key_of("", "Salzmann2024", &works).as_deref(), Some("salzmann2024"));
        assert_eq!(key_of("title: x", "interview", &works), None);
    }

    #[test]
    fn scanning_finds_research_notes_and_probes_with_their_front_matter() {
        let base = std::env::temp_dir().join(format!("druck-material-{}", std::process::id()));
        fs::create_dir_all(base.join("research/scans")).unwrap();
        fs::create_dir_all(base.join("notes/probes")).unwrap();
        fs::write(base.join("research/interview.md"), "---\ntitle: \"With Anna\"\ndate: 2024-03-14\nkey: s24\n---\n\nHow much?\n").unwrap();
        fs::write(base.join("research/scans/board.png"), b"").unwrap();
        fs::write(base.join("research/tariff.csv"), "a,b\n").unwrap();
        fs::write(base.join("research/.hidden.md"), "no").unwrap();
        fs::write(base.join("notes/timeline.md"), "# Timeline\n\n1612\n").unwrap();
        fs::write(base.join("notes/probes/gate.md"), "The gate is shut.\n").unwrap();
        fs::write(base.join("notes/sketch.png"), b"").unwrap();
        let items = scan(&base, &[work("k")]);
        let ids: Vec<_> = items.iter().map(|item| (item.id.as_str(), item.kind, item.form)).collect();
        assert_eq!(
            ids,
            [
                ("notes/probes/gate.md", Kind::Probes, Form::Text),
                ("notes/timeline.md", Kind::Notes, Form::Text),
                ("research/interview.md", Kind::Research, Form::Text),
                ("research/scans/board.png", Kind::Research, Form::Image),
                ("research/tariff.csv", Kind::Research, Form::Table),
                ("@k", Kind::Sources, Form::Source),
            ]
        );
        let interview = &items[2];
        assert_eq!(interview.title, "With Anna");
        assert_eq!(interview.date.as_deref(), Some("2024-03-14"));
        assert_eq!(interview.key.as_deref(), Some("s24"));
        assert_eq!(interview.text.as_deref(), Some("How much?\n"));
        assert_eq!(items[1].title, "Timeline");
        assert_eq!(items[5].title, "Salzmann 2024, Interview");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_chapter_uses_images_by_path_and_items_by_cited_key_with_their_sections() {
        let items = [
            item("research/interview.md", Form::Text, Some("s24")),
            item("research/board.png", Form::Image, None),
            item("@s24", Form::Source, Some("s24")),
            item("research/tariff.csv", Form::Table, None),
        ];
        let text = "# Tolls\n\nAs she says [@s24, p. 3].\n\n## The bridge\n\n![Board](research/board.png)\n\nAgain @s24 and [@s24].\n";
        let found = uses(&items, text);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].item, "research/interview.md");
        assert_eq!(found[0].shown, Where::Cited(3));
        let sections: Vec<_> = found[0].sections.iter().map(|(title, _)| title.as_str()).collect();
        assert_eq!(sections, ["Tolls", "The bridge"]);
        assert_eq!(&text[found[0].sections[0].1..found[0].sections[0].1 + 4], "@s24");
        assert_eq!(found[2].item, "research/board.png");
        assert_eq!(found[2].shown, Where::Figure("The bridge".to_owned()));
    }

    #[test]
    fn a_text_names_other_items_by_their_file_names() {
        let items = [
            item("research/scans/board.png", Form::Image, None),
            item("research/interview.md", Form::Text, None),
        ];
        let text = "Look at board.png in your scans, and interview.md.";
        let found = mentions(text, &items, "research/interview.md");
        assert_eq!(found, [(8..17, "research/scans/board.png".to_owned())]);
    }

    #[test]
    fn a_query_matches_every_word_in_title_path_or_key() {
        let items = [item("research/tariff.csv", Form::Table, None), item("notes/open.md", Form::Text, Some("tar"))];
        let found: Vec<_> = matching(&items, "TAR").iter().map(|item| item.id.as_str()).collect();
        assert_eq!(found, ["research/tariff.csv", "notes/open.md"]);
        assert!(matching(&items, "tar csv nothing").is_empty());
    }

    #[test]
    fn a_csv_becomes_a_pipe_table_with_quoted_commas_kept() {
        let rows = csv_rows("Load,Toll\n\"Horse, two sacks\",4 Kr\nFuder\n");
        assert_eq!(rows[1], ["Horse, two sacks", "4 Kr"]);
        assert_eq!(
            pipe_table(&rows).unwrap(),
            "| Load | Toll |\n|---|---|\n| Horse, two sacks | 4 Kr |\n| Fuder |  |"
        );
    }
}
