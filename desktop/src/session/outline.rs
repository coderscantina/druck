//! What a chapter's text says about itself: where a single file splits into pages, and the title and kind a sidebar row
//! shows. All of it is derived from the text, and nothing is stored.

use std::ops::Range;

use pulldown_cmark::{Event, Parser};

use crate::source::blocks::Block;
use crate::source::{BlockKind, Blocks};

/// What a page is, as its row shows it. No section type exists: this is read from the text on each change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Only the settings, or the title page template's placeholders.
    TitlePage,
    /// A `::: bottom` group and nothing else of substance.
    Copyright,
    /// Paragraphs that carry `{.dedication}`.
    Dedication,
    /// A `::: toc` and nothing else of substance.
    Contents,
    /// A `::: index` and nothing else of substance.
    Index,
    /// A `::: bibliography` and nothing else of substance.
    Bibliography,
    /// Its first heading says `{-}` or `{.unnumbered}`.
    Unnumbered,
    /// A level 1 heading that takes a number.
    Numbered,
    /// No heading, and none of the roles above.
    Plain,
}

impl Kind {
    /// Whether Druck writes the page: contents, index, or bibliography.
    pub fn is_made(self) -> bool {
        matches!(self, Self::Contents | Self::Index | Self::Bibliography)
    }

    /// Whether the page has a heading.
    pub fn has_heading(self) -> bool {
        matches!(self, Self::Unnumbered | Self::Numbered)
    }
}

/// A chapter's first heading, if any, and what its kind is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pub title: Option<String>,
    pub kind: Kind,
    /// Where the heading's words are in the text, for renaming.
    pub heading: Option<Range<usize>>,
    /// The first words of a plain page, cut at a word, to name it by.
    pub words: Option<String>,
}

/// The directives that start a part of the book.
pub const PARTS: [&str; 3] = ["front-matter", "main-matter", "back-matter"];
/// The directives whose page Druck writes, each standing in a section of its own.
const MADE: [&str; 3] = ["toc", "index", "bibliography"];
/// How long a plain page's name gets before it is cut at a word.
const WORDS_LENGTH: usize = 24;

/// Reads a chapter's outline.
pub fn outline(text: &str) -> Outline {
    let blocks = Blocks::parse(text);
    let heading = blocks.as_slice().iter().find_map(|block| match block.kind {
        BlockKind::Heading { level } => Some((level, block.range.clone())),
        _ => None,
    });
    let Some((level, range)) = heading else {
        let kind = role(text, &blocks);
        return Outline {
            title: None,
            kind,
            heading: None,
            words: (kind == Kind::Plain).then(|| first_words(text, &blocks)).flatten(),
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
        words: None,
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

/// A heading line's words without their Markdown, and whether it says `{-}` or `{.unnumbered}`.
pub fn heading_text(line: &str) -> (String, bool) {
    let words = heading_words(line);
    let (bare, attributes) = split_attributes(&line[words]);
    let unnumbered = attributes
        .split_whitespace()
        .any(|attribute| attribute == "-" || attribute == ".unnumbered");
    (plain_text(bare), unnumbered)
}

/// The name a `:::` line gives, or `""` for a closing line.
pub fn directive_name(line: &str) -> &str {
    line.trim_start_matches(':')
        .split_whitespace()
        .next()
        .unwrap_or_default()
}

/// Whether a directive holds text up to a closing line, rather than standing alone.
fn has_body(name: &str) -> bool {
    matches!(name, "columns" | "full-width" | "keep" | "bottom" | "table")
}

/// How many directive regions hold each block. A region's own opening line is outside it and its closing line inside.
pub fn depths(text: &str, blocks: &[Block]) -> Vec<usize> {
    let mut depth = 0usize;
    blocks
        .iter()
        .map(|block| {
            let here = depth;
            if block.kind == BlockKind::Directive {
                match directive_name(&text[block.range.clone()]) {
                    "" => depth = depth.saturating_sub(1),
                    name if has_body(name) => depth += 1,
                    _ => {}
                }
            }
            here
        })
        .collect()
}

/// What a block at the top level is, for reading a page's role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item<'a> {
    Settings,
    /// A directive that stands alone, or a region with all it holds.
    Directive(&'a str),
    Paragraph(&'a str),
    Other,
}

/// Whether a block says nothing a page shows by itself: blank lines, a comment, a part, or a page break.
fn is_bare(text: &str, block: &Block) -> bool {
    match block.kind {
        BlockKind::Blank | BlockKind::Comment => true,
        BlockKind::Directive => {
            let name = directive_name(&text[block.range.clone()]);
            name == "page-break" || PARTS.contains(&name)
        }
        _ => false,
    }
}

/// The top-level blocks of a text that say something, with a region counted once.
fn items<'a>(text: &'a str, blocks: &Blocks) -> Vec<Item<'a>> {
    let list = blocks.as_slice();
    let depth = depths(text, list);
    (list.iter().zip(depth))
        .filter(|(block, depth)| *depth == 0 && !is_bare(text, block))
        .map(|(block, _)| {
            let source = &text[block.range.clone()];
            match block.kind {
                BlockKind::FrontMatter => Item::Settings,
                BlockKind::Directive => Item::Directive(directive_name(source)),
                BlockKind::Paragraph => Item::Paragraph(source.trim()),
                _ => Item::Other,
            }
        })
        .collect()
}

/// The role of a page without a heading.
fn role(text: &str, blocks: &Blocks) -> Kind {
    let items = items(text, blocks);
    let substance: Vec<Item> = (items.iter().copied()).filter(|item| *item != Item::Settings).collect();
    let paragraphs: Option<Vec<&str>> = (substance.iter())
        .map(|item| match item {
            Item::Paragraph(words) => Some(*words),
            _ => None,
        })
        .collect();
    match substance.as_slice() {
        [] if items.contains(&Item::Settings) => Kind::TitlePage,
        [Item::Directive("toc")] => Kind::Contents,
        [Item::Directive("index")] => Kind::Index,
        [Item::Directive("bibliography")] => Kind::Bibliography,
        [Item::Directive("bottom")] => Kind::Copyright,
        [_, ..] => match paragraphs {
            Some(words) if words.iter().all(|words| has_class(words, "dedication")) => Kind::Dedication,
            Some(words)
                if words.iter().all(|words| only_placeholders(words))
                    && words.iter().any(|words| words.contains("{title}")) =>
            {
                Kind::TitlePage
            }
            _ => Kind::Plain,
        },
        [] => Kind::Plain,
    }
}

/// Whether a paragraph ends with attributes that name the class.
fn has_class(paragraph: &str, class: &str) -> bool {
    let (_, attributes) = split_attributes(paragraph);
    attributes
        .split_whitespace()
        .any(|attribute| attribute.strip_prefix('.') == Some(class))
}

/// Whether a paragraph is nothing but placeholders, perhaps emphasised, as the title page template writes them.
fn only_placeholders(paragraph: &str) -> bool {
    let mut rest = paragraph.trim_matches(|c: char| c == '*' || c == '_' || c.is_whitespace());
    let mut found = false;
    while let Some(after) = rest.strip_prefix('{') {
        let Some(end) = after.find('}') else {
            return false;
        };
        found = true;
        rest = after[end + 1..].trim_start_matches(|c: char| c == '*' || c == '_' || c.is_whitespace());
    }
    found && rest.is_empty()
}

/// The first words of a text's first paragraph, cut at a word with an ellipsis when long.
fn first_words(text: &str, blocks: &Blocks) -> Option<String> {
    let paragraph = (blocks.as_slice().iter()).find(|block| block.kind == BlockKind::Paragraph)?;
    let (bare, _) = split_attributes(text[paragraph.range.clone()].trim());
    let words = plain_text(&bare.split_whitespace().collect::<Vec<_>>().join(" "));
    if words.chars().count() <= WORDS_LENGTH {
        return (!words.is_empty()).then_some(words);
    }
    let cut: String = words.chars().take(WORDS_LENGTH).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if space > 0 => &cut[..space],
        _ => &cut,
    };
    Some(format!("{}\u{2026}", cut.trim_end_matches([',', ';', ':', '.'])))
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

/// Where a single file splits into sections, one per page or chapter. The ranges cover the text exactly, and no
/// range splits a region or a code block.
///
/// - Each level 1 heading starts a section, moved up over the comments directly above it, since those are its notes.
/// - Before the first of them, a page ends after the closing line of a `::: bottom` group and before a `::: page-break`,
///   which goes with the page after it. The settings are a section of their own when they give a title.
/// - A `::: toc`, `::: index`, or `::: bibliography` stands in a section of its own, with the page break and part
///   lines directly before it and the part lines after it.
///
/// A piece with nothing a page shows, such as blank lines or a lone part line, joins the section after it, or the one
/// before it at the end.
pub fn sections(text: &str) -> Vec<Range<usize>> {
    let blocks = Blocks::parse(text);
    let list = blocks.as_slice();
    let depth = depths(text, list);
    let top = |index: usize| depth[index] == 0;
    let name = |index: usize| match list[index].kind {
        BlockKind::Directive => directive_name(&text[list[index].range.clone()]),
        _ => "-",
    };
    let is_part = |index: usize| top(index) && PARTS.contains(&name(index));
    // The first block from `index` on that is not blank, where a page after a split starts.
    let filled_from = |index: usize| {
        (index..list.len())
            .find(|at| list[*at].kind != BlockKind::Blank)
            .unwrap_or(list.len())
    };
    let mut starts: Vec<usize> = vec![0];
    let mut chapters = false;
    for (index, block) in list.iter().enumerate() {
        if !top(index) && !(block.kind == BlockKind::Directive && depth[index] == 1 && name(index).is_empty()) {
            continue;
        }
        match block.kind {
            BlockKind::Heading { level: 1 } => {
                chapters = true;
                let mut start = index;
                for earlier in (0..index).rev() {
                    match list[earlier].kind {
                        BlockKind::Blank => {}
                        BlockKind::Comment => start = earlier,
                        _ => break,
                    }
                }
                starts.push(start);
            }
            BlockKind::FrontMatter if !chapters && gives_title(&text[block.range.clone()]) => {
                starts.push(filled_from(index + 1));
            }
            BlockKind::Directive if !chapters && name(index).is_empty() && closes_bottom(text, list, index) => {
                starts.push(filled_from(index + 1));
            }
            BlockKind::Directive if !chapters && top(index) && name(index) == "page-break" => starts.push(index),
            BlockKind::Directive if top(index) && MADE.contains(&name(index)) => {
                let mut start = index;
                for earlier in (0..index).rev() {
                    match list[earlier].kind {
                        BlockKind::Blank => {}
                        BlockKind::Directive if is_part(earlier) || (top(earlier) && name(earlier) == "page-break") => {
                            start = earlier;
                        }
                        _ => break,
                    }
                }
                let mut end = index + 1;
                while end < list.len() && (list[end].kind == BlockKind::Blank || is_part(end)) {
                    end += 1;
                }
                starts.extend([start, end]);
            }
            _ => {}
        }
    }
    starts.retain(|start| *start < list.len());
    starts.sort_unstable();
    starts.dedup();
    let pieces: Vec<Range<usize>> = (starts.iter().enumerate())
        .map(|(at, start)| *start..starts.get(at + 1).copied().unwrap_or(list.len()))
        .collect();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut carried: Option<usize> = None;
    for (at, piece) in pieces.iter().enumerate() {
        let start = list[piece.start].range.start;
        let end = list[piece.end - 1].range.end;
        let bare = list[piece.clone()].iter().all(|block| is_bare(text, block));
        if bare && at + 1 < pieces.len() {
            carried = carried.or(Some(start));
        } else if bare && let Some(last) = ranges.last_mut() {
            last.end = end;
        } else {
            ranges.push(carried.take().unwrap_or(start)..end);
        }
    }
    if ranges.is_empty() {
        ranges.push(0..text.len());
    }
    ranges
}

/// Whether the block at `index` is a closing line that closes a `::: bottom` group that stands at the top level.
pub(super) fn closes_bottom(text: &str, list: &[Block], index: usize) -> bool {
    if list[index].kind != BlockKind::Directive || !directive_name(&text[list[index].range.clone()]).is_empty() {
        return false;
    }
    let mut depth = 0usize;
    for block in list[..index].iter().rev() {
        if block.kind != BlockKind::Directive {
            continue;
        }
        match directive_name(&text[block.range.clone()]) {
            "" => depth += 1,
            name if has_body(name) && depth == 0 => return name == "bottom",
            name if has_body(name) => depth -= 1,
            _ => {}
        }
    }
    false
}

/// Whether settings name a title.
fn gives_title(settings: &str) -> bool {
    settings.lines().any(|line| {
        line.strip_prefix("title:")
            .is_some_and(|value| !value.trim().trim_matches(['"', '\'']).is_empty())
    })
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
    fn a_page_without_a_heading_is_read_by_its_role() {
        assert_eq!(outline("---\ntitle: A\n---\n\n").kind, Kind::TitlePage);
        assert_eq!(
            outline("**{title}**\n\n{subtitle}\n\n{author}\n\n::: page-break\n").kind,
            Kind::TitlePage
        );
        assert_eq!(outline("::: bottom\nImprint\n\nMore\n:::\n\n").kind, Kind::Copyright);
        assert_eq!(
            outline("For you. {.dedication}\n\n::: front-matter\n").kind,
            Kind::Dedication
        );
        assert_eq!(
            outline("::: page-break\n\n::: toc\n\n::: main-matter\n").kind,
            Kind::Contents
        );
        assert_eq!(outline("::: index\n").kind, Kind::Index);
        assert_eq!(outline("::: bibliography\n").kind, Kind::Bibliography);
        assert_eq!(outline("Imprint text.\n\n::: toc\n").kind, Kind::Plain);
        assert_eq!(
            outline("::: bottom\nImprint\n:::\n\nFor you. {.dedication}\n").kind,
            Kind::Plain
        );
    }

    #[test]
    fn a_plain_page_is_named_by_its_first_words_cut_at_a_word() {
        assert_eq!(outline("A short *line*.\n").words.as_deref(), Some("A short line."));
        let long = outline("<!-- c -->\n\nThe keeper left the island before the boat came.\n");
        assert_eq!(long.words.as_deref(), Some("The keeper left the\u{2026}"));
        assert_eq!(outline("::: toc\n").words, None);
    }

    #[test]
    fn a_hash_in_code_is_no_heading() {
        let text = "```sh\n# not a heading\n```\n\nText\n";
        assert_eq!(outline(text).title, None);
        assert_eq!(sections(text).len(), 1);
    }

    /// The sections' texts, which must cover the text exactly.
    fn pieces(text: &str) -> Vec<&str> {
        let found = sections(text);
        assert_eq!(found.first().map(|range| range.start), Some(0));
        assert_eq!(found.last().map(|range| range.end), Some(text.len()));
        assert!(found.windows(2).all(|pair| pair[0].end == pair[1].start));
        found.into_iter().map(|range| &text[range]).collect()
    }

    #[test]
    fn sections_cover_the_text_exactly() {
        let text = "---\ntitle: A\n---\n\nIntro\n\n# One\n\nText\n\n## Sub\n\n# Two\n";
        let found = pieces(text);
        assert_eq!(found.len(), 4);
        assert!(found[1].starts_with("Intro"));
        assert!(found[2].starts_with("# One"));
        assert!(found[3].starts_with("# Two"));
    }

    #[test]
    fn the_sample_book_splits_into_its_pages() {
        let text = include_str!("../../../samples/book.md");
        let kinds: Vec<Kind> = pieces(text).iter().map(|piece| outline(piece).kind).collect();
        use Kind::*;
        assert_eq!(
            kinds,
            [
                TitlePage, Copyright, Dedication, Unnumbered, Contents, Numbered, Numbered, Numbered, Unnumbered, Index
            ]
        );
        let found = pieces(text);
        assert!(found[2].ends_with("::: front-matter\n\n"));
        assert!(found[4].starts_with("::: page-break") && found[4].ends_with("::: main-matter\n\n"));
    }

    #[test]
    fn opening_pages_split_after_a_bottom_group_and_before_a_page_break() {
        let text =
            "Title\n\n::: page-break\n\n::: bottom\nImprint\n:::\n\nFor you.\n\n# One\n\n::: page-break\n\nMore\n";
        assert_eq!(
            pieces(text),
            [
                "Title\n\n",
                "::: page-break\n\n::: bottom\nImprint\n:::\n\n",
                "For you.\n\n",
                "# One\n\n::: page-break\n\nMore\n"
            ]
        );
    }

    #[test]
    fn settings_without_a_title_stay_with_the_page_after_them() {
        assert_eq!(pieces("---\nlang: de\n---\n\nText\n").len(), 1);
    }

    #[test]
    fn nothing_splits_inside_a_region_or_a_code_block() {
        let text = "::: keep\n::: bottom\nA\n:::\n\nB\n\n# Inside\n:::\n\n```\n::: toc\n```\n";
        assert_eq!(pieces(text).len(), 1);
    }

    #[test]
    fn a_made_page_takes_its_page_break_and_parts_and_lines_alone_join_a_neighbour() {
        let text = "# One\n\nText\n\n::: back-matter\n\n::: page-break\n\n::: index\n\n::: front-matter\n";
        assert_eq!(
            pieces(text),
            [
                "# One\n\nText\n\n",
                "::: back-matter\n\n::: page-break\n\n::: index\n\n::: front-matter\n"
            ]
        );
        assert_eq!(pieces("::: front-matter\n\n# One\n"), ["::: front-matter\n\n# One\n"]);
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
