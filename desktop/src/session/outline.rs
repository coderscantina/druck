//! What a chapter's text says about itself: where level 1 headings split a single file, and the title and kind a
//! sidebar row shows. All of it is derived from the text, and nothing is stored.

use std::ops::Range;

use pulldown_cmark::{Event, Parser};

use crate::source::{BlockKind, Blocks};

/// How a chapter's row looks. No section type exists: this is read from the text on each change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// No heading: a title page or an imprint.
    Plain,
    /// No heading, with a `::: toc`.
    Contents,
    /// Its first heading says `{-}` or `{.unnumbered}`.
    Unnumbered,
    /// A level 1 heading that takes a number.
    Numbered,
}

/// A chapter's first heading, if any, and what its kind is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pub title: Option<String>,
    pub kind: Kind,
    /// Where the heading's words are in the text, for renaming.
    pub heading: Option<Range<usize>>,
}

/// Reads a chapter's outline.
pub fn outline(text: &str) -> Outline {
    let blocks = Blocks::parse(text);
    let heading = blocks.as_slice().iter().find_map(|block| match block.kind {
        BlockKind::Heading { level } => Some((level, block.range.clone())),
        _ => None,
    });
    let Some((level, range)) = heading else {
        return Outline {
            title: None,
            kind: if is_only_contents(text) {
                Kind::Contents
            } else {
                Kind::Plain
            },
            heading: None,
        };
    };
    let line = text[range.clone()].lines().next().unwrap_or_default();
    let words = heading_words(line);
    let (bare, attributes) = split_attributes(&line[words.clone()]);
    let unnumbered = attributes
        .split_whitespace()
        .any(|attribute| attribute == "-" || attribute == ".unnumbered");
    let kind = if unnumbered || level != 1 {
        Kind::Unnumbered
    } else {
        Kind::Numbered
    };
    Outline {
        title: Some(plain_text(bare)),
        kind,
        heading: Some(range.start + words.start..range.start + words.start + bare.len()),
    }
}

/// A heading of a chapter's text, for the ticks at the edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    /// Where the heading's block starts in the text.
    pub start: usize,
}

/// The headings of levels 1 to 3, in reading order.
pub fn headings(text: &str) -> Vec<Heading> {
    let blocks = Blocks::parse(text);
    (blocks.as_slice().iter())
        .filter_map(|block| match block.kind {
            BlockKind::Heading { level } if level <= 3 => {
                let line = text[block.range.clone()].lines().next().unwrap_or_default();
                let words = heading_words(line);
                let (bare, _) = split_attributes(&line[words]);
                Some(Heading {
                    level,
                    title: plain_text(bare),
                    start: block.range.start,
                })
            }
            _ => None,
        })
        .collect()
}

/// The title of a part page: one level 1 heading and nothing else but directives and comments.
pub fn part_title(text: &str) -> Option<String> {
    let blocks = Blocks::parse(text);
    let mut heading = None;
    for block in blocks.as_slice() {
        match block.kind {
            BlockKind::Heading { level: 1 } if heading.is_none() => heading = Some(block.range.start),
            BlockKind::Blank | BlockKind::Comment | BlockKind::Directive | BlockKind::FrontMatter => {}
            _ => return None,
        }
    }
    headings(text)
        .into_iter()
        .find(|h| Some(h.start) == heading)
        .map(|h| h.title)
}

/// A heading line's words without their Markdown, and whether it says `{-}` or `{.unnumbered}`.
pub fn heading_text(line: &str) -> (String, bool) {
    let words = heading_words(line);
    let (bare, attributes) = split_attributes(&line[words]);
    let unnumbered = attributes
        .split_whitespace()
        .any(|attribute| attribute == "-" || attribute == ".unnumbered");
    (plain_text(bare), unnumbered)
}

/// Whether the text holds a `::: toc` and otherwise only directive lines and comments.
fn is_only_contents(text: &str) -> bool {
    let mut has_toc = false;
    for block in Blocks::parse(text).as_slice() {
        match block.kind {
            BlockKind::Directive => has_toc |= text[block.range.clone()].trim_end() == "::: toc",
            BlockKind::Blank | BlockKind::Comment | BlockKind::FrontMatter => {}
            _ => return false,
        }
    }
    has_toc
}

/// The part of a heading line that is its words and attributes: no `#`s, no closing `#`s.
fn heading_words(line: &str) -> Range<usize> {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if hashes == 0 {
        return 0..line.trim_end().len();
    }
    let body = &line[hashes..];
    let start = hashes + (body.len() - body.trim_start().len());
    let end = line.trim_end().trim_end_matches('#').trim_end().len();
    start..end.max(start)
}

/// Splits a trailing `{...}` off a heading's words.
fn split_attributes(words: &str) -> (&str, &str) {
    let trimmed = words.trim_end();
    if trimmed.ends_with('}')
        && let Some(open) = trimmed.rfind('{')
    {
        return (trimmed[..open].trim_end(), &trimmed[open + 1..trimmed.len() - 1]);
    }
    (trimmed, "")
}

/// Heading words without their inline Markdown.
fn plain_text(words: &str) -> String {
    let mut plain = String::new();
    for event in Parser::new(words) {
        if let Event::Text(text) | Event::Code(text) = event {
            plain.push_str(&text);
        }
    }
    if plain.is_empty() { words.to_owned() } else { plain }
}

/// Where a single file splits into sections: at each level 1 heading, moved up over comments directly above it, since
/// those are its notes. Text before the first heading is a section of its own unless it is blank. The ranges cover the
/// text exactly.
pub fn sections(text: &str) -> Vec<Range<usize>> {
    let blocks = Blocks::parse(text);
    let list = blocks.as_slice();
    let mut starts: Vec<usize> = Vec::new();
    for (index, block) in list.iter().enumerate() {
        if !matches!(block.kind, BlockKind::Heading { level: 1 }) {
            continue;
        }
        let mut start = block.range.start;
        for earlier in list[..index].iter().rev() {
            match earlier.kind {
                BlockKind::Blank => {}
                BlockKind::Comment => start = earlier.range.start,
                _ => break,
            }
        }
        starts.push(start);
    }
    let Some(&first) = starts.first() else {
        return std::iter::once(0..text.len()).collect();
    };
    if first != 0 && text[..first].trim().is_empty() {
        starts[0] = 0;
    } else if first != 0 {
        starts.insert(0, 0);
    }
    let ends = starts.iter().skip(1).copied().chain([text.len()]);
    starts
        .iter()
        .copied()
        .zip(ends)
        .map(|(start, end)| start..end)
        .collect()
}

/// A title for a file with no heading: its name without an ordering prefix, with spaces and a capital.
pub fn title_from_file(stem: &str) -> String {
    let bare = stem.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '_' || c == ' ');
    let bare = if bare.is_empty() { stem } else { bare };
    let words = bare.replace(['-', '_'], " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A file name for a title: lower case words joined by dashes.
pub fn slug(title: &str) -> String {
    let mut slug = String::new();
    for character in title.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "section".to_owned()
    } else {
        slug.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_stop_at_level_three_and_read_their_words() {
        let text = "# One {-}\n\n## Two *b*\n\n#### Four\n\n### Three\n";
        let found: Vec<_> = headings(text)
            .into_iter()
            .map(|h| (h.level, h.title, h.start))
            .collect();
        assert_eq!(
            found,
            [(1, "One".into(), 0), (2, "Two b".into(), 11), (3, "Three".into(), 34)]
        );
    }

    #[test]
    fn a_part_page_is_a_lone_heading_with_directives() {
        assert_eq!(
            part_title("# Part II {-}\n\n::: page-break\n").as_deref(),
            Some("Part II")
        );
        assert_eq!(part_title("# Chapter\n\nText.\n"), None);
    }

    #[test]
    fn a_title_comes_from_the_first_heading_without_its_markup() {
        let found = outline("<!-- note -->\n\n# The *first* chapter {#sec:first}\n\nText.\n\n# Another\n");
        assert_eq!(found.title.as_deref(), Some("The first chapter"));
        assert_eq!(found.kind, Kind::Numbered);
    }

    #[test]
    fn the_heading_range_covers_only_its_words() {
        let text = "# The chapter {#one} #\n";
        let found = outline(text);
        assert_eq!(&text[found.heading.expect("has a heading")], "The chapter");
        let text = "The chapter\n===\n\nText\n";
        let found = outline(text);
        assert_eq!(&text[found.heading.expect("has a heading")], "The chapter");
    }

    #[test]
    fn marked_headings_are_unnumbered_and_lower_levels_too() {
        assert_eq!(outline("# Preface {-}\n").kind, Kind::Unnumbered);
        assert_eq!(outline("# Preface {.unnumbered}\n").kind, Kind::Unnumbered);
        assert_eq!(outline("## Part of something\n").kind, Kind::Unnumbered);
    }

    #[test]
    fn text_without_a_heading_is_plain_or_contents() {
        assert_eq!(outline("::: bottom\nImprint\n:::\n").kind, Kind::Plain);
        assert_eq!(outline("::: toc\n").kind, Kind::Contents);
        assert_eq!(outline("::: front-matter\n\n::: toc\n").kind, Kind::Contents);
        assert_eq!(outline("Imprint text.\n\n::: toc\n").kind, Kind::Plain);
    }

    #[test]
    fn a_hash_in_code_is_no_heading() {
        let text = "```sh\n# not a heading\n```\n\nText\n";
        assert_eq!(outline(text).title, None);
        assert_eq!(sections(text).len(), 1);
    }

    #[test]
    fn sections_cover_the_text_exactly() {
        let text = "---\ntitle: A\n---\n\nIntro\n\n# One\n\nText\n\n## Sub\n\n# Two\n";
        let found = sections(text);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].start, 0);
        assert_eq!(found[2].end, text.len());
        assert!(found.windows(2).all(|pair| pair[0].end == pair[1].start));
        assert!(text[found[1].clone()].starts_with("# One"));
        assert!(text[found[2].clone()].starts_with("# Two"));
    }

    #[test]
    fn blank_text_before_the_first_heading_stays_with_it() {
        let text = "\n\n# One\n";
        assert_eq!(sections(text).len(), 1);
    }

    #[test]
    fn a_comment_above_a_heading_belongs_to_it() {
        let text = "# One\n\nText\n\n<!-- notes on two -->\n\n# Two\n";
        let found = sections(text);
        assert!(text[found[1].clone()].starts_with("<!-- notes"));
    }

    #[test]
    fn file_names_become_titles_and_titles_file_names() {
        assert_eq!(title_from_file("00-titelei"), "Titelei");
        assert_eq!(title_from_file("my_notes-2"), "My notes 2");
        assert_eq!(title_from_file("007"), "007");
        assert_eq!(slug("Über den Autor!"), "über-den-autor");
        assert_eq!(slug("***"), "section");
    }
}
