//! Block segmentation: the text as an exact partition of typed byte ranges.
//!
//! The scan works line by line and looks only forward from a block's first line, so every block is a pure function of
//! the text from its start. That gives incremental updates: rescan from just before the edit until a block boundary
//! lands on an old boundary past the edit, and everything after it is unchanged.
//!
//! It follows the engine's reading of Markdown (`src/markdown.rs`) for where blocks start: CommonMark with GFM tables
//! and footnotes, and `:::` directive lines at column 0 outside code. A `:::` line is a block of its own. What it
//! encloses is ordinary Markdown, so its contents segment as usual.
//!
//! The engine cuts comments out before it parses, so a comment is its own block only when nothing else stands on its
//! lines. `<!-- c --> text` is a paragraph, and a comment holding a blank line inside a paragraph does not end it.

use std::ops::Range;

use super::buffer::Splice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    FrontMatter,
    Heading {
        level: u8,
    },
    Paragraph,
    List,
    Quote,
    Code,
    Table,
    Directive,
    Comment,
    SceneBreak,
    FootnoteDefinition,
    /// Anything this layer does not understand, kept opaque.
    Raw,
    /// One or more blank lines between blocks.
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// Includes the line ending of the block's last line.
    pub range: Range<usize>,
}

/// The blocks a change replaced: `removed` old ones at `start` became `inserted` new ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChange {
    pub start: usize,
    pub removed: usize,
    pub inserted: usize,
}

/// The partition. There is always at least one block, an empty blank one for empty text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocks {
    list: Vec<Block>,
}

impl Blocks {
    pub fn parse(text: &str) -> Self {
        let mut list = Vec::new();
        let mut pos = 0;
        while pos < text.len() {
            let block = scan(text, pos);
            pos = block.range.end;
            list.push(block);
        }
        if list.is_empty() {
            list.push(Block {
                kind: BlockKind::Blank,
                range: 0..0,
            });
        }
        Self { list }
    }

    pub fn as_slice(&self) -> &[Block] {
        &self.list
    }

    /// The index of the block holding `offset`. The end of the text belongs to the last block.
    pub fn index_at(&self, offset: usize) -> usize {
        self.list
            .partition_point(|block| block.range.start <= offset)
            .saturating_sub(1)
    }

    /// Brings the partition up to date with `text`, which is the old text with `splice` applied.
    pub fn apply(&mut self, text: &str, splice: Splice) -> BlockChange {
        let delta = splice.new_end as isize - splice.old_end as isize;
        // A block's extent can depend on the first line after the blank lines that follow it, so the block two before
        // the edit may change too. Nothing further back can.
        // A comment still open before the edit can be closed by it, which changes the block the comment started in.
        let start = open_comment_before(text, splice.start).unwrap_or(splice.start);
        let start = self.index_at(start).saturating_sub(2);
        // Front matter still open can be closed by a line far below it, which moves where the first block ends.
        let first = &self.list[0];
        let settings_moved = first.kind == BlockKind::FrontMatter
            && splice.start >= first.range.end
            && front_matter_end(text) != Some(first.range.end);
        let start = if settings_moved { 0 } else { start };
        let mut pos = self.list[start].range.start;
        let mut next = start;
        let mut scanned = Vec::new();
        while pos < text.len() {
            if pos >= splice.new_end
                && let Some(old) = self.old_boundary(&mut next, pos, splice.old_end, delta)
            {
                next = old;
                return self.finish(start, next, scanned, delta);
            }
            let block = scan(text, pos);
            pos = block.range.end;
            scanned.push(block);
        }
        if scanned.is_empty() {
            scanned.push(Block {
                kind: BlockKind::Blank,
                range: pos..pos,
            });
        }
        let end = self.list.len();
        self.finish(start, end, scanned, delta)
    }

    /// The first old block past the edit that starts at the new offset `pos`, moving `next` forward as it searches.
    fn old_boundary(&self, next: &mut usize, pos: usize, old_end: usize, delta: isize) -> Option<usize> {
        while let Some(block) = self.list.get(*next) {
            let moved = block.range.start.wrapping_add_signed(delta);
            if block.range.start >= old_end && moved >= pos {
                return (moved == pos).then_some(*next);
            }
            *next += 1;
        }
        None
    }

    fn finish(&mut self, start: usize, end: usize, scanned: Vec<Block>, delta: isize) -> BlockChange {
        let inserted = scanned.len();
        for block in &mut self.list[end..] {
            block.range.start = block.range.start.wrapping_add_signed(delta);
            block.range.end = block.range.end.wrapping_add_signed(delta);
        }
        self.list.splice(start..end, scanned);
        BlockChange {
            start,
            removed: end - start,
            inserted,
        }
    }
}

/// What `text` needs at its end so that `next`, the text after it in the same file, still starts a block of its own:
/// nothing, a line break when `text` ends inside a line, or a blank line when its last block would take in `next`'s
/// first lines, as a paragraph takes a setext heading's words or a lazy continuation line. `text` starts a block, and
/// the file when `starts_file`, where front matter can open. `blocks` are `text`'s own, so only its last block is read
/// again. When no break keeps them apart, as after an unclosed code fence, it needs only what ends its last line. The
/// line break is `next`'s own, `\r\n` or `\n`.
pub fn break_before(text: &str, blocks: &Blocks, next: &str, starts_file: bool) -> &'static str {
    break_after_last(text, next, starts_file, || match blocks.list.first() {
        // Front matter opens only at the start of the file, so a text that does not start it but whose own blocks
        // open with some is read again whole, behind a blank line as the file has it.
        Some(first) if first.kind == BlockKind::FrontMatter && !starts_file => last_block_start(text, false),
        _ => (blocks.list.last()).map_or(0, |block| block.range.start),
    })
}

/// Where the last block of `text` starts, read whole, as in a file that `text` starts when `starts_file`.
fn last_block_start(text: &str, starts_file: bool) -> usize {
    let lead = if starts_file { "" } else { "\n" };
    let parsed = Blocks::parse(&format!("{lead}{text}"));
    (parsed.list.last()).map_or(0, |block| block.range.start.saturating_sub(lead.len()))
}

/// Where `text`'s first lines end: the start of its first blank line, with or without spaces and `\r`, or its end.
pub fn first_lines_end(text: &str) -> usize {
    skip_nonblank(text, 0)
}

/// [`break_before`] with `last` giving where `text`'s last block starts, asked only when the break is not plain.
fn break_after_last(text: &str, next: &str, starts_file: bool, last: impl FnOnce() -> usize) -> &'static str {
    if text.is_empty() || next.is_empty() {
        return "";
    }
    let open = !text.ends_with('\n');
    let last_line = text.trim_end_matches('\n').rsplit('\n').next().unwrap_or("");
    if !open && (text.ends_with("\n\n") || is_blank(last_line.strip_suffix('\r').unwrap_or(last_line))) {
        return "";
    }
    let (first, _) = line_at(next, 0);
    let crlf = next[first.len()..].starts_with("\r\n");
    let gaps: [&'static str; 2] = match (open, crlf) {
        (true, false) => ["\n", "\n\n"],
        (true, true) => ["\r\n", "\r\n\r\n"],
        (false, false) => ["", "\n"],
        (false, true) => ["", "\r\n"],
    };
    // A blank line above keeps a text that does not start the file from opening front matter.
    let lead = |from: usize| if starts_file && from == 0 { "" } else { "\n" };
    // Blocks before the last one ended inside `text`, and a block depends only on the text from its start, so only the
    // last one can take anything in.
    let last = last();
    // A blank line ends every block but those no break ends, so the first lines up to one are all it could take in.
    let head = &next[..first_lines_end(next)];
    let apart = |gap: &str| {
        let joined = format!("{}{}{gap}{head}", lead(last), &text[last..]);
        let boundary = joined.len() - head.len();
        (Blocks::parse(&joined).list.iter()).any(|block| block.range.start == boundary)
    };
    gaps.into_iter().find(|gap| apart(gap)).unwrap_or(gaps[0])
}

/// One line: its text without the line ending, and where the next line starts.
fn line_at(text: &str, pos: usize) -> (&str, usize) {
    let rest = &text[pos..];
    match rest.find('\n') {
        Some(at) => (rest[..at].strip_suffix('\r').unwrap_or(&rest[..at]), pos + at + 1),
        None => (rest, text.len()),
    }
}

/// The block that starts at `pos`, which is the start of a line.
fn scan(text: &str, pos: usize) -> Block {
    let (line, next) = line_at(text, pos);
    let block = |kind, end| Block { kind, range: pos..end };
    if pos == 0
        && let Some(end) = front_matter_end(text)
    {
        return block(BlockKind::FrontMatter, end);
    }
    if is_blank(line) {
        return block(BlockKind::Blank, skip_blank(text, next));
    }
    if line.starts_with(":::") {
        return block(BlockKind::Directive, next);
    }
    if indent(line) >= 4 {
        return block(BlockKind::Code, indented_code_end(text, next));
    }
    if let Some(fence) = fence_open(line) {
        return block(BlockKind::Code, fence_end(text, next, fence));
    }
    if is_comment_start(line) {
        return match whole_comment_end(text, pos) {
            Some(end) => block(BlockKind::Comment, end),
            None => {
                let (kind, end) = paragraph_end(text, past_comments(text, pos, next));
                block(kind, end)
            }
        };
    }
    if let Some(level) = atx_level(line) {
        return block(BlockKind::Heading { level }, next);
    }
    if is_thematic_break(line) {
        return block(BlockKind::SceneBreak, next);
    }
    if is_quote(line) {
        return block(BlockKind::Quote, quote_end(text, next));
    }
    if let Some(item) = list_item(line) {
        return block(BlockKind::List, list_end(text, past_comments(text, pos, next), item));
    }
    if is_footnote_definition(line) {
        return block(
            BlockKind::FootnoteDefinition,
            footnote_end(text, past_comments(text, pos, next)),
        );
    }
    if is_html_start(line) {
        return block(BlockKind::Raw, skip_nonblank(text, next));
    }
    if is_link_definition(line) {
        return block(BlockKind::Raw, next);
    }
    if is_table_start(text, line, next) {
        return block(BlockKind::Table, table_end(text, next));
    }
    let (kind, end) = paragraph_end(text, past_comments(text, pos, next));
    block(kind, end)
}

fn is_blank(line: &str) -> bool {
    line.bytes().all(|byte| byte == b' ' || byte == b'\t')
}

/// Leading spaces, with a tab advancing to the next multiple of four.
fn indent(line: &str) -> usize {
    let mut width = 0;
    for byte in line.bytes() {
        match byte {
            b' ' => width += 1,
            b'\t' => width += 4 - width % 4,
            _ => break,
        }
    }
    width
}

fn skip_blank(text: &str, mut pos: usize) -> usize {
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if !is_blank(line) {
            break;
        }
        pos = next;
    }
    pos
}

fn skip_nonblank(text: &str, mut pos: usize) -> usize {
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if is_blank(line) {
            break;
        }
        pos = next;
    }
    pos
}

/// Whether a front matter block ends in its closing line.
pub fn front_matter_closes(block: &str) -> bool {
    block
        .lines()
        .skip(1)
        .last()
        .is_some_and(|line| line == "---" || line == "...")
}

/// The end of front matter opening the text, by the engine's rule. Unclosed front matter ends at the first blank line, so
/// a stray `---` on the first line does not swallow the file. The engine reports it.
fn front_matter_end(text: &str) -> Option<usize> {
    let bom = if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let (first, mut pos) = line_at(text, bom);
    if first != "---" || !text[bom..pos].ends_with('\n') {
        return None;
    }
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if line == "---" || line == "..." {
            return Some(next);
        }
        pos = next;
    }
    let (_, mut pos) = line_at(text, bom);
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if is_blank(line) {
            return Some(pos);
        }
        pos = next;
    }
    Some(text.len())
}

fn indented_code_end(text: &str, mut pos: usize) -> usize {
    let mut end = pos;
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if !is_blank(line) {
            if indent(line) < 4 {
                break;
            }
            end = next;
        }
        pos = next;
    }
    end
}

#[derive(Clone, Copy)]
struct Fence {
    marker: u8,
    len: usize,
}

fn fence_open(line: &str) -> Option<Fence> {
    if indent(line) > 3 {
        return None;
    }
    let content = line.trim_start();
    let marker = *content.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let len = content.bytes().take_while(|byte| *byte == marker).count();
    if len < 3 || (marker == b'`' && content[len..].contains('`')) {
        return None;
    }
    Some(Fence { marker, len })
}

fn fence_end(text: &str, mut pos: usize, fence: Fence) -> usize {
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if closes_fence(line, fence.marker, fence.len) {
            return next;
        }
        pos = next;
    }
    text.len()
}

/// Whether `line` closes a code fence opened with `len` of `marker`, as CommonMark and so the engine read it: at most
/// three columns of indent, at least as many of the marker, then nothing but spaces and tabs. The block list and the
/// editor's fence lines both ask this, so they never disagree on where a code block ends.
pub fn closes_fence(line: &str, marker: u8, len: usize) -> bool {
    let line = line.trim_end_matches(['\n', '\r']);
    if indent(line) > 3 {
        return false;
    }
    let content = line.trim_start_matches(' ');
    let run = content.bytes().take_while(|byte| *byte == marker).count();
    run >= len && content.bytes().skip(run).all(|byte| matches!(byte, b' ' | b'\t'))
}

fn is_comment_start(line: &str) -> bool {
    indent(line) <= 3 && line.trim_start().starts_with("<!--")
}

/// The end of the comment block that starts on the line at `pos`, if nothing but comments stands on its lines. A comment
/// that is never closed runs to the end of the text, so that closing it later is an edit inside this block.
fn whole_comment_end(text: &str, pos: usize) -> Option<usize> {
    let mut at = pos + (text[pos..].len() - text[pos..].trim_start_matches([' ', '\t']).len());
    loop {
        let open = text[at..].strip_prefix("<!--")?;
        let body = at + "<!--".len();
        let Some(close) = open.find("-->") else {
            return Some(text.len());
        };
        let end = body + close + "-->".len();
        let (rest, next) = line_at(text, end);
        if is_blank(rest) {
            return Some(next);
        }
        at = end + (rest.len() - rest.trim_start_matches([' ', '\t']).len());
    }
}

/// `next`, the start of the line after the one at `pos`, moved past a comment that this line opens and a later line
/// closes. Lines inside the comment, blank ones too, are not lines of the block. A comment never closed moves nothing.
fn past_comments(text: &str, pos: usize, next: usize) -> usize {
    let mut from = pos;
    let mut line_end = next;
    while let Some(open) = open_comment_on_line(&text[from..line_end]) {
        let body = from + open + "<!--".len();
        let Some(close) = text[body..].find("-->") else {
            return line_end;
        };
        from = body + close + "-->".len();
        line_end = line_at(text, from).1;
    }
    line_end
}

/// Where a comment opens in `line` that does not close on it, outside inline code and escapes.
fn open_comment_on_line(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut at = 0;
    let mut code: Option<usize> = None;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' if code.is_none() => at += 2,
            b'`' => {
                let run = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
                match code {
                    Some(open) if open == run => code = None,
                    None => code = Some(run),
                    Some(_) => {}
                }
                at += run;
            }
            b'<' if code.is_none() && line[at..].starts_with("<!--") => {
                let body = at + "<!--".len();
                match line[body..].find("-->") {
                    Some(close) => at = body + close + "-->".len(),
                    None => return Some(at),
                }
            }
            _ => at += 1,
        }
    }
    None
}

/// The start of the comment still open at `offset`: the first `<!--` after the last `-->` before it.
fn open_comment_before(text: &str, offset: usize) -> Option<usize> {
    let before = &text[..offset];
    let from = before.rfind("-->").map_or(0, |close| close + "-->".len());
    before[from..].find("<!--").map(|open| from + open)
}

fn atx_level(line: &str) -> Option<u8> {
    if indent(line) > 3 {
        return None;
    }
    let content = line.trim_start();
    let level = content.bytes().take_while(|byte| *byte == b'#').count();
    let after = content.as_bytes().get(level);
    ((1..=6).contains(&level) && matches!(after, None | Some(b' ' | b'\t'))).then_some(level as u8)
}

fn is_thematic_break(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let mut marks = line.bytes().filter(|byte| *byte != b' ' && *byte != b'\t');
    let Some(first @ (b'*' | b'-' | b'_')) = marks.next() else {
        return false;
    };
    let mut count = 1;
    for mark in marks {
        if mark != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

/// The level of a setext underline, in which `=` makes 1 and `-` makes 2.
fn setext_level(line: &str) -> Option<u8> {
    if indent(line) > 3 {
        return None;
    }
    let content = line.trim();
    let level = match content.as_bytes().first()? {
        b'=' => 1,
        b'-' => 2,
        _ => return None,
    };
    content
        .bytes()
        .all(|byte| byte == content.as_bytes()[0])
        .then_some(level)
}

fn is_quote(line: &str) -> bool {
    indent(line) <= 3 && line.trim_start().starts_with('>')
}

fn quote_end(text: &str, mut pos: usize) -> usize {
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if is_blank(line) || (!is_quote(line) && interrupts_paragraph(text, pos, line, next)) {
            break;
        }
        pos = past_comments(text, pos, next);
    }
    pos
}

/// A list item's first line.
#[derive(Clone, Copy)]
struct ListItem {
    /// The bullet character, or the delimiter after a number. A different one starts a new list.
    delimiter: u8,
    start: Option<u64>,
    content_indent: usize,
    empty: bool,
}

fn list_item(line: &str) -> Option<ListItem> {
    let spaces = indent(line);
    if is_thematic_break(line) {
        return None;
    }
    let content = line.trim_start();
    let bytes = content.as_bytes();
    let (delimiter, start, marker) = match bytes.first()? {
        byte @ (b'-' | b'+' | b'*') => (*byte, None, 1),
        b'0'..=b'9' => {
            let digits = bytes.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let delimiter = *bytes.get(digits)?;
            if digits > 9 || (delimiter != b'.' && delimiter != b')') {
                return None;
            }
            (delimiter, content[..digits].parse().ok(), digits + 1)
        }
        _ => return None,
    };
    let rest = &content[marker..];
    let empty = is_blank(rest);
    if !empty && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let gap = indent(rest);
    let content_indent = spaces + marker + if empty || gap > 4 { 1 } else { gap };
    Some(ListItem {
        delimiter,
        start,
        content_indent,
        empty,
    })
}

fn list_end(text: &str, mut pos: usize, first: ListItem) -> usize {
    let mut content_indent = first.content_indent;
    let mut outer_indent = 0;
    let mut in_fence: Option<Fence> = None;
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if is_blank(line) {
            let after = skip_blank(text, next);
            if after >= text.len() {
                return pos;
            }
            let (following, _) = line_at(text, after);
            let continues = indent(following) >= content_indent
                || (indent(following) < content_indent
                    && list_item(following).is_some_and(|item| item.delimiter == first.delimiter));
            if !continues || following.starts_with(":::") {
                return pos;
            }
            pos = after;
            continue;
        }
        if line.starts_with(":::") {
            return pos;
        }
        let spaces = indent(line);
        if let Some(fence) = in_fence {
            let content = line.trim();
            if content.len() >= fence.len && content.bytes().all(|byte| byte == fence.marker) {
                in_fence = None;
            }
            pos = next;
            continue;
        }
        if spaces >= content_indent {
            in_fence = fence_open(&line[line.len() - line.trim_start().len()..]);
            pos = next;
            continue;
        }
        if let Some(item) = list_item(line) {
            if spaces <= outer_indent + 1 && item.delimiter != first.delimiter {
                return pos;
            }
            outer_indent = outer_indent.min(spaces);
            content_indent = item.content_indent;
            pos = next;
            continue;
        }
        if interrupts_paragraph(text, pos, line, next) {
            return pos;
        }
        pos = past_comments(text, pos, next);
    }
    pos
}

fn is_footnote_definition(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let content = line.trim_start();
    content.strip_prefix("[^").is_some_and(|rest| {
        rest.find(']')
            .is_some_and(|close| close > 0 && rest[close + 1..].starts_with(':'))
    })
}

fn footnote_end(text: &str, mut pos: usize) -> usize {
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if is_blank(line) {
            let after = skip_blank(text, next);
            if after >= text.len() || indent(line_at(text, after).0) < 4 {
                return pos;
            }
            pos = after;
            continue;
        }
        if indent(line) < 4 && (is_footnote_definition(line) || interrupts_paragraph(text, pos, line, next)) {
            return pos;
        }
        pos = past_comments(text, pos, next);
    }
    pos
}

fn is_html_start(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let content = line.trim_start();
    let Some(rest) = content.strip_prefix('<') else {
        return false;
    };
    if rest.starts_with(['!', '?']) {
        return true;
    }
    let name = rest.strip_prefix('/').unwrap_or(rest);
    let len = name
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        .count();
    if len == 0 || !name.as_bytes()[0].is_ascii_alphabetic() {
        return false;
    }
    name[len..].is_empty() || name[len..].starts_with([' ', '\t', '>', '/'])
}

/// `[label]: destination`. It defines a link and prints nothing, so it stays raw.
fn is_link_definition(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let content = line.trim_start();
    content.strip_prefix('[').is_some_and(|rest| {
        rest.find(']')
            .is_some_and(|close| close > 0 && rest[close + 1..].starts_with(':') && !is_blank(&rest[close + 2..]))
    })
}

fn is_table_start(text: &str, line: &str, next: usize) -> bool {
    if !line.contains('|') || next >= text.len() {
        return false;
    }
    let (delimiter, _) = line_at(text, next);
    is_delimiter_row(delimiter) && cell_count(line) == cell_count(delimiter)
}

fn is_delimiter_row(line: &str) -> bool {
    let content = line.trim();
    let has_dash = content.contains('-');
    let allowed = content
        .bytes()
        .all(|byte| matches!(byte, b'-' | b':' | b'|' | b' ' | b'\t'));
    let has_pipe_or_is_single = content.contains('|') || !content.is_empty();
    indent(line) <= 3 && has_dash && allowed && has_pipe_or_is_single
}

fn cell_count(line: &str) -> usize {
    let content = line.trim();
    let content = content.strip_prefix('|').unwrap_or(content);
    let content = content
        .strip_suffix('|')
        .filter(|rest| !rest.ends_with('\\'))
        .unwrap_or(content);
    let mut count = 1;
    let mut escaped = false;
    for byte in content.bytes() {
        match byte {
            b'\\' if !escaped => {
                escaped = true;
                continue;
            }
            b'|' if !escaped => count += 1,
            _ => {}
        }
        escaped = false;
    }
    count
}

fn table_end(text: &str, pos: usize) -> usize {
    let (_, mut pos) = line_at(text, pos);
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if is_blank(line) || interrupts_paragraph(text, pos, line, next) {
            break;
        }
        pos = next;
    }
    pos
}

/// Whether a line ends the paragraph above it rather than continuing it.
fn interrupts_paragraph(text: &str, pos: usize, line: &str, next: usize) -> bool {
    line.starts_with(":::")
        || (is_comment_start(line) && whole_comment_end(text, pos).is_some())
        || atx_level(line).is_some()
        || fence_open(line).is_some()
        || is_thematic_break(line)
        || is_quote(line)
        || is_footnote_definition(line)
        || is_table_start(text, line, next)
        // Only a bullet or the number 1 with content can interrupt a paragraph.
        || list_item(line).is_some_and(|item| !item.empty && item.start.is_none_or(|start| start == 1))
}

/// The end of the paragraph whose first line is already read, or of the heading it turns into at a setext underline.
fn paragraph_end(text: &str, mut pos: usize) -> (BlockKind, usize) {
    while pos < text.len() {
        let (line, next) = line_at(text, pos);
        if is_blank(line) {
            break;
        }
        if let Some(level) = setext_level(line) {
            return (BlockKind::Heading { level }, next);
        }
        if interrupts_paragraph(text, pos, line, next) {
            break;
        }
        pos = past_comments(text, pos, next);
    }
    (BlockKind::Paragraph, pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::buffer::{Buffer, Edit, EditKind, Selection};
    use std::time::Instant;

    fn kinds(text: &str) -> Vec<(BlockKind, &str)> {
        Blocks::parse(text)
            .as_slice()
            .iter()
            .map(|block| (block.kind, &text[block.range.clone()]))
            .collect()
    }

    #[test]
    fn segments_every_kind_of_block() {
        use BlockKind::*;
        let text = "---\ntitle: A\n---\n# One\n\nA *para*\nstill it\n\n- a\n- b\n\n  more\n\n> q\nlazy\n\n```rust\nx\n\ny\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n::: keep\ninside\n:::\n\n<!-- note\n\nstill note -->\n\n***\n\n[^n]: Note\n\n    second\n\n<div>\nraw\n</div>\n\n[a]: http://x\n\nTitle\n===\n";
        assert_eq!(
            kinds(text),
            [
                (FrontMatter, "---\ntitle: A\n---\n"),
                (Heading { level: 1 }, "# One\n"),
                (Blank, "\n"),
                (Paragraph, "A *para*\nstill it\n"),
                (Blank, "\n"),
                (List, "- a\n- b\n\n  more\n"),
                (Blank, "\n"),
                (Quote, "> q\nlazy\n"),
                (Blank, "\n"),
                (Code, "```rust\nx\n\ny\n```\n"),
                (Blank, "\n"),
                (Table, "| a | b |\n|---|---|\n| 1 | 2 |\n"),
                (Blank, "\n"),
                (Directive, "::: keep\n"),
                (Paragraph, "inside\n"),
                (Directive, ":::\n"),
                (Blank, "\n"),
                (Comment, "<!-- note\n\nstill note -->\n"),
                (Blank, "\n"),
                (SceneBreak, "***\n"),
                (Blank, "\n"),
                (FootnoteDefinition, "[^n]: Note\n\n    second\n"),
                (Blank, "\n"),
                (Raw, "<div>\nraw\n</div>\n"),
                (Blank, "\n"),
                (Raw, "[a]: http://x\n"),
                (Blank, "\n"),
                (Heading { level: 1 }, "Title\n===\n"),
            ]
        );
    }

    #[test]
    fn closing_front_matter_far_below_and_pasting_into_it_keep_the_blocks_right() {
        let mut text = "---\na: b\n\nBody\n".to_owned();
        let mut blocks = Blocks::parse(&text);
        let at = text.len();
        text.push_str("---\n");
        blocks.apply(
            &text,
            Splice {
                start: at,
                old_end: at,
                new_end: text.len(),
            },
        );
        assert_eq!(blocks.as_slice(), Blocks::parse(&text).as_slice());
        // Bytes pasted into closed front matter move its end into the middle of a character of the old text.
        let mut text = "---\nx: a\n---\n\nText\n".to_owned();
        let mut blocks = Blocks::parse(&text);
        let at = "---\nx: a".len();
        text.insert_str(at, "😀😀");
        blocks.apply(
            &text,
            Splice {
                start: at,
                old_end: at,
                new_end: at + "😀😀".len(),
            },
        );
        assert_eq!(blocks.as_slice(), Blocks::parse(&text).as_slice());
    }

    #[test]
    fn the_blocks_partition_the_text_exactly() {
        for text in ["", "\n", "a", "a\n\n\n", "\n\n# a", "a\r\n\r\nb\r\n"] {
            let blocks = Blocks::parse(text);
            let mut end = 0;
            for block in blocks.as_slice() {
                assert_eq!(block.range.start, end, "{text:?}");
                end = block.range.end;
            }
            assert_eq!(end, text.len());
        }
    }

    #[test]
    fn unclosed_constructs_run_to_the_end() {
        use BlockKind::*;
        assert_eq!(kinds("```\ncode\n\nmore"), [(Code, "```\ncode\n\nmore")]);
        assert_eq!(kinds("<!-- a\n\nb"), [(Comment, "<!-- a\n\nb")]);
        assert_eq!(kinds("---\na: b\n"), [(FrontMatter, "---\na: b\n")]);
        // Never closed, it ends at the first blank line, so the text after it stays text.
        assert_eq!(
            kinds("---\na: b\n\n# Title\n"),
            [
                (FrontMatter, "---\na: b\n"),
                (Blank, "\n"),
                (Heading { level: 1 }, "# Title\n")
            ]
        );
        assert!(front_matter_closes("---\na: b\n---\n"));
        assert!(!front_matter_closes("---\na: b\n"));
    }

    #[test]
    fn a_rule_or_directive_ends_a_paragraph_and_a_rule_below_text_makes_a_heading() {
        use BlockKind::*;
        assert_eq!(kinds("a\n---\n"), [(Heading { level: 2 }, "a\n---\n")]);
        assert_eq!(kinds("a\n***\n"), [(Paragraph, "a\n"), (SceneBreak, "***\n")]);
        assert_eq!(
            kinds("a\n::: toc\nb\n"),
            [(Paragraph, "a\n"), (Directive, "::: toc\n"), (Paragraph, "b\n")]
        );
    }

    #[test]
    fn a_comment_is_a_block_only_with_nothing_else_on_its_lines() {
        use BlockKind::*;
        assert_eq!(kinds("<!-- c --> text\n"), [(Paragraph, "<!-- c --> text\n")]);
        assert_eq!(kinds("<!-- a\nb --> text\n"), [(Paragraph, "<!-- a\nb --> text\n")]);
        assert_eq!(kinds("<!-- a --> <!-- b -->\n"), [(Comment, "<!-- a --> <!-- b -->\n")]);
        assert_eq!(
            kinds("a\n<!-- c -->\nb\n"),
            [(Paragraph, "a\n"), (Comment, "<!-- c -->\n"), (Paragraph, "b\n")]
        );
        assert_eq!(kinds("a\n<!-- c --> b\nd\n"), [(Paragraph, "a\n<!-- c --> b\nd\n")]);
    }

    #[test]
    fn a_comment_holding_a_blank_line_does_not_split_its_paragraph() {
        use BlockKind::*;
        assert_eq!(kinds("a <!-- b\n\nc --> d\n"), [(Paragraph, "a <!-- b\n\nc --> d\n")]);
        assert_eq!(
            kinds("a <!-- b\n\nc --> d\ne\n\nf\n"),
            [
                (Paragraph, "a <!-- b\n\nc --> d\ne\n"),
                (Blank, "\n"),
                (Paragraph, "f\n")
            ]
        );
        assert_eq!(
            kinds("a `<!--` b\n\nc --> d\n"),
            [(Paragraph, "a `<!--` b\n"), (Blank, "\n"), (Paragraph, "c --> d\n")]
        );
        assert_eq!(
            kinds("a <!-- open\n\nb\n"),
            [(Paragraph, "a <!-- open\n"), (Blank, "\n"), (Paragraph, "b\n")]
        );
    }

    #[test]
    fn closing_a_comment_far_below_updates_the_block_it_opened_in() {
        let text = "a <!-- b\n\nc\n\nd\n\ne\n\nf\n";
        let mut buffer = Buffer::new(text);
        let mut blocks = Blocks::parse(buffer.text());
        let at = text.len() - 1;
        let caret = Selection::caret(at);
        let splice = buffer
            .transact(
                EditKind::Typing,
                vec![Edit::new(at..at, " -->")],
                caret,
                caret,
                Instant::now(),
            )
            .unwrap();
        blocks.apply(buffer.text(), splice);
        assert_eq!(blocks, Blocks::parse(buffer.text()));
        assert_eq!(blocks.as_slice().len(), 1);
    }

    #[test]
    fn only_a_list_starting_at_one_interrupts_a_paragraph() {
        use BlockKind::*;
        assert_eq!(
            kinds("Im Jahr\n1914. war es\n"),
            [(Paragraph, "Im Jahr\n1914. war es\n")]
        );
        assert_eq!(kinds("a\n1. b\n"), [(Paragraph, "a\n"), (List, "1. b\n")]);
    }

    #[test]
    fn typing_a_marker_changes_the_block_kind() {
        let text = "para\n\nnext\n";
        let mut buffer = Buffer::new(text);
        let mut blocks = Blocks::parse(buffer.text());
        let now = Instant::now();
        let caret = Selection::caret(0);
        let splice = buffer
            .transact(EditKind::Typing, vec![Edit::new(0..0, "# ")], caret, caret, now)
            .unwrap();
        let change = blocks.apply(buffer.text(), splice);
        assert_eq!(blocks.as_slice()[0].kind, BlockKind::Heading { level: 1 });
        assert_eq!(blocks, Blocks::parse(buffer.text()));
        assert_eq!(change.start, 0);
    }

    /// The break `text` needs before `next`, checked against reading the whole text again.
    fn break_of(text: &str, next: &str, starts_file: bool) -> &'static str {
        let gap = break_before(text, &Blocks::parse(text), next, starts_file);
        let whole = break_after_last(text, next, starts_file, || last_block_start(text, starts_file));
        assert_eq!(gap, whole, "{text:?} before {next:?}, starting the file: {starts_file}");
        gap
    }

    #[test]
    fn a_text_gets_the_break_that_keeps_the_next_block_apart() {
        assert_eq!(break_of("Added.", "# Two\n", false), "\n");
        assert_eq!(break_of("Added.", "# Two\r\n", false), "\r\n");
        assert_eq!(break_of("Text.\n", "# Two\n", false), "");
        assert_eq!(break_of("Text.\n\n", "Two\n===\n", false), "");
        // A paragraph takes a setext heading's words, and a table takes a line as a row.
        assert_eq!(break_of("Added.", "Two\n===\n", false), "\n\n");
        assert_eq!(break_of("Added.\n", "Two\n===\n", false), "\n");
        assert_eq!(break_of("| a |\n|---|\n| 1 |\n", "Two\n===\n", false), "\n");
        // Raw HTML runs to a blank line, over a heading too.
        assert_eq!(break_of("<div>\n</div>\n", "# Two\n", false), "\n");
        assert_eq!(break_of("# One\n", "Two\n===\n", false), "");
        assert_eq!(break_of("---\ntitle: A\n---\n", "Text.\n", true), "");
        // No break ends an unclosed fence, so it gets none.
        assert_eq!(break_of("```\ncode\n", "# Two\n", false), "");
    }

    #[test]
    fn the_break_read_from_the_last_block_is_the_one_the_whole_text_gives() {
        let nexts = [
            "# Two\n",
            "Two\n===\n",
            "Two\r\n===\r\n",
            "lazy\n\nMore.\n",
            "- item\n",
            "\nTwo\n",
        ];
        let texts = [
            "# One\n\nText.\n\n```\nfenced\n\n# not a heading\n```\nAfter.",
            "# One\n\n```\nopen fence\n\nstill code",
            "# One\n\n<!-- a comment\n\nstill open -->\nText.\n",
            "# One\n\n- a\n\n- b\n  more",
            "# One\n\n[^1]: note\n\n    continued\n",
            "# One\n\n    indented\n\n    code\n",
            "# One\n\n> quote\nlazy",
            "---\ntitle: A\n---\nText.",
            "# One\r\n\r\nText.\r\n",
        ];
        for text in texts {
            for next in nexts {
                for starts_file in [true, false] {
                    break_of(text, next, starts_file);
                }
            }
        }
    }
}
