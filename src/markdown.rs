//! CommonMark parsing into the [document model](crate::document), with footnotes and layout
//! directives.
//!
//! Directive lines (`::: name` and `:::`) are found before parsing: every line starting with
//! `:::` outside a code block. They are blanked, so CommonMark sees a blank line there, and
//! applied between the top-level blocks they stand between.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::config::source::Source;
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Document, Footnote, Inline, InlineStyle, Location};

/// Parses the Markdown body that starts at 1-based line `first_line` of the document `source`.
///
/// Every construct that cannot be rendered yet is reported with its location. The result is
/// either the complete document or every such diagnostic, never a document with content dropped.
pub fn parse(body: &str, first_line: u64, source: &Source) -> Result<Document, Vec<Diagnostic>> {
    // Tables, strikethrough, and task lists are enabled only so they can be recognised and reported.
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
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
        }
    }

    fn finish(mut self) -> Result<Document, Vec<Diagnostic>> {
        let end = self.body.len();
        self.directives_before(end..end);
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
        let blocks = self.blocks.pop().unwrap_or_default();
        Ok(Document { blocks, footnotes })
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
            "keep" => Some(Fence::Keep),
            "columns" => {
                if open.contains(&Some(Fence::Columns)) {
                    self.report(offset, "columns cannot be nested");
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
                    format!("unknown layout directive \"{name}\"; use columns, full-width, keep, or page-break");
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
            link: self.links.last().cloned(),
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
        });
    }

    fn close_leaf(&mut self) {
        let Some(Leaf { at, level, content, .. }) = self.leaf.take() else {
            return;
        };
        self.push_block(match level {
            Some(level) => Block::Heading { at, level, content },
            None => Block::Paragraph { at, content },
        });
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
            Event::Start(tag) => self.start(tag, offset),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match self.code.as_mut() {
                Some(code) => code.text.push_str(&text),
                None => {
                    if &*text == "[" {
                        self.check_undefined_footnote(offset);
                    }
                    self.push_text(offset, &text, false);
                }
            },
            Event::Code(text) => self.push_text(offset, &text, true),
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

    fn footnote_reference(&mut self, offset: usize, label: &str) {
        let at = self.location(offset);
        if self.containers.iter().any(|c| matches!(c, Container::Footnote { .. })) {
            self.report_at(at, "a footnote cannot reference another footnote");
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

    /// Reports a construct and ignores everything up to its matching end.
    fn skip_reported(&mut self, offset: usize, message: &str) {
        self.close_implicit_leaf();
        self.report(offset, message);
        self.skip = 1;
    }

    fn start(&mut self, tag: Tag<'_>, offset: usize) {
        match tag {
            Tag::Paragraph => self.open_leaf(offset, None, false),
            Tag::Heading { level, .. } => self.open_leaf(offset, Some(level as u8), false),
            Tag::BlockQuote(_) => {
                self.close_implicit_leaf();
                self.containers.push(Container::Quote(self.location(offset)));
                self.blocks.push(Vec::new());
            }
            Tag::List(start) => {
                self.close_implicit_leaf();
                let at = self.location(offset);
                self.containers.push(Container::List {
                    at,
                    start,
                    items: Vec::new(),
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
            Tag::Image { .. } => self.skip_reported(offset, "images are not supported yet"),
            Tag::HtmlBlock => self.skip_reported(offset, "raw HTML is not rendered"),
            Tag::Table(_) => self.skip_reported(offset, "tables are not supported yet"),
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
                if let Some(Container::List { at, start, items }) = self.containers.pop() {
                    self.push_block(Block::List { at, start, items });
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
            TagEnd::Emphasis => self.emphasis -= 1,
            TagEnd::Strong => self.strong -= 1,
            TagEnd::Link => {
                self.links.pop();
            }
            _ => {}
        }
    }
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
                    content: vec![plain("Title")]
                },
                Block::Heading {
                    at: at(3, 1),
                    level: 2,
                    content: vec![plain("Setext")]
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
                },
                Block::List {
                    at: at(4, 1),
                    start: None,
                    items: vec![vec![
                        paragraph(4, 3, "a"),
                        Block::List {
                            at: at(5, 3),
                            start: None,
                            items: vec![vec![paragraph(5, 5, "b")]]
                        },
                    ]],
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
            link: Some("https://a.b/".into()),
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
            vec![(Some((5, 5)), "images are not supported yet".to_string())]
        );
    }

    #[test]
    fn reports_each_unsupported_construct() {
        let cases = [
            ("![alt](a.png)", "images are not supported yet"),
            ("<div>\nhi\n</div>", "raw HTML is not rendered"),
            ("a <b>x</b>", "raw HTML is not rendered"),
            ("---", "thematic breaks are not supported"),
            ("| a |\n|---|\n| b |", "tables are not supported yet"),
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
            diagnostics("![a](b.png)\n\n---\n\ntext <i>x</i>\n", 3),
            vec![
                (Some((3, 1)), "images are not supported yet".to_string()),
                (Some((5, 1)), "thematic breaks are not supported".to_string()),
                (Some((7, 6)), "raw HTML is not rendered".to_string()),
                (Some((7, 10)), "raw HTML is not rendered".to_string()),
            ]
        );
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
                "unknown layout directive \"float\"; use columns, full-width, keep, or page-break",
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
}
