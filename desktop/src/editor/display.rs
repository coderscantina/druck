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
    pub strike: bool,
    pub underline: bool,
    /// A visible marker, a reference, or a comment.
    pub quiet: bool,
    /// Marked text of an input method.
    pub marked: bool,
    /// Shows a chip in place of its source.
    pub chip: Option<ChipLook>,
}

/// How a chip looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipLook {
    /// A footnote's small mark.
    Mark,
    /// A citation or a cross-reference.
    Label,
    /// What the book cannot resolve.
    Error,
    /// Words an agent took out, which stand for no source.
    Removed,
}

/// A run of a block's source that shows as other text. `range` is relative to the block, on one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub range: Range<usize>,
    pub text: String,
    pub look: ChipLook,
    /// The caret passes over it as one step, and a key deletes all of it.
    pub atomic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    display: usize,
    source: usize,
    len: usize,
    /// How much source it stands for, which differs from `len` for a chip.
    source_len: usize,
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
    /// An empty line inside a hidden comment, which takes no room.
    pub inside_hidden: bool,
    /// Where each chip on the line shows, as displayed offsets.
    pub chips: Vec<(Range<usize>, ChipLook)>,
}

impl DisplayLine {
    /// The displayed offset of a source offset. Inside a hidden range, the place it would appear.
    pub fn to_display(&self, source: usize) -> usize {
        for segment in &self.segments {
            if source <= segment.source + segment.source_len {
                let into = source.saturating_sub(segment.source);
                return segment.display
                    + if segment.len == segment.source_len || into == 0 {
                        into
                    } else {
                        segment.len
                    };
            }
        }
        self.text.len()
    }

    /// Like [`Self::to_display`], but past a chip that stands at `source` for no source, as an agent's removed words do.
    pub fn to_display_past_chip(&self, source: usize) -> usize {
        let at = self.to_display(source);
        (self.segments.iter())
            .find(|segment| segment.source == source && segment.source_len == 0 && segment.display == at)
            .map_or(at, |segment| segment.display + segment.len)
    }

    /// The source offset of a displayed offset. Between a marker and text it is the end of the text before the marker,
    /// so typing continues the style just passed. The start of the line stays after any leading markers.
    pub fn to_source(&self, display: usize) -> usize {
        for segment in &self.segments {
            if display <= segment.display + segment.len {
                let into = display - segment.display;
                return segment.source
                    + if segment.len == segment.source_len || into == 0 {
                        into
                    } else {
                        segment.source_len
                    };
            }
        }
        self.source.end
    }

    /// A line whose whole content is hidden takes no room.
    pub fn is_collapsed(&self) -> bool {
        self.text.is_empty() && (!self.source.is_empty() || self.inside_hidden)
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

/// What stays hidden with the caret at `caret`: line markers unless the caret is on their line, inline markers and
/// comments unless the caret touches their span or `show_comments` asks for them, and the leading spaces of list lines
/// always.
pub fn hidden_ranges(
    kind: BlockKind,
    text: &str,
    syntax: &Syntax,
    caret: Option<usize>,
    trailing_line: bool,
    show_comments: bool,
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
            if span.kind == SpanKind::Comment {
                if !show_comments {
                    hidden.push(span.range.clone());
                }
            } else {
                hidden.push(span.open.clone());
                hidden.push(span.close.clone());
            }
        }
    }
    if kind == BlockKind::List {
        for line in &lines {
            let spaces = text[line.clone()].bytes().take_while(|byte| *byte == b' ').count();
            hidden.push(line.start..line.start + spaces);
        }
    }
    // A label or style at the end of a heading or paragraph, which the pickers show. It shows once the caret is inside
    // it, so a caret right after the words before it, as at the end of a title, leaves it hidden.
    if matches!(kind, BlockKind::Heading { .. } | BlockKind::Paragraph)
        && let Some(line) = lines.iter().rev().find(|line| !line.is_empty())
        && let Some(group) = super::rich::styles::closing_group(&text[line.clone()])
    {
        let before = text[line.start..line.start + group.start].trim_end_matches(' ').len();
        let range = line.start + before..line.start + group.end;
        if !caret.is_some_and(|caret| range.start < caret && caret <= range.end) {
            hidden.push(range);
        }
    }
    // A marker line between two versions shows its words alone, until the caret comes to it.
    if kind == BlockKind::Comment && caret.is_none() && crate::persist::merge::is_marker(text) {
        let line = text.trim_end();
        hidden.push(0.."<!-- ".len());
        hidden.push(line.len() - " -->".len()..line.len());
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
        SpanKind::Strike => style.strike = true,
        SpanKind::Underline => style.underline = true,
        SpanKind::Todo => style.bold = true,
        SpanKind::Superscript | SpanKind::Subscript | SpanKind::SmallCaps => {}
        SpanKind::FootnoteReference | SpanKind::Citation | SpanKind::CrossReference | SpanKind::Comment => {
            style.quiet = true;
        }
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
    chips: &[Chip],
) -> Vec<DisplayLine> {
    let lines = lines(text, trailing_line);
    lines
        .iter()
        .map(|line| display_line(kind, text, line.clone(), syntax, hidden, marked.as_ref(), chips))
        .collect()
}

fn display_line(
    kind: BlockKind,
    text: &str,
    line: Range<usize>,
    syntax: &Syntax,
    hidden: &[Range<usize>],
    marked: Option<&Range<usize>>,
    chips: &[Chip],
) -> DisplayLine {
    let parts = visible_parts(&line, hidden);
    let mut display = String::with_capacity(line.len());
    let mut segments = Vec::with_capacity(parts.len());
    let mut runs: Vec<(usize, RunStyle)> = Vec::new();
    let mut shown_chips = Vec::new();
    for part in &parts {
        let mut at = part.start;
        for chip in chips
            .iter()
            .filter(|chip| part.start <= chip.range.start && chip.range.end <= part.end)
        {
            if chip.range.start > at {
                let range = at..chip.range.start;
                push_plain(
                    text,
                    range,
                    kind,
                    syntax,
                    marked,
                    &mut display,
                    &mut segments,
                    &mut runs,
                );
            }
            segments.push(Segment {
                display: display.len(),
                source: chip.range.start,
                len: chip.text.len(),
                source_len: chip.range.len(),
            });
            shown_chips.push((display.len()..display.len() + chip.text.len(), chip.look));
            display.push_str(&chip.text);
            let style = RunStyle {
                chip: Some(chip.look),
                ..RunStyle::default()
            };
            push_run(&mut runs, (chip.text.len(), style));
            at = chip.range.end;
        }
        if at < part.end {
            push_plain(
                text,
                at..part.end,
                kind,
                syntax,
                marked,
                &mut display,
                &mut segments,
                &mut runs,
            );
        }
    }
    if segments.is_empty() {
        segments.push(Segment {
            display: 0,
            source: line.end,
            len: 0,
            source_len: 0,
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
    let inside_hidden = line.is_empty()
        && hidden
            .iter()
            .any(|range| range.start < line.start && line.end < range.end);
    DisplayLine {
        source: line,
        inside_hidden,
        chips: shown_chips,
        text: display,
        segments,
        runs,
        bullet,
        nesting,
    }
}

fn push_run(runs: &mut Vec<(usize, RunStyle)>, run: (usize, RunStyle)) {
    match runs.last_mut() {
        Some((len, style)) if *style == run.1 => *len += run.0,
        _ => runs.push(run),
    }
}

/// Adds a part of the source that shows as written.
#[allow(clippy::too_many_arguments, reason = "the pieces of one line under construction")]
fn push_plain(
    text: &str,
    range: Range<usize>,
    kind: BlockKind,
    syntax: &Syntax,
    marked: Option<&Range<usize>>,
    display: &mut String,
    segments: &mut Vec<Segment>,
    runs: &mut Vec<(usize, RunStyle)>,
) {
    segments.push(Segment {
        display: display.len(),
        source: range.start,
        len: range.len(),
        source_len: range.len(),
    });
    display.push_str(&text[range.clone()]);
    for run in style_runs(range, kind, syntax, marked) {
        push_run(runs, run);
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
        let hidden = hidden_ranges(kind, text, &syntax, caret, false, false);
        display_lines(kind, text, &syntax, &hidden, None, false, &[])
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
    fn a_comment_hides_like_a_marker_and_shows_when_asked() {
        let text = "a <!-- b --> c";
        assert_eq!(show(BlockKind::Paragraph, text, None)[0].text, "a  c");
        assert_eq!(show(BlockKind::Paragraph, text, Some(5))[0].text, text);
        let syntax = syntax(BlockKind::Paragraph, text);
        let hidden = hidden_ranges(BlockKind::Paragraph, text, &syntax, None, false, true);
        assert_eq!(
            display_lines(BlockKind::Paragraph, text, &syntax, &hidden, None, false, &[])[0].text,
            text
        );
    }

    #[test]
    fn the_lines_of_a_hidden_comment_take_no_room() {
        let lines = show(BlockKind::Paragraph, "a <!-- b\n\nc --> d\n", None);
        assert_eq!(lines[0].text, "a ");
        assert!(lines[1].is_collapsed());
        assert_eq!(lines[2].text, " d");
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
    fn a_closing_label_or_style_hides_until_the_caret_is_inside_it() {
        assert_eq!(show(HEADING, "# Title {#sec:a}\n", None)[0].text, "Title");
        assert_eq!(show(HEADING, "# Title {#sec:a}\n", Some(7))[0].text, "# Title");
        assert_eq!(show(HEADING, "# Title {#sec:a}\n", Some(9))[0].text, "# Title {#sec:a}");
        let lines = show(BlockKind::Paragraph, "One\ntwo {.lead}\n", None);
        assert_eq!((lines[0].text.as_str(), lines[1].text.as_str()), ("One", "two"));
        assert_eq!(show(BlockKind::Paragraph, "One\ntwo {.lead}\n", Some(5))[1].text, "two");
        assert_eq!(
            show(BlockKind::Paragraph, "One\ntwo {.lead}\n", Some(15))[1].text,
            "two {.lead}"
        );
        assert_eq!(show(BlockKind::Paragraph, "a {b} c\n", None)[0].text, "a {b} c");
    }

    #[test]
    fn a_marker_between_two_versions_shows_its_words() {
        let marker = "<!-- >>>>>>> from outside -->\n";
        assert_eq!(show(BlockKind::Comment, marker, None)[0].text, ">>>>>>> from outside");
        assert_eq!(show(BlockKind::Comment, marker, Some(3))[0].text, marker.trim_end());
    }

    #[test]
    fn a_chip_shows_in_place_of_its_source_and_maps_to_its_edges() {
        let text = "a [@k] b";
        let syntax = syntax(BlockKind::Paragraph, text);
        let chips = [Chip {
            range: 2..6,
            text: "(K 1)".to_owned(),
            look: ChipLook::Label,
            atomic: true,
        }];
        let line = &display_lines(BlockKind::Paragraph, text, &syntax, &[], None, false, &chips)[0];
        assert_eq!(line.text, "a (K 1) b");
        assert_eq!(line.chips, [(2..7, ChipLook::Label)]);
        assert_eq!(line.to_display(2), 2);
        assert_eq!(line.to_display(4), 7);
        assert_eq!(line.to_display(6), 7);
        assert_eq!(line.to_display(7), 8);
        assert_eq!(line.to_source(2), 2);
        assert_eq!(line.to_source(4), 6);
        assert_eq!(line.to_source(7), 6);
        assert_eq!(line.to_source(8), 7);
    }

    #[test]
    fn a_trailing_line_is_empty_and_marked_text_is_flagged() {
        let text = "ab\n";
        let syntax = syntax(BlockKind::Paragraph, text);
        let lines = display_lines(BlockKind::Paragraph, text, &syntax, &[], Some(1..2), true, &[]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].source, 3..3);
        assert!(lines[0].runs[1].1.marked);
    }
}
