//! The syntax markers inside one block, as ranges relative to the block's start, so a view can hide them.
//!
//! Emphasis, strong, code, and links come from pulldown-cmark, the engine's parser, run over the block alone.
//! Footnote references and citations are recognised here: the first need a definition the block does not have, and
//! the second come from the engine's own citation scanner.

use std::ops::Range;

use druck::bibliography::syntax::{self, Segment};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::blocks::BlockKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Emphasis,
    Strong,
    Code,
    Link,
    FootnoteReference,
    Citation,
    /// `<!-- -->` inside a paragraph.
    Comment,
}

/// One inline construct: `range` holds all of it, `open` and `close` its markers at either end. A span whose markers
/// are its content, such as a citation, has empty `open` and `close`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSpan {
    pub kind: SpanKind,
    pub range: Range<usize>,
    pub open: Range<usize>,
    pub close: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// `# ` or a closing ` #`, or a setext underline line.
    Heading,
    /// `> `
    Quote,
    /// `- `
    Bullet,
    /// `1. `
    Number,
}

/// A marker that belongs to a line of a block rather than to a span of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMarker {
    pub kind: MarkerKind,
    pub range: Range<usize>,
}

/// Whether a kind of block holds running text. The others stay raw.
pub fn has_inline(kind: BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::Heading { .. }
            | BlockKind::Paragraph
            | BlockKind::List
            | BlockKind::Quote
            | BlockKind::FootnoteDefinition
    )
}

/// The inline spans of a block, sorted by start, outer spans before the spans inside them.
pub fn inline_spans(kind: BlockKind, text: &str) -> Vec<InlineSpan> {
    if !has_inline(kind) {
        return Vec::new();
    }
    let mut spans = parser_spans(text);
    let opaque: Vec<Range<usize>> = spans
        .iter()
        .filter(|span| matches!(span.kind, SpanKind::Code | SpanKind::Link | SpanKind::Comment))
        .map(|span| span.range.clone())
        .collect();
    for gap in gaps(text.len(), &opaque) {
        marks_in(&text[gap.clone()], gap.start, &mut spans);
    }
    spans.sort_by_key(|span| (span.range.start, std::cmp::Reverse(span.range.end)));
    spans
}

fn parser_spans(text: &str) -> Vec<InlineSpan> {
    // The same constructs the engine parses, without the typographic rewriting, which does not move any range here.
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_GFM;
    let mut spans = Vec::new();
    let mut open: Vec<(SpanKind, Range<usize>, usize)> = Vec::new();
    let mut last_inner_end = 0;
    let mut in_image = 0;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Image { .. }) => in_image += 1,
            Event::End(TagEnd::Image) => in_image -= 1,
            _ if in_image > 0 => {}
            Event::Start(Tag::Emphasis) => open.push((SpanKind::Emphasis, range, 1)),
            Event::Start(Tag::Strong) => open.push((SpanKind::Strong, range, 2)),
            Event::Start(Tag::Link { .. }) => {
                open.push((SpanKind::Link, range, 1));
                last_inner_end = 0;
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong) => {
                if let Some((kind, whole, marker)) = open.pop() {
                    last_inner_end = last_inner_end.max(whole.end);
                    spans.push(InlineSpan {
                        kind,
                        open: whole.start..whole.start + marker,
                        close: whole.end - marker..whole.end,
                        range: whole,
                    });
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((kind, whole, marker)) = open.pop() {
                    let inner_end = last_inner_end.max(whole.start + marker);
                    spans.push(InlineSpan {
                        kind,
                        open: whole.start..whole.start + marker,
                        close: inner_end..whole.end,
                        range: whole,
                    });
                }
            }
            Event::Code(_) => {
                let bytes = text.as_bytes();
                let marker = bytes[range.start..range.end]
                    .iter()
                    .take_while(|byte| **byte == b'`')
                    .count();
                spans.push(InlineSpan {
                    kind: SpanKind::Code,
                    open: range.start..range.start + marker,
                    close: range.end - marker..range.end,
                    range: range.clone(),
                });
                last_inner_end = last_inner_end.max(range.end);
            }
            Event::InlineHtml(ref html) if html.starts_with("<!--") => {
                spans.push(InlineSpan {
                    kind: SpanKind::Comment,
                    open: range.start..range.start,
                    close: range.end..range.end,
                    range: range.clone(),
                });
            }
            _ => {
                if !open.is_empty() {
                    last_inner_end = last_inner_end.max(range.end);
                }
            }
        }
    }
    spans
}

/// Adds footnote references and citations found in `text`, which starts at `base` in the block.
fn marks_in(text: &str, base: usize, spans: &mut Vec<InlineSpan>) {
    let mut search = 0;
    while let Some(at) = text[search..].find("[^") {
        let start = search + at;
        let rest = &text[start + 2..];
        let Some(close) = rest
            .find([']', ' ', '\n', '\t'])
            .filter(|close| rest.as_bytes()[*close] == b']' && *close > 0)
        else {
            search = start + 2;
            continue;
        };
        let end = start + 2 + close + 1;
        // A definition's own label is not a reference.
        if !text[end..].starts_with(':') {
            let range = base + start..base + end;
            spans.push(InlineSpan {
                kind: SpanKind::FootnoteReference,
                open: range.start..range.start,
                close: range.end..range.end,
                range,
            });
        }
        search = end;
    }
    for segment in syntax::find(text) {
        if let Segment::Citation(citation) = segment {
            let range = base + citation.range.start..base + citation.range.end;
            spans.push(InlineSpan {
                kind: SpanKind::Citation,
                open: range.start..range.start,
                close: range.end..range.end,
                range,
            });
        }
    }
}

/// The parts of `0..len` that `covered`, in any order and possibly nested, does not touch.
fn gaps(len: usize, covered: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut sorted: Vec<&Range<usize>> = covered.iter().collect();
    sorted.sort_by_key(|range| range.start);
    let mut gaps = Vec::new();
    let mut pos = 0;
    for range in sorted {
        if range.start > pos {
            gaps.push(pos..range.start);
        }
        pos = pos.max(range.end);
    }
    if pos < len {
        gaps.push(pos..len);
    }
    gaps
}

/// The line markers of a block.
pub fn line_markers(kind: BlockKind, text: &str) -> Vec<LineMarker> {
    match kind {
        BlockKind::Heading { .. } => heading_markers(text),
        BlockKind::Quote => per_line(text, quote_marker),
        BlockKind::List => per_line(text, list_marker),
        _ => Vec::new(),
    }
}

fn per_line(text: &str, find: impl Fn(&str) -> Option<LineMarker>) -> Vec<LineMarker> {
    let mut start = 0;
    let mut markers = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(marker) = find(line) {
            markers.push(LineMarker {
                kind: marker.kind,
                range: start + marker.range.start..start + marker.range.end,
            });
        }
        start += line.len();
    }
    markers
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|byte| *byte == b' ').count()
}

fn heading_markers(text: &str) -> Vec<LineMarker> {
    let marker = |range| LineMarker {
        kind: MarkerKind::Heading,
        range,
    };
    let first_end = text.find('\n').map_or(text.len(), |at| at + 1);
    let first = text[..first_end].trim_end_matches(['\n', '\r']);
    let spaces = leading_spaces(first);
    if first[spaces..].starts_with('#') {
        let hashes = first[spaces..].bytes().take_while(|byte| *byte == b'#').count();
        let gap = first[spaces + hashes..]
            .bytes()
            .take_while(|byte| *byte == b' ' || *byte == b'\t')
            .count();
        let mut markers = vec![marker(0..spaces + hashes + gap)];
        let trimmed = first.trim_end();
        let body = trimmed.trim_end_matches('#');
        if body.len() < trimmed.len() && body.len() > spaces + hashes && body.ends_with([' ', '\t']) {
            markers.push(marker(body.trim_end().len()..first.len()));
        }
        return markers;
    }
    // A setext heading: the last line is the underline.
    let content = text.trim_end_matches(['\n', '\r']);
    let underline = content.rfind('\n').map_or(0, |at| at + 1);
    vec![marker(underline..content.len())]
}

fn quote_marker(line: &str) -> Option<LineMarker> {
    let mut end = leading_spaces(line);
    if end > 3 || !line[end..].starts_with('>') {
        return None;
    }
    while line[end..].starts_with('>') {
        end += 1;
        if line[end..].starts_with(' ') {
            end += 1;
        }
    }
    Some(LineMarker {
        kind: MarkerKind::Quote,
        range: 0..end,
    })
}

/// The marker itself and the gap after it. Leading spaces stay: they are the nesting.
fn list_marker(line: &str) -> Option<LineMarker> {
    let spaces = leading_spaces(line);
    let content = &line[spaces..];
    let bytes = content.as_bytes();
    let (kind, marker) = match bytes.first()? {
        b'-' | b'+' | b'*' => (MarkerKind::Bullet, 1),
        b'0'..=b'9' => {
            let digits = bytes.iter().take_while(|byte| byte.is_ascii_digit()).count();
            if !matches!(bytes.get(digits), Some(b'.' | b')')) {
                return None;
            }
            (MarkerKind::Number, digits + 1)
        }
        _ => return None,
    };
    let rest = &content[marker..];
    if !rest.starts_with([' ', '\t']) && !rest.trim_end_matches(['\n', '\r']).is_empty() {
        return None;
    }
    let gap = rest.bytes().take_while(|byte| *byte == b' ' || *byte == b'\t').count();
    Some(LineMarker {
        kind,
        range: spaces..spaces + marker + gap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<(SpanKind, &str, &str, &str)> {
        inline_spans(BlockKind::Paragraph, text)
            .into_iter()
            .map(|span| {
                (
                    span.kind,
                    &text[span.range.clone()],
                    &text[span.open.clone()],
                    &text[span.close.clone()],
                )
            })
            .collect()
    }

    #[test]
    fn finds_emphasis_strong_and_code_with_their_markers() {
        use SpanKind::*;
        assert_eq!(
            spans("a *b* **c** `d` ``e``"),
            [
                (Emphasis, "*b*", "*", "*"),
                (Strong, "**c**", "**", "**"),
                (Code, "`d`", "`", "`"),
                (Code, "``e``", "``", "``"),
            ]
        );
    }

    #[test]
    fn nested_spans_come_outer_first() {
        use SpanKind::*;
        assert_eq!(
            spans("***x***"),
            [(Emphasis, "***x***", "*", "*"), (Strong, "**x**", "**", "**")]
        );
    }

    #[test]
    fn a_link_hides_all_but_its_text() {
        assert_eq!(
            spans("see [the *docs*](http://x \"T\") now")[0],
            (SpanKind::Link, "[the *docs*](http://x \"T\")", "[", "](http://x \"T\")")
        );
    }

    #[test]
    fn finds_footnote_references_and_citations_but_not_inside_code_or_links() {
        use SpanKind::*;
        assert_eq!(
            spans("One[^a] [@smith, p. 3] and @lee. `[^b] @c` [x @d](u)")
                .into_iter()
                .filter(|span| matches!(span.0, FootnoteReference | Citation))
                .map(|span| (span.0, span.1))
                .collect::<Vec<_>>(),
            [
                (FootnoteReference, "[^a]"),
                (Citation, "[@smith, p. 3]"),
                (Citation, "@lee"),
            ]
        );
    }

    #[test]
    fn a_definition_label_is_not_a_reference() {
        let spans = inline_spans(BlockKind::FootnoteDefinition, "[^a]: Text[^b] *x*\n");
        let kinds: Vec<_> = spans.iter().map(|span| span.kind).collect();
        assert_eq!(kinds, [SpanKind::FootnoteReference, SpanKind::Emphasis]);
    }

    #[test]
    fn raw_blocks_have_no_spans() {
        for kind in [
            BlockKind::Code,
            BlockKind::Table,
            BlockKind::Directive,
            BlockKind::Comment,
            BlockKind::Raw,
        ] {
            assert!(inline_spans(kind, "*a* [^b]").is_empty());
        }
    }

    #[test]
    fn heading_markers_cover_prefix_suffix_and_underline() {
        let heading = BlockKind::Heading { level: 2 };
        let ranges = |text: &str| -> Vec<(usize, usize)> {
            line_markers(heading, text)
                .into_iter()
                .map(|marker| (marker.range.start, marker.range.end))
                .collect()
        };
        assert_eq!(ranges("## Title\n"), [(0, 3)]);
        assert_eq!(ranges("## Title ##\n"), [(0, 3), (8, 11)]);
        assert_eq!(ranges("Title\n---\n"), [(6, 9)]);
        assert_eq!(ranges("#"), [(0, 1)]);
    }

    #[test]
    fn quote_and_list_markers_are_per_line() {
        let markers = line_markers(BlockKind::Quote, "> a\n>> b\nlazy\n");
        assert_eq!(
            markers.iter().map(|marker| marker.range.clone()).collect::<Vec<_>>(),
            [0..2, 4..7]
        );
        let markers = line_markers(BlockKind::List, "- a\n  1. b\n\n  more\n");
        assert_eq!(
            markers
                .iter()
                .map(|marker| (marker.kind, marker.range.clone()))
                .collect::<Vec<_>>(),
            [(MarkerKind::Bullet, 0..2), (MarkerKind::Number, 6..9)]
        );
    }
}
