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
//! A custom style is applied with `{.name}` at the end of a heading or paragraph, or on a line of its
//! own directly before a list. A heading may combine it with its label, as in `{#sec:name .name}`.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::bibliography::syntax::{self, Segment};
use crate::config::source::Source;
use crate::diagnostic::Diagnostic;
use crate::document::{
    Block, Cell, Citation, Class, ColumnAlign, Document, Footnote, ImageFile, Inline, InlineStyle, LabelKind, Link,
    Location, Reference, Row,
};

/// Parses the Markdown body that starts at 1-based line `first_line` of the document `source`.
///
/// Tables become [`Block::Table`], with a `: Caption` paragraph after the table as its caption.
/// Every construct that cannot be rendered yet is reported with its location. The result is
/// either the complete document or every such diagnostic, never a document with content dropped.
pub fn parse(body: &str, first_line: u64, source: &Source) -> Result<Document, Vec<Diagnostic>> {
    // Strikethrough and task lists are enabled only so they can be recognised and reported.
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES;
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
    let mut builder = Builder::new(&text, first_line, source, directives);
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
}

enum Container {
    Quote(Location),
    List {
        at: Location,
        start: Option<u64>,
        items: Vec<Vec<Block>>,
        class: Option<Class>,
    },
    Item,
    /// `None` for an unknown directive name, which has been reported.
    Directive {
        at: Location,
        fence: Option<Fence>,
    },
    Footnote {
        at: Location,
        label: String,
    },
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
    /// The custom style of a heading.
    class: Option<Class>,
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
    align: Vec<Option<ColumnAlign>>,
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
    text: String,
}

struct Builder<'a> {
    body: &'a str,
    source: &'a Source,
    first_line: u64,
    /// Byte offset of each line start in `body`.
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
    /// Footnote definitions by case-folded label: written label, location, and content.
    definitions: HashMap<String, (String, Location, Vec<Block>)>,
    /// Image files in order of first use, and their indexes by path.
    images: Vec<ImageFile>,
    image_index: HashMap<String, usize>,
    /// Defined labels with their locations, and the references to check against them.
    labels: HashMap<String, Location>,
    references_to: Vec<(String, Location)>,
    citations: Vec<Citation>,
    /// Where `::: bibliography` stands, if it does.
    bibliography: Option<Location>,
    /// Source text before this offset was read as a label, reference, or citation, so text events in it are skipped.
    consumed: usize,
    /// A `{.name}` on a line of its own, waiting for the list it must precede.
    pending_class: Option<Class>,
}

impl<'a> Builder<'a> {
    fn new(body: &'a str, first_line: u64, source: &'a Source, directives: Vec<(usize, &'a str)>) -> Self {
        let newlines = body.match_indices('\n').map(|(index, _)| index + 1);
        Self {
            body,
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
            consumed: 0,
            pending_class: None,
        }
    }

    fn finish(mut self) -> Result<Document, Vec<Diagnostic>> {
        let end = self.body.len();
        self.directives_before(end..end);
        self.misplaced_class();
        while let Some(Container::Directive { at, fence }) = self.containers.pop() {
            if let Some(fence) = fence {
                self.report_at(at, format!("\"::: {}\" is never closed", fence_name(fence)));
            }
        }

        let mut footnotes = Vec::with_capacity(self.references.len());
        for (key, at) in std::mem::take(&mut self.references) {
            match self.definitions.remove(&key) {
                Some((_, defined, blocks)) => footnotes.push(Footnote { at: defined, blocks }),
                None => self.report_at(at, format!("footnote [^{key}] has no definition")),
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

        let mut unused: Vec<_> = self
            .definitions
            .drain()
            .map(|(_, (label, at, _))| (at, label))
            .collect();
        unused.sort_by_key(|(at, _)| (at.line, at.column));
        for (at, label) in unused {
            let message = format!("footnote [^{label}] is defined but never referenced");
            self.report_at(at, message);
        }

        if !self.diagnostics.is_empty() {
            self.diagnostics.sort_by_key(|d| d.location);
            return Err(self.diagnostics);
        }
        let mut blocks = self.blocks.pop().unwrap_or_default();
        if !self.citations.is_empty() && self.bibliography.is_none() {
            let at = self.location(self.body.trim_end().len());
            blocks.push(Block::Bibliography { at });
        }
        Ok(Document {
            blocks,
            footnotes,
            images: self.images,
            citations: self.citations,
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
        let name = text.trim_start_matches(':').trim();
        let open: Vec<Option<Fence>> = self
            .containers
            .iter()
            .filter_map(|container| match container {
                Container::Directive { fence, .. } => Some(*fence),
                _ => None,
            })
            .collect();
        let fence = match name {
            "" => {
                // Only directive containers are open between top-level blocks.
                let Some(Container::Directive { at, fence }) = self.containers.pop() else {
                    self.report(offset, "\":::\" closes no open layout directive");
                    return;
                };
                let blocks = self.blocks.pop().unwrap_or_default();
                match fence {
                    Some(Fence::Keep) => self.push_block(Block::Keep { at, blocks }),
                    Some(Fence::Columns) => self.push_block(Block::Columns { at, blocks }),
                    Some(Fence::FullWidth) => self.push_block(Block::FullWidth { at, blocks }),
                    None => {}
                }
                return;
            }
            "page-break" => {
                if open.contains(&Some(Fence::Keep)) {
                    self.report(offset, "page-break is not allowed inside keep");
                } else {
                    self.push_block(Block::PageBreak { at });
                }
                return;
            }
            "bibliography" => {
                if open.contains(&Some(Fence::Keep)) {
                    self.report(offset, "bibliography is not allowed inside keep");
                } else if let Some(first) = self.bibliography {
                    let message = format!("the bibliography is already placed on line {}", first.line);
                    self.report(offset, message);
                } else {
                    self.bibliography = Some(at);
                    self.push_block(Block::Bibliography { at });
                }
                return;
            }
            "keep" => Some(Fence::Keep),
            "columns" => {
                if open.contains(&Some(Fence::Columns)) {
                    self.report(offset, "columns cannot be nested");
                } else if open.contains(&Some(Fence::Keep)) {
                    self.report(
                        offset,
                        "columns is not allowed inside keep; put keep groups inside columns instead",
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
                let message =
                    format!("unknown directive \"{name}\"; use columns, full-width, keep, page-break, or bibliography");
                self.report(offset, message);
                None
            }
        };
        self.containers.push(Container::Directive { at, fence });
        self.blocks.push(Vec::new());
    }

    /// The file location of a byte offset in `body`.
    fn location(&self, offset: usize) -> Location {
        let line = self.line_starts.partition_point(|&start| start <= offset);
        let line_start = self.line_starts[line - 1];
        Location {
            line: line as u64 + self.first_line - 1,
            column: self.body[line_start..offset].chars().count() as u64 + 1,
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
            class: None,
            text_end: offset,
        });
    }

    fn close_leaf(&mut self) {
        let Some(Leaf {
            at,
            level,
            content,
            image,
            label,
            class,
            text_end,
            ..
        }) = self.leaf.take()
        else {
            return;
        };
        let block = match (level, image) {
            (Some(level), _) => Block::Heading {
                at,
                level,
                content,
                label,
                class,
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
        let label = self.caption_label(at, &mut content);
        if let Some(Block::Table {
            caption,
            label: table_label,
            ..
        }) = self.blocks.last_mut().and_then(|blocks| blocks.last_mut())
        {
            *caption = content;
            *table_label = label;
        }
    }

    /// Removes a `{#tbl:name}` label from the end of a table caption and registers it.
    fn caption_label(&mut self, at: Location, content: &mut Vec<Inline>) -> Option<String> {
        let Some(Inline::Text { text, style }) = content.last_mut() else {
            return None;
        };
        let trimmed = text.trim_end();
        if style.code || !trimmed.ends_with('}') {
            return None;
        }
        let start = trimmed.rfind("{#")?;
        let label = trimmed[start + 2..trimmed.len() - 1].to_owned();
        text.truncate(start);
        text.truncate(text.trim_end().len());
        if text.is_empty() {
            content.pop();
        }
        if content.is_empty() {
            self.report_at(at, "a table caption needs text before its label");
        }
        self.define(at, &label, LabelKind::Table)
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
            let message = format!("label {label} is already defined on line {}", first.line);
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
        self.push_inline(
            offset,
            Inline::Text {
                text: text.to_owned(),
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
                    let end = range.end;
                    self.text(&text, range);
                    if let Some(leaf) = self.leaf.as_mut() {
                        leaf.text_end = end;
                    }
                }
            },
            Event::Code(text) => self.push_text(offset, &text, true),
            // A citation group may span lines.
            Event::SoftBreak if offset < self.consumed => {}
            Event::SoftBreak => self.push_text(offset, " ", false),
            Event::HardBreak => self.push_inline(offset, Inline::LineBreak),
            Event::Rule => {
                self.close_implicit_leaf();
                self.report(offset, "thematic breaks are not supported");
            }
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
        if text == "[" {
            self.check_undefined_footnote(offset);
            if self.bracketed_reference(offset) || self.bracketed_citation(offset) {
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
        for (index, _) in text.match_indices('@') {
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
        if !self.links.is_empty() || !self.body[offset + 1..].trim_start().starts_with('@') {
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

    /// Reads a `{#fig:name}` label directly after an image that ends at `end`, spaces allowed between.
    fn end_image(&mut self, end: usize) {
        let Some(image) = self.leaf.as_mut().and_then(|leaf| leaf.image.as_mut()) else {
            return;
        };
        image.closed = true;
        let rest = &self.body[end..];
        let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let Some(attribute) = rest[spaces..].strip_prefix("{#") else {
            return;
        };
        let Some(close) = attribute
            .find('}')
            .filter(|&close| !attribute[..close].contains(char::is_whitespace))
        else {
            return;
        };
        let label = attribute[..close].to_owned();
        self.consumed = end + spaces + "{#".len() + close + 1;
        let at = self.location(end + spaces);
        let label = self.define(at, &label, LabelKind::Figure);
        if let Some(leaf) = self.leaf.as_mut() {
            leaf.label = label;
        }
    }

    fn footnote_reference(&mut self, offset: usize, label: &str) {
        let at = self.location(offset);
        if self.containers.iter().any(|c| matches!(c, Container::Footnote { .. })) {
            self.report_at(at, "a footnote cannot reference another footnote");
            return;
        }
        if self.table.as_ref().is_some_and(|table| table.in_header) {
            let message = "a table header cannot hold footnotes, because it repeats on every page";
            self.report_at(at, message);
            return;
        }
        let key = label.to_lowercase();
        if let Some(&index) = self.referenced.get(&key) {
            let first = self.references[index].1.line;
            let message =
                format!("footnote [^{label}] is already referenced on line {first}; reference each footnote once");
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
                self.open_leaf(offset, Some(level as u8), false);
                let brace = self.body[range].rfind('{').map_or(offset, |index| offset + index);
                if !attrs.is_empty() || classes.len() > 1 {
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
                }
            }
            Tag::BlockQuote(_) => {
                self.close_implicit_leaf();
                self.containers.push(Container::Quote(self.location(offset)));
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
                self.containers.push(Container::Item);
                self.blocks.push(Vec::new());
            }
            Tag::CodeBlock(kind) => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                let first_line = match kind {
                    CodeBlockKind::Fenced(_) => at.line + 1,
                    CodeBlockKind::Indented => at.line,
                };
                self.code = Some(CodeBlock {
                    at,
                    first_line,
                    text: String::new(),
                });
            }
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strong => self.strong += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Strikethrough => self.report(offset, "strikethrough is not supported"),
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
                    align: align.into_iter().map(column_align).collect(),
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
                    class: None,
                    text_end: start,
                });
            }
            Tag::FootnoteDefinition(label) => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                self.containers.push(Container::Footnote {
                    at,
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
        };
        let too_many = cell_count(&self.body[table.row_source.clone()]) > table.align.len();
        let caption = matches!(row.cells.split_first(), Some((first, rest))
            if starts_with_caption_marker(&first.content) && rest.iter().all(|cell| cell.content.is_empty()));
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

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.close_leaf(),
            TagEnd::BlockQuote(_) => {
                let blocks = self.blocks.pop().unwrap_or_default();
                if let Some(Container::Quote(at)) = self.containers.pop() {
                    self.push_block(Block::Quote { at, blocks });
                }
            }
            TagEnd::Item => {
                self.close_implicit_leaf();
                let blocks = self.blocks.pop().unwrap_or_default();
                self.containers.pop();
                if let Some(Container::List { items, .. }) = self.containers.last_mut() {
                    items.push(blocks);
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
                    self.push_block(Block::List {
                        at,
                        start,
                        items,
                        class,
                    });
                }
            }
            TagEnd::CodeBlock => {
                if let Some(CodeBlock { at, first_line, text }) = self.code.take() {
                    let lines = text.lines().map(str::to_owned).collect();
                    self.push_block(Block::Code { at, first_line, lines });
                }
            }
            TagEnd::FootnoteDefinition => {
                self.close_implicit_leaf();
                let blocks = self.blocks.pop().unwrap_or_default();
                if let Some(Container::Footnote { at, label }) = self.containers.pop() {
                    let key = label.to_lowercase();
                    match self.definitions.get(&key) {
                        Some((_, first, _)) => {
                            let message = format!("footnote [^{label}] is already defined on line {}", first.line);
                            self.report_at(at, message);
                        }
                        None => {
                            self.definitions.insert(key, (label, at, blocks));
                        }
                    }
                }
            }
            TagEnd::TableCell => {
                if let (Some(Leaf { at, content, .. }), Some(table)) = (self.leaf.take(), self.table.as_mut()) {
                    table.cells.push(Cell { at, content });
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.header = Some(Row {
                        at: table.row_at,
                        cells: std::mem::take(&mut table.cells),
                    });
                    table.in_header = false;
                }
            }
            TagEnd::TableRow => self.end_table_row(),
            TagEnd::Table => {
                if let Some(TableState {
                    at,
                    align,
                    header: Some(header),
                    rows,
                    ..
                }) = self.table.take()
                {
                    self.push_block(Block::Table {
                        at,
                        align,
                        header,
                        rows,
                        caption: Vec::new(),
                        label: None,
                    });
                }
            }
            TagEnd::Emphasis => self.emphasis -= 1,
            TagEnd::Strong => self.strong -= 1,
            TagEnd::Link => {
                self.links.pop();
            }
            _ => {}
        }
    }
}

const ALONE: &str = "an image must stand alone in its paragraph";
const BROKEN_IMAGE: &str = "this image is not valid Markdown: write ![Caption](file.png), with <> around a path \
    that has spaces; a caption cannot hold footnotes";

fn column_align(align: Alignment) -> Option<ColumnAlign> {
    match align {
        Alignment::None => None,
        Alignment::Left => Some(ColumnAlign::Left),
        Alignment::Center => Some(ColumnAlign::Center),
        Alignment::Right => Some(ColumnAlign::Right),
    }
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

fn fence_name(fence: Fence) -> &'static str {
    match fence {
        Fence::Columns => "columns",
        Fence::FullWidth => "full-width",
        Fence::Keep => "keep",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Source {
        Source::Document("/doc.md".into())
    }

    fn blocks(body: &str) -> Vec<Block> {
        parse(body, 1, &source()).expect("parses").blocks
    }

    fn diagnostics(body: &str, first_line: u64) -> Vec<(Option<(u64, u64)>, String)> {
        let errors = parse(body, first_line, &source()).expect_err("reports");
        errors.into_iter().map(|d| (d.location, d.message)).collect()
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
                },
                Block::Heading {
                    at: at(3, 1),
                    level: 2,
                    content: vec![plain("Setext")],
                    label: None,
                    class: None,
                },
                paragraph(6, 1, "Text"),
                Block::Quote {
                    at: at(8, 1),
                    blocks: vec![paragraph(8, 3, "Quoted")],
                },
                Block::Code {
                    at: at(10, 1),
                    first_line: 11,
                    lines: vec!["let a = 1;".into(), "".into(), "let b = 2;".into()],
                },
                Block::Code {
                    at: at(16, 5),
                    first_line: 16,
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
        let document = parse("intro\n\n> ünï **bold** after\n", 10, &source()).unwrap();
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
            ("---", "thematic breaks are not supported"),
            ("- [ ] todo", "task lists are not supported"),
            ("~~gone~~", "strikethrough is not supported"),
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
                (Some((5, 1)), "thematic breaks are not supported".to_string()),
                (Some((7, 6)), "raw HTML is not rendered".to_string()),
                (Some((7, 10)), "raw HTML is not rendered".to_string()),
            ]
        );
    }

    #[test]
    fn parses_images_alone_in_a_paragraph_with_the_description_as_caption() {
        let document = parse(
            "![A *plot*](img/a.png)\n\n- ![](b.svg)\n\n![Again](img/a.png)\n",
            1,
            &source(),
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
                },
                Block::List {
                    at: at(3, 1),
                    start: None,
                    items: vec![vec![Block::Image {
                        at: at(3, 3),
                        image: 1,
                        caption: Vec::new(),
                        label: None,
                    }]],
                    class: None,
                },
                Block::Image {
                    at: at(5, 1),
                    image: 0,
                    caption: vec![plain("Again")],
                    label: None,
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
                "unknown directive \"float\"; use columns, full-width, keep, page-break, or bibliography",
            ),
            ("::: keep\nx", (1, 1), "\"::: keep\" is never closed"),
            ("x\n:::", (2, 1), "\":::\" closes no open layout directive"),
            (
                "::: keep\n::: page-break\n:::",
                (2, 1),
                "page-break is not allowed inside keep",
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
        let document = parse(body, 1, &source()).unwrap();
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
        assert!(parse("a \\[^x] b", 1, &source()).is_ok());
    }

    fn cell(line: u64, column: u64, content: Vec<Inline>) -> Cell {
        Cell {
            at: at(line, column),
            content,
        }
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
                align: vec![
                    Some(ColumnAlign::Left),
                    Some(ColumnAlign::Center),
                    Some(ColumnAlign::Right),
                    None
                ],
                header: Row {
                    at: at(1, 1),
                    cells: vec![
                        cell(1, 3, vec![plain("A")]),
                        cell(1, 7, vec![plain("B")]),
                        cell(1, 11, vec![plain("C")]),
                        cell(1, 15, vec![plain("D")]),
                    ],
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
                    },
                    Row {
                        at: at(4, 1),
                        cells: vec![
                            cell(4, 3, vec![plain("short")]),
                            cell(4, 10, Vec::new()),
                            cell(4, 10, Vec::new()),
                            cell(4, 10, Vec::new()),
                        ],
                    },
                ],
                caption: Vec::new(),
                label: None,
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
    fn numbers_footnotes_in_table_cells_in_reading_order() {
        let body = "A[^a]\n\n| h |\n|---|\n| b[^b] |\n\nC[^c]\n\n[^a]: a\n\n[^b]: b\n\n[^c]: c\n";
        let document = parse(body, 1, &source()).unwrap();
        let Block::Table { rows, .. } = &document.blocks[1] else {
            panic!("table")
        };
        assert_eq!(rows[0].cells[0].content, vec![plain("b"), Inline::FootnoteRef(1)]);
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
        let document = parse(body, 1, &source()).expect("parses");
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
                "x [@a, see below] y\n",
                (1, 3),
                "unsupported locator `see below`; use `p. 12`, `pp. 3-5`, or `S. 12`",
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
}
