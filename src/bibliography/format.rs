//! Bibliography entries as inline content.
//!
//! Both styles use the same layout, so that a reference reads the same in either; the numeric style only adds a label
//! and orders by first citation. A reference is a list of sentences:
//!
//! 1. The lead and the year: `Smith, Ada and Bob Jones (2024).` The lead is the authors, else the editors with
//!    "(Ed.)", else a report's institution, else the title itself. The author-date style adds its a, b suffix to
//!    the year. A missing year of a web resource reads "n.d." or "o. J.".
//! 2. The title, unless it already leads. Books, theses, and reports set it in italics.
//! 3. The details, by type, as listed on [`details`].
//! 4. The DOI or URL as a link, DOI first. For web resources the URL.
//!
//! A sentence ends with a period unless it already ends with `.`, `?`, `!`, or a link.

use crate::config::theme::Lang;
use crate::document::{Inline, InlineStyle, Link};

use super::entry::{Entry, Kind, Lead, ThesisKind};
use super::words::{Words, words};

/// A sentence under construction. Adjacent pieces with the same style merge.
#[derive(Default)]
struct Sentence(Vec<Inline>);

impl Sentence {
    fn push(&mut self, text: &str, style: InlineStyle) {
        if text.is_empty() {
            return;
        }
        if let Some(Inline::Text {
            text: last,
            style: last_style,
        }) = self.0.last_mut()
            && *last_style == style
        {
            last.push_str(text);
        } else {
            self.0.push(Inline::Text {
                text: text.to_owned(),
                style,
            });
        }
    }

    fn plain(&mut self, text: &str) -> &mut Self {
        self.push(text, InlineStyle::default());
        self
    }

    fn italic(&mut self, text: &str) -> &mut Self {
        self.push(
            text,
            InlineStyle {
                emphasis: true,
                ..InlineStyle::default()
            },
        );
        self
    }

    fn link(&mut self, url: &str) -> &mut Self {
        self.push(
            url,
            InlineStyle {
                link: Some(Link::Url(url.to_owned())),
                ..InlineStyle::default()
            },
        );
        self
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds the closing period if the sentence needs one.
    fn finish(mut self) -> Vec<Inline> {
        let needs_period = match self.0.last() {
            Some(Inline::Text { text, style }) => style.link.is_none() && !text.ends_with(['.', '?', '!']),
            _ => false,
        };
        if needs_period {
            self.plain(".");
        }
        self.0
    }
}

/// Formats one reference. `suffix` is the disambiguation letter after the year, empty if none.
pub fn reference(entry: &Entry, lang: Lang, suffix: &str) -> Vec<Inline> {
    let words = words(lang);
    let title_in_italics = matches!(entry.kind, Kind::Book | Kind::Thesis(_) | Kind::Report);
    let mut title = Sentence::default();
    if title_in_italics {
        title.italic(&entry.title);
    } else {
        title.plain(&entry.title);
    }
    let mut lead = Sentence::default();
    let lead_has_title = match entry.lead() {
        Lead::Authors(names) => {
            lead.plain(&names.full(lang));
            false
        }
        Lead::Editors(names) => {
            let label = if names.count() > 1 || names.et_al {
                words.editors
            } else {
                words.editor
            };
            lead.plain(&format!("{} ({label})", names.full(lang)));
            false
        }
        Lead::Institution(institution) => {
            lead.plain(institution);
            false
        }
        Lead::Title => {
            lead.0.extend(title.0.iter().cloned());
            true
        }
    };
    let year = entry.year.as_deref().unwrap_or(words.no_date);
    lead.plain(&format!(" ({year}{suffix})"));

    let mut sentences = vec![lead];
    if !lead_has_title {
        sentences.push(title);
    }
    sentences.extend(details(entry, words));
    sentences.extend(link(entry, words));
    let mut inlines = Vec::new();
    for sentence in sentences.into_iter().filter(|sentence| !sentence.is_empty()) {
        if !inlines.is_empty() {
            inlines.push(Inline::Text {
                text: " ".to_owned(),
                style: InlineStyle::default(),
            });
        }
        inlines.extend(sentence.finish());
    }
    merge_adjacent(inlines)
}

/// The type-specific sentences:
///
/// - article: `*Journal* 12(3), 45–67.`
/// - book: `2nd ed.` (German `2. Aufl.`), then `Address: Publisher.`
/// - conference paper: `In: *Proceedings*, 45–67.`, then `Address: Publisher.`
/// - thesis: `PhD thesis, School.` The `type` field replaces the wording.
/// - report: `Technical report 42, Institution, Address.` The institution is left out when it leads.
/// - web resource: `Publisher.` and `Accessed 5 May 2024.` (German `Abgerufen am`), around the URL.
fn details(entry: &Entry, words: &Words) -> Vec<Sentence> {
    let mut sentences = Vec::new();
    let mut add = |build: &dyn Fn(&mut Sentence)| {
        let mut sentence = Sentence::default();
        build(&mut sentence);
        sentences.push(sentence);
    };
    let place_and_publisher = |sentence: &mut Sentence| {
        let parts: Vec<&str> = [entry.address.as_deref(), entry.publisher.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        sentence.plain(&parts.join(": "));
    };
    match entry.kind {
        Kind::Article => add(&|s| {
            s.italic(entry.container.as_deref().unwrap_or_default());
            match (&entry.volume, &entry.number) {
                (Some(volume), Some(number)) => s.plain(&format!(" {volume}({number})")),
                (Some(volume), None) => s.plain(&format!(" {volume}")),
                (None, Some(number)) => s.plain(&format!(" ({number})")),
                (None, None) => s,
            };
            if let Some(pages) = &entry.pages {
                s.plain(&format!(", {pages}"));
            }
        }),
        Kind::Book => {
            add(&|s| {
                if let Some(edition) = &entry.edition {
                    // German editions written as digits get the ordinal period: "2. Aufl."
                    let ordinal = if words.edition == "Aufl." && edition.chars().all(|c| c.is_ascii_digit()) {
                        "."
                    } else {
                        ""
                    };
                    s.plain(&format!("{edition}{ordinal} {}", words.edition));
                }
            });
            add(&place_and_publisher);
        }
        Kind::InProceedings => {
            add(&|s| {
                s.plain(&format!("{}: ", words.in_));
                s.italic(entry.container.as_deref().unwrap_or_default());
                if let Some(pages) = &entry.pages {
                    s.plain(&format!(", {pages}"));
                }
            });
            add(&place_and_publisher);
        }
        Kind::Thesis(kind) => add(&|s| {
            let default = match kind {
                ThesisKind::Phd => words.phd_thesis,
                ThesisKind::Masters => words.masters_thesis,
                ThesisKind::Other => words.thesis,
            };
            let type_ = entry.type_.as_deref().unwrap_or(default);
            let parts: Vec<&str> = [Some(type_), entry.institution.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            s.plain(&parts.join(", "));
        }),
        Kind::Report => add(&|s| {
            let type_ = entry.type_.as_deref().unwrap_or(words.report);
            let kind = match &entry.number {
                Some(number) => format!("{type_} {number}"),
                None => type_.to_owned(),
            };
            let institution = entry
                .institution
                .as_deref()
                .filter(|_| !matches!(entry.lead(), Lead::Institution(_)));
            let parts: Vec<&str> = [Some(kind.as_str()), institution, entry.address.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            s.plain(&parts.join(", "));
        }),
        Kind::Online => add(&|s| {
            s.plain(entry.publisher.as_deref().unwrap_or_default());
        }),
    }
    sentences
}

/// The link sentences: DOI or URL, and for web resources the access date after the URL.
fn link(entry: &Entry, words: &Words) -> Vec<Sentence> {
    let doi = entry.doi.as_deref().map(|doi| {
        if doi.starts_with("http") {
            doi.to_owned()
        } else {
            format!("https://doi.org/{doi}")
        }
    });
    let url = if entry.kind == Kind::Online {
        entry.url.clone().or(doi)
    } else {
        doi.or_else(|| entry.url.clone())
    };
    let mut link = Sentence::default();
    if let Some(url) = url {
        link.link(&url);
    }
    let mut accessed = Sentence::default();
    if let (Kind::Online, Some(date)) = (entry.kind, &entry.urldate) {
        accessed.plain(&format!("{} {date}", words.accessed));
    }
    vec![link, accessed]
}

fn merge_adjacent(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut merged = Sentence::default();
    for inline in inlines {
        match inline {
            Inline::Text { text, style } => merged.push(&text, style),
            other => merged.0.push(other),
        }
    }
    merged.0
}
