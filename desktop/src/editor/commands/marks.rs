//! Inline marks toggled from what the source says, never from a remembered style.
//!
//! A command works on *runs*: the running text in the selection, without whitespace or markers at its edges, one run per
//! line where each line has its own marker, as in a list.
//!
//! - **State:** a mark is on when every visible character of every run has it, off when none does, and mixed otherwise.
//! - **Toggling a mark that is on** removes it from the selected text only. `**abc def**` with `def` selected becomes
//!   `**abc** def`, and all of `def` selected in `**def**` leaves plain `def`.
//! - **Toggling a mark that is off or mixed** puts it on all of the selection and merges with any span of that mark the
//!   selection touches. `**ab** cd` with `b c` selected becomes `**ab c**d`. The selection grows to hold any other span
//!   it would otherwise cross, and to hold code, comments, references, and citations whole.
//! - `^x^` and `~x~` hold no spaces, so they mark each word on its own.
//! - A result is parsed again. It must read as asked and keep every other span, or the command writes nothing.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

use super::{Bias, Command, applied, moved, normalized, selection_after};
use crate::source::inline::{InlineSpan, SpanKind, has_inline, inline_spans, line_markers};
use crate::source::{BlockKind, Blocks, Edit, Selection, motion};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
    Superscript,
    Subscript,
    SmallCaps,
}

impl Mark {
    #[cfg(test)]
    pub const ALL: [Self; 7] = [
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::Strike,
        Self::Superscript,
        Self::Subscript,
        Self::SmallCaps,
    ];

    fn syntax(self) -> Syntax<'static> {
        let (kind, open, close) = match self {
            Self::Bold => (SpanKind::Strong, "**", "**"),
            Self::Italic => (SpanKind::Emphasis, "*", "*"),
            Self::Underline => (SpanKind::Underline, "[", "]{.underline}"),
            Self::Strike => (SpanKind::Strike, "~~", "~~"),
            Self::Superscript => (SpanKind::Superscript, "^", "^"),
            Self::Subscript => (SpanKind::Subscript, "~", "~"),
            Self::SmallCaps => (SpanKind::SmallCaps, "[", "]{.smallcaps}"),
        };
        Syntax { kind, open, close }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkState {
    #[default]
    Off,
    Mixed,
    On,
}

/// What the toolbar shows for a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarkStates {
    pub bold: MarkState,
    pub italic: MarkState,
    pub underline: MarkState,
    pub strike: MarkState,
    pub superscript: MarkState,
    pub subscript: MarkState,
    pub small_caps: MarkState,
    pub link: MarkState,
}

impl MarkStates {
    pub fn of(&self, mark: Mark) -> MarkState {
        match mark {
            Mark::Bold => self.bold,
            Mark::Italic => self.italic,
            Mark::Underline => self.underline,
            Mark::Strike => self.strike,
            Mark::Superscript => self.superscript,
            Mark::Subscript => self.subscript,
            Mark::SmallCaps => self.small_caps,
        }
    }
}

#[derive(Clone, Copy)]
struct Syntax<'a> {
    kind: SpanKind,
    open: &'a str,
    close: &'a str,
}

/// One block a command reaches: its text and spans, and the runs in it, all relative to its start.
struct Reached<'a> {
    start: usize,
    kind: BlockKind,
    text: &'a str,
    spans: Vec<InlineSpan>,
    runs: Vec<Range<usize>>,
}

fn is_space(byte: u8) -> bool {
    byte.is_ascii_whitespace()
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// The content of a span: inside its markers.
fn content(span: &InlineSpan) -> Range<usize> {
    span.open.end..span.close.start
}

/// A span written as one unit, which a mark can go around but never into.
fn is_opaque(kind: SpanKind) -> bool {
    matches!(
        kind,
        SpanKind::Code | SpanKind::Comment | SpanKind::FootnoteReference | SpanKind::Citation | SpanKind::Todo
    )
}

/// Trailing `{#id .style placement=top}` or `{-}`, which belongs to the block and not to its text.
fn trailing_attribute(text: &str) -> Option<usize> {
    let body = text.trim_end();
    if !body.ends_with('}') {
        return None;
    }
    let open = body.rfind('{')?;
    let inner = &body[open + 1..body.len() - 1];
    let is_attribute = !inner.is_empty()
        && inner
            .split_whitespace()
            .all(|word| word.starts_with(['.', '#']) || word == "-" || word.contains('='));
    is_attribute.then_some(open)
}

/// `{...}` groups, such as placeholders and index marks. A selection edge never lands inside one.
fn brace_groups(text: &str) -> Vec<Range<usize>> {
    let mut groups = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find('{') {
        let open = from + open;
        let Some(close) = text[open..].find(['}', '\n']) else {
            break;
        };
        if text.as_bytes()[open + close] == b'}' {
            groups.push(open..open + close + 1);
        }
        from = open + close + 1;
    }
    groups
}

/// Images and the attribute after them. They stand alone in the block and no mark can hold them.
fn images(text: &str) -> Vec<Range<usize>> {
    if !text.contains("![") {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    for (event, range) in Parser::new(text).into_offset_iter() {
        if let Event::Start(Tag::Image { .. }) = event {
            let end = if text[range.end..].starts_with('{') {
                text[range.end..]
                    .find('}')
                    .map_or(range.end, |close| range.end + close + 1)
            } else {
                range.end
            };
            ranges.push(range.start..end);
        }
    }
    ranges
}

/// Where a block's running text is, each stretch one a single mark can cover.
fn content_ranges(kind: BlockKind, text: &str) -> Vec<Range<usize>> {
    if !has_inline(kind) {
        return Vec::new();
    }
    let body = text.trim_end_matches(['\n', '\r']).len();
    let mut ranges = match kind {
        BlockKind::Heading { .. } => {
            let markers = line_markers(kind, text);
            let setext = text[..body].contains('\n');
            let start = if setext {
                0
            } else {
                markers.first().map_or(0, |marker| marker.range.end)
            };
            let end = if setext {
                markers.first().map_or(body, |marker| marker.range.start)
            } else {
                markers.get(1).map_or(body, |marker| marker.range.start)
            };
            let heading = start..end.max(start);
            vec![heading]
        }
        BlockKind::List | BlockKind::Quote => {
            let markers = line_markers(kind, text);
            let mut lines = Vec::new();
            let mut start = 0;
            for line in text.split_inclusive('\n') {
                let end = start + line.trim_end_matches(['\n', '\r']).len();
                let lead = markers
                    .iter()
                    .find(|marker| start <= marker.range.start && marker.range.start <= end)
                    .map_or(start, |marker| marker.range.end);
                lines.push(lead.min(end)..end);
                start += line.len();
            }
            lines
        }
        BlockKind::FootnoteDefinition => {
            let label = text.find("]:").map_or(0, |close| close + "]:".len());
            let start = label + text[label..].len() - text[label..].trim_start_matches([' ', '\t']).len();
            let start = if text[start..].starts_with("{-}") {
                start + "{-}".len()
            } else {
                start
            };
            paragraphs(text, start..body)
        }
        _ => paragraphs(text, 0..body),
    };
    if matches!(kind, BlockKind::Paragraph | BlockKind::Heading { .. })
        && let Some(last) = ranges.last_mut()
        && let Some(attribute) = trailing_attribute(&text[last.clone()])
    {
        last.end = last.start + attribute;
    }
    for image in images(text) {
        ranges = ranges
            .into_iter()
            .flat_map(|range| {
                if overlaps(&range, &image) {
                    vec![range.start..image.start, image.end..range.end]
                } else {
                    vec![range]
                }
            })
            .filter(|range| range.start < range.end)
            .collect();
    }
    ranges
}

/// The parts of `range` between blank lines outside comments. No mark crosses a blank line.
fn paragraphs(text: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let comments: Vec<Range<usize>> = comment_ranges(text);
    let mut parts = Vec::new();
    let mut start = range.start;
    let mut from = range.start;
    while let Some(at) = text[from..range.end].find("\n\n") {
        let at = from + at;
        from = at + 2;
        if comments.iter().any(|comment| comment.contains(&at)) {
            continue;
        }
        parts.push(start..at);
        start = from;
    }
    parts.push(start..range.end);
    parts
}

fn comment_ranges(text: &str) -> Vec<Range<usize>> {
    if !text.contains("<!--") {
        return Vec::new();
    }
    inline_spans(BlockKind::Paragraph, text)
        .into_iter()
        .filter(|span| span.kind == SpanKind::Comment)
        .map(|span| span.range)
        .collect()
}

/// `range` without whitespace and markers at its edges.
fn trimmed(text: &str, spans: &[InlineSpan], mut range: Range<usize>) -> Range<usize> {
    let bytes = text.as_bytes();
    loop {
        let start = range.start;
        while range.start < range.end && is_space(bytes[range.start]) {
            range.start += 1;
        }
        for span in spans {
            let marker = if span.kind == SpanKind::Comment {
                &span.range
            } else {
                &span.open
            };
            if marker.contains(&range.start) {
                range.start = marker.end.min(range.end);
            } else if span.close.contains(&range.start) {
                range.start = span.close.end.min(range.end);
            }
        }
        if range.start == start {
            break;
        }
    }
    loop {
        let end = range.end;
        while range.start < range.end && is_space(bytes[range.end - 1]) {
            range.end -= 1;
        }
        for span in spans {
            let marker = if span.kind == SpanKind::Comment {
                &span.range
            } else {
                &span.close
            };
            if range.end > 0 && marker.contains(&(range.end - 1)) {
                range.end = marker.start.max(range.start);
            } else if range.end > 0 && span.open.contains(&(range.end - 1)) {
                range.end = span.open.start.max(range.start);
            }
        }
        if range.end == end {
            break;
        }
    }
    range
}

/// The blocks a selection reaches that hold text a mark can go on.
fn reached<'a>(text: &'a str, blocks: &Blocks, range: &Range<usize>) -> Vec<Reached<'a>> {
    let mut found = Vec::new();
    for block in &blocks.as_slice()[blocks.index_at(range.start)..] {
        if block.range.start >= range.end {
            break;
        }
        if !has_inline(block.kind) {
            continue;
        }
        let start = block.range.start;
        let block_text = &text[block.range.clone()];
        let relative = range.start.saturating_sub(start)..(range.end - start).min(block_text.len());
        let spans = inline_spans(block.kind, block_text);
        let runs: Vec<Range<usize>> = content_ranges(block.kind, block_text)
            .into_iter()
            .map(|part| part.start.max(relative.start)..part.end.min(relative.end))
            .filter(|part| part.start < part.end)
            .map(|part| trimmed(block_text, &spans, part))
            .filter(|part| part.start < part.end)
            .collect();
        if !runs.is_empty() {
            found.push(Reached {
                start,
                kind: block.kind,
                text: block_text,
                spans,
                runs,
            });
        }
    }
    found
}

/// Whether any and whether all of the visible text of `run` lies in a span of `kind`.
fn coverage(text: &str, spans: &[InlineSpan], run: &Range<usize>, kind: SpanKind) -> (bool, bool) {
    let scripted = matches!(kind, SpanKind::Superscript | SpanKind::Subscript);
    let mut visible = vec![false; run.len()];
    for (at, character) in text[run.clone()].char_indices() {
        // What a superscript cannot hold is no part of whether a selection is superscript.
        let counts = if scripted {
            holds_in_script(character)
        } else {
            !character.is_whitespace()
        };
        visible[at..at + character.len_utf8()].fill(counts);
    }
    let mut clear = |range: &Range<usize>| {
        let start = range.start.clamp(run.start, run.end) - run.start;
        let end = range.end.clamp(run.start, run.end) - run.start;
        visible[start..end].fill(false);
    };
    for span in spans {
        clear(&span.open);
        clear(&span.close);
        if span.kind == SpanKind::Comment {
            clear(&span.range);
        }
    }
    let mut covered = vec![false; visible.len()];
    for span in spans.iter().filter(|span| span.kind == kind) {
        let inside = content(span);
        let start = inside.start.clamp(run.start, run.end) - run.start;
        let end = inside.end.clamp(run.start, run.end) - run.start;
        covered[start..end].fill(true);
    }
    let any = visible
        .iter()
        .zip(&covered)
        .any(|(visible, covered)| *visible && *covered);
    let all = visible
        .iter()
        .zip(&covered)
        .all(|(visible, covered)| !*visible || *covered);
    (any, all)
}

fn state_of(found: &[Reached<'_>], kind: SpanKind) -> MarkState {
    let (mut any, mut all) = (false, true);
    for block in found {
        for run in &block.runs {
            let (some, every) = coverage(block.text, &block.spans, run, kind);
            any |= some;
            all &= every;
        }
    }
    match (any, all) {
        (true, true) => MarkState::On,
        (true, false) => MarkState::Mixed,
        _ => MarkState::Off,
    }
}

/// The selection a command acts on. A caret means the span of the mark it stands in, else the word it touches.
fn target(text: &str, blocks: &Blocks, selection: Selection, kind: SpanKind) -> Range<usize> {
    if !selection.is_empty() {
        return selection.range();
    }
    let caret = selection.head;
    let block = &blocks.as_slice()[blocks.index_at(caret)];
    if has_inline(block.kind) {
        let relative = caret - block.range.start;
        let spans = inline_spans(block.kind, &text[block.range.clone()]);
        let holding = spans
            .iter()
            .rfind(|span| span.kind == kind && span.range.start <= relative && relative <= span.range.end);
        if let Some(span) = holding {
            let inside = content(span);
            return block.range.start + inside.start..block.range.start + inside.end;
        }
    }
    motion::word_range(text, caret)
}

/// What the toolbar shows: each mark's state over the selection.
pub fn states(text: &str, blocks: &Blocks, selection: Selection) -> MarkStates {
    let found = reached(text, blocks, &selection.range());
    let state = |mark: Mark| state_of(&found, mark.syntax().kind);
    MarkStates {
        bold: state(Mark::Bold),
        italic: state(Mark::Italic),
        underline: state(Mark::Underline),
        strike: state(Mark::Strike),
        superscript: state(Mark::Superscript),
        subscript: state(Mark::Subscript),
        small_caps: state(Mark::SmallCaps),
        link: state_of(&found, SpanKind::Link),
    }
}

/// Whether the selection holds anything a mark can go on, which is when the toolbar has something to offer.
pub fn has_markable_text(text: &str, blocks: &Blocks, selection: Selection) -> bool {
    !reached(text, blocks, &selection.range()).is_empty()
}

/// Toggles a mark on the selection. Nothing to do, or nothing valid to write, is `None`.
pub fn toggle(text: &str, blocks: &Blocks, selection: Selection, mark: Mark) -> Option<Command> {
    let syntax = mark.syntax();
    let range = target(text, blocks, selection, syntax.kind);
    let found = reached(text, blocks, &range);
    if found.is_empty() {
        return None;
    }
    let on = state_of(&found, syntax.kind) == MarkState::On;
    command(text, selection, &found, |block| {
        if on {
            unmark(block, syntax)
        } else {
            mark_block(block, syntax)
        }
    })
}

/// Links the selection to `url`, or changes the link it stands in. An address that Markdown cannot hold is `None`.
pub fn set_link(text: &str, blocks: &Blocks, selection: Selection, url: &str) -> Option<Command> {
    let destination = link_destination(url)?;
    let close = format!("]({destination})");
    let syntax = Syntax {
        kind: SpanKind::Link,
        open: "[",
        close: &close,
    };
    let range = target(text, blocks, selection, SpanKind::Link);
    let found = reached(text, blocks, &range);
    if found.is_empty() {
        return None;
    }
    command(text, selection, &found, |block| mark_block(block, syntax))
}

/// Takes the links in the selection off their text.
pub fn remove_link(text: &str, blocks: &Blocks, selection: Selection) -> Option<Command> {
    let range = target(text, blocks, selection, SpanKind::Link);
    let found = reached(text, blocks, &range);
    command(text, selection, &found, |block| {
        let edits: Vec<Edit> = block
            .spans
            .iter()
            .filter(|span| span.kind == SpanKind::Link)
            .filter(|span| block.runs.iter().any(|run| overlaps(run, &span.range)))
            .flat_map(|span| [Edit::new(span.open.clone(), ""), Edit::new(span.close.clone(), "")])
            .collect();
        (!edits.is_empty()).then_some(edits)
    })
}

/// The address of the link the selection stands in.
pub fn link_at(text: &str, blocks: &Blocks, selection: Selection) -> Option<String> {
    let range = target(text, blocks, selection, SpanKind::Link);
    for block in reached(text, blocks, &range) {
        for span in &block.spans {
            if span.kind == SpanKind::Link && block.runs.iter().any(|run| overlaps(run, &span.range)) {
                let inner = block.text[span.close.clone()].strip_prefix("](")?.strip_suffix(')')?;
                let address = inner.strip_prefix('<').map_or_else(
                    || inner.split_whitespace().next().unwrap_or(""),
                    |rest| rest.split('>').next().unwrap_or(""),
                );
                return Some(address.to_owned());
            }
        }
    }
    None
}

/// How an address is written in a link: in angle brackets when it holds spaces or parentheses.
fn link_destination(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.contains(['<', '>', '\n']) {
        return None;
    }
    Some(if url.contains([' ', '(', ')']) {
        format!("<{url}>")
    } else {
        url.to_owned()
    })
}

fn command(
    text: &str,
    selection: Selection,
    found: &[Reached<'_>],
    edits_of: impl Fn(&Reached<'_>) -> Option<Vec<Edit>>,
) -> Option<Command> {
    let mut edits = Vec::new();
    for block in found {
        if let Some(block_edits) = edits_of(block) {
            edits.extend(
                block_edits
                    .into_iter()
                    .map(|edit| Edit::new(edit.range.start + block.start..edit.range.end + block.start, edit.text)),
            );
        }
    }
    let edits = normalized(edits, text);
    if edits.is_empty() {
        return None;
    }
    let selection = selection_after(selection, &edits);
    Some(Command { edits, selection })
}

/// Puts a mark on a block's runs: as selected, or if that does not parse, on whole words. `**(a)**b` is not strong in
/// Markdown, but `**(a)b**` is.
fn mark_block(block: &Reached<'_>, syntax: Syntax<'_>) -> Option<Vec<Edit>> {
    [false, true].into_iter().find_map(|whole_words| {
        let mut edits = Vec::new();
        for run in &block.runs {
            edits.extend(mark_run(block, run, syntax, whole_words));
        }
        let edits = normalized(edits, block.text);
        (!edits.is_empty() && reads_as(block, &edits, syntax.kind, MarkState::On)).then_some(edits)
    })
}

fn mark_run(block: &Reached<'_>, run: &Range<usize>, syntax: Syntax<'_>, whole_words: bool) -> Vec<Edit> {
    let text = block.text;
    let bytes = text.as_bytes();
    let mut range = run.clone();
    if whole_words {
        while range.start > 0 && !is_space(bytes[range.start - 1]) {
            range.start -= 1;
        }
        while range.end < bytes.len() && !is_space(bytes[range.end]) {
            range.end += 1;
        }
    }
    let groups = brace_groups(text);
    loop {
        let before = range.clone();
        for span in &block.spans {
            if span.kind == syntax.kind {
                if overlaps(&range, &span.range) {
                    let inside = content(span);
                    range.start = range.start.min(inside.start);
                    range.end = range.end.max(inside.end);
                }
            } else if overlaps(&range, &span.range) {
                let inside = range.start >= span.open.end && range.end <= span.close.start;
                if is_opaque(span.kind) || !inside {
                    let holds = range.start <= span.range.start && span.range.end <= range.end;
                    if !holds {
                        range.start = range.start.min(span.range.start);
                        range.end = range.end.max(span.range.end);
                    }
                }
            }
        }
        for group in &groups {
            if overlaps(&range, group) && !(range.start <= group.start && group.end <= range.end) {
                range.start = range.start.min(group.start);
                range.end = range.end.max(group.end);
            }
        }
        if range == before {
            break;
        }
    }
    let mut edits: Vec<Edit> = block
        .spans
        .iter()
        .filter(|span| span.kind == syntax.kind && overlaps(&range, &span.range))
        .flat_map(|span| [Edit::new(span.open.clone(), ""), Edit::new(span.close.clone(), "")])
        .collect();
    let scripted = matches!(syntax.kind, SpanKind::Superscript | SpanKind::Subscript);
    let pieces = if scripted {
        script_pieces(text, &range)
    } else {
        vec![range]
    };
    for piece in pieces {
        edits.push(Edit::new(piece.start..piece.start, syntax.open));
        edits.push(Edit::new(piece.end..piece.end, syntax.close));
    }
    edits
}

fn holds_in_script(character: char) -> bool {
    character.is_alphanumeric() || "+-=(),.%°±−".contains(character)
}

/// The stretches of `range` a superscript or subscript can hold, by the engine's rule.
fn script_pieces(text: &str, range: &Range<usize>) -> Vec<Range<usize>> {
    let mut pieces = Vec::new();
    let mut start = None;
    for (at, character) in text[range.clone()].char_indices() {
        let at = range.start + at;
        match (holds_in_script(character), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                pieces.push(from..at);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        pieces.push(from..range.end);
    }
    pieces
}

/// Takes a mark off the runs, keeping it on the rest of each span. If what stays does not parse, the whole span goes.
fn unmark(block: &Reached<'_>, syntax: Syntax<'_>) -> Option<Vec<Edit>> {
    let held: Vec<&InlineSpan> = block
        .spans
        .iter()
        .filter(|span| span.kind == syntax.kind)
        .filter(|span| block.runs.iter().any(|run| overlaps(run, &span.range)))
        .collect();
    if held.is_empty() {
        return None;
    }
    let splits: Vec<Edit> = held.iter().flat_map(|span| split_span(block, span)).collect();
    let splits = normalized(splits, block.text);
    if reads_as(block, &splits, syntax.kind, MarkState::Off) {
        return Some(splits);
    }
    Some(normalized(
        held.iter()
            .flat_map(|span| [Edit::new(span.open.clone(), ""), Edit::new(span.close.clone(), "")])
            .collect(),
        block.text,
    ))
}

/// The edits that leave `span` on its content outside the runs. Whitespace at the cut stays outside the mark.
fn split_span(block: &Reached<'_>, span: &InlineSpan) -> Vec<Edit> {
    let text = block.text;
    let inside = content(span);
    let selected = block
        .runs
        .iter()
        .filter(|run| overlaps(run, &inside))
        .fold(None::<Range<usize>>, |hull, run| {
            let start = run.start.max(inside.start);
            let end = run.end.min(inside.end);
            Some(hull.map_or(start..end, |hull| hull.start.min(start)..hull.end.max(end)))
        })
        .unwrap_or(inside.clone());
    let left = text[inside.start..selected.start].trim_end();
    let right = text[selected.end..inside.end].trim_start();
    let open_written = &text[span.open.clone()];
    let close_written = &text[span.close.clone()];
    // `_` does not pair inside a word, so a span that gets cut is rewritten with `*`.
    let open = open_written.replace('_', "*");
    let close = close_written.replace('_', "*");
    let mut edits = Vec::new();
    if left.is_empty() {
        edits.push(Edit::new(span.open.clone(), ""));
    } else {
        if open != open_written {
            edits.push(Edit::new(span.open.clone(), open.clone()));
        }
        let at = inside.start + left.len();
        edits.push(Edit::new(at..at, close.clone()));
    }
    if right.is_empty() {
        edits.push(Edit::new(span.close.clone(), ""));
    } else {
        let at = inside.end - right.len();
        edits.push(Edit::new(at..at, open));
        if close != close_written {
            edits.push(Edit::new(span.close.clone(), close));
        }
    }
    edits
}

/// Whether a block with `edits` applied is still one block of its kind, keeps every other span, and shows `kind` as
/// asked over its runs.
fn reads_as(block: &Reached<'_>, edits: &[Edit], kind: SpanKind, wanted: MarkState) -> bool {
    let new_text = applied(block.text, edits);
    let parsed = Blocks::parse(&new_text);
    if parsed.as_slice().len() != 1 || parsed.as_slice()[0].kind != block.kind {
        return false;
    }
    let spans = inline_spans(block.kind, &new_text);
    let others = |spans: &[InlineSpan]| {
        let mut kinds: Vec<String> = spans
            .iter()
            .filter(|span| span.kind != kind)
            .map(|span| format!("{:?}", span.kind))
            .collect();
        kinds.sort();
        kinds
    };
    if others(&block.spans) != others(&spans) {
        return false;
    }
    let runs: Vec<Range<usize>> = block
        .runs
        .iter()
        .map(|run| moved(run.start, edits, Bias::After)..moved(run.end, edits, Bias::Before))
        .collect();
    let (mut any, mut all) = (false, true);
    for run in &runs {
        if run.start >= run.end {
            continue;
        }
        let (some, every) = coverage(&new_text, &spans, run, kind);
        any |= some;
        all &= every;
    }
    match wanted {
        MarkState::On => any && all,
        _ => !any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `text`, where `|` marks the selection's two ends, with `mark` toggled, and `|` where the selection is after.
    fn toggled(text: &str, mark: Mark) -> String {
        run(text, |source, blocks, selection| {
            toggle(source, blocks, selection, mark)
        })
    }

    fn run(text: &str, command: impl Fn(&str, &Blocks, Selection) -> Option<Command>) -> String {
        let (source, selection) = parts(text);
        let blocks = Blocks::parse(&source);
        let Some(command) = command(&source, &blocks, selection) else {
            return format!("no command for {text}");
        };
        let mut result = applied(&source, &command.edits);
        let range = command.selection.range();
        result.insert(range.end, '|');
        if !range.is_empty() {
            result.insert(range.start, '|');
        }
        result
    }

    fn parts(text: &str) -> (String, Selection) {
        let first = text.find('|').expect("a selection");
        let second = text[first + 1..].find('|').map(|at| first + at);
        let source = text.replace('|', "");
        let selection = Selection {
            anchor: first,
            head: second.unwrap_or(first),
        };
        (source, selection)
    }

    fn state(text: &str, mark: Mark) -> MarkState {
        let (source, selection) = parts(text);
        states(&source, &Blocks::parse(&source), selection).of(mark)
    }

    #[test]
    fn a_mark_goes_around_the_selection_without_its_spaces() {
        assert_eq!(toggled("a |b c| d", Mark::Bold), "a **|b c|** d");
        assert_eq!(toggled("a| b c |d", Mark::Italic), "a| *b c* |d");
        assert_eq!(toggled("a |b c| d", Mark::Underline), "a [|b c|]{.underline} d");
        assert_eq!(toggled("a |b c| d", Mark::SmallCaps), "a [|b c|]{.smallcaps} d");
        assert_eq!(toggled("a |b c| d", Mark::Strike), "a ~~|b c|~~ d");
    }

    #[test]
    fn selecting_inside_marks_and_removing_them_leaves_clean_text() {
        assert_eq!(toggled("a **|b c|** d", Mark::Bold), "a |b c| d");
        assert_eq!(toggled("a *|b c|* d", Mark::Italic), "a |b c| d");
        assert_eq!(toggled("a _|b c|_ d", Mark::Italic), "a |b c| d");
        assert_eq!(toggled("a [|b c|]{.underline} d", Mark::Underline), "a |b c| d");
        assert_eq!(toggled("a [|b c|]{.smallcaps} d", Mark::SmallCaps), "a |b c| d");
        assert_eq!(toggled("a ~~|b c|~~ d", Mark::Strike), "a |b c| d");
        assert_eq!(toggled("x^|2|^", Mark::Superscript), "x|2|");
        assert_eq!(toggled("H~|2|~O", Mark::Subscript), "H|2|O");
    }

    #[test]
    fn a_selection_over_the_markers_counts_as_the_text_inside_them() {
        assert_eq!(toggled("a |**b**| c", Mark::Bold), "a |b| c");
    }

    #[test]
    fn removing_from_part_of_a_span_leaves_the_rest_marked() {
        assert_eq!(toggled("**abc |def| ghi**", Mark::Bold), "**abc** |def| **ghi**");
        assert_eq!(toggled("**|abc| def**", Mark::Bold), "|abc| **def**");
        assert_eq!(toggled("**abc |def|**", Mark::Bold), "**abc** |def|");
        assert_eq!(toggled("__ab|cd|ef__", Mark::Bold), "**ab**|cd|**ef**");
        assert_eq!(
            toggled("[ab |cd| ef]{.underline}", Mark::Underline),
            "[ab]{.underline} |cd| [ef]{.underline}"
        );
    }

    #[test]
    fn nested_marks_come_off_one_at_a_time() {
        assert_eq!(toggled("***|x|***", Mark::Bold), "*|x|*");
        assert_eq!(toggled("***|x|***", Mark::Italic), "**|x|**");
        assert_eq!(toggled("**|x|**", Mark::Italic), "***|x|***");
        assert_eq!(toggled("~~[|x|]{.underline}~~", Mark::Strike), "[|x|]{.underline}");
    }

    #[test]
    fn a_mixed_selection_is_marked_whole_and_then_unmarked_whole() {
        assert_eq!(state("**|ab** cd| e", Mark::Bold), MarkState::Mixed);
        assert_eq!(toggled("**|ab** cd| e", Mark::Bold), "**|ab cd|** e");
        assert_eq!(state("**|ab cd|** e", Mark::Bold), MarkState::On);
        assert_eq!(toggled("**a|b** c|d", Mark::Bold), "**a|b c|**d");
        assert_eq!(toggled("|a **b** c|", Mark::Bold), "**|a b c|**");
        assert_eq!(toggled("|a **b** *c* d|", Mark::Bold), "**|a b *c* d|**");
    }

    #[test]
    fn a_selection_grows_to_hold_a_span_it_would_cross() {
        assert_eq!(toggled("*a|b* c|d", Mark::Bold), "***a|b* c|**d");
        assert_eq!(toggled("[a|b](u) c|d", Mark::Italic), "*[a|b](u) c|*d");
    }

    #[test]
    fn code_references_and_citations_stay_whole() {
        assert_eq!(toggled("a `c|o|de` b", Mark::Bold), "a **`c|o|de`** b");
        assert_eq!(toggled("a [^|n] b| c", Mark::Bold), "a **[^|n] b|** c");
        assert_eq!(toggled("a |b {wor|ds} c", Mark::Bold), "a **|b {wor|ds}** c");
    }

    #[test]
    fn superscripts_mark_each_word_and_skip_what_they_cannot_hold() {
        assert_eq!(toggled("|ab cd|", Mark::Superscript), "^|ab^ ^cd|^");
        assert_eq!(toggled("|ab!cd|", Mark::Subscript), "~|ab~!~cd|~");
        assert_eq!(state("^|ab^ ^cd|^", Mark::Superscript), MarkState::On);
        assert_eq!(toggled("~|ab~!~cd|~", Mark::Subscript), "|ab!cd|");
    }

    #[test]
    fn each_list_item_and_quote_line_is_marked_on_its_own() {
        assert_eq!(toggled("- |a\n- b|\n", Mark::Bold), "- **|a**\n- **b|**\n");
        assert_eq!(toggled("> |a\n> b|\n", Mark::Italic), "> *|a*\n> *b|*\n");
        assert_eq!(toggled("- **|a**\n- **b|**\n", Mark::Bold), "- |a\n- b|\n");
    }

    #[test]
    fn a_selection_across_blocks_is_one_command_that_skips_raw_blocks() {
        assert_eq!(
            toggled("|a\n\n```\nb\n```\n\nc|\n", Mark::Bold),
            "**|a**\n\n```\nb\n```\n\n**c|**\n"
        );
        assert_eq!(toggled("# |a\n\nb|\n", Mark::Bold), "# **|a**\n\n**b|**\n");
    }

    #[test]
    fn markers_at_the_edges_of_a_block_stay_out_of_the_mark() {
        assert_eq!(toggled("|# Title #|\n", Mark::Italic), "|# *Title* #|\n");
        assert_eq!(toggled("|# Title {#sec:a}|\n", Mark::Italic), "|# *Title* {#sec:a}|\n");
        assert_eq!(toggled("|Text {.note}|\n", Mark::Bold), "**|Text** {.note}|\n");
        assert_eq!(toggled("|Title\n===|\n", Mark::Italic), "*|Title*\n===|\n");
        assert_eq!(toggled("|[^n]: Note|\n", Mark::Bold), "|[^n]: **Note|**\n");
    }

    #[test]
    fn a_caret_marks_the_word_it_touches_and_unmarks_the_span_it_stands_in() {
        assert_eq!(toggled("a wo|rd b", Mark::Bold), "a **wo|rd** b");
        assert_eq!(toggled("a **wo|rd** b", Mark::Bold), "a wo|rd b");
        assert_eq!(toggled("a  |  b", Mark::Bold), "no command for a  |  b");
    }

    #[test]
    fn what_cannot_be_marked_writes_nothing() {
        assert_eq!(
            toggled("```\n|code|\n```\n", Mark::Bold),
            "no command for ```\n|code|\n```\n"
        );
        assert_eq!(
            toggled("|![Cap](a.png)|\n", Mark::Bold),
            "no command for |![Cap](a.png)|\n"
        );
        assert_eq!(toggled("|<!-- c -->|\n", Mark::Bold), "no command for |<!-- c -->|\n");
    }

    #[test]
    fn a_mark_that_would_not_parse_grows_to_whole_words() {
        assert_eq!(toggled("a|(b)|c d", Mark::Bold), "**a|(b)|c** d");
    }

    #[test]
    fn links_wrap_change_and_come_off() {
        let link = |text: &str, url: &str| {
            let url = url.to_owned();
            run(text, move |source, blocks, selection| {
                set_link(source, blocks, selection, &url)
            })
        };
        assert_eq!(link("a |b c| d", "https://x"), "a [|b c|](https://x) d");
        assert_eq!(link("[|b|](old) d", "new"), "[|b|](new) d");
        assert_eq!(link("a |b| d", "my file.md"), "a [|b|](<my file.md>) d");
        assert_eq!(run("a [b|c](u \"T\") d", remove_link), "a b|c d");
        let (source, selection) = parts("[b|c](http://u \"T\")");
        assert_eq!(
            link_at(&source, &Blocks::parse(&source), selection).as_deref(),
            Some("http://u")
        );
        let (source, selection) = parts("a |b| c");
        assert_eq!(link_at(&source, &Blocks::parse(&source), selection), None);
        assert_eq!(link("a |b|", "<bad>"), "no command for a |b|");
    }
}
