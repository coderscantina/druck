//! The little Markdown a chat reply needs: paragraphs, headings, lists, quotes, code, and bold, italic, code, and link
//! spans. Each block is plain text with ranges to style, which gpui draws as one run of text.

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Strong,
    Emphasis,
    Code,
    Link,
}

/// Text with styled ranges. A range is in bytes of `text`, and ranges may nest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<(Range<usize>, Style)>,
}

/// The styles that apply to one stretch of text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub link: bool,
}

impl Inline {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The text cut into runs that do not overlap, each with every style over it, in order and without gaps.
    pub fn runs(&self) -> Vec<(Range<usize>, Flags)> {
        let mut edges = vec![0, self.text.len()];
        edges.extend(self.spans.iter().flat_map(|(range, _)| [range.start, range.end]));
        edges.sort_unstable();
        edges.dedup();
        edges
            .windows(2)
            .map(|pair| {
                let run = pair[0]..pair[1];
                let mut flags = Flags::default();
                for (range, style) in &self.spans {
                    if range.start <= run.start && run.end <= range.end {
                        match style {
                            Style::Strong => flags.strong = true,
                            Style::Emphasis => flags.emphasis = true,
                            Style::Code => flags.code = true,
                            Style::Link => flags.link = true,
                        }
                    }
                }
                (run, flags)
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Inline),
    /// Level 1 to 6.
    Heading(u8, Inline),
    Quote(Inline),
    /// One list item, with its nesting depth from 0 and its marker: a bullet or the item's number.
    Item {
        depth: usize,
        marker: String,
        text: Inline,
    },
    Code(String),
    Rule,
}

/// Blocks of `source`. Text that ends in the middle of a construct, as a streamed reply does, still parses.
pub fn parse(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current = Inline::default();
    let mut open: Vec<(usize, Style)> = Vec::new();
    // Per open list: the next number for an ordered one.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut markers: Vec<String> = Vec::new();
    let mut code: Option<String> = None;
    let mut heading: Option<u8> = None;
    let mut in_quote = 0usize;

    let flush = |blocks: &mut Vec<Block>, current: &mut Inline, make: &dyn Fn(Inline) -> Block| {
        if !current.is_empty() {
            blocks.push(make(std::mem::take(current)));
        }
    };

    for event in Parser::new_ext(source, Options::empty()) {
        match event {
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                if let Some(marker) = markers.last_mut().filter(|marker| !marker.is_empty()) {
                    // The first paragraph of an item carries its marker.
                    let (depth, marker) = (lists.len() - 1, std::mem::take(marker));
                    flush(&mut blocks, &mut current, &|text| Block::Item {
                        depth,
                        marker: marker.clone(),
                        text,
                    });
                } else if in_quote > 0 {
                    flush(&mut blocks, &mut current, &Block::Quote);
                } else {
                    flush(&mut blocks, &mut current, &Block::Paragraph);
                }
            }
            Event::Start(Tag::Heading { level, .. }) => heading = Some(level_number(level)),
            Event::End(TagEnd::Heading(_)) => {
                let level = heading.take().unwrap_or(1);
                flush(&mut blocks, &mut current, &|text| Block::Heading(level, text));
            }
            Event::Start(Tag::BlockQuote(_)) => in_quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut blocks, &mut current, &Block::Quote);
                in_quote = in_quote.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                // A tight item's text is still in `current` when a nested list starts.
                if let Some(marker) = markers.last_mut() {
                    let (depth, marker) = (lists.len() - 1, std::mem::take(marker));
                    flush(&mut blocks, &mut current, &|text| Block::Item {
                        depth,
                        marker: marker.clone(),
                        text,
                    });
                }
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let marker = match lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number += 1;
                        marker
                    }
                    _ => "\u{2022}".to_owned(),
                };
                markers.push(marker);
            }
            Event::End(TagEnd::Item) => {
                let marker = markers.pop().unwrap_or_default();
                let depth = lists.len().saturating_sub(1);
                flush(&mut blocks, &mut current, &|text| Block::Item {
                    depth,
                    marker: marker.clone(),
                    text,
                });
            }
            Event::Start(Tag::CodeBlock(_)) => code = Some(String::new()),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(text) = code.take() {
                    blocks.push(Block::Code(text.trim_end_matches('\n').to_owned()));
                }
            }
            Event::Start(Tag::Strong) => open.push((current.text.len(), Style::Strong)),
            Event::Start(Tag::Emphasis) => open.push((current.text.len(), Style::Emphasis)),
            Event::Start(Tag::Link { .. }) => open.push((current.text.len(), Style::Link)),
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link) => {
                if let Some((start, style)) = open.pop() {
                    current.spans.push((start..current.text.len(), style));
                }
            }
            Event::Code(text) => {
                let start = current.text.len();
                current.text.push_str(&text);
                current.spans.push((start..current.text.len(), Style::Code));
            }
            Event::Text(text) => match code.as_mut() {
                Some(code) => code.push_str(&text),
                None => current.text.push_str(&text),
            },
            Event::SoftBreak => current.text.push(' '),
            Event::HardBreak => current.text.push('\n'),
            Event::Rule => blocks.push(Block::Rule),
            _ => {}
        }
    }
    // A reply cut off in a construct leaves text behind.
    flush(&mut blocks, &mut current, &Block::Paragraph);
    blocks
}

fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(block: &Block) -> &str {
        match block {
            Block::Paragraph(inline) | Block::Quote(inline) | Block::Heading(_, inline) => &inline.text,
            Block::Item { text, .. } => &text.text,
            Block::Code(code) => code,
            Block::Rule => "",
        }
    }

    #[test]
    fn spans_mark_bold_italic_and_code_in_one_paragraph() {
        let blocks = parse("A **bold** and *slanted* `word`, then a [link](https://x.y).");
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("a paragraph")
        };
        assert_eq!(inline.text, "A bold and slanted word, then a link.");
        let styled: Vec<_> = inline
            .spans
            .iter()
            .map(|(range, style)| (&inline.text[range.clone()], *style))
            .collect();
        assert_eq!(
            styled,
            [
                ("bold", Style::Strong),
                ("slanted", Style::Emphasis),
                ("word", Style::Code),
                ("link", Style::Link)
            ]
        );
    }

    #[test]
    fn nested_styles_become_runs_that_do_not_overlap() {
        let Block::Paragraph(inline) = &parse("a **b *c* d** e")[0] else {
            panic!("a paragraph")
        };
        let runs: Vec<_> = inline
            .runs()
            .into_iter()
            .map(|(range, flags)| (&inline.text[range], flags.strong, flags.emphasis))
            .collect();
        assert_eq!(
            runs,
            [
                ("a ", false, false),
                ("b ", true, false),
                ("c", true, true),
                (" d", true, false),
                (" e", false, false)
            ]
        );
    }

    #[test]
    fn headings_lists_quotes_and_code_become_blocks() {
        let blocks = parse("# Title\n\n- one\n- two\n  1. nested\n\n> quoted\n\n```rust\nlet x = 1;\n```\n");
        assert!(matches!(&blocks[0], Block::Heading(1, _)));
        let items: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Item { depth, marker, text } => Some((*depth, marker.as_str(), text.text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            items,
            [(0, "\u{2022}", "one"), (0, "\u{2022}", "two"), (1, "1.", "nested")]
        );
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, Block::Quote(_)) && text(b) == "quoted")
        );
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, Block::Code(_)) && text(b) == "let x = 1;")
        );
    }

    #[test]
    fn a_reply_cut_off_mid_construct_still_shows_its_text() {
        let blocks = parse("Here is code:\n\n```\nlet half = ");
        assert_eq!(text(&blocks[0]), "Here is code:");
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, Block::Code(code) if code.starts_with("let half")))
        );
        // Until it closes, an emphasis marker is the text it is.
        assert_eq!(text(&parse("a **bold that never")[0]), "a **bold that never");
    }

    #[test]
    fn soft_breaks_join_lines_into_one_paragraph() {
        let blocks = parse("one\ntwo\n\nthree");
        assert_eq!(blocks.iter().map(text).collect::<Vec<_>>(), ["one two", "three"]);
    }
}
