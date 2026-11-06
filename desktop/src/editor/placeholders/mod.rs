//! Placeholders in Write: each `{name}` shows the value a render writes, and typing `{` and letters completes a name.
//! The values come from the book's settings, the engine's statistics of the whole book, and the day, through
//! [`druck::editor::placeholders`], so Write never counts or formats on its own. The logic is here without gpui, and
//! the card is in [`card`].

pub mod card;
mod strings;
#[cfg(test)]
mod tests;

use std::ops::Range;

use druck::editor::placeholders::{self as engine, Date, Placeholder, Statistics};
use druck::editor::resolved_config::FrontMatter;

pub use self::strings::Key;
use crate::editor::commands::Command;
use crate::editor::display::{Chip, ChipLook};
use crate::source::inline::{InlineSpan, SpanKind, has_inline, inline_spans};
use crate::source::{BlockKind, Blocks, Edit, Selection};

/// How a placeholder's value shows in the text, a setting under Writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    /// On a quiet tint, as a chip.
    #[default]
    Chip,
    /// As plain text with a faint dotted line under it.
    Underline,
}

/// The group a name is listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Statistics,
    Book,
    Date,
}

impl Group {
    pub fn label(self) -> &'static str {
        crate::t!(match self {
            Self::Statistics => Key::Statistics,
            Self::Book => Key::Book,
            Self::Date => Key::Date,
        })
    }

    /// Where the value comes from, for the hint at a placeholder the caret is in.
    pub fn hint(self) -> &'static str {
        crate::t!(match self {
            Self::Statistics => Key::StatisticsHint,
            Self::Book => Key::BookHint,
            Self::Date => Key::DateHint,
        })
    }
}

/// The names the text takes, in the order the card lists them. `{meta.key}` follows the book's own keys.
const NAMES: [(&str, Group); 18] = [
    ("words", Group::Statistics),
    ("reading-time", Group::Statistics),
    ("sentences", Group::Statistics),
    ("paragraphs", Group::Statistics),
    ("chars", Group::Statistics),
    ("chars-no-spaces", Group::Statistics),
    ("figures", Group::Statistics),
    ("tables", Group::Statistics),
    ("title", Group::Book),
    ("subtitle", Group::Book),
    ("author", Group::Book),
    ("date", Group::Book),
    ("abstract", Group::Book),
    ("draft", Group::Book),
    ("year", Group::Date),
    ("build-date", Group::Date),
    // Never listed: the page placeholders are for headers and footers. They stand here so a lookup names them.
    ("page", Group::Book),
    ("section", Group::Book),
];

/// How many of [`NAMES`] the card lists.
const LISTED: usize = 16;

/// What a name says in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Value(String),
    /// Known, but this book gives it none: `{subtitle}` without a subtitle, or `{draft}` outside a draft.
    NoValue,
    /// Known, and not counted yet.
    Counting,
    Unknown,
    /// `{page}` and its kind, which only headers and footers have.
    PageDependent,
}

/// What the values come from: the book's settings, the statistics of all its chapters, and the day.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values {
    /// The book's language, `"en"` or `"de"`, which counts and dates follow.
    lang: String,
    title: Option<String>,
    subtitle: Option<String>,
    author: Option<String>,
    date: Option<String>,
    abstract_: Option<String>,
    draft: bool,
    meta: Vec<(String, String)>,
    statistics: Option<Statistics>,
    today: Option<Date>,
}

impl Values {
    /// The values a book with these settings, statistics, and language has today.
    pub fn new(settings: Option<&FrontMatter>, statistics: Option<Statistics>, lang: &str) -> Self {
        let mut values = Self {
            lang: if lang == "de" { "de" } else { "en" }.to_owned(),
            statistics,
            today: engine::today(),
            ..Self::default()
        };
        if let Some(settings) = settings {
            values.title.clone_from(&settings.title);
            values.subtitle.clone_from(&settings.subtitle);
            values.author = settings.author.as_ref().map(|names| names.0.join(", "));
            values.date.clone_from(&settings.date);
            values.abstract_.clone_from(&settings.abstract_);
            values.draft = settings.draft.unwrap_or(false);
            values.meta = settings
                .meta
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), meta_text(serde_json::to_value(value).ok()?)?)))
                .collect();
        }
        values
    }

    /// What `{name}` shows in this book.
    pub fn lookup(&self, name: &str) -> Lookup {
        let Ok(placeholder) = Placeholder::parse(name) else {
            return Lookup::Unknown;
        };
        if placeholder.is_page_dependent() {
            return Lookup::PageDependent;
        }
        let count = |pick: fn(&Statistics) -> usize| {
            (self.statistics.as_ref()).map(|statistics| engine::count(pick(statistics), &self.lang))
        };
        let text = match &placeholder {
            Placeholder::Title => self.title.clone(),
            Placeholder::Subtitle => self.subtitle.clone(),
            Placeholder::Author => self.author.clone(),
            Placeholder::Date => self.date.clone(),
            Placeholder::Abstract => self.abstract_.clone(),
            Placeholder::Meta(key) => (self.meta.iter())
                .find(|(known, _)| known == key)
                .map(|(_, value)| value.clone()),
            Placeholder::Draft => self.draft.then(|| engine::draft_label(&self.lang)),
            Placeholder::BuildDate => self.today.map(|today| engine::long_date(today, &self.lang)),
            Placeholder::Year => self.today.map(|today| today.year.to_string()),
            statistic => {
                let counted = match statistic {
                    Placeholder::Chars => count(|s| s.chars),
                    Placeholder::CharsNoSpaces => count(|s| s.chars_no_spaces),
                    Placeholder::Words => count(|s| s.words),
                    Placeholder::Sentences => count(|s| s.sentences),
                    Placeholder::Paragraphs => count(|s| s.paragraphs),
                    Placeholder::Figures => count(|s| s.figures),
                    Placeholder::Tables => count(|s| s.tables),
                    _ => (self.statistics.as_ref())
                        .map(|statistics| engine::count(engine::reading_time(statistics, &self.lang), &self.lang)),
                };
                return counted.map_or(Lookup::Counting, Lookup::Value);
            }
        };
        // As the engine has it, a blank value is none.
        match text.filter(|text| !text.trim().is_empty()) {
            Some(text) => Lookup::Value(text.split_whitespace().collect::<Vec<_>>().join(" ")),
            None => Lookup::NoValue,
        }
    }

    /// The names the card lists, with their groups: the fixed names, then the book's `meta` keys under Book.
    pub fn names(&self) -> Vec<(String, Group)> {
        let fixed = NAMES[..LISTED].iter().map(|(name, group)| ((*name).to_owned(), *group));
        let meta = self.meta.iter().map(|(key, _)| (format!("meta.{key}"), Group::Book));
        let mut names: Vec<(String, Group)> = fixed.chain(meta).collect();
        // Grouped as the card shows them, keeping each group's order.
        names.sort_by_key(|(_, group)| *group as u8);
        names
    }

    /// Words a minute for `{reading-time}` in this book's language, which the card says.
    pub fn reading_speed(&self) -> usize {
        engine::words_per_minute(&self.lang)
    }
}

/// A `meta` value as the text shows it: a list joins its entries with commas.
fn meta_text(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Array(lines) => {
            let entries: Vec<&str> = (lines.iter())
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            (!entries.is_empty()).then(|| entries.join(", "))
        }
        _ => None,
    }
}

/// The group a name is listed under.
pub fn group_of(name: &str) -> Group {
    if name.starts_with("meta.") {
        return Group::Book;
    }
    NAMES
        .iter()
        .find(|(known, _)| *known == name)
        .map_or(Group::Book, |(_, group)| *group)
}

/// What the card calls a name and what it says it counts.
pub fn describe(name: &str, values: &Values) -> (String, String) {
    let pair = |title: Key, counts: Key| (crate::t!(title).to_owned(), crate::t!(counts).to_owned());
    match name {
        "words" => pair(Key::Words, Key::WordsCounts),
        "reading-time" => (
            crate::t!(Key::ReadingTime).to_owned(),
            crate::t!(Key::ReadingTimeCounts, speed = values.reading_speed()),
        ),
        "sentences" => pair(Key::Sentences, Key::SentencesCounts),
        "paragraphs" => pair(Key::Paragraphs, Key::ParagraphsCounts),
        "chars" => pair(Key::Chars, Key::CharsCounts),
        "chars-no-spaces" => pair(Key::CharsNoSpaces, Key::CharsNoSpacesCounts),
        "figures" => pair(Key::Figures, Key::FiguresCounts),
        "tables" => pair(Key::Tables, Key::TablesCounts),
        "title" => pair(Key::Title, Key::TitleCounts),
        "subtitle" => pair(Key::Subtitle, Key::SubtitleCounts),
        "author" => pair(Key::Author, Key::AuthorCounts),
        "date" => pair(Key::DateName, Key::DateCounts),
        "abstract" => pair(Key::Abstract, Key::AbstractCounts),
        "draft" => pair(Key::Draft, Key::DraftCounts),
        "year" => pair(Key::Year, Key::YearCounts),
        "build-date" => pair(Key::BuildDate, Key::BuildDateCounts),
        other => (
            other.strip_prefix("meta.").unwrap_or(other).to_owned(),
            crate::t!(Key::MetaCounts).to_owned(),
        ),
    }
}

/// What a lookup shows in a list or a hint: the value, or why there is none.
pub fn shown(lookup: &Lookup) -> String {
    match lookup {
        Lookup::Value(value) => value.clone(),
        Lookup::NoValue => crate::t!(Key::NoValue).to_owned(),
        Lookup::Counting => crate::t!(Key::Counting).to_owned(),
        Lookup::Unknown => crate::t!(Key::Unknown).to_owned(),
        Lookup::PageDependent => crate::t!(Key::OnlyInBands).to_owned(),
    }
}

/// A value longer than this may wrap; a shorter one stays on one row, as a chip does.
const KEEP_TOGETHER: usize = 32;

/// The chips of a block's placeholders and todos. A placeholder the caret stands inside shows its source, so it can be
/// typed in. One without a value, with an unknown name, or in a heading shows its source in the accent color.
pub fn chips(
    kind: BlockKind,
    text: &str,
    spans: &[InlineSpan],
    caret: Option<usize>,
    values: &Values,
    display: Display,
) -> Vec<Chip> {
    let mut chips = Vec::new();
    for span in spans {
        match span.kind {
            SpanKind::Placeholder => {
                if caret.is_some_and(|caret| span.range.start < caret && caret < span.range.end) {
                    continue;
                }
                let written = &text[span.range.clone()];
                let name = &written[1..written.len() - 1];
                let lookup = if matches!(kind, BlockKind::Heading { .. }) {
                    Lookup::Unknown
                } else {
                    values.lookup(name)
                };
                let (shown, look) = match lookup {
                    Lookup::Value(value) => (value, ChipLook::Value),
                    Lookup::Counting => ("…".to_owned(), ChipLook::Value),
                    _ => (written.to_owned(), ChipLook::Open),
                };
                let shown = if shown.chars().count() <= KEEP_TOGETHER {
                    shown.replace(' ', "\u{a0}")
                } else {
                    shown
                };
                chips.push(Chip {
                    range: span.range.clone(),
                    text: shown,
                    look,
                    atomic: true,
                });
            }
            // The label stands in front of the words while the todo's markers are hidden.
            SpanKind::Todo if display == Display::Chip && span.open.end < span.close.start => {
                if caret.is_some_and(|caret| span.range.start <= caret && caret <= span.range.end) {
                    continue;
                }
                chips.push(Chip {
                    range: span.open.end..span.open.end,
                    text: format!("{}\u{a0}", crate::t!(Key::ToDo)),
                    look: ChipLook::NoteLabel,
                    atomic: false,
                });
            }
            _ => {}
        }
    }
    chips
}

/// The hint a placeholder shows while the caret is inside it: its value and where that comes from, or why it has none.
pub fn hint(kind: BlockKind, name: &str, values: &Values) -> String {
    if matches!(kind, BlockKind::Heading { .. }) {
        return crate::t!(Key::NotInHeading).to_owned();
    }
    let lookup = values.lookup(name);
    match lookup {
        Lookup::Value(_) | Lookup::Counting => format!("{} · {}", shown(&lookup), group_of(name).hint()),
        _ => shown(&lookup),
    }
}

/// The placeholder the caret stands inside, as its range in the chapter and its name.
pub fn placeholder_at(text: &str, blocks: &Blocks, caret: usize) -> Option<(Range<usize>, String)> {
    let block = &blocks.as_slice()[blocks.index_at(caret)];
    let body = &text[block.range.clone()];
    let at = caret - block.range.start;
    inline_spans(block.kind, body)
        .into_iter()
        .find(|span| span.kind == SpanKind::Placeholder && span.range.start < at && at < span.range.end)
        .map(|span| {
            let name = body[span.range.start + 1..span.range.end - 1].to_owned();
            (
                block.range.start + span.range.start..block.range.start + span.range.end,
                name,
            )
        })
}

/// A name being typed after `{`: the brace's offset in the chapter and the letters after it, up to the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub start: usize,
    pub typed: String,
}

/// Whether a character may stand in a placeholder name after its first letter.
fn name_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

/// The name typed before the caret, where the text reads placeholders: not in code, a heading, a comment, or a raw
/// block, and not after `\{`. A caret inside a name already closed is not typing one.
pub fn query_at(text: &str, blocks: &Blocks, caret: usize) -> Option<Query> {
    let block = &blocks.as_slice()[blocks.index_at(caret)];
    if !has_inline(block.kind) || matches!(block.kind, BlockKind::Heading { .. }) {
        return None;
    }
    let before = &text[block.range.start..caret];
    let typed_len = before.len() - before.trim_end_matches(name_char).len();
    let brace = caret - typed_len - 1;
    let typed = &text[brace + 1..caret];
    if brace < block.range.start
        || text.as_bytes()[brace] != b'{'
        || !typed.starts_with(|character: char| character.is_ascii_alphabetic())
        || druck::editor::marks::escaped(text, brace)
    {
        return None;
    }
    let after = &text[caret..block.range.end];
    if after.trim_start_matches(name_char).starts_with('}') {
        return None;
    }
    // A code span still being typed has no closing backtick yet.
    if text[block.range.start..brace].matches('`').count() % 2 == 1 {
        return None;
    }
    let body = &text[block.range.clone()];
    let at = brace - block.range.start;
    let inside_other = inline_spans(block.kind, body).into_iter().any(|span| {
        matches!(span.kind, SpanKind::Code | SpanKind::Comment | SpanKind::Link)
            && span.range.start <= at
            && at < span.range.end
    });
    (!inside_other).then(|| Query {
        start: brace,
        typed: typed.to_owned(),
    })
}

/// Whether a name matches what is typed: it starts with it, or holds it.
pub fn matches(name: &str, typed: &str) -> bool {
    name.contains(&typed.to_ascii_lowercase())
}

/// The best name for what is typed: the first that starts with it, else the first that holds it.
pub fn best(names: &[(String, Group)], typed: &str) -> Option<usize> {
    let typed = typed.to_ascii_lowercase();
    (names.iter().position(|(name, _)| name.starts_with(&typed)))
        .or_else(|| names.iter().position(|(name, _)| name.contains(&typed)))
}

/// What a chosen name adds after what is typed, as the ghost shows it: the rest of the name and its brace. A name that
/// does not start with what is typed has no ghost.
pub fn ghost(name: &str, typed: &str) -> Option<String> {
    name.strip_prefix(&typed.to_ascii_lowercase())
        .map(|rest| format!("{rest}}}"))
}

/// The edit that writes `{name}` over what is typed, and over a `}` already standing after the caret.
pub fn insert(text: &str, query: &Query, caret: usize, name: &str) -> Command {
    let end = if text[caret..].starts_with('}') {
        caret + 1
    } else {
        caret
    };
    let written = format!("{{{name}}}");
    Command {
        selection: Selection::caret(query.start + written.len()),
        edits: vec![Edit::new(query.start..end, written)],
    }
}

/// The name of a block's style, as the left margin shows it while the caret is in the block: its custom style, else a
/// heading's level, as the Style tab names it. A block in its kind's own style, other than a heading, has none.
pub fn style_name(text: &str, blocks: &Blocks, index: usize) -> Option<String> {
    if let Some(custom) = crate::editor::rich::styles::style_of(text, blocks, index) {
        return Some(sentence_case(&custom));
    }
    let kind = blocks.as_slice()[index].kind;
    matches!(kind, BlockKind::Heading { .. }).then(|| {
        let key = crate::editor::rich::styles::base_style(kind);
        crate::inspector::style_property::style_label(key).to_owned()
    })
}

/// `wide-heading` as "Wide heading".
pub fn sentence_case(name: &str) -> String {
    let words = name.replace(['-', '_'], " ");
    let mut characters = words.chars();
    characters.next().map_or_else(String::new, |first| {
        first
            .to_uppercase()
            .chain(characters.flat_map(char::to_lowercase))
            .collect()
    })
}
