//! CommonMark parsing into the [document model](crate::document), with footnotes, layout
//! directives, labels, and cross-references.
//!
//! Directive lines (`::: name` and `:::`) are found before parsing: every line starting with
//! `:::` outside a code block. They are blanked, so CommonMark sees a blank line there, and
//! applied between the top-level blocks they stand between.
//!
//! Labels carry a kind prefix: `# Heading {#sec:name}`, `![Caption](file){#fig:name}`, and
//! `: Caption {#tbl:name}`. References are `@sec:name` or `[@sec:name]` for the number, and
//! `[@sec:name, page]` for the page. An `@key` or `[@key]` without one of the three prefixes is a
//! citation, see [`syntax`]; inside link text it stays text. References and citations are read from
//! the source text, so an escaped `\@` stays text.
//!
//! A `{name}` in text is a [placeholder](Placeholder) such as `{words}`, unless its brace is escaped.
//! Placeholders whose value depends on the page are rejected, as are placeholders in headings.
//!
//! An index mark `{^entry!sub-entry}` marks its position for the index, and `[text]{^}` indexes the text in
//! brackets. Either may end with `|see entry` or `|see also entry`. Marks are read from the source like
//! citations, so Markdown inside them stays as written. `::: index` places the index; without it a document
//! with marks gets one at the end.
//!
//! HTML comments are cut out before anything else, see [`comments`]; other raw HTML is an error.
//!
//! Inline marks are `~~strike~~`, `^superscript^`, `~subscript~`, `[text]{.underline}`, and `[text]{.smallcaps}`.
//! pulldown-cmark reads `~~` as strikethrough and a single `~` as subscript when both are enabled, so a tilde
//! has one meaning per run length. It pairs them only at word edges and leaves `H~2~O` as text, which
//! [`Builder::script`] pairs from the source. Superscript is only read there: pulldown-cmark's would pair the
//! carets of two index marks, as in `{^a} and {^b}`. A `{todo: text}` is a note to the author that shows in
//! the output and is listed in [`Document::todos`].
//!
//! A custom style is applied with `{.name}` at the end of a heading or paragraph, or on a line of its
//! own directly before a list. A heading may combine it with its label, as in `{#sec:name .name}`.
//!
//! A `::: table` directive holds a list table: a list with an item per row, each holding a nested list
//! with an item per cell. The first row is the header. Rows take `{.name}` and cells `{.name span=n}`
//! at the start of their item; the directive takes `{align="left right" widths="* auto"}`.
//!
//! Punctuation is set typographically: `--` and `---` become en and em dashes, `...` an ellipsis,
//! and straight quotes become the curly quotes of the document language, “…” and ‘…’ in English,
//! „…“ and ‚…‘ in German. A backslash keeps a character straight, as in `\"`.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::bibliography::syntax::{self, Segment};
use crate::config::source::InputSource;
use crate::config::template::Placeholder;
use crate::config::theme::{CalloutKind, Lang};
use crate::diagnostic::Diagnostic;
use crate::document::{
    Block, Cell, Citation, Class, Column, ColumnAlign, ColumnWidth, Document, Footnote, ImageFile, IndexMark, Inline,
    InlineStyle, LabelKind, Link, Location, Matter, Placement, Reference, Row, See, Todo,
};
use crate::highlight;
use crate::spacing;

mod comments;
pub mod marks;

/// Parses the Markdown body that starts at 1-based line `first_line` of the document `source`.
///
/// Tables become [`Block::Table`], with a `: Caption` paragraph after the table as its caption.
/// Every construct that cannot be rendered yet is reported with its location. The result is
/// either the complete document or every such diagnostic, never a document with content dropped.
#[cfg(test)]
pub fn parse(body: &str, first_line: u64, source: &InputSource, lang: Lang) -> Result<Document, Vec<Diagnostic>> {
    parse_with(body, first_line, source, lang, false)
}

/// Like [`parse`], and joins abbreviations, units, and page references with no-break spaces in text
/// when `no_break_spaces` is set, see [`crate::spacing`].
pub fn parse_with(
    original: &str,
    first_line: u64,
    source: &InputSource,
    lang: Lang,
    no_break_spaces: bool,
) -> Result<Document, Vec<Diagnostic>> {
    // Task lists are enabled only so they can be recognised and reported. GFM adds only the alert kinds of
    // block quotations.
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_GFM;
    let comments = comments::find(original, options);
    let (written, cuts) = comments.strip(original);
    let body = written.as_ref();
    let directives = directive_lines(body, options);
    let mut text = String::with_capacity(body.len());
    let mut end = 0;
    for line in &directives {
        text.push_str(&body[end..line.start]);
        text.extend(std::iter::repeat_n(' ', line.len()));
        end = line.end;
    }
    text.push_str(&body[end..]);

    let directives = directives.into_iter().map(|line| (line.start, &body[line])).collect();
    let mut builder = Builder::new(&text, original, cuts, first_line, source, lang, directives);
    builder.no_break_spaces = no_break_spaces;
    if let Some(start) = comments.unterminated {
        let at = builder.locate(start);
        builder.report_at(at, "this comment is never closed; end it with -->");
    }
    for (event, range) in Parser::new_ext(&text, options).into_offset_iter() {
        builder.event(event, range);
    }
    builder.finish()
}

/// Byte ranges of the lines that start with `:::` outside code blocks, without line endings.
fn directive_lines(body: &str, options: Options) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if content.starts_with(":::") {
            lines.push(start..start + content.len());
        }
        start += line.len();
    }
    if lines.is_empty() {
        return lines;
    }
    let code: Vec<Range<usize>> = Parser::new_ext(body, options)
        .into_offset_iter()
        .filter_map(|(event, range)| matches!(event, Event::Start(Tag::CodeBlock(_))).then_some(range))
        .collect();
    lines.retain(|line| {
        let before = code.partition_point(|block| block.start <= line.start);
        before == 0 || code[before - 1].end <= line.start
    });
    lines
}

/// Directive containers. `page-break` has no body and is not a container.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fence {
    Columns,
    FullWidth,
    Keep,
    Bottom,
    Table,
}

enum Container {
    Quote(Location),
    Callout(Location, CalloutKind),
    List {
        at: Location,
        start: Option<u64>,
        items: Vec<ListItem>,
        class: Option<Class>,
    },
    Item(ListItem),
    /// `None` for an unknown directive name, which has been reported.
    Directive {
        at: Location,
        fence: Option<Fence>,
        columns: ColumnAttributes,
    },
    Footnote {
        at: Location,
        /// Where the definition starts, which a margin note's `{-}` follows.
        offset: usize,
        label: String,
    },
}

/// A list item. Items of a `::: table` carry their attributes, and a row its cells once its nested list ends.
struct ListItem {
    at: Location,
    blocks: Vec<Block>,
    attributes: Option<ItemAttributes>,
    cells: Option<Vec<Cell>>,
}

/// The attributes at the start of a row or cell item of a list table.
struct ItemAttributes {
    class: Option<Class>,
    span: Option<usize>,
}

/// The `align` and `widths` attributes of `::: table`, one entry per column, and its `placement`.
#[derive(Default)]
struct ColumnAttributes {
    align: Option<Vec<ColumnAlign>>,
    widths: Option<Vec<ColumnWidth>>,
    placement: Option<Placement>,
}

/// What a list item of a `::: table` stands for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TableItem {
    Row,
    Cell,
}

/// A paragraph or heading whose inline content is being collected.
struct Leaf {
    at: Location,
    /// `Some` for headings.
    level: Option<u8>,
    /// Tight list items hold text without a Paragraph event.
    implicit: bool,
    content: Vec<Inline>,
    /// The image this paragraph consists of. Its description is the content.
    image: Option<LeafImage>,
    /// The label of a heading or figure.
    label: Option<String>,
    /// A figure's `placement=...` and where it is written.
    placement: Option<(Placement, Location)>,
    /// The custom style of a heading.
    class: Option<Class>,
    /// Whether a heading is marked `{-}` or `{.unnumbered}`, and `{.unlisted}`.
    unnumbered: bool,
    unlisted: bool,
    /// The source offset where the last text event ended, which locates a trailing `{.name}`.
    text_end: usize,
}

struct LeafImage {
    at: Location,
    /// An index into [`Document::images`].
    index: usize,
    /// Whether the image's description has ended, so further content is an error.
    closed: bool,
}

/// A table whose rows are being collected.
struct TableState {
    at: Location,
    columns: Vec<Column>,
    header: Option<Row>,
    rows: Vec<Row>,
    in_header: bool,
    /// The start of the current row and the byte range of its source text without the line ending.
    row_at: Location,
    row_source: Range<usize>,
    cells: Vec<Cell>,
}

struct CodeBlock {
    at: Location,
    first_line: u64,
    language: Option<String>,
    text: String,
}

struct Builder<'a> {
    /// The text that is parsed: the written text without comments and with blanked directive lines.
    body: &'a str,
    /// The text as written, which locations refer to, and where its comments were cut from `body`.
    original: &'a str,
    cuts: comments::Cuts,
    source: &'a InputSource,
    first_line: u64,
    /// Byte offset of each line start in `original`.
    line_starts: Vec<usize>,
    diagnostics: Vec<Diagnostic>,
    /// One block list per open container, plus the document's own at the bottom.
    blocks: Vec<Vec<Block>>,
    containers: Vec<Container>,
    leaf: Option<Leaf>,
    table: Option<TableState>,
    code: Option<CodeBlock>,
    emphasis: u32,
    strong: u32,
    marks: Marks,
    /// The marks that [`Builder::script`] and `[text]{.name}` opened, by the offset of their closing text.
    closers: Vec<Closer>,
    todos: Vec<Todo>,
    links: Vec<String>,
    /// Nesting depth inside a construct that is reported and otherwise ignored.
    skip: u32,
    /// Nesting depth of CommonMark elements. Directives apply only at depth 0.
    depth: u32,
    /// Directive lines with their byte offset, in order, and the index of the next one to apply.
    directives: Vec<(usize, &'a str)>,
    next_directive: usize,
    /// Referenced footnote labels in reference order, case-folded, with the reference location.
    references: Vec<(String, Location)>,
    referenced: HashMap<String, usize>,
    /// Footnote definitions by case-folded label: written label, location, content, and whether it is a
    /// margin note.
    definitions: HashMap<String, (String, Location, Vec<Block>, bool)>,
    /// Image files in order of first use, and their indexes by path.
    images: Vec<ImageFile>,
    image_index: HashMap<String, usize>,
    /// Defined labels with their locations, and the references to check against them.
    labels: HashMap<String, Location>,
    references_to: Vec<(String, Location)>,
    citations: Vec<Citation>,
    /// Where `::: bibliography` stands, if it does.
    bibliography: Option<Location>,
    index_marks: Vec<IndexMark>,
    /// Where `::: index` stands, if it does.
    index: Option<Location>,
    /// The bracketed text of a `[text]{^}` mark being read.
    span: Option<Span>,
    /// The last matter directive and where it stands.
    matter: Option<(Matter, Location)>,
    /// Where `::: toc` stands, if it does.
    contents: Option<Location>,
    /// Source text before this offset was read as a label, reference, or citation, so text events in it are skipped.
    consumed: usize,
    /// A `{.name}` on a line of its own, waiting for the list it must precede.
    pending_class: Option<Class>,
    quotes: Quotes,
    /// The tables, by location, whose placement an attribute has set.
    /// Whether text gets no-break spaces, see [`crate::spacing`].
    no_break_spaces: bool,
    placed_tables: HashSet<(u64, u64)>,
}

impl<'a> Builder<'a> {
    fn new(
        body: &'a str,
        original: &'a str,
        cuts: comments::Cuts,
        first_line: u64,
        source: &'a InputSource,
        lang: Lang,
        directives: Vec<(usize, &'a str)>,
    ) -> Self {
        let newlines = original.match_indices('\n').map(|(index, _)| index + 1);
        Self {
            body,
            original,
            cuts,
            source,
            first_line,
            line_starts: std::iter::once(0).chain(newlines).collect(),
            diagnostics: Vec::new(),
            blocks: vec![Vec::new()],
            containers: Vec::new(),
            leaf: None,
            table: None,
            code: None,
            emphasis: 0,
            strong: 0,
            marks: Marks::default(),
            closers: Vec::new(),
            todos: Vec::new(),
            links: Vec::new(),
            skip: 0,
            depth: 0,
            directives,
            next_directive: 0,
            references: Vec::new(),
            referenced: HashMap::new(),
            definitions: HashMap::new(),
            images: Vec::new(),
            image_index: HashMap::new(),
            labels: HashMap::new(),
            references_to: Vec::new(),
            citations: Vec::new(),
            bibliography: None,
            index_marks: Vec::new(),
            index: None,
            span: None,
            matter: None,
            contents: None,
            consumed: 0,
            pending_class: None,
            quotes: Quotes::new(lang),
            no_break_spaces: false,
            placed_tables: HashSet::new(),
        }
    }

    fn finish(mut self) -> Result<Document, Vec<Diagnostic>> {
        let end = self.body.len();
        self.directives_before(end..end);
        self.misplaced_class();
        while let Some(Container::Directive { at, fence, .. }) = self.containers.pop() {
            if let Some(fence) = fence {
                self.report_at(at, format!("\"::: {}\" is never closed", fence_name(fence)));
            }
        }

        let mut footnotes = Vec::with_capacity(self.references.len());
        let mut margin_notes = Vec::new();
        // What each reference becomes: footnotes and margin notes are numbered apart.
        let mut targets = Vec::with_capacity(self.references.len());
        for (key, at) in std::mem::take(&mut self.references) {
            match self.definitions.remove(&key) {
                Some((_, defined, blocks, true)) => {
                    targets.push(Inline::MarginNote(margin_notes.len()));
                    margin_notes.push(Footnote { at: defined, blocks });
                }
                Some((_, defined, blocks, false)) => {
                    targets.push(Inline::FootnoteRef(footnotes.len()));
                    footnotes.push(Footnote { at: defined, blocks });
                }
                None => {
                    targets.push(Inline::FootnoteRef(0));
                    self.report_at(at, format!("footnote [^{key}] has no definition"));
                }
            }
        }
        for (label, at) in std::mem::take(&mut self.references_to) {
            if !self.labels.contains_key(&label) {
                self.report_at(
                    at,
                    format!("@{label} refers to an undefined label; define it with {{#{label}}}"),
                );
            }
        }

        self.check_see_references();

        let mut unused: Vec<_> = self
            .definitions
            .drain()
            .map(|(_, (label, at, ..))| (at, label))
            .collect();
        unused.sort_by_key(|(at, _)| (at.line, at.column));
        for (at, label) in unused {
            let message = format!("footnote [^{label}] is defined but never referenced");
            self.report_at(at, message);
        }

        if !self.diagnostics.is_empty() {
            self.diagnostics.sort_by_key(|d| d.position);
            return Err(self.diagnostics);
        }
        let mut blocks = self.blocks.pop().unwrap_or_default();
        if !margin_notes.is_empty() {
            // References are only allowed outside footnotes, so the body holds them all.
            each_inline(&mut blocks, &mut |inline| {
                if let Inline::FootnoteRef(reference) = inline {
                    *inline = targets[*reference].clone();
                }
            });
        }
        if !self.citations.is_empty() && self.bibliography.is_none() {
            let at = self.location(self.body.trim_end().len());
            blocks.push(Block::Bibliography { at });
        }
        if !self.index_marks.is_empty() && self.index.is_none() {
            let at = self.location(self.body.trim_end().len());
            blocks.push(Block::Index { at });
        }
        Ok(Document {
            blocks,
            footnotes,
            margin_notes,
            images: self.images,
            citations: self.citations,
            index_marks: self.index_marks,
            todos: self.todos,
        })
    }

    /// Applies the directive lines before `range` and reports those inside it. Directive lines
    /// always lie inside the body, so `end..end` applies all that remain.
    fn directives_before(&mut self, range: Range<usize>) {
        while let Some(&(offset, text)) = self.directives.get(self.next_directive) {
            if offset >= range.end {
                break;
            }
            self.next_directive += 1;
            if offset < range.start {
                self.directive(offset, text);
            } else {
                let message = "a layout directive cannot interrupt a list, quotation, code block, or footnote; \
                    leave a blank line before it";
                self.report(offset, message);
            }
        }
    }

    fn directive(&mut self, offset: usize, text: &str) {
        self.misplaced_class();
        let at = self.location(offset);
        let line = text.trim_start_matches(':').trim();
        let (name, attributes) = line.split_at(line.find([' ', '\t', '{']).unwrap_or(line.len()));
        let attributes = attributes.trim_start();
        let mut columns = ColumnAttributes::default();
        if !attributes.is_empty() {
            match column_attributes(name, attributes) {
                Ok(parsed) => columns = parsed,
                Err(message) => self.report(offset, message),
            }
        }
        let open: Vec<Option<Fence>> = self
            .containers
            .iter()
            .filter_map(|container| match container {
                Container::Directive { fence, .. } => Some(*fence),
                _ => None,
            })
            .collect();
        // The innermost group that stays on one page, by name, for messages.
        let kept = open
            .iter()
            .rev()
            .flatten()
            .find(|fence| matches!(fence, Fence::Keep | Fence::Bottom))
            .map(|fence| fence_name(*fence));
        let fence = match name {
            "" => {
                // Only directive containers are open between top-level blocks.
                let Some(Container::Directive { at, fence, columns }) = self.containers.pop() else {
                    self.report(offset, "\":::\" closes no open layout directive");
                    return;
                };
                let blocks = self.blocks.pop().unwrap_or_default();
                match fence {
                    Some(Fence::Keep) => self.push_block(Block::Keep { at, blocks }),
                    Some(Fence::Bottom) => self.push_block(Block::Bottom { at, blocks }),
                    Some(Fence::Columns) => self.push_block(Block::Columns { at, blocks }),
                    Some(Fence::FullWidth) => self.push_block(Block::FullWidth { at, blocks }),
                    Some(Fence::Table) => self.close_table(at, blocks, columns),
                    None => {}
                }
                return;
            }
            "page-break" => {
                if let Some(kept) = kept {
                    self.report(offset, format!("page-break is not allowed inside {kept}"));
                } else {
                    self.push_block(Block::PageBreak { at });
                }
                return;
            }
            "bibliography" => {
                if let Some(kept) = kept {
                    self.report(offset, format!("bibliography is not allowed inside {kept}"));
                } else if let Some(first) = self.bibliography {
                    let message = format!(
                        "the bibliography is already placed on {}",
                        self.source.line_name(first.line)
                    );
                    self.report(offset, message);
                } else {
                    self.bibliography = Some(at);
                    self.push_block(Block::Bibliography { at });
                }
                return;
            }
            "index" => {
                if let Some(kept) = kept {
                    self.report(offset, format!("index is not allowed inside {kept}"));
                } else if let Some(first) = self.index {
                    let message = format!("the index is already placed on {}", self.source.line_name(first.line));
                    self.report(offset, message);
                } else {
                    self.index = Some(at);
                    self.push_block(Block::Index { at });
                }
                return;
            }
            "front-matter" | "main-matter" | "back-matter" => {
                let matter = match name {
                    "front-matter" => Matter::Front,
                    "main-matter" => Matter::Main,
                    _ => Matter::Back,
                };
                match self.matter {
                    _ if !open.is_empty() => {
                        self.report(offset, format!("{name} is only allowed outside other directives"))
                    }
                    Some((previous, first)) if previous >= matter => {
                        let message = format!(
                            "{name} cannot follow the {} on {}; use front-matter, main-matter, and back-matter \
                             once each, in this order",
                            matter_name(previous),
                            self.source.line_name(first.line)
                        );
                        self.report(offset, message);
                    }
                    _ => {
                        self.matter = Some((matter, at));
                        self.push_block(Block::Matter { at, matter });
                    }
                }
                return;
            }
            "toc" => {
                if kept.is_some() || open.contains(&Some(Fence::Columns)) {
                    self.report(offset, "toc is not allowed inside keep, bottom, or columns");
                } else if let Some(first) = self.contents {
                    let message = format!(
                        "the table of contents is already placed on {}",
                        self.source.line_name(first.line)
                    );
                    self.report(offset, message);
                } else {
                    self.contents = Some(at);
                    self.push_block(Block::Contents { at });
                }
                return;
            }
            "keep" => Some(Fence::Keep),
            "bottom" => {
                if !open.is_empty() {
                    self.report(offset, "bottom is only allowed outside other directives");
                }
                Some(Fence::Bottom)
            }
            "table" => Some(Fence::Table),
            "columns" => {
                if open.contains(&Some(Fence::Columns)) {
                    self.report(offset, "columns cannot be nested");
                } else if let Some(kept) = kept {
                    self.report(
                        offset,
                        format!("columns is not allowed inside {kept}; put keep groups inside columns instead"),
                    );
                }
                Some(Fence::Columns)
            }
            "full-width" => {
                if open.last() != Some(&Some(Fence::Columns)) {
                    self.report(offset, "full-width is only allowed directly inside columns");
                }
                Some(Fence::FullWidth)
            }
            _ => {
                let message = format!(
                    "unknown directive \"{name}\"; use columns, full-width, keep, bottom, table, page-break, bibliography, \
                     index, toc, front-matter, main-matter, or back-matter"
                );
                self.report(offset, message);
                None
            }
        };
        self.containers.push(Container::Directive { at, fence, columns });
        self.blocks.push(Vec::new());
    }

    /// Whether the footnote definition at `offset` starts with `{-}`, which makes it a margin note.
    fn margin_mark(&self, offset: usize) -> bool {
        let rest = &self.body[offset..];
        let Some(colon) = rest.find("]:") else {
            return false;
        };
        let text = rest[colon + 2..].trim_start_matches([' ', '\t']);
        text.strip_prefix("{-}")
            .is_some_and(|after| after.is_empty() || after.starts_with(char::is_whitespace))
    }

    /// Whether a figure or table here may go to the top or bottom of a page: in the main text or in a
    /// `full-width` block of a column section.
    fn placeable(&self) -> bool {
        let mut fences = Vec::with_capacity(self.containers.len());
        for container in &self.containers {
            match container {
                Container::Directive {
                    fence: Some(Fence::Table),
                    ..
                } => {}
                Container::Directive { fence, .. } => fences.push(*fence),
                _ => return false,
            }
        }
        matches!(fences.as_slice(), [] | [Some(Fence::Columns), Some(Fence::FullWidth)])
    }

    /// The placement of a figure or table here, from its attribute if written: `None` for the theme's
    /// default, `Here` where it cannot be placed. `notes` tells whether the block holds note references,
    /// which keep a table in place. A top or bottom attribute where it cannot apply is an error.
    fn placement(&mut self, written: Option<(Placement, Location)>, notes: bool) -> Option<Placement> {
        let placeable = self.placeable();
        match written {
            Some((Placement::Here, _)) => Some(Placement::Here),
            Some((_, at)) if !placeable => {
                let message = "placement=top and placement=bottom apply only in the main text or in a full-width \
                    block of a column section";
                self.report_at(at, message);
                None
            }
            Some((_, at)) if notes => {
                self.report_at(
                    at,
                    "a table placed at the top or bottom cannot hold footnotes or margin notes",
                );
                None
            }
            Some((placement, _)) => Some(placement),
            None if !placeable || notes => Some(Placement::Here),
            None => None,
        }
    }

    /// Applies a written placement to the table at `at` that was just pushed. A table takes one.
    fn place_table(&mut self, at: Location, written: (Placement, Location)) {
        if !self.placed_tables.insert((at.line, at.column)) {
            self.report_at(written.1, "this table's placement is already set");
            return;
        }
        let notes = match self.blocks.last().and_then(|blocks| blocks.last()) {
            Some(Block::Table { rows, .. }) => rows.iter().any(|row| !table_notes(row).is_empty()),
            _ => return,
        };
        let placement = self.placement(Some(written), notes);
        if let Some(Block::Table { placement: slot, .. }) = self.blocks.last_mut().and_then(|blocks| blocks.last_mut())
        {
            *slot = placement;
        }
    }

    /// The file location of a byte offset in `body`.
    /// Where `offset` in the parsed text is written.
    fn location(&self, offset: usize) -> Location {
        self.locate(self.cuts.original(offset))
    }

    /// The location of `offset` in the text as written.
    fn locate(&self, offset: usize) -> Location {
        let line = self.line_starts.partition_point(|&start| start <= offset);
        let line_start = self.line_starts[line - 1];
        Location {
            line: line as u64 + self.first_line - 1,
            column: self.original[line_start..offset].chars().count() as u64 + 1,
        }
    }

    fn report(&mut self, offset: usize, message: impl Into<String>) {
        let at = self.location(offset);
        self.report_at(at, message);
    }

    fn report_at(&mut self, at: Location, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::new(Some(self.source.clone()), message).at(at.line, at.column));
    }

    fn style(&self, code: bool) -> InlineStyle {
        InlineStyle {
            emphasis: self.emphasis > 0,
            strong: self.strong > 0,
            code,
            link: self.links.last().cloned().map(Link::Url),
            unbreakable: false,
            small_caps: self.marks.on(Mark::SmallCaps),
            strike: self.marks.on(Mark::Strike),
            underline: self.marks.on(Mark::Underline),
            superscript: self.marks.on(Mark::Superscript),
            subscript: self.marks.on(Mark::Subscript),
            todo: false,
        }
    }

    fn push_block(&mut self, block: Block) {
        if let Some(blocks) = self.blocks.last_mut() {
            blocks.push(block);
        }
    }

    fn open_leaf(&mut self, offset: usize, level: Option<u8>, implicit: bool) {
        if level.is_none() && self.body[offset..].starts_with(":::") {
            let message = "a layout directive must start at the beginning of a line, outside lists, quotations, \
                and footnotes";
            self.report(offset, message);
        }
        self.leaf = Some(Leaf {
            at: self.location(offset),
            level,
            implicit,
            content: Vec::new(),
            image: None,
            label: None,
            placement: None,
            class: None,
            unnumbered: false,
            unlisted: false,
            text_end: offset,
        });
        if level.is_none() {
            self.item_attributes(offset);
        }
    }

    /// Whether the innermost open list item is a row or a cell of a `::: table`.
    fn table_item(&self) -> Option<TableItem> {
        let table = |container: &Container| {
            matches!(
                container,
                Container::Directive {
                    fence: Some(Fence::Table),
                    ..
                }
            )
        };
        match self.containers.as_slice() {
            [.., directive, Container::List { .. }, Container::Item(_)] if table(directive) => Some(TableItem::Row),
            [
                ..,
                directive,
                Container::List { .. },
                Container::Item(_),
                Container::List { .. },
                Container::Item(_),
            ] if table(directive) => Some(TableItem::Cell),
            _ => None,
        }
    }

    /// Reads attributes such as `{.name span=2}` at `offset`, where a paragraph starts, if it is the first
    /// content of a row or cell item of a `::: table`, and stores them on the item.
    fn item_attributes(&mut self, offset: usize) {
        let Some(kind) = self.table_item() else { return };
        let fresh = matches!(self.containers.last(), Some(Container::Item(item)) if item.attributes.is_none());
        if !fresh || !self.blocks.last().is_some_and(Vec::is_empty) {
            return;
        }
        let Some((attributes, length)) = attributes(&self.body[offset..]) else {
            return;
        };
        let at = self.location(offset);
        let (mut class, mut span) = (None, None);
        for attribute in attributes {
            match (attribute, kind) {
                (Attribute::Class(name), _) if class.is_none() => {
                    class = Some(Class {
                        name: name.to_owned(),
                        at,
                    });
                }
                (Attribute::Pair("span", value), TableItem::Cell) if span.is_none() => match value.parse::<usize>() {
                    Ok(columns) if columns > 0 => span = Some(columns),
                    _ => self.report(offset, format!("span=\"{value}\" is not a number of columns")),
                },
                (_, TableItem::Row) => self.report(offset, "a table row takes one style, as in {.name}"),
                (_, TableItem::Cell) => {
                    self.report(offset, "a table cell takes one style and a span, as in {.name span=2}")
                }
            }
        }
        let rest = &self.body[offset + length..];
        let end = offset + length + rest.len() - rest.trim_start().len();
        self.consumed = self.consumed.max(end);
        if let Some(Container::Item(item)) = self.containers.last_mut() {
            item.attributes = Some(ItemAttributes { class, span });
        }
    }

    /// The cells of a row from the items of its nested list. Cells hold paragraphs and lists.
    fn cells(&mut self, items: Vec<ListItem>) -> Vec<Cell> {
        items
            .into_iter()
            .map(|item| {
                self.cell_blocks(&item.blocks);
                let (class, span) = item.attributes.map_or((None, None), |a| (a.class, a.span));
                Cell {
                    at: item.at,
                    blocks: item.blocks,
                    span: span.unwrap_or(1),
                    class,
                }
            })
            .collect()
    }

    /// Reports blocks in a list table cell other than paragraphs and lists.
    fn cell_blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::Paragraph { .. } => {}
                Block::List { items, .. } => items.iter().for_each(|item| self.cell_blocks(item)),
                block => self.report_at(block.at(), "a table cell holds only paragraphs and lists"),
            }
        }
    }

    /// The table from the items of the list in a `::: table`: the first row is the header, and every row's
    /// cells span as many columns as the header's.
    fn list_table(&mut self, at: Location, items: Vec<ListItem>) -> Option<Block> {
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let stray = item.blocks.first().map(Block::at);
            match (stray, item.cells) {
                (None, Some(cells)) => rows.push(Row {
                    at: item.at,
                    cells,
                    class: item.attributes.and_then(|attributes| attributes.class),
                }),
                (stray, _) => self.report_at(
                    stray.unwrap_or(item.at),
                    "a table row holds only its cells, as a nested list with an item per cell",
                ),
            }
        }
        if rows.is_empty() {
            return None;
        }
        let header = rows.remove(0);
        let span = |row: &Row| row.cells.iter().map(|cell| cell.span).sum::<usize>();
        let count = span(&header);
        for row in &rows {
            let spanned = span(row);
            if spanned != count {
                let message = format!("the cells of this row span {spanned} columns, but the header's span {count}");
                self.report_at(row.at, message);
            }
        }
        let notes: Vec<Location> = header
            .cells
            .iter()
            .flat_map(|cell| footnotes(&cell.blocks))
            .map(|index| self.references[index].1)
            .collect();
        for at in notes {
            self.report_at(at, HEADER_NOTE);
        }
        let marks: Vec<Location> = header
            .cells
            .iter()
            .flat_map(|cell| marks(&cell.blocks))
            .map(|index| self.index_marks[index].at)
            .collect();
        for at in marks {
            self.report_at(at, HEADER_MARK);
        }
        let column = Column {
            align: None,
            width: ColumnWidth::Auto,
        };
        let notes = rows.iter().any(|row| !table_notes(row).is_empty());
        Some(Block::Table {
            at,
            columns: vec![column; count],
            header,
            rows,
            caption: Vec::new(),
            label: None,
            placement: self.placement(None, notes),
        })
    }

    /// Pushes the table that a `::: table` directive at `at` holds, with its column attributes applied.
    fn close_table(&mut self, at: Location, blocks: Vec<Block>, attributes: ColumnAttributes) {
        let Ok(
            [
                Block::Table {
                    mut columns,
                    header,
                    rows,
                    caption,
                    label,
                    placement,
                    ..
                },
            ],
        ) = <[Block; 1]>::try_from(blocks)
        else {
            self.report_at(
                at,
                "a table directive holds one list, with an item per row and a nested item per cell",
            );
            return;
        };
        let count = columns.len();
        let mut mismatch = |name: &str, entries: usize| {
            if entries != count {
                let message = format!("{name} has {entries} entries, but the table has {count} columns");
                self.report_at(at, message);
            }
        };
        if let Some(align) = &attributes.align {
            mismatch("align", align.len());
        }
        if let Some(widths) = &attributes.widths {
            mismatch("widths", widths.len());
        }
        for (index, column) in columns.iter_mut().enumerate() {
            if let Some(align) = attributes.align.as_ref().and_then(|align| align.get(index)) {
                column.align = Some(*align);
            }
            if let Some(width) = attributes.widths.as_ref().and_then(|widths| widths.get(index)) {
                column.width = *width;
            }
        }
        // A caption inside the directive has placed the table already, at the table's own location.
        let placement = match placement {
            Some(Placement::Top | Placement::Bottom) => placement,
            _ => self.placement(None, rows.iter().any(|row| !table_notes(row).is_empty())),
        };
        self.push_block(Block::Table {
            at,
            columns,
            header,
            rows,
            caption,
            label,
            placement,
        });
        if let Some(written) = attributes.placement {
            self.place_table(at, (written, at));
        }
    }

    fn close_leaf(&mut self) {
        let Some(Leaf {
            at,
            level,
            content,
            image,
            label,
            placement,
            class,
            unnumbered,
            unlisted,
            text_end,
            ..
        }) = self.leaf.take()
        else {
            return;
        };
        self.unclosed_span();
        self.close_marks();
        if level.is_none() {
            self.lone_marks(&content);
        }
        let block = match (level, image) {
            (Some(level), _) => Block::Heading {
                at,
                level,
                content,
                label,
                class,
                unnumbered,
                unlisted,
            },
            (None, Some(image)) => {
                if label.is_some() && content.is_empty() {
                    self.report_at(
                        image.at,
                        "a figure label needs a caption, as in ![Caption](file.png){#fig:name}",
                    );
                }
                Block::Image {
                    at: image.at,
                    image: image.index,
                    caption: content,
                    label,
                    placement: self.placement(placement, false),
                }
            }
            (None, None) => {
                let mut content = content;
                if strip_caption_marker(&mut content) {
                    self.table_caption(at, content);
                    return;
                }
                let class = match self.take_class(&mut content, text_end) {
                    Some((class, true)) => {
                        self.pending_class = Some(class);
                        None
                    }
                    Some((class, false)) if content.is_empty() => {
                        let message = format!(
                            "{{.{}}} needs text before it, or a line of its own directly before a list",
                            class.name
                        );
                        self.report_at(class.at, message);
                        return;
                    }
                    class => class.map(|(class, _)| class),
                };
                if content.is_empty() {
                    return;
                }
                Block::Paragraph { at, content, class }
            }
        };
        self.push_block(block);
    }

    /// Removes a `{.name}` attribute from the end of a paragraph whose last text event ended at
    /// `text_end`, and returns it with whether it stands on a line of its own.
    fn take_class(&self, content: &mut Vec<Inline>, text_end: usize) -> Option<(Class, bool)> {
        let Some(Inline::Text { text, style }) = content.last_mut() else {
            return None;
        };
        let open = text.rfind("{.").filter(|_| !style.code)?;
        let name = text[open + 2..].strip_suffix('}')?;
        if name.is_empty() || label_name_length(name) != name.len() {
            return None;
        }
        let attribute = &text[open..];
        let start = text_end.checked_sub(attribute.len())?;
        let escapes = self.body[..start].len() - self.body[..start].trim_end_matches('\\').len();
        if self.body.get(start..text_end) != Some(attribute) || escapes % 2 == 1 {
            return None;
        }
        let line_start = self.body[..start].rfind('\n').map_or(0, |newline| newline + 1);
        let own_line = self.body[line_start..start]
            .trim_start_matches([' ', '\t', '>'])
            .is_empty();
        let class = Class {
            name: name.to_owned(),
            at: self.location(start),
        };
        text.truncate(open);
        text.truncate(text.trim_end().len());
        if text.is_empty() {
            content.pop();
        }
        Some((class, own_line))
    }

    /// Reports a `{.name}` on a line of its own that no list follows.
    fn misplaced_class(&mut self) {
        if let Some(class) = self.pending_class.take() {
            let message = format!(
                "{{.{}}} on a line of its own must directly precede a list; to style a paragraph or heading, \
                 write it at the end of its text",
                class.name
            );
            self.report_at(class.at, message);
        }
    }

    /// Attaches a `: Caption` paragraph to the table directly before it.
    fn table_caption(&mut self, at: Location, content: Vec<Inline>) {
        let follows_table = matches!(
            self.blocks.last().and_then(|blocks| blocks.last()),
            Some(Block::Table { caption, .. }) if caption.is_empty()
        );
        if !follows_table {
            self.report_at(at, "a table caption must directly follow its table");
            return;
        }
        let footnotes: Vec<Location> = content
            .iter()
            .filter_map(|inline| match inline {
                Inline::FootnoteRef(index) => Some(self.references[*index].1),
                _ => None,
            })
            .collect();
        for footnote in footnotes {
            self.report_at(footnote, "a table caption cannot hold footnotes");
        }
        let mut content = content;
        let (label, placement) = self.caption_label(at, &mut content);
        let mut table_at = None;
        if let Some(Block::Table {
            at,
            caption,
            label: table_label,
            ..
        }) = self.blocks.last_mut().and_then(|blocks| blocks.last_mut())
        {
            *caption = content;
            *table_label = label;
            table_at = Some(*at);
        }
        if let (Some(table), Some(placement)) = (table_at, placement) {
            self.place_table(table, placement);
        }
    }

    /// Removes attributes such as `{#tbl:name placement=top}` from the end of a table caption, registers
    /// the label, and returns it with the placement.
    fn caption_label(
        &mut self,
        at: Location,
        content: &mut Vec<Inline>,
    ) -> (Option<String>, Option<(Placement, Location)>) {
        let Some(Inline::Text { text, style }) = content.last_mut() else {
            return (None, None);
        };
        let trimmed = text.trim_end();
        if style.code || !trimmed.ends_with('}') {
            return (None, None);
        }
        let Some((start, attributes)) = trimmed.rfind('{').and_then(|start| {
            let (attributes, length) = block_attributes(&trimmed[start..])?;
            (start + length == trimmed.len()).then_some((start, attributes))
        }) else {
            return (None, None);
        };
        let (mut label, mut placement) = (None, None);
        let mut problems = Vec::new();
        for attribute in attributes {
            match attribute {
                BlockAttribute::Label(name) if label.is_none() => label = Some(name.to_owned()),
                BlockAttribute::Placement(Ok(value)) if placement.is_none() => placement = Some((value, at)),
                BlockAttribute::Placement(Err(message)) => problems.push(message),
                _ => problems
                    .push("a table caption takes a label and a placement, as in {#tbl:name placement=top}".to_owned()),
            }
        }
        text.truncate(start);
        text.truncate(text.trim_end().len());
        if text.is_empty() {
            content.pop();
        }
        for message in problems {
            self.report_at(at, message);
        }
        let Some(label) = label else {
            return (None, placement);
        };
        if content.is_empty() {
            self.report_at(at, "a table caption needs text before its label");
        }
        (self.define(at, &label, LabelKind::Table), placement)
    }

    /// Registers a label of `kind` defined at `at`, or reports why it cannot be one.
    fn define(&mut self, at: Location, label: &str, kind: LabelKind) -> Option<String> {
        let prefix = kind.prefix();
        let valid = label
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix(':'))
            .is_some_and(|name| label_name_length(name) == name.len() && !name.is_empty());
        if !valid {
            let what = match kind {
                LabelKind::Section => "a heading",
                LabelKind::Figure => "a figure",
                LabelKind::Table => "a table",
            };
            let message = format!(
                "\"{label}\" is not a label for {what}: write {{#{prefix}:name}} with letters, digits, - and _"
            );
            self.report_at(at, message);
            return None;
        }
        if let Some(first) = self.labels.get(label) {
            let message = format!(
                "label {label} is already defined on {}",
                self.source.line_name(first.line)
            );
            self.report_at(at, message);
            return None;
        }
        self.labels.insert(label.to_owned(), at);
        Some(label.to_owned())
    }

    fn close_implicit_leaf(&mut self) {
        if self.leaf.as_ref().is_some_and(|leaf| leaf.implicit) {
            self.close_leaf();
        }
    }

    fn push_inline(&mut self, offset: usize, inline: Inline) {
        if self.leaf.is_none() {
            self.open_leaf(offset, None, true);
        }
        if self
            .leaf
            .as_ref()
            .is_some_and(|leaf| leaf.image.as_ref().is_some_and(|image| image.closed))
        {
            self.report(offset, ALONE);
            // Reported once; the rest of the paragraph is ordinary text.
            if let Some(leaf) = self.leaf.as_mut() {
                leaf.image = None;
            }
        }
        let Some(leaf) = self.leaf.as_mut() else {
            return;
        };
        match (leaf.content.last_mut(), inline) {
            (
                Some(Inline::Text {
                    text: last,
                    style: last_style,
                }),
                Inline::Text { text, style },
            ) if !style.code && *last_style == style => last.push_str(&text),
            (_, inline) => leaf.content.push(inline),
        }
    }

    fn push_text(&mut self, offset: usize, text: &str, code: bool) {
        let style = self.style(code);
        let text = if self.no_break_spaces && !code {
            spacing::join(text)
        } else {
            text.into()
        };
        self.push_inline(
            offset,
            Inline::Text {
                text: text.into_owned(),
                style,
            },
        );
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        let offset = range.start;
        if self.depth == 0 {
            // A block's range can include the blank lines after it, which may be blanked directives.
            let end = range.start + self.body[range.clone()].trim_end().len();
            self.directives_before(range.start..end);
        }
        if !matches!(event, Event::Start(Tag::List(_))) {
            self.misplaced_class();
        }
        match event {
            Event::Start(_) => self.depth += 1,
            Event::End(_) => self.depth -= 1,
            _ => {}
        }
        if self.skip > 0 {
            match event {
                Event::Start(_) => self.skip += 1,
                Event::End(_) => self.skip -= 1,
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(TagEnd::Image) => self.end_image(range.end),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match self.code.as_mut() {
                Some(code) => code.text.push_str(&text),
                None => {
                    // Opened before the text is read, so attributes at the start of a table item are seen.
                    if self.leaf.is_none() {
                        self.open_leaf(offset, None, true);
                    }
                    let end = range.end;
                    let text = self.quotes.quote(&text, self.body, range.clone()).unwrap_or(&text);
                    self.text(text, range);
                    if let Some(leaf) = self.leaf.as_mut() {
                        leaf.text_end = end;
                    }
                }
            },
            Event::Code(_) if offset < self.consumed => {}
            Event::Code(text) => self.push_text(offset, &text, true),
            // A citation group may span lines.
            Event::SoftBreak if offset < self.consumed => {}
            Event::SoftBreak => self.push_text(offset, " ", false),
            Event::HardBreak => self.push_inline(offset, Inline::LineBreak),
            Event::Rule => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                self.push_block(Block::SceneBreak { at });
            }
            Event::InlineHtml(_) if offset < self.consumed => {}
            Event::InlineHtml(_) => self.report(offset, "raw HTML is not rendered"),
            Event::FootnoteReference(label) => self.footnote_reference(offset, &label),
            Event::TaskListMarker(_) => self.report(offset, "task lists are not supported"),
            // Block HTML is skipped with its tag; math and the rest are not enabled.
            _ => {}
        }
    }

    /// Pushes a text event, reading cross-references from its source. Text that a label or reference
    /// already consumed is skipped.
    fn text(&mut self, text: &str, range: Range<usize>) {
        let (mut text, mut offset) = (text, range.start);
        if offset < self.consumed {
            let skipped = &self.body[offset..self.consumed.min(range.end)];
            if range.end <= self.consumed || !text.starts_with(skipped) {
                return;
            }
            text = &text[skipped.len()..];
            offset = self.consumed;
        }
        // The `]` of a `[text]{.name}` ends the marked text, and the attribute after it is skipped.
        let closing = self
            .closers
            .iter()
            .position(|closer| closer.end > closer.at + 1 && (offset..offset + text.len()).contains(&closer.at));
        if let Some(position) = closing.filter(|_| self.body.get(offset..offset + text.len()) == Some(text)) {
            let closer = self.closers.remove(position);
            let split = closer.at - offset;
            if split > 0 {
                self.text(&text[..split], offset..closer.at);
            }
            self.marks.close(closer.mark);
            self.consumed = closer.end;
            self.text(&text[split..], closer.at..range.end);
            return;
        }
        // The `]` of a `[text]{^}` mark ends its text, and the mark after it is read from the source.
        if let Some(close) = self.span.as_ref().map(|span| span.close)
            && (offset..offset + text.len()).contains(&close)
            && self.body.get(offset..offset + text.len()) == Some(text)
        {
            let split = close - offset;
            if split > 0 {
                self.text(&text[..split], offset..close);
            }
            self.close_span();
            self.text(&text[split..], close..range.end);
            return;
        }
        if text == "[" && !self.escaped(offset) {
            self.check_undefined_footnote(offset);
            let opened = self.bracketed_reference(offset)
                || self.bracketed_citation(offset)
                || self.open_span(offset)
                || self.open_attribute_mark(offset);
            if opened {
                return;
            }
        }
        // CommonMark keeps an image it cannot parse as text, starting with a separate `![`.
        if text == "![" && self.body[offset..].starts_with("![") {
            self.report(offset, BROKEN_IMAGE);
        }
        // Escapes and entities change the text, so only text that matches its source holds references.
        if self.body.get(offset..offset + text.len()) != Some(text) {
            self.push_text(offset, text, false);
            return;
        }
        let mut start = 0;
        for (index, mark) in text.match_indices(['@', '{', '^', '~']) {
            if mark == "^" || mark == "~" {
                if index >= start {
                    let at = offset + index;
                    if let Some(position) = self.closers.iter().position(|closer| closer.at == at) {
                        self.push_text_before(offset + start, &text[start..index]);
                        let closer = self.closers.remove(position);
                        self.marks.close(closer.mark);
                        start = index + 1;
                    } else if let Some(closer) = self.script(at) {
                        self.push_text_before(offset + start, &text[start..index]);
                        self.open_mark(at, closer.mark);
                        self.closers.push(closer);
                        start = index + 1;
                    }
                }
                continue;
            }
            if mark == "{" {
                let escapes =
                    self.body[..offset + index].len() - self.body[..offset + index].trim_end_matches('\\').len();
                if self.body[offset + index + 1..].starts_with("todo:") && escapes.is_multiple_of(2) && index >= start {
                    self.push_text_before(offset + start, &text[start..index]);
                    let end = self.todo(offset + index) - offset;
                    if end > text.len() {
                        self.consumed = offset + end;
                        return;
                    }
                    start = end;
                    continue;
                }
                if self.body[offset + index + 1..].starts_with('^') && escapes.is_multiple_of(2) && index >= start {
                    if index > start {
                        self.push_text(offset + start, &text[start..index], false);
                    }
                    let end = self.index_mark(offset + index) - offset;
                    if end > text.len() {
                        self.consumed = offset + end;
                        return;
                    }
                    start = end;
                    continue;
                }
                let Some(length) =
                    field_name(&text[index + 1..]).filter(|_| escapes.is_multiple_of(2) && index >= start)
                else {
                    continue;
                };
                if index > start {
                    self.push_text(offset + start, &text[start..index], false);
                }
                self.field(offset + index, &text[index + 1..index + 1 + length]);
                start = index + length + 2;
                continue;
            }
            let before = text[..index]
                .chars()
                .next_back()
                .or_else(|| self.body[..offset].chars().next_back());
            let escapes = self.body[..offset + index].len() - self.body[..offset + index].trim_end_matches('\\').len();
            if before.is_some_and(char::is_alphanumeric) || escapes % 2 == 1 || index < start {
                continue;
            }
            let at = offset + index;
            let reference = reference_label(&text[index + 1..]);
            let citation = match reference {
                None if self.links.is_empty() => narrative(self.body, at),
                _ => None,
            };
            if reference.is_none() && citation.is_none() {
                continue;
            }
            if index > start {
                self.push_text(offset + start, &text[start..index], false);
            }
            if let Some(length) = reference {
                let label = text[index + 1..index + 1 + length].to_owned();
                self.reference(at, label, false);
                start = index + 1 + length;
            } else if let Some((line, citation)) = citation {
                let end = line + citation.range.end - offset;
                self.citation(line, citation);
                if end > text.len() {
                    // A locator in brackets is in the text events that follow.
                    self.consumed = offset + end;
                    return;
                }
                start = end;
            }
        }
        if start < text.len() {
            self.push_text(offset + start, &text[start..], false);
        }
    }

    /// Reads `[@label]` or `[@label, page]` at the `[` at `offset`, and whether it was one.
    fn bracketed_reference(&mut self, offset: usize) -> bool {
        let Some(inner) = self.body[offset..].strip_prefix("[@") else {
            return false;
        };
        let Some(length) = reference_label(inner) else {
            return false;
        };
        let label = inner[..length].to_owned();
        let rest = &inner[length..];
        let (page, close) = if rest.starts_with(']') {
            (false, "]".len())
        } else if rest.starts_with(", page]") {
            (true, ", page]".len())
        } else {
            let message = format!("write a cross-reference in brackets as [@{label}] or [@{label}, page]");
            self.report(offset, message);
            return false;
        };
        self.consumed = offset + "[@".len() + length + close;
        self.reference(offset, label, page);
        true
    }

    /// Reads a citation group such as `[@a, p. 3; @b]` at the `[` at `offset`, and whether it was one.
    /// The group ends at the first `]` and may span lines.
    fn bracketed_citation(&mut self, offset: usize) -> bool {
        if !self.links.is_empty() {
            return false;
        }
        let Some(close) = self.body[offset..].find(']') else {
            return false;
        };
        let end = offset + close + 1;
        match syntax::find(&self.body[offset..end]).into_iter().next() {
            Some(Segment::Citation(citation)) => self.citation(offset, citation),
            Some(Segment::Invalid { message, .. }) => self.report(offset, message),
            _ => return false,
        }
        self.consumed = end;
        true
    }

    /// Adds a citation whose byte ranges start at `base`.
    fn citation(&mut self, base: usize, citation: syntax::Citation) {
        let offset = base + citation.range.start;
        if self.leaf.as_ref().is_some_and(|leaf| leaf.level.is_some()) {
            self.report(offset, "a heading cannot hold a citation");
            return;
        }
        let keys = citation
            .items
            .iter()
            .map(|item| self.location(base + item.range.start))
            .collect();
        let index = self.citations.len();
        self.citations.push(Citation {
            at: self.location(offset),
            syntax: citation,
            keys,
        });
        let style = self.style(false);
        self.push_inline(offset, Inline::Citation { index, style });
    }

    /// Adds the placeholder `name`, written in braces at `offset`.
    fn field(&mut self, offset: usize, name: &str) {
        let placeholder = match Placeholder::parse(name) {
            Ok(placeholder) => placeholder,
            Err(message) => return self.report(offset, format!("{message}; write \\{{ for a brace")),
        };
        if placeholder.is_page_dependent() {
            let message = format!("{{{placeholder}}} depends on the page and is available only in headers and footers");
            return self.report(offset, message);
        }
        if self.leaf.as_ref().is_some_and(|leaf| leaf.level.is_some()) {
            return self.report(offset, "a heading cannot hold a placeholder");
        }
        let (at, style) = (self.location(offset), self.style(false));
        self.push_inline(offset, Inline::Field { placeholder, at, style });
    }

    /// Reads the index mark whose `{^` is at `offset` and returns where it ends. A mark after the bracketed
    /// text of an open span takes that text for an empty entry.
    fn index_mark(&mut self, offset: usize) -> usize {
        let spec = offset + "{^".len();
        let close = self.body[spec..]
            .find('}')
            .filter(|&close| !self.body[spec..spec + close].contains("\n\n"));
        let Some(close) = close.map(|close| spec + close) else {
            self.report(offset, "this index mark is not closed; end it with }");
            return spec;
        };
        let end = close + 1;
        let span = self.span.take();
        let text = span.as_ref().map(|span| self.span_text(span));
        let parsed = index_mark(&self.body[spec..close], text.as_ref().map(|(text, _)| text.as_str()));
        let (mut terms, mut see) = match parsed {
            Ok(parsed) => parsed,
            Err(message) => {
                self.report(offset, message);
                return end;
            }
        };
        let lang = self.quotes.lang;
        for term in terms.iter_mut().chain(see.iter_mut().flat_map(|see| &mut see.terms)) {
            *term = punctuate(term, lang);
        }
        match span {
            Some(span) => {
                let at = self.index_marks[span.mark].at;
                if let Some((_, false)) = text {
                    self.report_at(at, "the text of an index mark can hold only text and formatting");
                }
                self.index_marks[span.mark] = IndexMark { at, terms, see };
            }
            None if self.mark_allowed(offset) => {
                let at = self.location(offset);
                self.index_marks.push(IndexMark { at, terms, see });
                self.push_inline(offset, Inline::IndexMark(self.index_marks.len() - 1));
            }
            None => {}
        }
        end
    }

    /// Whether an index mark may stand at `offset`, else reports why not.
    fn mark_allowed(&mut self, offset: usize) -> bool {
        if self.leaf.as_ref().is_some_and(|leaf| leaf.level.is_some()) {
            self.report(offset, HEADING_MARK);
            return false;
        }
        if self.table.as_ref().is_some_and(|table| table.in_header) {
            self.report(offset, HEADER_MARK);
            return false;
        }
        true
    }

    /// Starts a `[text]{^}` mark at the `[` at `offset`, if one starts there, and returns whether it did. The
    /// mark's anchor goes where the text starts. The text may not hold brackets.
    fn open_span(&mut self, offset: usize) -> bool {
        let Some(close) = self.body[offset + 1..].find(['[', ']']).map(|close| offset + 1 + close) else {
            return false;
        };
        if self.span.is_some() || !self.body[close..].starts_with("]{^") || self.body[offset..close].contains("\n\n") {
            return false;
        }
        // A misplaced mark is reported here and read on, so its text does not cause further errors.
        self.mark_allowed(offset);
        let mark = self.index_marks.len();
        self.index_marks.push(IndexMark {
            at: self.location(offset),
            terms: Vec::new(),
            see: None,
        });
        self.push_inline(offset, Inline::IndexMark(mark));
        let start = self.leaf.as_ref().map_or(0, |leaf| leaf.content.len());
        self.span = Some(Span { close, mark, start });
        true
    }

    /// Reads the mark after the `]` of the open span.
    fn close_span(&mut self) {
        let Some(close) = self.span.as_ref().map(|span| span.close) else {
            return;
        };
        let end = self.index_mark(close + "]".len());
        self.consumed = self.consumed.max(end);
    }

    /// The text collected since `span` opened, and whether it is only text.
    fn span_text(&self, span: &Span) -> (String, bool) {
        let content = self
            .leaf
            .as_ref()
            .map_or(&[][..], |leaf| &leaf.content[span.start.min(leaf.content.len())..]);
        let only_text = content
            .iter()
            .all(|inline| matches!(inline, Inline::Text { .. } | Inline::LineBreak));
        let text = content
            .iter()
            .map(|inline| match inline {
                Inline::Text { text, .. } => text.as_str(),
                _ => " ",
            })
            .collect();
        (text, only_text)
    }

    /// Reports a `[text]{^}` mark whose bracketed text did not end in its paragraph.
    fn unclosed_span(&mut self) {
        if let Some(span) = self.span.take() {
            let message = "the bracketed text of an index mark must end in its paragraph, as in [text]{^}";
            self.report_at(self.index_marks[span.mark].at, message);
        }
    }

    /// Reports an index mark in content without text, since it has nothing to stand beside.
    fn lone_marks(&mut self, content: &[Inline]) {
        let has_text = content.iter().any(|inline| match inline {
            Inline::Text { text, .. } => !text.trim().is_empty(),
            Inline::IndexMark(_) | Inline::Anchor(_) | Inline::MarginNote(_) | Inline::LineBreak => false,
            Inline::FootnoteRef(_) | Inline::Ref(_) | Inline::Citation { .. } | Inline::Field { .. } => true,
        });
        let first = content.iter().find_map(|inline| match inline {
            Inline::IndexMark(mark) => Some(*mark),
            _ => None,
        });
        if let Some(mark) = first.filter(|_| !has_text) {
            let message = "an index mark needs text beside it; put it in the paragraph it refers to";
            self.report_at(self.index_marks[mark].at, message);
        }
    }

    /// Reports `see` references to entries that no mark makes, at the mark.
    fn check_see_references(&mut self) {
        let entries: HashSet<&[String]> = self
            .index_marks
            .iter()
            .flat_map(|mark| (1..=mark.terms.len()).map(|length| &mark.terms[..length]))
            .collect();
        let missing: Vec<(Location, String)> = self
            .index_marks
            .iter()
            .filter_map(|mark| {
                let see = mark.see.as_ref()?;
                (!entries.contains(see.terms.as_slice())).then(|| (mark.at, see.terms.join("!")))
            })
            .collect();
        for (at, target) in missing {
            let message = format!("the index has no entry \"{target}\" for this reference; mark it with {{^{target}}}");
            self.report_at(at, message);
        }
    }

    /// Whether a backslash escapes the character at `offset`.
    fn escaped(&self, offset: usize) -> bool {
        marks::escaped(self.body, offset)
    }

    /// Pushes `text` ahead of a mark, unless it is empty.
    fn push_text_before(&mut self, offset: usize, text: &str) {
        if !text.is_empty() {
            self.push_text(offset, text, false);
        }
    }

    /// Starts `mark`, which an end event or the closer pushed with it ends.
    fn open_mark(&mut self, offset: usize, mark: Mark) {
        let other = match mark {
            Mark::Superscript => Mark::Subscript,
            Mark::Subscript => Mark::Superscript,
            _ => mark,
        };
        if other != mark && self.marks.on(other) {
            self.report(offset, "superscript and subscript cannot be nested");
        }
        self.marks.open(mark);
    }

    /// Ends the marks that no closing text ended, at the end of their paragraph.
    fn close_marks(&mut self) {
        for closer in std::mem::take(&mut self.closers) {
            self.marks.close(closer.mark);
        }
    }

    /// The superscript or subscript that the `^` or `~` at `at` opens, as a closer.
    fn script(&self, at: usize) -> Option<Closer> {
        let script = marks::script(self.body, at)?;
        Some(Closer {
            at: script.close,
            end: script.close + 1,
            mark: if script.superscript {
                Mark::Superscript
            } else {
                Mark::Subscript
            },
        })
    }

    /// Starts the `[text]{.underline}` or `[text]{.smallcaps}` whose `[` is at `offset`, if one starts there,
    /// and returns whether it did.
    fn open_attribute_mark(&mut self, offset: usize) -> bool {
        let Some(attribute) = marks::attribute(self.body, offset) else {
            return false;
        };
        let mark = if attribute.small_caps {
            Mark::SmallCaps
        } else {
            Mark::Underline
        };
        self.marks.open(mark);
        self.closers.push(Closer {
            at: attribute.close,
            end: attribute.end,
            mark,
        });
        true
    }

    /// Reads the `{todo: text}` whose brace is at `offset` and returns where it ends. Its text shows in the
    /// output as written, with line breaks as spaces. It may span lines but not a blank line.
    fn todo(&mut self, offset: usize) -> usize {
        let note = offset + "{todo:".len();
        let Some(close) = marks::todo_close(self.body, offset) else {
            self.report(offset, "this todo is not closed; end it with }");
            return note;
        };
        let text = self.body[note..close].split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            self.report(offset, "write the note after {todo:, as in {todo: check the date}");
        } else if self.leaf.as_ref().is_some_and(|leaf| leaf.level.is_some()) {
            self.report(offset, "a heading cannot hold a todo");
        } else {
            let style = InlineStyle {
                todo: true,
                ..self.style(false)
            };
            self.todos.push(Todo {
                at: self.location(offset),
                text: text.clone(),
            });
            self.push_inline(
                offset,
                Inline::Text {
                    text: format!("TODO: {text}"),
                    style,
                },
            );
        }
        close + 1
    }

    fn reference(&mut self, offset: usize, label: String, page: bool) {
        if self.leaf.as_ref().is_some_and(|leaf| leaf.level.is_some()) {
            self.report(offset, "a heading cannot hold a cross-reference");
            return;
        }
        if !self.links.is_empty() {
            self.report(offset, "a cross-reference cannot be inside a link");
            return;
        }
        let at = self.location(offset);
        self.references_to.push((label.clone(), at));
        let style = self.style(false);
        self.push_inline(offset, Inline::Ref(Reference { label, page, style }));
    }

    /// Reads attributes such as `{#fig:name placement=top}` directly after an image that ends at `end`,
    /// spaces allowed between.
    fn end_image(&mut self, end: usize) {
        let Some(image) = self.leaf.as_mut().and_then(|leaf| leaf.image.as_mut()) else {
            return;
        };
        image.closed = true;
        let rest = &self.body[end..];
        let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let Some((attributes, length)) = block_attributes(&rest[spaces..]) else {
            return;
        };
        self.consumed = end + spaces + length;
        let at = self.location(end + spaces);
        let (mut label, mut placement) = (None, None);
        for attribute in attributes {
            match attribute {
                BlockAttribute::Label(name) if label.is_none() => label = self.define(at, name, LabelKind::Figure),
                BlockAttribute::Placement(Ok(value)) if placement.is_none() => placement = Some((value, at)),
                BlockAttribute::Placement(Err(message)) => self.report_at(at, message),
                _ => self.report_at(
                    at,
                    "an image takes a label and a placement, as in {#fig:name placement=top}",
                ),
            }
        }
        if let Some(leaf) = self.leaf.as_mut() {
            leaf.label = label;
            leaf.placement = placement;
        }
    }

    fn footnote_reference(&mut self, offset: usize, label: &str) {
        let at = self.location(offset);
        if self.containers.iter().any(|c| matches!(c, Container::Footnote { .. })) {
            self.report_at(at, "a footnote cannot reference another footnote");
            return;
        }
        if self.table.as_ref().is_some_and(|table| table.in_header) {
            self.report_at(at, HEADER_NOTE);
            return;
        }
        let key = label.to_lowercase();
        if let Some(&index) = self.referenced.get(&key) {
            let first = self.source.line_name(self.references[index].1.line);
            let message = format!("footnote [^{label}] is already referenced on {first}; reference each footnote once");
            self.report_at(at, message);
            return;
        }
        let index = self.references.len();
        self.referenced.insert(key.clone(), index);
        self.references.push((key, at));
        self.push_inline(offset, Inline::FootnoteRef(index));
    }

    /// CommonMark keeps a reference to an undefined footnote as literal text, starting with a
    /// separate `[` text event. An escaped `\\[` never starts that way.
    fn check_undefined_footnote(&mut self, offset: usize) {
        let Some(rest) = self.body[offset..].strip_prefix("[^") else {
            return;
        };
        let Some(end) = rest.find(']') else { return };
        let label = &rest[..end];
        if !label.is_empty() && !label.contains(char::is_whitespace) {
            self.report(offset, format!("footnote [^{label}] has no definition"));
        }
    }

    /// Starts an image, whose description becomes the caption. An image must be the only content of
    /// its paragraph and name a local file.
    fn image(&mut self, offset: usize, path: &str, title: &str) {
        let in_footnote = self.containers.iter().any(|c| matches!(c, Container::Footnote { .. }));
        let leaf = self.leaf.as_ref();
        let problem = if self.table.is_some() {
            Some("images are not allowed in table cells")
        } else if in_footnote {
            Some("images are not allowed in footnotes")
        } else if leaf.is_some_and(|leaf| leaf.level.is_some()) {
            Some("images are not allowed in headings")
        } else if !self.links.is_empty() {
            Some("an image cannot be a link")
        } else if leaf.is_some_and(|leaf| !leaf.content.is_empty() || leaf.image.is_some()) {
            Some(ALONE)
        } else if path.is_empty() {
            Some("an image needs a file path")
        } else if path.contains("://") || path.starts_with("data:") {
            Some("remote images are not fetched; use a local file")
        } else if !title.is_empty() {
            Some("image titles are not used; write the caption as the image description, as in ![Caption](file.png)")
        } else {
            None
        };
        if let Some(problem) = problem {
            // The description is skipped with the image; the paragraph stays open.
            self.report(offset, problem);
            self.skip = 1;
            return;
        }
        if self.leaf.is_none() {
            self.open_leaf(offset, None, true);
        }
        let at = self.location(offset);
        let index = *self.image_index.entry(path.to_owned()).or_insert_with(|| {
            self.images.push(ImageFile {
                at,
                path: path.to_owned(),
            });
            self.images.len() - 1
        });
        if let Some(leaf) = self.leaf.as_mut() {
            leaf.image = Some(LeafImage {
                at,
                index,
                closed: false,
            });
        }
    }

    /// Reports a construct and ignores everything up to its matching end.
    fn skip_reported(&mut self, offset: usize, message: &str) {
        self.close_implicit_leaf();
        self.report(offset, message);
        self.skip = 1;
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        let offset = range.start;
        match tag {
            Tag::Paragraph => self.open_leaf(offset, None, false),
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
                if self.containers.iter().any(|c| matches!(c, Container::Footnote { .. })) {
                    self.skip_reported(offset, "headings are not allowed in footnotes");
                    return;
                }
                self.open_leaf(offset, Some(level as u8), false);
                let source = &self.body[range];
                let brace = source.rfind('{');
                // pulldown-cmark drops one-character attributes, so `{-}` is read from the source.
                let dash = brace.is_some_and(|brace| {
                    let block = source[brace + 1..].trim_end().trim_end_matches('#').trim_end();
                    block
                        .strip_suffix('}')
                        .is_some_and(|block| block.split_ascii_whitespace().any(|token| token == "-"))
                });
                let brace = brace.map_or(offset, |index| offset + index);
                let flag = |name: &str| classes.iter().any(|class| class.as_ref() == name);
                let (unnumbered, unlisted) = (dash || flag("unnumbered"), flag("unlisted"));
                let classes: Vec<_> = classes
                    .iter()
                    .filter(|class| !matches!(class.as_ref(), "unnumbered" | "unlisted"))
                    .collect();
                // CommonMark reads an index mark at the end of a heading as its attributes.
                if self.body[brace..].starts_with("{^") {
                    self.report(brace, HEADING_MARK);
                } else if !attrs.is_empty() || classes.len() > 1 {
                    let message = "a heading takes a label and one style, as in # Heading {#sec:name .style}";
                    self.report(brace, message);
                }
                let class = match classes.first() {
                    Some(name) if label_name_length(name) != name.len() => {
                        let message = format!("style name \"{name}\" may contain only letters, digits, - and _");
                        self.report(brace, message);
                        None
                    }
                    Some(name) => Some(Class {
                        name: name.to_string(),
                        at: self.location(brace),
                    }),
                    None => None,
                };
                let label = id.and_then(|id| self.define(self.location(offset), &id, LabelKind::Section));
                if let Some(leaf) = self.leaf.as_mut() {
                    leaf.label = label;
                    leaf.class = class;
                    leaf.unnumbered = unnumbered;
                    leaf.unlisted = unlisted;
                }
            }
            Tag::BlockQuote(kind) => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                self.containers.push(match kind {
                    Some(kind) => Container::Callout(at, callout_kind(kind)),
                    None => Container::Quote(at),
                });
                self.blocks.push(Vec::new());
            }
            Tag::List(start) => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                let class = self.pending_class.take();
                self.containers.push(Container::List {
                    at,
                    start,
                    items: Vec::new(),
                    class,
                });
            }
            Tag::Item => {
                self.containers.push(Container::Item(ListItem {
                    at: self.location(offset),
                    blocks: Vec::new(),
                    attributes: None,
                    cells: None,
                }));
                self.blocks.push(Vec::new());
            }
            Tag::CodeBlock(kind) => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                let (first_line, language) = match &kind {
                    CodeBlockKind::Fenced(info) => (at.line + 1, highlight::language_of(info).map(str::to_owned)),
                    CodeBlockKind::Indented => (at.line, None),
                };
                self.code = Some(CodeBlock {
                    at,
                    first_line,
                    language,
                    text: String::new(),
                });
            }
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strong => self.strong += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Strikethrough => self.open_mark(offset, Mark::Strike),
            Tag::Superscript => self.open_mark(offset, Mark::Superscript),
            Tag::Subscript => self.open_mark(offset, Mark::Subscript),
            Tag::Image { dest_url, title, .. } => self.image(offset, &dest_url, &title),
            Tag::HtmlBlock => self.skip_reported(offset, "raw HTML is not rendered"),
            Tag::Table(align) => {
                if self.containers.iter().any(|c| matches!(c, Container::Footnote { .. })) {
                    self.skip_reported(offset, "tables are not allowed in footnotes");
                    return;
                }
                self.close_implicit_leaf();
                let at = self.location(offset);
                self.table = Some(TableState {
                    at,
                    columns: align
                        .into_iter()
                        .map(|align| Column {
                            align: column_align(align),
                            width: ColumnWidth::Auto,
                        })
                        .collect(),
                    header: None,
                    rows: Vec::new(),
                    in_header: false,
                    row_at: at,
                    row_source: offset..offset,
                    cells: Vec::new(),
                });
            }
            Tag::TableHead | Tag::TableRow => {
                let row_at = self.location(offset);
                let end = offset + self.body[range].trim_end().len();
                if let Some(table) = self.table.as_mut() {
                    table.in_header = matches!(tag, Tag::TableHead);
                    table.row_at = row_at;
                    table.row_source = offset..end;
                    table.cells.clear();
                }
            }
            Tag::TableCell => {
                // A padded cell starts after the row, and a cell starts before its leading spaces.
                let end = self.table.as_ref().map_or(offset, |table| table.row_source.end);
                let start = offset.min(end);
                let start = start + (self.body[start..end].len() - self.body[start..end].trim_start().len());
                self.leaf = Some(Leaf {
                    at: self.location(start),
                    level: None,
                    implicit: false,
                    content: Vec::new(),
                    image: None,
                    label: None,
                    placement: None,
                    class: None,
                    unnumbered: false,
                    unlisted: false,
                    text_end: start,
                });
            }
            Tag::FootnoteDefinition(label) => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                self.containers.push(Container::Footnote {
                    at,
                    offset,
                    label: label.into_string(),
                });
                self.blocks.push(Vec::new());
            }
            // Remaining tags need extensions that are not enabled.
            _ => {}
        }
    }

    /// Keeps the finished body row, unless it is a caption written without a blank line before it.
    /// pulldown-cmark drops cells beyond the header's without an event, so they are counted in
    /// the source.
    fn end_table_row(&mut self) {
        let Some(table) = self.table.as_mut() else {
            return;
        };
        let row = Row {
            at: table.row_at,
            cells: std::mem::take(&mut table.cells),
            class: None,
        };
        let too_many = cell_count(&self.body[table.row_source.clone()]) > table.columns.len();
        let caption = matches!(row.cells.split_first(), Some((first, rest))
            if starts_with_caption_marker(pipe_cell(first)) && rest.iter().all(|cell| pipe_cell(cell).is_empty()));
        let at = row.at;
        if !caption {
            table.rows.push(row);
        }
        if too_many {
            self.report_at(at, "this row has more cells than the table header");
        }
        if caption {
            self.report_at(at, "leave a blank line between a table and its caption");
        }
    }

    /// Pushes a finished list, or in a `::: table` the table its items hold or the cells of a row.
    fn end_list(&mut self, at: Location, start: Option<u64>, items: Vec<ListItem>, class: Option<Class>) {
        let rows = matches!(
            self.containers.last(),
            Some(Container::Directive {
                fence: Some(Fence::Table),
                ..
            })
        );
        let cells = self.table_item() == Some(TableItem::Row);
        if !rows && !cells {
            let items = items.into_iter().map(|item| item.blocks).collect();
            self.push_block(Block::List {
                at,
                start,
                items,
                class,
            });
            return;
        }
        if let Some(class) = class {
            let message = format!(
                "style the rows and cells of a table at the start of their item, as in - {{.{}}}",
                class.name
            );
            self.report_at(class.at, message);
        }
        if rows {
            if let Some(table) = self.list_table(at, items) {
                self.push_block(table);
            }
            return;
        }
        let cells = self.cells(items);
        if let Some(Container::Item(row)) = self.containers.last_mut()
            && row.cells.is_none()
        {
            row.cells = Some(cells);
            return;
        }
        self.report_at(at, "a table row holds one list of cells");
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.close_leaf(),
            TagEnd::BlockQuote(_) => {
                let blocks = self.blocks.pop().unwrap_or_default();
                match self.containers.pop() {
                    Some(Container::Quote(at)) => self.push_block(Block::Quote { at, blocks }),
                    Some(Container::Callout(at, kind)) => self.push_block(Block::Callout { at, kind, blocks }),
                    _ => {}
                }
            }
            TagEnd::Item => {
                self.close_implicit_leaf();
                let blocks = self.blocks.pop().unwrap_or_default();
                if let Some(Container::Item(item)) = self.containers.pop()
                    && let Some(Container::List { items, .. }) = self.containers.last_mut()
                {
                    items.push(ListItem { blocks, ..item });
                }
            }
            TagEnd::List(_) => {
                if let Some(Container::List {
                    at,
                    start,
                    items,
                    class,
                }) = self.containers.pop()
                {
                    self.end_list(at, start, items, class);
                }
            }
            TagEnd::CodeBlock => {
                if let Some(CodeBlock {
                    at,
                    first_line,
                    language,
                    text,
                }) = self.code.take()
                {
                    let lines = text.lines().map(str::to_owned).collect();
                    self.push_block(Block::Code {
                        at,
                        first_line,
                        language,
                        lines,
                    });
                }
            }
            TagEnd::FootnoteDefinition => {
                self.close_implicit_leaf();
                let mut blocks = self.blocks.pop().unwrap_or_default();
                if let Some(Container::Footnote { at, offset, label }) = self.containers.pop() {
                    let margin = self.margin_mark(offset) && strip_margin_mark(&mut blocks);
                    let key = label.to_lowercase();
                    match self.definitions.get(&key) {
                        Some((_, first, ..)) => {
                            let message = format!(
                                "footnote [^{label}] is already defined on {}",
                                self.source.line_name(first.line)
                            );
                            self.report_at(at, message);
                        }
                        None => {
                            self.definitions.insert(key, (label, at, blocks, margin));
                        }
                    }
                }
            }
            TagEnd::TableCell => {
                self.unclosed_span();
                if let Some(leaf) = &self.leaf {
                    let content = leaf.content.clone();
                    self.lone_marks(&content);
                }
                if let (Some(Leaf { at, content, .. }), Some(table)) = (self.leaf.take(), self.table.as_mut()) {
                    let paragraph = Block::Paragraph {
                        at,
                        content,
                        class: None,
                    };
                    table.cells.push(Cell {
                        at,
                        blocks: vec![paragraph],
                        span: 1,
                        class: None,
                    });
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.header = Some(Row {
                        at: table.row_at,
                        cells: std::mem::take(&mut table.cells),
                        class: None,
                    });
                    table.in_header = false;
                }
            }
            TagEnd::TableRow => self.end_table_row(),
            TagEnd::Table => {
                if let Some(TableState {
                    at,
                    columns,
                    header: Some(header),
                    rows,
                    ..
                }) = self.table.take()
                {
                    let notes = rows.iter().any(|row| !table_notes(row).is_empty());
                    let placement = self.placement(None, notes);
                    self.push_block(Block::Table {
                        at,
                        columns,
                        header,
                        rows,
                        caption: Vec::new(),
                        label: None,
                        placement,
                    });
                }
            }
            TagEnd::Emphasis => self.emphasis -= 1,
            TagEnd::Strong => self.strong -= 1,
            TagEnd::Strikethrough => self.marks.close(Mark::Strike),
            TagEnd::Superscript => self.marks.close(Mark::Superscript),
            TagEnd::Subscript => self.marks.close(Mark::Subscript),
            TagEnd::Link => {
                self.links.pop();
            }
            _ => {}
        }
    }
}

const ALONE: &str = "an image must stand alone in its paragraph";
const HEADER_NOTE: &str = "a table header cannot hold footnotes, because it repeats on every page";
const HEADING_MARK: &str = "a heading cannot hold an index mark; put it in the text below the heading";
const HEADER_MARK: &str = "a table header cannot hold index marks, because it repeats on every page";
const BROKEN_IMAGE: &str = "this image is not valid Markdown: write ![Caption](file.png), with <> around a path \
    that has spaces; a caption cannot hold footnotes";

fn callout_kind(kind: BlockQuoteKind) -> CalloutKind {
    match kind {
        BlockQuoteKind::Note => CalloutKind::Note,
        BlockQuoteKind::Tip => CalloutKind::Tip,
        BlockQuoteKind::Important => CalloutKind::Important,
        BlockQuoteKind::Warning => CalloutKind::Warning,
        BlockQuoteKind::Caution => CalloutKind::Caution,
    }
}

fn column_align(align: Alignment) -> Option<ColumnAlign> {
    match align {
        Alignment::None => None,
        Alignment::Left => Some(ColumnAlign::Left),
        Alignment::Center => Some(ColumnAlign::Center),
        Alignment::Right => Some(ColumnAlign::Right),
    }
}

/// The length of the placeholder name in `{name}` when `text` starts after the brace: a letter, then
/// letters, digits, `-`, `_`, and `.`. Other braces, such as `{.name}` and `{span=2}`, are text.
fn field_name(text: &str) -> Option<usize> {
    let length = text.find('}')?;
    let name = &text[..length];
    let valid = name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    valid.then_some(length)
}

/// The number of cells a table row's source line holds, counting unescaped pipes. One leading and
/// one trailing pipe only frame the row.
fn cell_count(line: &str) -> usize {
    let line = line.trim();
    let (mut pipes, mut escaped, mut last_is_pipe) = (0, false, false);
    for c in line.chars() {
        last_is_pipe = c == '|' && !escaped;
        pipes += usize::from(last_is_pipe);
        escaped = c == '\\' && !escaped;
    }
    pipes + 1 - usize::from(line.starts_with('|')) - usize::from(last_is_pipe)
}

/// The content of a pipe table cell, which is one paragraph.
fn pipe_cell(cell: &Cell) -> &[Inline] {
    match cell.blocks.as_slice() {
        [Block::Paragraph { content, .. }] => content,
        _ => &[],
    }
}

/// The footnotes referenced in `blocks`, which hold paragraphs and lists.
fn footnotes(blocks: &[Block]) -> Vec<usize> {
    let mut notes = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph { content, .. } => notes.extend(content.iter().filter_map(|inline| match inline {
                Inline::FootnoteRef(index) => Some(*index),
                _ => None,
            })),
            Block::List { items, .. } => notes.extend(items.iter().flat_map(|item| footnotes(item))),
            _ => {}
        }
    }
    notes
}

/// The index marks in `blocks`, which hold paragraphs and lists.
fn marks(blocks: &[Block]) -> Vec<usize> {
    let mut marks = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph { content, .. } => marks.extend(content.iter().filter_map(|inline| match inline {
                Inline::IndexMark(index) => Some(*index),
                _ => None,
            })),
            Block::List { items, .. } => marks.extend(items.iter().flat_map(|item| self::marks(item))),
            _ => {}
        }
    }
    marks
}

/// The inline marks that apply to the text being read, with how many of each are open.
#[derive(Default)]
struct Marks([u32; 5]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Strike,
    Superscript,
    Subscript,
    Underline,
    SmallCaps,
}

impl Marks {
    fn on(&self, mark: Mark) -> bool {
        self.0[mark as usize] > 0
    }

    fn open(&mut self, mark: Mark) {
        self.0[mark as usize] += 1;
    }

    fn close(&mut self, mark: Mark) {
        self.0[mark as usize] = self.0[mark as usize].saturating_sub(1);
    }
}

/// A mark that the source closes with its own text: the closing `^` or `~` at `at`, or the `]` of a
/// `[text]{.name}` at `at` with its attribute ending at `end`.
struct Closer {
    at: usize,
    end: usize,
    mark: Mark,
}

/// The bracketed text of a `[text]{^}` mark: where its `]` is, its mark, and where its content starts in the
/// paragraph.
struct Span {
    close: usize,
    mark: usize,
    start: usize,
}

/// Maps the English quotes of smart punctuation to the quotes of a language, see [`Quotes::quote`].
struct Quotes {
    lang: Lang,
    /// Whether an opening single quote waits for its closing quote.
    single_open: bool,
}

impl Quotes {
    fn new(lang: Lang) -> Self {
        Self {
            lang,
            single_open: false,
        }
    }

    /// The quote of the document language for a curly quote that smart punctuation made of a straight
    /// one, which comes in English form. CommonMark pairs single quotes like the closing check here,
    /// so the first closing quote after a paired opening one closes it; any other is an apostrophe.
    fn quote(&mut self, text: &str, body: &str, range: Range<usize>) -> Option<&'static str> {
        let [double_open, double_close, single_open, single_close] = match self.lang {
            Lang::En => ["“", "”", "‘", "’"],
            Lang::De => ["„", "“", "‚", "‘"],
        };
        let quote = match (text, &body[range.clone()]) {
            ("“", "\"") => double_open,
            ("”", "\"") => double_close,
            ("‘", "'") => {
                self.single_open = true;
                single_open
            }
            ("’", "'") => {
                let before = body[..range.start].chars().next_back();
                let after = body[range.end..].chars().next();
                let closes = before.is_some_and(|c| !c.is_whitespace()) && !after.is_some_and(char::is_alphanumeric);
                if self.single_open && closes {
                    self.single_open = false;
                    single_close
                } else {
                    "’"
                }
            }
            _ => return None,
        };
        Some(quote)
    }
}

/// `text` with the punctuation that body text gets, for text read from the source outside the
/// Markdown parser, such as the terms of index marks. Other Markdown syntax in it stays as typed.
fn punctuate(text: &str, lang: Lang) -> String {
    let mut escaped = String::with_capacity(text.len() * 2);
    for (index, c) in text.char_indices() {
        let ellipsis = c == '.' && (text[index..].starts_with("...") || text[..index].ends_with('.'));
        if c.is_ascii_punctuation() && !matches!(c, '\'' | '"' | '-') && !ellipsis {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    let mut quotes = Quotes::new(lang);
    let mut out = String::with_capacity(text.len());
    for (event, range) in Parser::new_ext(&escaped, Options::ENABLE_SMART_PUNCTUATION).into_offset_iter() {
        match event {
            Event::Text(part) => out.push_str(quotes.quote(&part, &escaped, range).unwrap_or(&part)),
            Event::SoftBreak => out.push(' '),
            _ => {}
        }
    }
    out
}

/// The most terms of an index mark: an entry and two levels of sub-entries.
const MARK_LEVELS: usize = 3;

/// Reads the terms and the `see` reference of an index mark from the text between `{^` and `}`. `text`, the
/// bracketed text before the mark, stands in for an empty entry.
fn index_mark(spec: &str, text: Option<&str>) -> Result<(Vec<String>, Option<See>), String> {
    let terms = |part: &str| -> Result<Vec<String>, String> {
        let terms: Vec<String> = part
            .split('!')
            .map(|term| term.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        if terms.len() > MARK_LEVELS {
            let message = "an index mark takes an entry and at most two levels of sub-entries, as in \
                {^entry!sub-entry!detail}";
            return Err(message.to_owned());
        }
        Ok(terms)
    };
    let (entry, see) = match spec.split_once('|') {
        Some((entry, see)) => (entry, Some(see)),
        None => (spec, None),
    };
    let mut entry = terms(entry)?;
    if let (Some(first), Some(text)) = (entry.first_mut(), text)
        && first.is_empty()
    {
        *first = text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    if entry.iter().any(String::is_empty) {
        let message = "an index mark needs a term for the entry and each sub-entry, as in {^entry!sub-entry} or \
            [text]{^}";
        return Err(message.to_owned());
    }
    let Some(see) = see else {
        return Ok((entry, None));
    };
    let see = see.trim_start();
    let word = |prefix: &str| {
        see.strip_prefix(prefix)
            .filter(|rest| rest.starts_with(char::is_whitespace))
    };
    let (also, target) = match (word("see also").or(word("siehe auch")), word("see").or(word("siehe"))) {
        (Some(target), _) => (true, target),
        (None, Some(target)) => (false, target),
        (None, None) => {
            return Err("write a cross-reference in an index mark as |see entry or |see also entry".to_owned());
        }
    };
    let target = terms(target)?;
    if target.iter().any(String::is_empty) {
        return Err("a see reference needs the entry it refers to, as in |see entry!sub-entry".to_owned());
    }
    Ok((entry, Some(See { also, terms: target })))
}

/// The footnote and margin note references in the cells of a table row.
fn table_notes(row: &Row) -> Vec<usize> {
    row.cells.iter().flat_map(|cell| footnotes(&cell.blocks)).collect()
}

/// Calls `visit` on every inline of `blocks` and the blocks they hold.
fn each_inline(blocks: &mut [Block], visit: &mut impl FnMut(&mut Inline)) {
    for block in blocks {
        match block {
            Block::Paragraph { content, .. } | Block::Heading { content, .. } => {
                content.iter_mut().for_each(&mut *visit)
            }
            Block::Image { caption, .. } => caption.iter_mut().for_each(&mut *visit),
            Block::List { items, .. } => items.iter_mut().for_each(|item| each_inline(item, visit)),
            Block::Table {
                header, rows, caption, ..
            } => {
                caption.iter_mut().for_each(&mut *visit);
                for row in std::iter::once(header).chain(rows) {
                    row.cells
                        .iter_mut()
                        .for_each(|cell| each_inline(&mut cell.blocks, visit));
                }
            }
            Block::Quote { blocks, .. }
            | Block::Callout { blocks, .. }
            | Block::Keep { blocks, .. }
            | Block::Bottom { blocks, .. }
            | Block::Columns { blocks, .. }
            | Block::FullWidth { blocks, .. } => each_inline(blocks, visit),
            Block::Code { .. }
            | Block::PageBreak { .. }
            | Block::Matter { .. }
            | Block::Contents { .. }
            | Block::SceneBreak { .. }
            | Block::Bibliography { .. }
            | Block::Index { .. } => {}
        }
    }
}

/// Removes the `{-}` that starts a margin note's first paragraph, and returns whether it was there.
fn strip_margin_mark(blocks: &mut Vec<Block>) -> bool {
    let Some(Block::Paragraph { content, .. }) = blocks.first_mut() else {
        return false;
    };
    let Some(Inline::Text { text, style }) = content.first_mut() else {
        return false;
    };
    let Some(rest) = text.strip_prefix("{-}").filter(|_| !style.code) else {
        return false;
    };
    *text = rest.trim_start().to_owned();
    if text.is_empty() {
        content.remove(0);
        if content.is_empty() {
            blocks.remove(0);
        }
    }
    true
}

/// An attribute after an image or at the end of a table caption.
enum BlockAttribute<'s> {
    /// `#name`, the label without its `#`.
    Label(&'s str),
    /// `placement=value`, or the problem with the value.
    Placement(Result<Placement, String>),
    Other,
}

/// The attributes in the braces that `text` starts with and the length up to the closing brace, if
/// the braces hold a label or a placement. Attributes are separated by spaces and stay on one line.
fn block_attributes(text: &str) -> Option<(Vec<BlockAttribute<'_>>, usize)> {
    let inner = text.strip_prefix('{')?;
    let close = inner.find(['}', '\n'])?;
    if !inner[close..].starts_with('}') {
        return None;
    }
    let attributes: Vec<BlockAttribute> = inner[..close]
        .split_whitespace()
        .map(|word| {
            if let Some(name) = word.strip_prefix('#') {
                BlockAttribute::Label(name)
            } else if let Some(value) = word.strip_prefix("placement=") {
                BlockAttribute::Placement(placement_value(value))
            } else {
                BlockAttribute::Other
            }
        })
        .collect();
    let ours = attributes
        .iter()
        .any(|attribute| !matches!(attribute, BlockAttribute::Other));
    ours.then_some((attributes, close + 2))
}

/// A `placement` value: `here`, `top`, or `bottom`.
fn placement_value(value: &str) -> Result<Placement, String> {
    match value.trim_matches('"') {
        "here" => Ok(Placement::Here),
        "top" => Ok(Placement::Top),
        "bottom" => Ok(Placement::Bottom),
        other => Err(format!("placement \"{other}\" is not here, top, or bottom")),
    }
}

/// An attribute in braces: `.name`, or `key=value` with the value in quotes if it holds spaces.
enum Attribute<'s> {
    Class(&'s str),
    Pair(&'s str, &'s str),
}

/// The attributes in the braces that `text` starts with, and the length up to the closing brace, or
/// `None` if `text` does not start with attributes. Names are letters, digits, `-`, and `_`.
fn attributes(text: &str) -> Option<(Vec<Attribute<'_>>, usize)> {
    let mut rest = text.strip_prefix('{')?;
    let mut list = Vec::new();
    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        if let Some(after) = rest.strip_prefix('}') {
            return (!list.is_empty()).then_some((list, text.len() - after.len()));
        }
        let class = rest.strip_prefix('.');
        let name = label_name_length(class.unwrap_or(rest));
        if name == 0 {
            return None;
        }
        if let Some(class) = class {
            list.push(Attribute::Class(&class[..name]));
            rest = &class[name..];
        } else {
            let (key, after) = rest.split_at(name);
            let after = after.strip_prefix('=')?;
            let (value, after) = match after.strip_prefix('"') {
                Some(quoted) => {
                    let end = quoted.find(['"', '\n'])?;
                    quoted[end..]
                        .starts_with('"')
                        .then_some((&quoted[..end], &quoted[end + 1..]))?
                }
                None => after.split_at(after.find([' ', '\t', '}', '\n']).unwrap_or(after.len())),
            };
            list.push(Attribute::Pair(key, value));
            rest = after;
        }
        if !rest.starts_with([' ', '\t', '}']) {
            return None;
        }
    }
}

/// The `align` and `widths` attributes written after a directive's name.
fn column_attributes(name: &str, text: &str) -> Result<ColumnAttributes, String> {
    const SYNTAX: &str = "write table attributes as {align=\"left right\" widths=\"* auto\" placement=top}";
    if name != "table" {
        return Err(format!("\"{name}\" takes no attributes; only table does"));
    }
    let Some((list, _)) = attributes(text).filter(|(_, length)| *length == text.len()) else {
        return Err(SYNTAX.to_owned());
    };
    let mut columns = ColumnAttributes::default();
    for attribute in list {
        match attribute {
            Attribute::Pair("align", value) if columns.align.is_none() => {
                let align = value.split_whitespace().map(|word| match word {
                    "left" => Ok(ColumnAlign::Left),
                    "center" => Ok(ColumnAlign::Center),
                    "right" => Ok(ColumnAlign::Right),
                    _ => Err(format!("align \"{word}\" is not left, center, or right")),
                });
                columns.align = Some(align.collect::<Result<_, _>>()?);
            }
            Attribute::Pair("widths", value) if columns.widths.is_none() => {
                let widths = value.split_whitespace().map(|word| match word {
                    "auto" => Ok(ColumnWidth::Auto),
                    "*" => Ok(ColumnWidth::Fill),
                    _ => Err(format!("width \"{word}\" is not * or auto")),
                });
                columns.widths = Some(widths.collect::<Result<_, _>>()?);
            }
            Attribute::Pair("placement", value) if columns.placement.is_none() => {
                columns.placement = Some(placement_value(value)?);
            }
            _ => return Err(SYNTAX.to_owned()),
        }
    }
    Ok(columns)
}

fn starts_with_caption_marker(content: &[Inline]) -> bool {
    matches!(content.first(), Some(Inline::Text { text, style }) if !style.code && text.starts_with(": "))
}

/// Removes the `: ` that starts a table caption and returns whether there was one.
fn strip_caption_marker(content: &mut Vec<Inline>) -> bool {
    if !starts_with_caption_marker(content) {
        return false;
    }
    if let Some(Inline::Text { text, .. }) = content.first_mut() {
        text.drain(..2);
        if text.is_empty() {
            content.remove(0);
        }
    }
    true
}

/// The length of the `prefix:name` cross-reference label that `text` starts with, if any. Names are
/// ASCII letters, digits, `-`, and `_`.
fn reference_label(text: &str) -> Option<usize> {
    LabelKind::of(text)?;
    let (prefix, rest) = text.split_once(':')?;
    let name = label_name_length(rest);
    (name > 0).then_some(prefix.len() + 1 + name)
}

/// The narrative citation that starts with the `@` at `at`, if there is one. Its line is scanned so
/// that the citation syntax decides whether the `@` starts a word. Returns the line's offset, where
/// the citation's byte ranges start, and the citation.
fn narrative(body: &str, at: usize) -> Option<(usize, syntax::Citation)> {
    let start = body[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let end = body[at..].find('\n').map_or(body.len(), |newline| at + newline);
    syntax::find(&body[start..end])
        .into_iter()
        .find_map(|segment| match segment {
            Segment::Citation(citation) if start + citation.range.start == at => Some((start, citation)),
            _ => None,
        })
}

/// The length of the label name that `text` starts with.
fn label_name_length(text: &str) -> usize {
    text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(text.len())
}

fn matter_name(matter: Matter) -> &'static str {
    match matter {
        Matter::Front => "front-matter",
        Matter::Main => "main-matter",
        Matter::Back => "back-matter",
    }
}

fn fence_name(fence: Fence) -> &'static str {
    match fence {
        Fence::Columns => "columns",
        Fence::FullWidth => "full-width",
        Fence::Keep => "keep",
        Fence::Bottom => "bottom",
        Fence::Table => "table",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> InputSource {
        InputSource::Document("/doc.md".into())
    }

    fn blocks(body: &str) -> Vec<Block> {
        parse(body, 1, &source(), Lang::En).expect("parses").blocks
    }

    fn diagnostics(body: &str, first_line: u64) -> Vec<(Option<(u64, u64)>, String)> {
        let errors = parse(body, first_line, &source(), Lang::En).expect_err("reports");
        errors
            .into_iter()
            .map(|d| (d.location.map(|at| (at.line, at.column)), d.message))
            .collect()
    }

    fn text(text: &str, style: InlineStyle) -> Inline {
        Inline::Text {
            text: text.into(),
            style,
        }
    }

    fn plain(value: &str) -> Inline {
        text(value, InlineStyle::default())
    }

    fn at(line: u64, column: u64) -> Location {
        Location { line, column }
    }

    fn paragraph(line: u64, column: u64, value: &str) -> Block {
        Block::Paragraph {
            at: at(line, column),
            content: vec![plain(value)],
            class: None,
        }
    }

    fn inlines(body: &str) -> Vec<Inline> {
        match blocks(body).remove(0) {
            Block::Paragraph { content, .. } => content,
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    #[test]
    fn parses_headings_paragraphs_quotes_and_code() {
        let body =
            "# Title\n\nSetext\n---\n\nText\n\n> Quoted\n\n```rust\nlet a = 1;\n\nlet b = 2;\n```\n\n    indented\n";
        assert_eq!(
            blocks(body),
            vec![
                Block::Heading {
                    at: at(1, 1),
                    level: 1,
                    content: vec![plain("Title")],
                    label: None,
                    class: None,
                    unnumbered: false,
                    unlisted: false,
                },
                Block::Heading {
                    at: at(3, 1),
                    level: 2,
                    content: vec![plain("Setext")],
                    label: None,
                    class: None,
                    unnumbered: false,
                    unlisted: false,
                },
                paragraph(6, 1, "Text"),
                Block::Quote {
                    at: at(8, 1),
                    blocks: vec![paragraph(8, 3, "Quoted")],
                },
                Block::Code {
                    at: at(10, 1),
                    first_line: 11,
                    language: Some("rust".into()),
                    lines: vec!["let a = 1;".into(), "".into(), "let b = 2;".into()],
                },
                Block::Code {
                    at: at(16, 5),
                    first_line: 16,
                    language: None,
                    lines: vec!["indented".into()]
                },
            ]
        );
    }

    #[test]
    fn parses_ordered_and_tight_bullet_lists() {
        assert_eq!(
            blocks("3. one\n4. two\n\n- a\n  - b\n"),
            vec![
                Block::List {
                    at: at(1, 1),
                    start: Some(3),
                    items: vec![vec![paragraph(1, 4, "one")], vec![paragraph(2, 4, "two")]],
                    class: None,
                },
                Block::List {
                    at: at(4, 1),
                    start: None,
                    items: vec![vec![
                        paragraph(4, 3, "a"),
                        Block::List {
                            at: at(5, 3),
                            start: None,
                            items: vec![vec![paragraph(5, 5, "b")]],
                            class: None,
                        },
                    ]],
                    class: None,
                },
            ]
        );
    }

    #[test]
    fn flattens_nested_inline_styles() {
        let em = InlineStyle {
            emphasis: true,
            ..Default::default()
        };
        let both = InlineStyle {
            emphasis: true,
            strong: true,
            ..Default::default()
        };
        let link = |style: InlineStyle| InlineStyle {
            link: Some(Link::Url("https://a.b/".into())),
            ..style
        };
        let code = InlineStyle {
            code: true,
            ..Default::default()
        };
        assert_eq!(
            inlines("a *b **c** d* `x` [e *f*](https://a.b/) <https://a.b/> &amp; \\*"),
            vec![
                plain("a "),
                text("b ", em.clone()),
                text("c", both),
                text(" d", em.clone()),
                plain(" "),
                text("x", code),
                plain(" "),
                text("e ", link(InlineStyle::default())),
                text("f", link(em)),
                plain(" "),
                text("https://a.b/", link(InlineStyle::default())),
                plain(" & *"),
            ]
        );
    }

    #[test]
    fn joins_soft_breaks_and_keeps_hard_breaks() {
        assert_eq!(
            inlines("one\ntwo  \nthree\\\nfour"),
            vec![
                plain("one two"),
                Inline::LineBreak,
                plain("three"),
                Inline::LineBreak,
                plain("four")
            ]
        );
    }

    #[test]
    fn locations_use_the_file_line_and_character_columns() {
        let document = parse("intro\n\n> ünï **bold** after\n", 10, &source(), Lang::En).unwrap();
        let Block::Quote { at: quote, blocks } = &document.blocks[1] else {
            panic!("quote")
        };
        assert_eq!(*quote, at(12, 1));
        assert!(matches!(
            blocks[0],
            Block::Paragraph {
                at: Location { line: 12, column: 3 },
                ..
            }
        ));
        assert_eq!(
            diagnostics("ünï ![alt](a.png)", 5),
            vec![(Some((5, 5)), ALONE.to_string())]
        );
    }

    #[test]
    fn reports_each_unsupported_construct() {
        let cases = [
            ("<div>\nhi\n</div>", "raw HTML is not rendered"),
            ("a <b>x</b>", "raw HTML is not rendered"),
            ("- [ ] todo", "task lists are not supported"),
            ("<!-- ok --><span>x</span>", "raw HTML is not rendered"),
        ];
        for (body, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(errors.iter().any(|(_, found)| found == message), "{body:?}: {errors:?}");
        }
    }

    #[test]
    fn collects_every_diagnostic_with_its_location() {
        assert_eq!(
            diagnostics("a ![b](c.png)\n\n---\n\ntext <i>x</i>\n", 3),
            vec![
                (Some((3, 3)), ALONE.to_string()),
                (Some((7, 6)), "raw HTML is not rendered".to_string()),
                (Some((7, 10)), "raw HTML is not rendered".to_string()),
            ]
        );
    }

    /// The text of the first paragraph of `body` in `lang`, with code spans in backticks.
    fn typeset(body: &str, lang: Lang) -> String {
        let Block::Paragraph { content, .. } = parse(body, 1, &source(), lang).expect("parses").blocks.remove(0) else {
            panic!("expected a paragraph");
        };
        content
            .iter()
            .map(|inline| match inline {
                Inline::Text { text, style } if style.code => format!("`{text}`"),
                Inline::Text { text, .. } => text.clone(),
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    #[test]
    fn joins_with_no_break_spaces_only_in_text_and_only_when_enabled() {
        let body = "Siehe z. B. auf S. 12 und `z. B. 5 min` unter [Link](https://x.org/a) in 5 min.\n";
        let text_of = |on: bool| {
            let Block::Paragraph { content, .. } = parse_with(body, 1, &source(), Lang::De, on)
                .expect("parses")
                .blocks
                .remove(0)
            else {
                panic!("expected a paragraph");
            };
            content
                .iter()
                .map(|inline| match inline {
                    Inline::Text { text, style } if style.code => format!("`{text}`"),
                    Inline::Text { text, .. } => text.clone(),
                    other => panic!("unexpected {other:?}"),
                })
                .collect::<String>()
        };
        assert_eq!(
            text_of(false),
            "Siehe z. B. auf S. 12 und `z. B. 5 min` unter Link in 5 min."
        );
        assert_eq!(
            text_of(true),
            "Siehe z.\u{a0}B. auf S.\u{a0}12 und `z. B. 5 min` unter Link in 5\u{a0}min."
        );
    }

    #[test]
    fn sets_dashes_ellipses_and_quotes_of_the_language() {
        let body = "\"Peter's 'best' day\" -- 1990--95 --- and so on...\n";
        assert_eq!(typeset(body, Lang::En), "“Peter’s ‘best’ day” – 1990–95 — and so on…");
        assert_eq!(typeset(body, Lang::De), "„Peter’s ‚best‘ day“ – 1990–95 — and so on…");
        assert_eq!(
            typeset("Geht's 'gut', ihr Jungs'?\n", Lang::De),
            "Geht’s ‚gut‘, ihr Jungs’?"
        );
    }

    #[test]
    fn punctuates_index_terms_like_text() {
        let document = parse(
            "Text{^Peter's \"lamp\"!1990--95...|see C++ *x*}{^C++ *x*}.\n",
            1,
            &source(),
            Lang::De,
        )
        .unwrap();
        let mark = &document.index_marks[0];
        assert_eq!(mark.terms, ["Peter’s „lamp“", "1990–95…"]);
        assert_eq!(mark.see.as_ref().unwrap().terms, ["C++ *x*"]);
    }

    #[test]
    fn keeps_escaped_and_code_punctuation_straight() {
        assert_eq!(
            typeset("\\\"a\\\" `\"b\" -- c...`\n", Lang::De),
            "\"a\" `\"b\" -- c...`"
        );
    }

    #[test]
    fn parses_images_alone_in_a_paragraph_with_the_description_as_caption() {
        let document = parse(
            "![A *plot*](img/a.png)\n\n- ![](b.svg)\n\n![Again](img/a.png)\n",
            1,
            &source(),
            Lang::En,
        )
        .unwrap();
        let em = InlineStyle {
            emphasis: true,
            ..Default::default()
        };
        assert_eq!(
            document.blocks,
            vec![
                Block::Image {
                    at: at(1, 1),
                    image: 0,
                    caption: vec![plain("A "), text("plot", em)],
                    label: None,
                    placement: None,
                },
                Block::List {
                    at: at(3, 1),
                    start: None,
                    items: vec![vec![Block::Image {
                        at: at(3, 3),
                        image: 1,
                        caption: Vec::new(),
                        label: None,
                        placement: Some(Placement::Here),
                    }]],
                    class: None,
                },
                Block::Image {
                    at: at(5, 1),
                    image: 0,
                    caption: vec![plain("Again")],
                    label: None,
                    placement: None,
                },
            ]
        );
        let paths: Vec<_> = document
            .images
            .iter()
            .map(|file| (file.at, file.path.as_str()))
            .collect();
        assert_eq!(paths, vec![(at(1, 1), "img/a.png"), (at(3, 3), "b.svg")]);
    }

    #[test]
    fn reports_images_that_are_not_alone_local_and_untitled() {
        let cases = [
            ("See ![a](a.png)", (1, 5), ALONE),
            ("![a](a.png) and text", (1, 12), ALONE),
            ("![a](a.png)\ncaption", (1, 12), ALONE),
            ("# ![a](a.png)", (1, 3), "images are not allowed in headings"),
            ("[![a](a.png)](https://x.y)", (1, 2), "an image cannot be a link"),
            (
                "a[^n]\n\n[^n]: ![a](a.png)",
                (3, 7),
                "images are not allowed in footnotes",
            ),
            (
                "![a](https://x.y/a.png)",
                (1, 1),
                "remote images are not fetched; use a local file",
            ),
            (
                "![a](data:image/png;base64,AAAA)",
                (1, 1),
                "remote images are not fetched; use a local file",
            ),
            ("![a](<>)", (1, 1), "an image needs a file path"),
            ("![a](my file.png)", (1, 1), BROKEN_IMAGE),
            ("![Note[^n]](a.png)\n\n[^n]: n", (1, 1), BROKEN_IMAGE),
            (
                "![a](a.png \"Title\")",
                (1, 1),
                "image titles are not used; write the caption as the image description, as in ![Caption](file.png)",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert_eq!(errors, vec![(Some(location), message.to_string())], "{body:?}");
        }
    }

    #[test]
    fn parses_book_parts_the_contents_and_scene_breaks() {
        let body = "::: front-matter\n::: toc\n::: main-matter\na\n\n***\n\nb\n\n---\n::: back-matter";
        assert_eq!(
            blocks(body),
            vec![
                Block::Matter {
                    at: at(1, 1),
                    matter: Matter::Front
                },
                Block::Contents { at: at(2, 1) },
                Block::Matter {
                    at: at(3, 1),
                    matter: Matter::Main
                },
                paragraph(4, 1, "a"),
                Block::SceneBreak { at: at(6, 1) },
                paragraph(8, 1, "b"),
                Block::SceneBreak { at: at(10, 1) },
                Block::Matter {
                    at: at(11, 1),
                    matter: Matter::Back
                },
            ]
        );
    }

    #[test]
    fn reads_unnumbered_and_unlisted_headings_beside_a_label_and_style() {
        let flags = |body: &str| match &blocks(body)[0] {
            Block::Heading {
                unnumbered,
                unlisted,
                class,
                ..
            } => (*unnumbered, *unlisted, class.as_ref().map(|class| class.name.clone())),
            block => panic!("not a heading: {block:?}"),
        };

        assert_eq!(flags("# Preface {-}"), (true, false, None));
        assert_eq!(flags("# Preface {.unnumbered .unlisted}"), (true, true, None));
        assert_eq!(
            flags("# Note {#sec:n - .aside}"),
            (true, false, Some("aside".to_owned()))
        );
        assert_eq!(flags("# Range 1-2"), (false, false, None));
    }

    #[test]
    fn reports_misplaced_and_repeated_book_parts() {
        let cases = [
            (
                "::: main-matter\n::: front-matter",
                (2, 1),
                "front-matter cannot follow the main-matter on line 1; use front-matter, main-matter, and \
                 back-matter once each, in this order",
            ),
            (
                "::: keep\n::: main-matter\n:::",
                (2, 1),
                "main-matter is only allowed outside other directives",
            ),
            (
                "::: toc\n::: toc",
                (2, 1),
                "the table of contents is already placed on line 1",
            ),
            (
                "::: columns\n::: toc\n:::",
                (2, 1),
                "toc is not allowed inside keep, bottom, or columns",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }

    #[test]
    fn parses_directives_between_blocks_but_not_inside_code() {
        let body = "::: keep\nOne\n\nTwo\n:::\n::: page-break\n\n:::: columns\nA\n\n::: full-width\nB\n:::\n::::\n\n```\n::: keep\n```\n";
        assert_eq!(
            blocks(body),
            vec![
                Block::Keep {
                    at: at(1, 1),
                    blocks: vec![paragraph(2, 1, "One"), paragraph(4, 1, "Two")],
                },
                Block::PageBreak { at: at(6, 1) },
                Block::Columns {
                    at: at(8, 1),
                    blocks: vec![
                        paragraph(9, 1, "A"),
                        Block::FullWidth {
                            at: at(11, 1),
                            blocks: vec![paragraph(12, 1, "B")],
                        },
                    ],
                },
                Block::Code {
                    at: at(16, 1),
                    first_line: 17,
                    language: None,
                    lines: vec!["::: keep".into()],
                },
            ]
        );
    }

    #[test]
    fn reports_directive_errors_at_their_lines() {
        let cases = [
            (
                "::: float\nx\n:::",
                (1, 1),
                "unknown directive \"float\"; use columns, full-width, keep, bottom, table, page-break, bibliography, \
                 index, toc, front-matter, main-matter, or back-matter",
            ),
            ("::: keep\nx", (1, 1), "\"::: keep\" is never closed"),
            ("x\n:::", (2, 1), "\":::\" closes no open layout directive"),
            (
                "::: keep\n::: page-break\n:::",
                (2, 1),
                "page-break is not allowed inside keep",
            ),
            (
                "::: bottom\n::: page-break\n:::",
                (2, 1),
                "page-break is not allowed inside bottom",
            ),
            (
                "::: columns\n::: bottom\nx\n:::\n:::",
                (2, 1),
                "bottom is only allowed outside other directives",
            ),
            ("::: columns\n::: columns\n:::\n:::", (2, 1), "columns cannot be nested"),
            (
                "::: keep\n::: columns\n:::\n:::",
                (2, 1),
                "columns is not allowed inside keep; put keep groups inside columns instead",
            ),
            (
                "::: full-width\n:::",
                (1, 1),
                "full-width is only allowed directly inside columns",
            ),
            (
                "- a\n\n::: keep\n- b\n\n:::",
                (3, 1),
                "a layout directive cannot interrupt a list, quotation, code block, or footnote; leave a blank line before it",
            ),
            (
                "> ::: keep",
                (1, 3),
                "a layout directive must start at the beginning of a line, outside lists, quotations, and footnotes",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }

    #[test]
    fn numbers_footnotes_in_reference_order() {
        let body = "[^b]: Bee.\n\nA[^a] and b[^B].\n\n[^a]: Ay.\n";
        let document = parse(body, 1, &source(), Lang::En).unwrap();
        assert_eq!(
            document.blocks,
            vec![Block::Paragraph {
                at: at(3, 1),
                content: vec![
                    plain("A"),
                    Inline::FootnoteRef(0),
                    plain(" and b"),
                    Inline::FootnoteRef(1),
                    plain("."),
                ],
                class: None,
            }]
        );
        assert_eq!(
            document.footnotes,
            vec![
                Footnote {
                    at: at(5, 1),
                    blocks: vec![paragraph(5, 7, "Ay.")],
                },
                Footnote {
                    at: at(1, 1),
                    blocks: vec![paragraph(1, 7, "Bee.")],
                },
            ]
        );
    }

    #[test]
    fn reports_footnote_errors() {
        let cases = [
            ("a[^x] b", (1, 2), "footnote [^x] has no definition"),
            (
                "a\n\n[^x]: unused",
                (3, 1),
                "footnote [^x] is defined but never referenced",
            ),
            (
                "a[^x] b[^x]\n\n[^x]: n",
                (1, 8),
                "footnote [^x] is already referenced on line 1; reference each footnote once",
            ),
            (
                "a[^x]\n\n[^x]: n\n[^x]: m",
                (4, 1),
                "footnote [^x] is already defined on line 3",
            ),
            (
                "a[^x] b[^y]\n\n[^x]: n[^y]\n\n[^y]: m",
                (3, 8),
                "a footnote cannot reference another footnote",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
        assert!(parse("a \\[^x] b", 1, &source(), Lang::En).is_ok());
    }

    #[test]
    fn reads_margin_notes_apart_from_numbered_footnotes() {
        let body = "A[^m] b[^n] c[^o].\n\n[^m]: {-} Aside.\n\n[^n]: Note.\n\n[^o]:   {-}\n    Second aside.\n";
        let document = parse(body, 1, &source(), Lang::En).unwrap();
        let [Block::Paragraph { content, .. }] = document.blocks.as_slice() else {
            panic!("one paragraph")
        };
        let marks: Vec<&Inline> = content
            .iter()
            .filter(|inline| !matches!(inline, Inline::Text { .. }))
            .collect();
        assert_eq!(
            marks,
            [&Inline::MarginNote(0), &Inline::FootnoteRef(0), &Inline::MarginNote(1)]
        );
        assert_eq!(document.footnotes[0].blocks, vec![paragraph(5, 7, "Note.")]);
        let texts: Vec<String> = document
            .margin_notes
            .iter()
            .map(|note| match note.blocks.as_slice() {
                [Block::Paragraph { content, .. }] => crate::layout::structure::plain(content),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(texts, ["Aside.", "Second aside."]);
        // An escaped mark is text, and so is `{-}` later in a note.
        let document = parse("A[^m].\n\n[^m]: \\{-} Not {-} an aside.\n", 1, &source(), Lang::En).unwrap();
        assert!(document.margin_notes.is_empty());
        let errors = diagnostics("A[^n]\n\n[^n]: Note[^m].\n\n[^m]: {-} Aside.\n", 1);
        assert!(errors.contains(&(Some((3, 11)), "a footnote cannot reference another footnote".to_owned())));
    }

    #[test]
    fn reads_placement_after_images_captions_and_on_the_table_directive() {
        let placement = |block: &Block| match block {
            Block::Image { placement, .. } | Block::Table { placement, .. } => *placement,
            other => panic!("{other:?}"),
        };
        let body = "![A](a.png){#fig:a placement=top}\n\n![B](b.png){placement=bottom}\n\n![C](c.png)\n\n\
            | A |\n|---|\n| 1 |\n\n: Caption {#tbl:a placement=bottom}\n\n::: table {placement=top}\n| B |\n|---|\n\
            | 2 |\n:::\n\n- ![In a list](d.png)\n";
        let document = parse(body, 1, &source(), Lang::En).unwrap();
        let found: Vec<Option<Placement>> = document.blocks[..5].iter().map(placement).collect();
        use Placement::{Bottom, Here, Top};
        assert_eq!(found, [Some(Top), Some(Bottom), None, Some(Bottom), Some(Top)]);
        let Block::List { items, .. } = &document.blocks[5] else {
            panic!("a list")
        };
        assert_eq!(
            placement(&items[0][0]),
            Some(Here),
            "the theme's default does not apply in a list"
        );
        let Block::Image { label, .. } = &document.blocks[0] else {
            panic!("an image")
        };
        assert_eq!(label.as_deref(), Some("fig:a"));

        let cases = [
            (
                "![A](a.png){placement=left}",
                (1, 12),
                "placement \"left\" is not here, top, or bottom",
            ),
            (
                "![A](a.png){placement=top .wide}",
                (1, 12),
                "an image takes a label and a placement, as in {#fig:name placement=top}",
            ),
            (
                "> ![A](a.png){placement=top}",
                (1, 14),
                "placement=top and placement=bottom apply only in the main text or in a full-width block of a column \
                 section",
            ),
            (
                "::: columns\n![A](a.png){placement=top}\n:::",
                (2, 12),
                "placement=top and placement=bottom apply only in the main text or in a full-width block of a column \
                 section",
            ),
            (
                "| A |\n|---|\n| 1[^n] |\n\n: Noted {placement=top}\n\n[^n]: Note.",
                (5, 1),
                "a table placed at the top or bottom cannot hold footnotes or margin notes",
            ),
            (
                "::: table {placement=top}\n| A |\n|---|\n| 1 |\n:::\n\n: Twice {placement=bottom}",
                (7, 1),
                "this table's placement is already set",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }

    fn cell(line: u64, column: u64, content: Vec<Inline>) -> Cell {
        Cell {
            at: at(line, column),
            blocks: vec![Block::Paragraph {
                at: at(line, column),
                content,
                class: None,
            }],
            span: 1,
            class: None,
        }
    }

    fn columns(align: &[Option<ColumnAlign>]) -> Vec<Column> {
        let column = |align| Column {
            align,
            width: ColumnWidth::Auto,
        };
        align.iter().copied().map(column).collect()
    }

    fn table(body: &str) -> Block {
        blocks(body).remove(0)
    }

    #[test]
    fn parses_tables_with_alignment_styles_and_padded_cells() {
        let em = InlineStyle {
            emphasis: true,
            ..Default::default()
        };
        let code = InlineStyle {
            code: true,
            ..Default::default()
        };
        let link = InlineStyle {
            link: Some(Link::Url("https://a.b/".into())),
            ..Default::default()
        };
        let body = "| A | B | C | D |\n|:--|:-:|--:|---|\n| *x* | `y` | [z](https://a.b/) | w |\n| short |\n";
        assert_eq!(
            table(body),
            Block::Table {
                at: at(1, 1),
                columns: columns(&[
                    Some(ColumnAlign::Left),
                    Some(ColumnAlign::Center),
                    Some(ColumnAlign::Right),
                    None
                ]),
                header: Row {
                    at: at(1, 1),
                    cells: vec![
                        cell(1, 3, vec![plain("A")]),
                        cell(1, 7, vec![plain("B")]),
                        cell(1, 11, vec![plain("C")]),
                        cell(1, 15, vec![plain("D")]),
                    ],
                    class: None,
                },
                rows: vec![
                    Row {
                        at: at(3, 1),
                        cells: vec![
                            cell(3, 3, vec![text("x", em)]),
                            cell(3, 9, vec![text("y", code)]),
                            cell(3, 15, vec![text("z", link)]),
                            cell(3, 35, vec![plain("w")]),
                        ],
                        class: None,
                    },
                    Row {
                        at: at(4, 1),
                        cells: vec![
                            cell(4, 3, vec![plain("short")]),
                            cell(4, 10, Vec::new()),
                            cell(4, 10, Vec::new()),
                            cell(4, 10, Vec::new()),
                        ],
                        class: None,
                    },
                ],
                caption: Vec::new(),
                label: None,
                placement: None,
            }
        );
    }

    #[test]
    fn takes_the_paragraph_after_a_table_as_its_caption() {
        let em = InlineStyle {
            emphasis: true,
            ..Default::default()
        };
        let blocks = blocks("| a |\n|---|\n| b |\n\n: The *table*\n\nAfter\n");
        assert_eq!(blocks.len(), 2);
        let Block::Table { caption, .. } = &blocks[0] else {
            panic!("table")
        };
        assert_eq!(caption, &vec![plain("The "), text("table", em)]);
        assert_eq!(blocks[1], paragraph(7, 1, "After"));
    }

    #[test]
    fn parses_tables_in_lists_quotes_and_layout_containers() {
        let body = "::: columns\n| a |\n|---|\n:::\n\n- | a |\n  |---|\n  | b |\n\n> | a |\n> |---|\n";
        let blocks = blocks(body);
        let Block::Columns { blocks: inner, .. } = &blocks[0] else {
            panic!("columns")
        };
        assert!(matches!(
            inner[0],
            Block::Table {
                at: Location { line: 2, column: 1 },
                ..
            }
        ));
        let Block::List { items, .. } = &blocks[1] else {
            panic!("list")
        };
        let Block::Table { rows, .. } = &items[0][0] else {
            panic!("table")
        };
        assert_eq!(rows[0].cells, vec![cell(8, 5, vec![plain("b")])]);
        let Block::Quote { blocks: quoted, .. } = &blocks[2] else {
            panic!("quote")
        };
        assert!(matches!(
            quoted[0],
            Block::Table {
                at: Location { line: 10, column: 3 },
                ..
            }
        ));
    }

    #[test]
    fn reports_table_errors() {
        let cases = [
            (
                "| a |\n|---|\n| b | c |",
                (3, 1),
                "this row has more cells than the table header",
            ),
            (
                "| a |\n|---|\n| b | c \\| d |\n| e |",
                (3, 1),
                "this row has more cells than the table header",
            ),
            (
                "| a |\n|---|\n| ![i](i.png) |",
                (3, 3),
                "images are not allowed in table cells",
            ),
            (
                "| a[^n] |\n|---|\n| b |\n\n[^n]: n",
                (1, 4),
                "a table header cannot hold footnotes, because it repeats on every page",
            ),
            (
                "a[^n]\n\n[^n]: | a |\n    |---|",
                (3, 7),
                "tables are not allowed in footnotes",
            ),
            (
                "a[^n]\n\n[^n]: Note\n\n    # Heading",
                (5, 5),
                "headings are not allowed in footnotes",
            ),
            (
                "text\n\n: Caption",
                (3, 1),
                "a table caption must directly follow its table",
            ),
            (
                "| a |\n|---|\n\n: One\n\n: Two",
                (6, 1),
                "a table caption must directly follow its table",
            ),
            (
                "| a |\n|---|\n\n: Caption {#tbl-a}",
                (4, 1),
                "\"tbl-a\" is not a label for a table: write {#tbl:name} with letters, digits, - and _",
            ),
            (
                "| a |\n|---|\n\n: Caption[^n]\n\n[^n]: n",
                (4, 10),
                "a table caption cannot hold footnotes",
            ),
            (
                "| a |\n|---|\n| b |\n: Caption",
                (4, 1),
                "leave a blank line between a table and its caption",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }

    #[test]
    fn parses_list_tables_with_column_attributes_block_cells_spans_and_styles() {
        let body = "::: table {align=\"left right\" widths=\"* auto\"}\n- - Item\n  - Amount\n- {.group}\n  - {span=2} \
            **Design**\n- - Styleguide\n\n    {.detail}\n    - Logo\n  - {.amount} 2,400\n:::\n\n: Costs {#tbl:costs}\n";
        let Block::Table {
            at: table_at,
            columns,
            header,
            rows,
            caption,
            label,
            ..
        } = table(body)
        else {
            panic!("a table")
        };
        assert_eq!(table_at, at(1, 1));
        let fill = |align, width| Column {
            align: Some(align),
            width,
        };
        assert_eq!(
            columns,
            [
                fill(ColumnAlign::Left, ColumnWidth::Fill),
                fill(ColumnAlign::Right, ColumnWidth::Auto)
            ]
        );
        assert_eq!(header.cells.len(), 2);
        assert_eq!((caption, label.as_deref()), (vec![plain("Costs")], Some("tbl:costs")));
        let strong = InlineStyle {
            strong: true,
            ..Default::default()
        };
        assert_eq!(rows[0].class, class("group", 4, 3));
        assert_eq!(rows[0].cells.len(), 1);
        assert_eq!((rows[0].cells[0].span, &rows[0].cells[0].class), (2, &None));
        assert_eq!(pipe_cell(&rows[0].cells[0]), [text("Design", strong)]);
        let [styleguide, amount] = rows[1].cells.as_slice() else {
            panic!("two cells")
        };
        assert!(
            matches!(styleguide.blocks.as_slice(), [Block::Paragraph { .. }, Block::List { class: detail, .. }]
            if *detail == class("detail", 8, 5))
        );
        assert_eq!((amount.at, &amount.class), (at(10, 3), &class("amount", 10, 5)));
        assert_eq!(pipe_cell(amount), [plain("2,400")]);
    }

    #[test]
    fn reports_list_table_errors_at_their_location() {
        let cases = [
            (
                "::: table\n- - A\n  - B\n- - x\n:::\n",
                (4, 1),
                "the cells of this row span 1 columns, but the header's span 2",
            ),
            (
                "::: table\n- Text\n  - A\n:::\n",
                (2, 3),
                "a table row holds only its cells, as a nested list with an item per cell",
            ),
            (
                "::: table\n- - A\n  - # Head\n:::\n",
                (3, 5),
                "a table cell holds only paragraphs and lists",
            ),
            (
                "::: table\n- {span=2}\n  - A\n:::\n",
                (2, 3),
                "a table row takes one style, as in {.name}",
            ),
            (
                "::: table\n- - {span=x} A\n:::\n",
                (2, 5),
                "span=\"x\" is not a number of columns",
            ),
            (
                "::: table {align=\"left\"}\n- - A\n  - B\n:::\n",
                (1, 1),
                "align has 1 entries, but the table has 2 columns",
            ),
            (
                "::: table {widths=\"50%\"}\n- - A\n:::\n",
                (1, 1),
                "width \"50%\" is not * or auto",
            ),
            (
                "::: keep {align=\"left\"}\nA\n:::\n",
                (1, 1),
                "\"keep\" takes no attributes; only table does",
            ),
            (
                "::: table\nText\n:::\n",
                (1, 1),
                "a table directive holds one list, with an item per row and a nested item per cell",
            ),
            ("::: table\n- - A[^n]\n:::\n\n[^n]: Note.\n", (2, 6), HEADER_NOTE),
        ];
        for (body, (line, column), message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some((line, column)), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }

    #[test]
    fn numbers_footnotes_in_table_cells_in_reading_order() {
        let body = "A[^a]\n\n| h |\n|---|\n| b[^b] |\n\nC[^c]\n\n[^a]: a\n\n[^b]: b\n\n[^c]: c\n";
        let document = parse(body, 1, &source(), Lang::En).unwrap();
        let Block::Table { rows, .. } = &document.blocks[1] else {
            panic!("table")
        };
        assert_eq!(pipe_cell(&rows[0].cells[0]), [plain("b"), Inline::FootnoteRef(1)]);
        let notes: Vec<_> = document.footnotes.iter().map(|note| note.at.line).collect();
        assert_eq!(notes, vec![9, 11, 13]);
    }

    fn reference(label: &str, page: bool, style: InlineStyle) -> Inline {
        Inline::Ref(Reference {
            label: label.into(),
            page,
            style,
        })
    }

    #[test]
    fn parses_labels_and_references_and_leaves_escaped_at_signs_as_text() {
        let body = "# Intro {#sec:intro}\n\n![A chart](c.svg){#fig:chart}\n\n| a |\n|---|\n\n: Data {#tbl:data}\n\n\
            See @sec:intro, *[@fig:chart, page]* and [@tbl:data]. Not @smith2024, \\@fig:chart, or a@fig:chart.\n";
        let blocks = blocks(body);
        let em = InlineStyle {
            emphasis: true,
            ..Default::default()
        };

        assert!(matches!(&blocks[0], Block::Heading { label: Some(l), .. } if l == "sec:intro"));
        assert!(matches!(&blocks[1], Block::Image { label: Some(l), caption, .. }
            if l == "fig:chart" && *caption == vec![plain("A chart")]));
        assert!(matches!(&blocks[2], Block::Table { label: Some(l), caption, .. }
            if l == "tbl:data" && *caption == vec![plain("Data")]));
        let Block::Paragraph { content, .. } = &blocks[3] else {
            panic!("paragraph")
        };
        assert_eq!(
            *content,
            vec![
                plain("See "),
                reference("sec:intro", false, InlineStyle::default()),
                plain(", "),
                reference("fig:chart", true, em),
                plain(" and "),
                reference("tbl:data", false, InlineStyle::default()),
                plain(". Not "),
                Inline::Citation {
                    index: 0,
                    style: InlineStyle::default(),
                },
                plain(", @fig:chart, or a@fig:chart."),
            ]
        );
    }

    #[test]
    fn reads_placeholders_in_text_and_leaves_attributes_escapes_and_code_alone() {
        let strong = InlineStyle {
            strong: true,
            ..Default::default()
        };
        let field = |placeholder, column, style| Inline::Field {
            placeholder,
            at: at(1, column),
            style,
        };
        assert_eq!(
            inlines("About **{words}** words, {meta.client}; not \\{year}, {a b}, `{year}` {.x}\n"),
            vec![
                plain("About "),
                field(Placeholder::Words, 9, strong.clone()),
                plain(" words, "),
                field(Placeholder::Meta("client".into()), 26, InlineStyle::default()),
                plain("; not {year}, {a b}, "),
                text(
                    "{year}",
                    InlineStyle {
                        code: true,
                        ..Default::default()
                    }
                ),
            ]
        );
    }

    #[test]
    fn reports_unknown_page_and_heading_placeholders() {
        let body = "# On {words} words\n\nPage {page} and {wrods}.\n";
        let errors = diagnostics(body, 1);
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert_eq!(
            errors[0],
            (Some((1, 6)), "a heading cannot hold a placeholder".to_owned())
        );
        assert_eq!(
            errors[1],
            (
                Some((3, 6)),
                "{page} depends on the page and is available only in headers and footers".to_owned()
            )
        );
        assert!(
            errors[2].1.starts_with("unknown placeholder {wrods}"),
            "{}",
            errors[2].1
        );
        assert!(errors[2].1.ends_with("; write \\{ for a brace"), "{}", errors[2].1);
    }

    fn styled(text: &str, style: InlineStyle) -> Inline {
        self::text(text, style)
    }

    #[test]
    fn cuts_comments_everywhere_but_code_and_keeps_later_locations() {
        let body =
            "One.\n\n<!-- a\n\nb -->\n\nTwo <!-- c --> {todo: x}.\n\n```\n<!-- code -->\n```\n\nLast `<!-- y -->`.\n";
        let blocks = blocks(body);
        assert_eq!(blocks[0], paragraph(1, 1, "One."));
        let Block::Paragraph { at: two, content, .. } = &blocks[1] else {
            panic!("expected a paragraph");
        };
        assert_eq!((*two, content[0].clone()), (at(7, 1), plain("Two  ")));
        assert_eq!(
            blocks[2],
            Block::Code {
                at: at(9, 1),
                first_line: 10,
                language: None,
                lines: vec!["<!-- code -->".into()],
            }
        );
        let Block::Paragraph { content, .. } = &blocks[3] else {
            panic!("expected a paragraph");
        };
        assert_eq!(
            content[1],
            styled(
                "<!-- y -->",
                InlineStyle {
                    code: true,
                    ..Default::default()
                }
            )
        );
        // The todo stands where it is written, after the comments before it.
        let document = parse(body, 1, &source(), Lang::En).expect("parses");
        assert_eq!(
            document.todos,
            vec![Todo {
                at: at(7, 16),
                text: "x".into()
            }]
        );
    }

    #[test]
    fn a_comment_between_paragraphs_is_a_blank_line() {
        let commented = blocks("One.\n\n<!-- note -->\n\nTwo.\nThree.\n<!-- note -->\nFour.\n");
        let removed = blocks("One.\n\n\n\nTwo.\nThree.\n\nFour.\n");
        assert_eq!(commented, removed);
        assert_eq!(commented.len(), 3);
    }

    #[test]
    fn reports_unterminated_comments_at_their_start_and_other_html_at_its_own_location() {
        let errors = diagnostics("One.\n\nTwo <!-- never\nclosed\n", 1);
        assert_eq!(
            errors,
            vec![(Some((3, 5)), "this comment is never closed; end it with -->".to_owned())]
        );
        let errors = diagnostics("<!-- fine -->\n<div>x</div>\n\nA <!-- c --><b>y</b>\n", 5);
        let found: Vec<_> = errors.iter().map(|(at, _)| *at).collect();
        assert_eq!(found, [Some((6, 1)), Some((8, 13)), Some((8, 17))], "{errors:?}");
    }

    #[test]
    fn reads_one_meaning_for_each_tilde_and_caret() {
        let mark = |text: &str, style: InlineStyle| styled(text, style);
        let strike = InlineStyle {
            strike: true,
            ..Default::default()
        };
        let sup = InlineStyle {
            superscript: true,
            ..Default::default()
        };
        let sub = InlineStyle {
            subscript: true,
            ..Default::default()
        };
        assert_eq!(
            inlines("~~a~~ ~b~ ^c^ H~2~O m^2^ 10^-3^, **~~d~~**\n"),
            vec![
                mark("a", strike.clone()),
                plain(" "),
                mark("b", sub.clone()),
                plain(" "),
                mark("c", sup.clone()),
                plain(" H"),
                mark("2", sub),
                plain("O m"),
                mark("2", sup.clone()),
                plain(" 10"),
                mark("-3", sup),
                plain(", "),
                mark(
                    "d",
                    InlineStyle {
                        strike: true,
                        strong: true,
                        ..Default::default()
                    }
                ),
            ]
        );
        assert_eq!(
            inlines("~/a and ~/b, 2^10 and 3^4, a ~ b ~ c, x~~y, \\~z~ \\^w^\n"),
            vec![plain("~/a and ~/b, 2^10 and 3^4, a ~ b ~ c, x~~y, ~z~ ^w^")]
        );
    }

    #[test]
    fn reads_underline_and_small_caps_spans_and_leaves_other_names_alone() {
        let underline = InlineStyle {
            underline: true,
            ..Default::default()
        };
        let caps = InlineStyle {
            small_caps: true,
            emphasis: true,
            ..Default::default()
        };
        assert_eq!(
            inlines("A [x]{.underline} and [y *z*]{.smallcaps} end [q]{.other} [r]{.underline}\n"),
            vec![
                plain("A "),
                styled("x", underline.clone()),
                plain(" and "),
                styled(
                    "y ",
                    InlineStyle {
                        small_caps: true,
                        ..Default::default()
                    }
                ),
                styled("z", caps),
                plain(" end [q]{.other} "),
                styled("r", underline),
            ]
        );
        let Block::Paragraph { class, .. } = &blocks("Text [q]{.other}\n")[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(class.as_ref().map(|class| class.name.as_str()), Some("other"));
    }

    #[test]
    fn reports_superscript_inside_subscript() {
        let errors = diagnostics("~a ^b^ c~\n", 1);
        assert_eq!(
            errors,
            vec![(Some((1, 4)), "superscript and subscript cannot be nested".to_owned())]
        );
    }

    #[test]
    fn reads_todos_inline_and_as_paragraphs_and_keeps_placeholders() {
        let todo = InlineStyle {
            todo: true,
            ..Default::default()
        };
        let document = parse(
            "Check {todo: the *date*\nagain} now, {words}.\n\n{todo: whole}\n",
            1,
            &source(),
            Lang::En,
        )
        .expect("parses");
        assert_eq!(
            document.todos,
            vec![
                Todo {
                    at: at(1, 7),
                    text: "the *date* again".into()
                },
                Todo {
                    at: at(4, 1),
                    text: "whole".into()
                },
            ]
        );
        let Block::Paragraph { content, .. } = &document.blocks[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(content[1], styled("TODO: the *date* again", todo.clone()));
        assert!(matches!(
            content[3],
            Inline::Field {
                placeholder: Placeholder::Words,
                ..
            }
        ));
        assert_eq!(
            document.blocks[1],
            Block::Paragraph {
                at: at(4, 1),
                content: vec![styled("TODO: whole", todo)],
                class: None
            }
        );
        let errors = diagnostics("# A {todo: x} b\n\n{todo:}\n\n{todo: open\n\nb} {todo}\n", 1);
        let messages: Vec<_> = errors.iter().map(|(at, message)| (*at, message.as_str())).collect();
        assert_eq!(messages[0], (Some((1, 5)), "a heading cannot hold a todo"));
        assert_eq!(
            messages[1],
            (
                Some((3, 1)),
                "write the note after {todo:, as in {todo: check the date}"
            )
        );
        assert_eq!(messages[2], (Some((5, 1)), "this todo is not closed; end it with }"));
        assert!(messages[3].1.starts_with("unknown placeholder {todo}"), "{messages:?}");
    }

    #[test]
    fn reports_label_and_reference_errors_at_their_location() {
        let cases = [
            (
                "# A {#fig:a}\n",
                (1, 1),
                "\"fig:a\" is not a label for a heading: write {#sec:name} with letters, digits, - and _",
            ),
            (
                "# A {#sec:a .wide data=x}\n",
                (1, 5),
                "a heading takes a label and one style, as in # Heading {#sec:name .style}",
            ),
            (
                "# A {#sec:a}\n\n# B {#sec:a}\n",
                (3, 1),
                "label sec:a is already defined on line 1",
            ),
            (
                "![](c.svg){#fig:c}\n",
                (1, 1),
                "a figure label needs a caption, as in ![Caption](file.png){#fig:name}",
            ),
            (
                "![C](c.svg){#tbl:c}\n",
                (1, 12),
                "\"tbl:c\" is not a label for a figure: write {#fig:name} with letters, digits, - and _",
            ),
            (
                "See @fig:missing.\n",
                (1, 5),
                "@fig:missing refers to an undefined label; define it with {#fig:missing}",
            ),
            (
                "# See @sec:a {#sec:a}\n",
                (1, 7),
                "a heading cannot hold a cross-reference",
            ),
            (
                "# A {#sec:a}\n\n[see @sec:a](https://a.b/)\n",
                (3, 6),
                "a cross-reference cannot be inside a link",
            ),
            (
                "# A {#sec:a}\n\n[@sec:a; p. 3]\n",
                (3, 1),
                "write a cross-reference in brackets as [@sec:a] or [@sec:a, page]",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }

    fn class(name: &str, line: u64, column: u64) -> Option<Class> {
        Some(Class {
            name: name.to_owned(),
            at: at(line, column),
        })
    }

    #[test]
    fn reads_styles_at_the_end_of_headings_and_paragraphs_and_before_lists() {
        let body = "# Step {#sec:s .step}\n\nKicker {.eyebrow}\n\n{.checks}\n- a\n\nIntro:\n{.checks}\n- b\n  {.dashes}\n  - c\n\nNot \\{.x}\n";
        let blocks = blocks(body);
        assert!(matches!(&blocks[0], Block::Heading { label: Some(l), class: c, .. }
            if l == "sec:s" && *c == class("step", 1, 8)));
        assert_eq!(
            blocks[1],
            Block::Paragraph {
                at: at(3, 1),
                content: vec![plain("Kicker")],
                class: class("eyebrow", 3, 8),
            }
        );
        assert!(matches!(&blocks[2], Block::List { class: c, .. } if *c == class("checks", 5, 1)));
        assert_eq!(blocks[3], paragraph(8, 1, "Intro:"));
        let Block::List { class: c, items, .. } = &blocks[4] else {
            panic!("a list")
        };
        assert_eq!(*c, class("checks", 9, 1));
        assert!(matches!(&items[0][1], Block::List { class: c, .. } if *c == class("dashes", 11, 3)));
        assert_eq!(blocks[5], paragraph(14, 1, "Not {.x}"));
    }

    #[test]
    fn reports_misplaced_style_attributes_at_their_location() {
        let own_line = "{.x} on a line of its own must directly precede a list; to style a paragraph or heading, \
            write it at the end of its text";
        let cases = [
            ("{.x}\n\nText\n", (1, 1), own_line),
            ("- a\n{.x}\n- b\n", (2, 1), own_line),
            (
                "# A {.a .b}\n",
                (1, 5),
                "a heading takes a label and one style, as in # Heading {#sec:name .style}",
            ),
            (
                "- {.x}\n",
                (1, 3),
                "{.x} needs text before it, or a line of its own directly before a list",
            ),
        ];
        for (body, location, message) in cases {
            assert_eq!(
                diagnostics(body, 1),
                vec![(Some(location), message.to_string())],
                "{body:?}"
            );
        }
    }

    fn citation(index: usize) -> Inline {
        Inline::Citation {
            index,
            style: InlineStyle::default(),
        }
    }

    #[test]
    fn parses_citations_with_their_key_locations_and_adds_the_bibliography_at_the_end() {
        let body = "As [@a, p. 3; @b] and @c [pp. 3-5] show,\n[@d;\n@e] and [ask @f](https://x.org/).\n\n# End\n";
        let document = parse(body, 1, &source(), Lang::En).expect("parses");
        let link = InlineStyle {
            link: Some(Link::Url("https://x.org/".into())),
            ..Default::default()
        };
        assert_eq!(
            document.blocks[0],
            Block::Paragraph {
                at: at(1, 1),
                content: vec![
                    plain("As "),
                    citation(0),
                    plain(" and "),
                    citation(1),
                    plain(" show, "),
                    citation(2),
                    plain(" and "),
                    text("ask @f", link),
                    plain("."),
                ],
                class: None,
            }
        );
        let keys: Vec<_> = document
            .citations
            .iter()
            .map(|citation| citation.keys.clone())
            .collect();
        assert_eq!(
            keys,
            [vec![at(1, 5), at(1, 15)], vec![at(1, 23)], vec![at(2, 2), at(3, 1)]]
        );
        assert!(document.citations[1].syntax.items[0].locator.is_some());
        assert_eq!(document.blocks[2], Block::Bibliography { at: at(5, 6) });

        let placed = blocks("::: columns\n::: bibliography\n:::\n\nText [@a].\n");
        assert_eq!(
            placed[0],
            Block::Columns {
                at: at(1, 1),
                blocks: vec![Block::Bibliography { at: at(2, 1) }],
            }
        );
        assert_eq!(placed.len(), 2, "the marker places the only bibliography");
    }

    #[test]
    fn reports_citation_and_bibliography_errors_at_their_location() {
        let cases = [
            (
                "x [@a see below] y\n",
                (1, 3),
                "unexpected `see below` after the key `a`; a locator or suffix follows a comma",
            ),
            (
                "See [@a; @sec:b].\n",
                (1, 5),
                "a citation group cannot hold a cross-reference; write them apart, as in [@key] and @sec:name",
            ),
            (
                "See [@sec:b; @a].\n",
                (1, 5),
                "write a cross-reference in brackets as [@sec:b] or [@sec:b, page]",
            ),
            ("# Heading @a\n", (1, 11), "a heading cannot hold a citation"),
            (
                "::: bibliography\n\n::: bibliography\n",
                (3, 1),
                "the bibliography is already placed on line 1",
            ),
            (
                "::: keep\n::: bibliography\n:::\n",
                (2, 1),
                "bibliography is not allowed inside keep",
            ),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }

    #[test]
    fn parses_github_alerts_as_callouts_and_leaves_other_quotations_alone() {
        let kinds = [
            ("NOTE", CalloutKind::Note),
            ("TIP", CalloutKind::Tip),
            ("IMPORTANT", CalloutKind::Important),
            ("WARNING", CalloutKind::Warning),
            ("CAUTION", CalloutKind::Caution),
        ];
        for (marker, expected) in kinds {
            let blocks = blocks(&format!("> [!{marker}]\n> Text.\n>\n> - Item\n"));
            let [Block::Callout { kind, blocks, .. }] = blocks.as_slice() else {
                panic!("{marker}: {blocks:?}")
            };
            assert_eq!(*kind, expected);
            assert!(matches!(
                blocks.as_slice(),
                [Block::Paragraph { .. }, Block::List { .. }]
            ));
        }
        for body in ["> Quoted\n", "> [!HINT]\n> Text.\n", "> [!NOTE] Own title\n> Text.\n"] {
            assert!(matches!(blocks(body).as_slice(), [Block::Quote { .. }]), "{body:?}");
        }
    }

    fn mark(line: u64, column: u64, terms: &[&str], see: Option<(bool, &[&str])>) -> IndexMark {
        let owned = |terms: &[&str]| terms.iter().map(|term| (*term).to_owned()).collect();
        IndexMark {
            at: at(line, column),
            terms: owned(terms),
            see: see.map(|(also, terms)| See {
                also,
                terms: owned(terms),
            }),
        }
    }

    #[test]
    fn parses_index_marks_with_visible_text_sub_entries_and_see_references() {
        let body = "Text{^Hyphenation!patterns!German} on [line *breaking*]{^}.\n\n\
            - [Knuth]{^Knuth, Donald|see also TeX} and {^TeX|siehe Knuth, Donald}\n\n\
            > Quoted {^quote}.[^n]\n\n\
            | A | B |\n|---|---|\n| cell{^cell} | `{^code}` \\{^escaped} |\n\n\
            [^n]: Noted{^note}.\n";
        let document = parse(body, 1, &source(), Lang::En).expect("parses");
        assert_eq!(
            document.index_marks,
            [
                mark(1, 5, &["Hyphenation", "patterns", "German"], None),
                mark(1, 39, &["line breaking"], None),
                mark(3, 3, &["Knuth, Donald"], Some((true, &["TeX"]))),
                mark(3, 44, &["TeX"], Some((false, &["Knuth, Donald"]))),
                mark(5, 10, &["quote"], None),
                mark(9, 7, &["cell"], None),
                mark(11, 12, &["note"], None),
            ]
        );
        let Block::Paragraph { content, .. } = &document.blocks[0] else {
            panic!("a paragraph")
        };
        let emphasis = InlineStyle {
            emphasis: true,
            ..InlineStyle::default()
        };
        assert_eq!(
            content,
            &[
                plain("Text"),
                Inline::IndexMark(0),
                plain(" on "),
                Inline::IndexMark(1),
                plain("line "),
                text("breaking", emphasis),
                plain("."),
            ]
        );
        assert_eq!(document.blocks.last(), Some(&Block::Index { at: at(11, 20) }));
        let Block::Table { rows, .. } = &document.blocks[3] else {
            panic!("a table")
        };
        let code = InlineStyle {
            code: true,
            ..InlineStyle::default()
        };
        let cell = rows[0].cells[1].blocks.clone();
        let expected = vec![text("{^code}", code), plain(" {^escaped}")];
        assert!(
            matches!(&cell[..], [Block::Paragraph { content, .. }] if *content == expected),
            "{cell:?}"
        );
    }

    #[test]
    fn places_the_index_where_the_directive_stands_also_without_marks() {
        let placed = blocks("::: columns\n::: index\n:::\n\nText{^a}.\n");
        assert_eq!(
            placed[0],
            Block::Columns {
                at: at(1, 1),
                blocks: vec![Block::Index { at: at(2, 1) }],
            }
        );
        assert_eq!(placed.len(), 2, "the directive places the only index");
        assert_eq!(blocks("::: index\n"), [Block::Index { at: at(1, 1) }]);
    }

    #[test]
    fn reports_index_mark_errors_at_their_location() {
        let empty =
            "an index mark needs a term for the entry and each sub-entry, as in {^entry!sub-entry} or [text]{^}";
        let cases = [
            ("# Heading{^a}\n", (1, 10), HEADING_MARK),
            ("# Heading {^a} text\n", (1, 11), HEADING_MARK),
            (
                "| A{^a} |\n|---|\n| b |\n",
                (1, 4),
                "a table header cannot hold index marks, because it repeats on every page",
            ),
            ("Text {^}.\n", (1, 6), empty),
            ("Text {^a!!b}.\n", (1, 6), empty),
            (
                "Text {^a!b!c!d}.\n",
                (1, 6),
                "an index mark takes an entry and at most two levels of sub-entries, as in {^entry!sub-entry!detail}",
            ),
            (
                "Text {^a|compare b}.\n",
                (1, 6),
                "write a cross-reference in an index mark as |see entry or |see also entry",
            ),
            (
                "Text {^a|see }.\n",
                (1, 6),
                "a see reference needs the entry it refers to, as in |see entry!sub-entry",
            ),
            (
                "Text {^a|see b!c}{^b}.\n",
                (1, 6),
                "the index has no entry \"b!c\" for this reference; mark it with {^b!c}",
            ),
            (
                "Text {^a\n\nmore}.\n",
                (1, 6),
                "this index mark is not closed; end it with }",
            ),
            (
                "{^lonely}\n",
                (1, 1),
                "an index mark needs text beside it; put it in the paragraph it refers to",
            ),
            (
                "[*a* @sec:x]{^}\n\n# X {#sec:x}\n",
                (1, 1),
                "the text of an index mark can hold only text and formatting",
            ),
            (
                "::: index\n\n::: index\n",
                (3, 1),
                "the index is already placed on line 1",
            ),
            ("::: keep\n::: index\n:::\n", (2, 1), "index is not allowed inside keep"),
        ];
        for (body, location, message) in cases {
            let errors = diagnostics(body, 1);
            assert!(
                errors.contains(&(Some(location), message.to_string())),
                "{body:?}: {errors:?}"
            );
        }
    }
}
