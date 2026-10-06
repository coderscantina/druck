//! CommonMark parsing into the [document model](crate::document).

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::config::source::Source;
use crate::diagnostic::Diagnostic;
use crate::document::{Block, Document, Inline, InlineStyle, Location};

/// Parses the Markdown body that starts at 1-based line `first_line` of the document `source`.
///
/// Every construct that cannot be rendered yet is reported with its location. The result is
/// either the complete document or every such diagnostic, never a document with content dropped.
pub fn parse(body: &str, first_line: u64, source: &Source) -> Result<Document, Vec<Diagnostic>> {
    // Extensions are enabled only so their syntax is recognised and reported, not rendered.
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut builder = Builder::new(body, first_line, source);
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        builder.event(event, range.start);
    }
    builder.finish()
}

enum Container {
    Quote(Location),
    List {
        at: Location,
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Item,
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
}

impl<'a> Builder<'a> {
    fn new(body: &'a str, first_line: u64, source: &'a Source) -> Self {
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
        }
    }

    fn finish(mut self) -> Result<Document, Vec<Diagnostic>> {
        if !self.diagnostics.is_empty() {
            return Err(self.diagnostics);
        }
        let blocks = self.blocks.pop().unwrap_or_default();
        Ok(Document { blocks })
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
            let line = self.body[offset..].lines().next().unwrap_or_default().trim_end();
            self.report(offset, format!("layout directive \"{line}\" is not supported yet"));
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

    fn event(&mut self, event: Event<'_>, offset: usize) {
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
                None => self.push_text(offset, &text, false),
            },
            Event::Code(text) => self.push_text(offset, &text, true),
            Event::SoftBreak => self.push_text(offset, " ", false),
            Event::HardBreak => self.push_inline(offset, Inline::LineBreak),
            Event::Rule => {
                self.close_implicit_leaf();
                self.report(offset, "thematic breaks are not supported");
            }
            Event::InlineHtml(_) => self.report(offset, "raw HTML is not rendered"),
            Event::FootnoteReference(_) => self.report(offset, "footnotes are not supported yet"),
            Event::TaskListMarker(_) => self.report(offset, "task lists are not supported"),
            // Block HTML is skipped with its tag; math and the rest are not enabled.
            _ => {}
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
            Tag::FootnoteDefinition(_) => self.skip_reported(offset, "footnotes are not supported yet"),
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
            TagEnd::Emphasis => self.emphasis -= 1,
            TagEnd::Strong => self.strong -= 1,
            TagEnd::Link => {
                self.links.pop();
            }
            _ => {}
        }
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
            ("a[^1]\n\n[^1]: note", "footnotes are not supported yet"),
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
    fn reports_layout_directives_but_not_when_inside_code() {
        assert_eq!(
            diagnostics("::: columns\n\ntext\n\n:::\n", 1),
            vec![
                (
                    Some((1, 1)),
                    "layout directive \"::: columns\" is not supported yet".to_string()
                ),
                (
                    Some((5, 1)),
                    "layout directive \":::\" is not supported yet".to_string()
                ),
            ]
        );
        assert_eq!(
            blocks("```\n::: columns\n:::\n```\n"),
            vec![Block::Code {
                at: at(1, 1),
                first_line: 2,
                lines: vec!["::: columns".into(), ":::".into()]
            }]
        );
    }
}
