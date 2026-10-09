//! The EPUB stylesheet, derived from the resolved theme.
//!
//! Block styles become rules for the elements that [`super::content`] writes: font sizes in `rem` of the
//! body size, so readers can scale the text, and spacing in `em` of the element's own size. Containers carry
//! the text properties of their frame style, such as `blockquote` the `quote` style, and paragraphs inherit
//! them. Only a paragraph that follows another one has a first-line indent, as in the PDF. Custom styles
//! and the built-in styles that slots, chapter numbers, and list markers name become `s-` classes.
//!
//! Page geometry, headers and footers, page variants, columns, watermarks, and keep rules other than keeping
//! headings with what follows are left out: readers lay out pages themselves.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt::Write;

use super::fonts::FontFace;
use crate::config::resolved::{Config, CustomStyle, Rule, SceneMark, Style};
use crate::config::theme::{Align, FontStyle, RuleBelow, SlotStyle, TemplateStyle};
use crate::config::values::{Length, Pt};

/// The class of a built-in or custom style that elements name, such as `s-chapter-number`.
pub fn class(name: &str) -> String {
    format!("s-{name}")
}

/// The rules for `config` and the font families they name.
pub fn rules(config: &Config) -> (String, BTreeSet<String>) {
    let mut sheet = Sheet {
        css: String::new(),
        body: config.styles.body.size,
        families: RefCell::default(),
        code_font: &config.inline.code_font,
    };
    let styles = &config.styles;
    let body = &styles.body;
    sheet.rule("body", sheet.text(body));
    sheet.rule(
        "p",
        vec![("margin", sheet.margins(body)), ("text-indent", "0".to_owned())],
    );
    sheet.indent("p + p", body);
    let headings = [
        &styles.heading_1,
        &styles.heading_2,
        &styles.heading_3,
        &styles.heading_4,
        &styles.heading_5,
        &styles.heading_6,
    ];
    for (level, style) in (1..).zip(headings) {
        let mut declarations = sheet.block(style);
        declarations.extend(avoid_break_after());
        sheet.rule(&format!("h{level}"), declarations);
    }
    sheet.chapters(config);

    sheet.rule("blockquote", sheet.block(&styles.quote));
    sheet.rule("blockquote p", vec![("margin", sheet.margins(&styles.quote))]);
    sheet.indent("blockquote p + p", &styles.quote);
    sheet.lists(config);
    let mut code = sheet.block(&styles.code_block);
    code.push(("white-space", "pre-wrap".to_owned()));
    sheet.rule("pre", code);
    let inherit = || "inherit".to_owned();
    sheet.rule(
        "pre code",
        vec![
            ("font-family", inherit()),
            ("font-size", "1em".to_owned()),
            ("color", inherit()),
        ],
    );
    let inline = &config.inline;
    let code_family = sheet.family(&inline.code_font);
    sheet.rule(
        "code",
        vec![
            ("font-family", code_family),
            ("font-size", length(inline.code_size, body.size)),
            ("color", inline.code_color.to_string()),
        ],
    );
    let underline = if inline.link_underline { "underline" } else { "none" };
    sheet.rule(
        "a",
        vec![
            ("color", inline.link_color.to_string()),
            ("text-decoration", underline.to_owned()),
        ],
    );
    sheet.figures(config);
    sheet.tables(config);
    sheet.notes(config);
    sheet.bibliography(config);
    sheet.contents(config);
    sheet.scene_break(config);
    sheet.title_page();
    sheet.rule(
        ".page-break",
        vec![
            ("page-break-after", "always".to_owned()),
            ("break-after", "page".to_owned()),
        ],
    );
    sheet.named(config);
    // A column's or title group's alignment wins over a row's style, so it comes last.
    for align in ["left", "center", "right"] {
        sheet.rule(&format!(".align-{align}"), vec![("text-align", align.to_owned())]);
    }
    (sheet.css, sheet.families.into_inner())
}

/// `@font-face` rules for the embedded faces.
pub fn font_faces(faces: &[FontFace]) -> String {
    let mut css = String::new();
    for face in faces {
        let style = match face.face.style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
        };
        writeln!(
            css,
            "@font-face {{ font-family: {}; font-weight: {}; font-style: {style}; src: url(\"{}\"); }}",
            string(&face.family),
            face.weight,
            face.path
        )
        .expect("writing to a string succeeds");
    }
    css
}

type Declarations = Vec<(&'static str, String)>;

struct Sheet<'a> {
    css: String,
    /// The body size, which is 1 rem.
    body: Pt,
    /// The families the rules name, collected as they are written.
    families: RefCell<BTreeSet<String>>,
    code_font: &'a str,
}

impl Sheet<'_> {
    fn rule(&mut self, selector: &str, declarations: Declarations) {
        if declarations.is_empty() {
            return;
        }
        let body: Vec<String> = declarations
            .iter()
            .map(|(name, value)| format!("{name}: {value};"))
            .collect();
        writeln!(self.css, "{selector} {{ {} }}", body.join(" ")).expect("writing to a string succeeds");
    }

    /// A family with a generic fallback: monospace for the code font, else serif.
    fn family(&self, family: &str) -> String {
        self.families.borrow_mut().insert(family.to_owned());
        let generic = if family == self.code_font { "monospace" } else { "serif" };
        format!("{}, {generic}", string(family))
    }

    /// The text properties of a style, which paragraphs inside its element inherit.
    fn text(&self, style: &Style) -> Declarations {
        let hyphens = if style.hyphenate { "auto" } else { "manual" };
        let font_style = match style.style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
        };
        let tracking = if style.tracking == 0.0 {
            "normal".to_owned()
        } else {
            format!("{}em", number(style.tracking))
        };
        vec![
            ("font-family", self.family(&style.font)),
            ("font-size", format!("{}rem", number(style.size.0 / self.body.0))),
            ("font-weight", style.weight.get().to_string()),
            ("font-style", font_style.to_owned()),
            ("color", style.color.to_string()),
            ("line-height", number(style.line_height)),
            ("text-align", align(style.align).to_owned()),
            ("-webkit-hyphens", hyphens.to_owned()),
            ("hyphens", hyphens.to_owned()),
            ("letter-spacing", tracking),
            (
                "text-transform",
                if style.uppercase { "uppercase" } else { "none" }.to_owned(),
            ),
        ]
    }

    /// Space before and after a block in `style`, and its indent on both sides.
    fn margins(&self, style: &Style) -> String {
        let em = |length: Pt| em(length, style.size);
        let (before, after, indent) = (em(style.space_before), em(style.space_after), em(style.indent));
        format!("{before} {indent} {after}")
    }

    /// The text properties and margins of a block in `style`.
    fn block(&self, style: &Style) -> Declarations {
        let mut declarations = self.text(style);
        declarations.push(("margin", self.margins(style)));
        if style.keep_with_next {
            declarations.extend(avoid_break_after());
        }
        declarations
    }

    /// The first-line indent of a paragraph that follows another one in `style`.
    fn indent(&mut self, selector: &str, style: &Style) {
        self.rule(selector, vec![("text-indent", em(style.first_line_indent, style.size))]);
    }

    /// A chapter heading sunk by `chapters.sink`, its number above it, the ornament, and the opening of its
    /// first paragraph. The number takes the heading's space above it.
    fn chapters(&mut self, config: &Config) {
        let chapters = &config.chapters;
        let heading = &config.styles.heading_1;
        let mut chapter = Vec::new();
        if chapters.sink.0 > 0.0 {
            chapter.push(("margin-top", em(chapters.sink, heading.size)));
        }
        if chapters.ornament.is_some() {
            chapter.push(("margin-bottom", "0".to_owned()));
        }
        self.rule("h1.chapter", chapter);
        self.rule(
            ".chapter-number",
            vec![("display", "block".to_owned()), ("margin-top", "0".to_owned())],
        );
        if let Some(ornament) = &chapters.ornament {
            let space = format!(
                "{} 0 {}",
                em(ornament.space_before, heading.size),
                em(heading.space_after, heading.size)
            );
            let size = format!("{}rem", number(heading.size.0 / self.body.0));
            self.rule(
                ".ornament",
                vec![
                    ("font-size", size),
                    ("margin", space),
                    ("text-align", "center".to_owned()),
                    ("text-indent", "0".to_owned()),
                ],
            );
            self.rule(".ornament img", image_width(ornament.width, heading.size));
        }
        if chapters.lead_in > 0 {
            self.rule(
                ".lead-in",
                vec![
                    ("font-variant", "small-caps".to_owned()),
                    ("font-variant-caps", "small-caps".to_owned()),
                ],
            );
        }
        if chapters.drop_cap > 0 {
            // A floated capital as tall as the lines it spans, from the cap height of the first, taking cap
            // height as 0.7 em; readers that support `initial-letter` size it exactly.
            let span = chapters.drop_cap as f64;
            let size = ((span - 1.0) * config.styles.body.line_height + 0.7) / 0.7;
            let upright = || ("font-style", "normal".to_owned());
            self.rule(
                "p.opening::first-letter",
                vec![
                    ("float", "left".to_owned()),
                    ("font-size", format!("{}em", number(size))),
                    ("line-height", "0.85".to_owned()),
                    ("margin", "0 0.25em 0 0".to_owned()),
                    upright(),
                ],
            );
            let lines = chapters.drop_cap;
            writeln!(
                self.css,
                "@supports (initial-letter: {lines}) or (-webkit-initial-letter: {lines}) {{ p.opening::first-letter \
                 {{ float: none; font-size: 1em; line-height: inherit; margin: 0 0.25em 0 0; \
                 -webkit-initial-letter: {lines}; initial-letter: {lines}; }} }}"
            )
            .expect("writing to a string succeeds");
        }
    }

    /// Lists in the `list` style, indented by `lists.indent`, with the theme's bullets by depth.
    fn lists(&mut self, config: &Config) {
        let lists = &config.lists;
        let style = &config.styles.list;
        let mut declarations = self.text(style);
        declarations.push((
            "margin",
            format!(
                "{} 0 {}",
                em(style.space_before, style.size),
                em(style.space_after, style.size)
            ),
        ));
        declarations.push(("padding-left", em(lists.indent, style.size)));
        self.rule("ul, ol", declarations);
        self.rule("li ul, li ol", vec![("margin", "0".to_owned())]);
        self.rule("li + li", vec![("margin-top", em(lists.item_spacing, style.size))]);
        self.rule("li p", vec![("margin", "0".to_owned())]);
        self.indent("li p + p", style);
        self.bullets("ul", &lists.bullets);
        if let Some(marker) = &lists.marker {
            self.marker("li", config.named_style(marker));
        }
        for (name, custom) in &config.custom_styles {
            let CustomStyle::List { bullets, marker, .. } = custom else {
                continue;
            };
            let selector = format!("ul.{}", class(name));
            if let Some(bullets) = bullets {
                self.bullets(&selector, bullets);
            }
            if let Some(marker) = marker {
                self.marker(&format!(".{} > li", class(name)), config.named_style(marker));
            }
        }
    }

    /// Bullets by depth for lists matching `selector`, the last repeating.
    fn bullets(&mut self, selector: &str, bullets: &[String]) {
        for (depth, bullet) in bullets.iter().enumerate() {
            let nested = format!("{}{selector}", "ul ".repeat(depth));
            let marker = string(&format!("{bullet} "));
            self.rule(
                &nested,
                vec![("list-style-type", "disc".to_owned()), ("list-style-type", marker)],
            );
        }
    }

    fn marker(&mut self, item: &str, style: &Style) {
        let mut declarations = self.text(style);
        declarations.retain(|(name, _)| {
            matches!(
                *name,
                "font-family" | "font-size" | "font-weight" | "font-style" | "color"
            )
        });
        self.rule(&format!("{item}::marker"), declarations);
    }

    /// Images centered with the caption style's spacing around the figure and above the caption.
    fn figures(&mut self, config: &Config) {
        let caption = &config.styles.caption;
        let around = format!("{} 0", em(caption.space_after, self.body));
        self.rule(
            ".figure, .image",
            vec![
                ("margin", around),
                ("text-align", "center".to_owned()),
                ("text-indent", "0".to_owned()),
            ],
        );
        self.rule("img", vec![("max-width", "100%".to_owned())]);
        let mut declarations = self.text(caption);
        declarations.push((
            "margin",
            format!(
                "{} {}",
                em(caption.space_before, caption.size),
                em(caption.indent, caption.size)
            ),
        ));
        self.rule("figcaption", declarations);
        let mut declarations = self.text(caption);
        declarations.push(("caption-side", "top".to_owned()));
        declarations.push(("padding-bottom", em(caption.space_before, caption.size)));
        self.rule("caption", declarations);
    }

    /// Tables spaced like figures, with the cell and header styles and padding, and the theme's rules.
    fn tables(&mut self, config: &Config) {
        let tables = &config.tables;
        let styles = &config.styles;
        let around = format!("{} auto", em(styles.caption.space_after, self.body));
        self.rule(
            "table",
            vec![("border-collapse", "collapse".to_owned()), ("margin", around)],
        );
        for (selector, style) in [("th", &styles.table_header), ("td", &styles.table_cell)] {
            let mut declarations = self.text(style);
            declarations.push(("padding", em(tables.cell_padding, style.size)));
            declarations.push(("vertical-align", "top".to_owned()));
            self.rule(selector, declarations);
            self.rule(&format!("{selector} p"), vec![("margin", self.margins(style))]);
            self.indent(&format!("{selector} p + p"), style);
        }
        self.rule("thead > tr:first-child > *", border("top", tables.top_rule));
        self.rule("thead > tr > *", border("bottom", tables.header_rule));
        self.rule("tbody > tr > *", border("bottom", tables.row_rule));
        for (name, custom) in &config.custom_styles {
            if let CustomStyle::Paragraph {
                rule_below: Some(below),
                ..
            } = custom
            {
                let rule = match below {
                    RuleBelow::None => None,
                    RuleBelow::Header => tables.header_rule,
                    RuleBelow::Row => tables.row_rule,
                };
                let mut declarations = border("bottom", rule);
                if rule.is_none() {
                    declarations.push(("border-bottom", "none".to_owned()));
                }
                self.rule(&format!("tr.{} > *", class(name)), declarations);
            }
        }
    }

    /// Footnotes at the end of a chapter in the `footnote` style, below a separator, and their markers.
    fn notes(&mut self, config: &Config) {
        let style = &config.styles.footnote;
        let notes = &config.footnotes;
        let size = format!("{}rem", number(style.size.0 / self.body.0));
        self.rule(
            "section.footnotes",
            vec![("font-size", size), ("margin-top", em(notes.gap, style.size))],
        );
        self.rule(
            "section.footnotes::before",
            vec![
                ("content", "\"\"".to_owned()),
                ("display", "block".to_owned()),
                ("width", em(notes.separator_width, style.size)),
                (
                    "border-top",
                    format!(
                        "{} solid {}",
                        em(notes.separator_thickness, style.size),
                        notes.separator_color
                    ),
                ),
                ("margin-bottom", em(notes.spacing, style.size)),
            ],
        );
        self.rule("aside.footnote", self.text(style));
        self.rule(
            "aside.footnote + aside.footnote",
            vec![("margin-top", em(notes.spacing, style.size))],
        );
        self.rule("aside.footnote p", vec![("margin", self.margins(style))]);
        self.indent("aside.footnote p + p", style);
        // List, quotation, and code styles shrink with the note, as in the PDF.
        let ratio = style.size.0 / self.body.0;
        for (selector, inner) in [
            ("aside.footnote ul, aside.footnote ol", &config.styles.list),
            ("aside.footnote blockquote", &config.styles.quote),
            ("aside.footnote pre", &config.styles.code_block),
        ] {
            let size = format!("{}rem", number(inner.size.0 / self.body.0 * ratio));
            self.rule(selector, vec![("font-size", size)]);
        }
        let inline = &config.inline;
        let marker = match inline.footnote_marker_size {
            Length::Em(em) => em,
            Length::Pt(pt) => pt / self.body.0,
        };
        let raise = match inline.footnote_marker_raise {
            Length::Em(em) => em,
            Length::Pt(pt) => pt / self.body.0,
        };
        self.rule(
            "a.noteref",
            vec![
                ("font-size", format!("{}em", number(marker))),
                ("vertical-align", format!("{}em", number(raise / marker))),
                ("line-height", "0".to_owned()),
                ("text-decoration", "none".to_owned()),
            ],
        );
    }

    /// Bibliography entries with a hanging indent, spaced at least `bibliography.entry-spacing` apart.
    fn bibliography(&mut self, config: &Config) {
        let style = &config.styles.bibliography;
        let bibliography = &config.bibliography;
        let hang = em(bibliography.hanging_indent, style.size);
        let mut declarations = self.block(style);
        declarations.push(("padding-left", hang.clone()));
        declarations.push(("text-indent", format!("-{hang}")));
        self.rule("p.entry", declarations);
        let between = Pt(bibliography
            .entry_spacing
            .0
            .max(style.space_before.0)
            .max(style.space_after.0));
        self.rule("p.entry + p.entry", vec![("margin-top", em(between, style.size))]);
    }

    /// The contents page: the `toc-heading` style and one entry per heading, indented by level.
    fn contents(&mut self, config: &Config) {
        let styles = &config.styles;
        self.rule("nav#toc h1", self.block(&styles.toc_heading));
        self.rule(
            "nav#toc ol",
            vec![
                ("list-style", "none".to_owned()),
                ("margin", "0".to_owned()),
                ("padding", "0".to_owned()),
            ],
        );
        let entry = &styles.toc_entry;
        self.rule(
            "nav#toc ol ol",
            vec![("padding-left", em(config.toc.level_indent, entry.size))],
        );
        self.rule("nav#toc li", self.block(entry));
        for (level, style) in config.toc.level_styles.iter().enumerate() {
            let selector = format!("nav#toc > ol{} > li", " > li > ol".repeat(level));
            self.rule(&selector, self.block(style));
        }
        self.rule(
            "nav#toc a",
            vec![("color", "inherit".to_owned()), ("text-decoration", "none".to_owned())],
        );
    }

    fn scene_break(&mut self, config: &Config) {
        let style = &config.styles.scene_break;
        let mut declarations = self.block(style);
        declarations.push(("text-indent", "0".to_owned()));
        self.rule(".scene-break", declarations);
        if let SceneMark::Image { width, .. } = &config.scene_break.mark {
            self.rule(".scene-break img", image_width(*width, style.size));
        }
    }

    /// Title slots add no spacing of their own; each carries its `space-before` itself.
    fn title_page(&mut self) {
        self.rule(
            ".titlepage p",
            vec![("margin", "0".to_owned()), ("text-indent", "0".to_owned())],
        );
        self.rule(
            ".cover",
            vec![("margin", "0".to_owned()), ("text-align", "center".to_owned())],
        );
        self.rule(
            ".cover img",
            vec![("max-width", "100%".to_owned()), ("max-height", "100vh".to_owned())],
        );
    }

    /// A class for every custom style, and for the built-in styles that title slots, chapter numbers, and
    /// list markers name.
    fn named(&mut self, config: &Config) {
        let mut names: BTreeSet<&str> = config.custom_styles.keys().map(String::as_str).collect();
        names.insert(&config.chapters.number_style);
        let slots = config
            .title_block
            .slots
            .iter()
            .chain(config.title_page.iter().flat_map(|group| &group.slots));
        names.extend(slots.map(|slot| slot_name(&slot.style)));
        names.insert("abstract-heading");
        for name in names {
            let style = config.named_style(name);
            let selector = format!(".{}", class(name));
            self.rule(&selector, self.block(style));
            if matches!(config.custom_styles.get(name), Some(CustomStyle::Paragraph { .. })) {
                self.indent(&format!("p + p{selector}"), style);
            }
        }
    }
}

/// The theme name of a slot style.
pub fn slot_name(style: &SlotStyle) -> &str {
    match style {
        SlotStyle::Template(template) => match template {
            TemplateStyle::Title => "title",
            TemplateStyle::Subtitle => "subtitle",
            TemplateStyle::Author => "author",
            TemplateStyle::Date => "date",
            TemplateStyle::AbstractHeading => "abstract-heading",
            TemplateStyle::Abstract => "abstract",
            TemplateStyle::Body => "body",
        },
        SlotStyle::Custom(name) => name,
    }
}

fn avoid_break_after() -> Declarations {
    vec![
        ("page-break-after", "avoid".to_owned()),
        ("break-after", "avoid".to_owned()),
    ]
}

fn image_width(width: Pt, size: Pt) -> Declarations {
    vec![("width", em(width, size)), ("max-width", "100%".to_owned())]
}

fn border(side: &'static str, rule: Option<Rule>) -> Declarations {
    let name = match side {
        "top" => "border-top",
        _ => "border-bottom",
    };
    rule.map(|rule| (name, format!("{}pt solid {}", number(rule.thickness.0), rule.color)))
        .into_iter()
        .collect()
}

fn align(align: Align) -> &'static str {
    match align {
        Align::Justify => "justify",
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

/// `length` in ems of `size`.
fn em(length: Pt, size: Pt) -> String {
    match number(length.0 / size.0).as_str() {
        "0" => "0".to_owned(),
        value => format!("{value}em"),
    }
}

/// An inline length, whose `em` refers to the surrounding text, in ems of `size` if absolute.
fn length(length: Length, size: Pt) -> String {
    match length {
        Length::Em(value) => format!("{}em", number(value)),
        Length::Pt(value) => em(Pt(value), size),
    }
}

/// A number with at most four decimals and no trailing zeros.
fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".to_owned() } else { text.to_owned() }
}

/// A CSS string.
fn string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '\n' => quoted.push_str("\\a "),
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn css(theme: serde_json::Value) -> String {
        rules(&super::super::tests::config("", theme)).0
    }

    #[test]
    fn writes_sizes_relative_to_the_body_and_spacing_relative_to_the_element() {
        let css = css(json!({
            "version": 1,
            "styles": {
                "body": { "size": "10pt", "first-line-indent": "1.5em", "space-after": "0pt" },
                "heading-2": { "size": "15pt", "space-before": "30pt", "tracking": 0.05, "uppercase": true }
            }
        }));
        assert!(css.contains("p + p { text-indent: 1.5em; }"), "{css}");
        let heading = css.lines().find(|line| line.starts_with("h2 {")).unwrap();
        for declaration in [
            "font-size: 1.5rem;",
            "margin: 2em 0 ",
            "letter-spacing: 0.05em;",
            "text-transform: uppercase;",
            "break-after: avoid;",
        ] {
            assert!(heading.contains(declaration), "{declaration} in {heading}");
        }
    }

    #[test]
    fn writes_custom_styles_as_classes_indented_only_after_a_paragraph() {
        let css = css(json!({
            "version": 1,
            "custom-styles": {
                "note": { "based-on": "body", "style": "italic", "align": "center", "first-line-indent": "1em" },
                "total": { "based-on": "table-cell", "weight": "bold", "rule-below": "none" }
            }
        }));
        let note = css.lines().find(|line| line.starts_with(".s-note {")).unwrap();
        assert!(
            note.contains("font-style: italic;") && note.contains("text-align: center;"),
            "{note}"
        );
        assert!(!note.contains("text-indent"), "{note}");
        assert!(css.contains("p + p.s-note { text-indent: 1em; }"), "{css}");
        assert!(css.contains("tr.s-total > * { border-bottom: none; }"), "{css}");
    }

    #[test]
    fn writes_chapter_openings_from_the_chapters_section() {
        let css = css(json!({
            "version": 1,
            "styles": { "heading-1": { "size": "20pt" } },
            "chapters": { "sink": "40pt", "drop-cap": 3, "lead-in": 2 }
        }));
        assert!(css.contains("h1.chapter { margin-top: 2em; }"), "{css}");
        assert!(css.contains("initial-letter: 3;"), "{css}");
        assert!(css.contains("p.opening::first-letter { float: left;"), "{css}");
        assert!(css.contains(".lead-in { font-variant: small-caps;"), "{css}");
    }

    #[test]
    fn quotes_family_names_and_falls_back_to_a_generic_family() {
        assert_eq!(string("Say \"x\" \\"), "\"Say \\\"x\\\" \\\\\"");
        let css = css(json!({"version": 1}));
        assert!(css.contains("font-family: \"Libertinus Serif\", serif;"), "{css}");
        assert!(css.contains("font-family: \"Libertinus Mono\", monospace;"), "{css}");
    }
}
