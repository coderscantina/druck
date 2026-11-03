//! What one source line shows: the source minus its hidden markers, with a style per run and a map between displayed
//! and source offsets. Holds no gpui types. Offsets called source here are relative to the block.

use std::ops::Range;

use crate::source::BlockKind;
use crate::source::inline::{InlineSpan, LineMarker, MarkerKind, SpanKind, has_inline};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: bool,
    /// A visible marker, a reference, or a comment.
    pub quiet: bool,
    /// Marked text of an input method.
    pub marked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    display: usize,
    source: usize,
    len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayLine {
    /// The line's content in the source, without its line ending.
    pub source: Range<usize>,
    pub text: String,
    segments: Vec<Segment>,
    pub runs: Vec<(usize, RunStyle)>,
    /// The marker to draw in the margin in place of a hidden list marker.
    pub bullet: Option<String>,
    /// Leading spaces of a list line, which become indentation.
    pub nesting: usize,
}

impl DisplayLine {
    /// The displayed offset of a source offset. Inside a hidden range, the place it would appear.
    pub fn to_display(&self, source: usize) -> usize {
        for segment in &self.segments {
            if source <= segment.source + segment.len {
                return segment.display + source.saturating_sub(segment.source);
            }
        }
        self.text.len()
    }

    /// The source offset of a displayed offset. Between a marker and text it is the end of the text before the marker,
    /// so typing continues the style just passed. The start of the line stays after any leading markers.
    pub fn to_source(&self, display: usize) -> usize {
        for segment in &self.segments {
            if display <= segment.display + segment.len {
                return segment.source + display - segment.display;
            }
        }
        self.source.end
    }

    /// A line whose whole content is hidden takes no room.
    pub fn is_collapsed(&self) -> bool {
        self.text.is_empty() && !self.source.is_empty()
    }
}

/// The syntax of one block, cached between frames.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Syntax {
    pub spans: Vec<InlineSpan>,
    pub markers: Vec<LineMarker>,
}

/// The source lines of a block. A `trailing_line` is the empty last line after a final line ending.
fn lines(text: &str, trailing_line: bool) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches('\n');
        let content = content.strip_suffix('\r').unwrap_or(content);
        lines.push(start..start + content.len());
        start += line.len();
    }
    if lines.is_empty() || trailing_line {
        lines.push(start..start);
    }
    lines
}

fn line_holds(lines: &[Range<usize>], caret: Option<usize>, at: usize) -> bool {
    let Some(caret) = caret else {
        return false;
    };
    lines
        .iter()
        .find(|line| line.start <= at && at <= line.end)
        .is_some_and(|line| line.start <= caret && caret <= line.end)
}

/// What stays hidden with the caret at `caret`: line markers unless the caret is on their line, inline markers unless
/// the caret touches their span, and the leading spaces of list lines always.
pub fn hidden_ranges(
    kind: BlockKind,
    text: &str,
    syntax: &Syntax,
    caret: Option<usize>,
    trailing_line: bool,
) -> Vec<Range<usize>> {
    let lines = lines(text, trailing_line);
    let mut hidden = Vec::new();
    for marker in &syntax.markers {
        if !line_holds(&lines, caret, marker.range.start) {
            hidden.push(marker.range.clone());
        }
    }
    for span in &syntax.spans {
        let revealed = caret.is_some_and(|caret| span.range.start <= caret && caret <= span.range.end);
        if !revealed {
            hidden.push(span.open.clone());
            hidden.push(span.close.clone());
        }
    }
    if kind == BlockKind::List {
        for line in &lines {
            let spaces = text[line.clone()].bytes().take_while(|byte| *byte == b' ').count();
            hidden.push(line.start..line.start + spaces);
        }
    }
    hidden.retain(|range| !range.is_empty());
    hidden.sort_by_key(|range| range.start);
    hidden
}

fn span_style(kind: SpanKind, style: &mut RunStyle) {
    match kind {
        SpanKind::Emphasis => style.italic = !style.italic,
        SpanKind::Strong => style.bold = true,
        SpanKind::Code => style.code = true,
        SpanKind::Link => style.link = true,
        SpanKind::FootnoteReference | SpanKind::Citation | SpanKind::Comment => style.quiet = true,
    }
}

/// Builds the display of every line of a block from its source, syntax, and hidden ranges.
pub fn display_lines(
    kind: BlockKind,
    text: &str,
    syntax: &Syntax,
    hidden: &[Range<usize>],
    marked: Option<Range<usize>>,
    trailing_line: bool,
) -> Vec<DisplayLine> {
    let lines = lines(text, trailing_line);
    lines
        .iter()
        .map(|line| display_line(kind, text, line.clone(), syntax, hidden, marked.as_ref()))
        .collect()
}

fn display_line(
    kind: BlockKind,
    text: &str,
    line: Range<usize>,
    syntax: &Syntax,
    hidden: &[Range<usize>],
    marked: Option<&Range<usize>>,
) -> DisplayLine {
    let segments_source = visible_parts(&line, hidden);
    let mut display = String::with_capacity(line.len());
    let mut segments = Vec::with_capacity(segments_source.len());
    let mut runs: Vec<(usize, RunStyle)> = Vec::new();
    for part in &segments_source {
        segments.push(Segment {
            display: display.len(),
            source: part.start,
            len: part.len(),
        });
        display.push_str(&text[part.clone()]);
        for run in style_runs(part.clone(), kind, syntax, marked) {
            match runs.last_mut() {
                Some((len, style)) if *style == run.1 => *len += run.0,
                _ => runs.push(run),
            }
        }
    }
    if segments.is_empty() {
        segments.push(Segment {
            display: 0,
            source: line.end,
            len: 0,
        });
    }
    let nesting = if kind == BlockKind::List {
        text[line.clone()].bytes().take_while(|byte| *byte == b' ').count()
    } else {
        0
    };
    let bullet = syntax
        .markers
        .iter()
        .find(|marker| line.contains(&marker.range.start) && hidden.contains(&marker.range))
        .and_then(|marker| match marker.kind {
            MarkerKind::Bullet => Some("•".to_owned()),
            MarkerKind::Number => Some(text[marker.range.clone()].trim_end().to_owned()),
            MarkerKind::Heading | MarkerKind::Quote => None,
        });
    DisplayLine {
        source: line,
        text: display,
        segments,
        runs,
        bullet,
        nesting,
    }
}

/// The parts of a line outside the hidden ranges.
fn visible_parts(line: &Range<usize>, hidden: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut pos = line.start;
    for range in hidden {
        if range.end <= pos || range.start >= line.end {
            continue;
        }
        if range.start > pos {
            parts.push(pos..range.start);
        }
        pos = pos.max(range.end);
    }
    if pos < line.end {
        parts.push(pos..line.end);
    }
    parts
}

/// The styled runs of a visible part of a line.
fn style_runs(
    part: Range<usize>,
    kind: BlockKind,
    syntax: &Syntax,
    marked: Option<&Range<usize>>,
) -> Vec<(usize, RunStyle)> {
    let mut cuts = vec![part.start, part.end];
    let mut cut = |at: usize| {
        if part.start < at && at < part.end {
            cuts.push(at);
        }
    };
    for span in &syntax.spans {
        for at in [span.range.start, span.open.end, span.close.start, span.range.end] {
            cut(at);
        }
    }
    for marker in &syntax.markers {
        cut(marker.range.start);
        cut(marker.range.end);
    }
    if let Some(marked) = marked {
        cut(marked.start);
        cut(marked.end);
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .map(|pair| (pair[1] - pair[0], style_at(pair[0], kind, syntax, marked)))
        .collect()
}

fn style_at(at: usize, kind: BlockKind, syntax: &Syntax, marked: Option<&Range<usize>>) -> RunStyle {
    let mut style = RunStyle::default();
    if !has_inline(kind) {
        style.marked = marked.is_some_and(|marked| marked.contains(&at));
        return style;
    }
    for span in &syntax.spans {
        if !span.range.contains(&at) {
            continue;
        }
        if span.open.contains(&at) || span.close.contains(&at) {
            style.quiet = true;
        } else {
            span_style(span.kind, &mut style);
        }
    }
    if syntax.markers.iter().any(|marker| marker.range.contains(&at)) {
        style.quiet = true;
    }
    style.marked = marked.is_some_and(|marked| marked.contains(&at));
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::inline::{inline_spans, line_markers};

    fn syntax(kind: BlockKind, text: &str) -> Syntax {
        Syntax {
            spans: inline_spans(kind, text),
            markers: line_markers(kind, text),
        }
    }

    fn show(kind: BlockKind, text: &str, caret: Option<usize>) -> Vec<DisplayLine> {
        let syntax = syntax(kind, text);
        let hidden = hidden_ranges(kind, text, &syntax, caret, false);
        display_lines(kind, text, &syntax, &hidden, None, false)
    }

    const HEADING: BlockKind = BlockKind::Heading { level: 1 };

    #[test]
    fn markers_hide_until_the_caret_touches_them() {
        let text = "a **b** c";
        assert_eq!(show(BlockKind::Paragraph, text, None)[0].text, "a b c");
        assert_eq!(show(BlockKind::Paragraph, text, Some(0))[0].text, "a b c");
        assert_eq!(show(BlockKind::Paragraph, text, Some(2))[0].text, text);
        assert_eq!(show(BlockKind::Paragraph, text, Some(7))[0].text, text);
        assert_eq!(show(BlockKind::Paragraph, text, Some(8))[0].text, "a b c");
        assert_eq!(show(HEADING, "# Title\n", None)[0].text, "Title");
        assert_eq!(show(HEADING, "# Title\n", Some(3))[0].text, "# Title");
    }

    #[test]
    fn only_the_caret_line_of_a_list_reveals_its_marker() {
        let lines = show(BlockKind::List, "- a\n  - b\n", Some(1));
        assert_eq!(lines[0].text, "- a");
        assert_eq!(lines[0].bullet, None);
        assert_eq!(lines[1].text, "b");
        assert_eq!(lines[1].bullet.as_deref(), Some("•"));
        assert_eq!(lines[1].nesting, 2);
    }

    #[test]
    fn offsets_map_both_ways_around_hidden_markers() {
        let line = &show(BlockKind::Paragraph, "a **b** c", None)[0];
        assert_eq!(line.to_source(0), 0);
        assert_eq!(line.to_source(1), 1);
        assert_eq!(line.to_source(2), 2);
        assert_eq!(line.to_source(3), 5);
        assert_eq!(line.to_source(4), 8);
        assert_eq!(line.to_display(3), 2);
        assert_eq!(line.to_display(4), 2);
        assert_eq!(line.to_display(5), 3);
        assert_eq!(line.to_display(7), 3);
        assert_eq!(line.to_display(9), 5);
    }

    #[test]
    fn the_start_of_a_line_stays_after_a_hidden_prefix() {
        let line = &show(HEADING, "# Title\n", None)[0];
        assert_eq!(line.to_source(0), 2);
    }

    #[test]
    fn styles_follow_spans_and_merge_equal_neighbours() {
        let line = &show(BlockKind::Paragraph, "a **b** *c*d", None)[0];
        let bold = RunStyle {
            bold: true,
            ..RunStyle::default()
        };
        let italic = RunStyle {
            italic: true,
            ..RunStyle::default()
        };
        let plain = RunStyle::default();
        assert_eq!(line.runs, [(2, plain), (1, bold), (1, plain), (1, italic), (1, plain)]);
    }

    #[test]
    fn an_underline_line_collapses_when_hidden() {
        let lines = show(BlockKind::Heading { level: 1 }, "Title\n===\n", None);
        assert_eq!(lines[0].text, "Title");
        assert!(lines[1].is_collapsed());
    }

    #[test]
    fn raw_blocks_show_every_byte() {
        for kind in [
            BlockKind::Table,
            BlockKind::Comment,
            BlockKind::Directive,
            BlockKind::Code,
        ] {
            let line = &show(kind, "**a** [^b] # `c`", Some(0))[0];
            assert_eq!(line.text, "**a** [^b] # `c`");
        }
    }

    #[test]
    fn a_trailing_line_is_empty_and_marked_text_is_flagged() {
        let text = "ab\n";
        let syntax = syntax(BlockKind::Paragraph, text);
        let lines = display_lines(BlockKind::Paragraph, text, &syntax, &[], Some(1..2), true);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].source, 3..3);
        assert!(lines[0].runs[1].1.marked);
    }
}
