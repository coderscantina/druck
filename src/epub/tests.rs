//! The body as XHTML: files, links, notes, chapters, tables, and the navigation document.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Value, json};

use super::content::{self, Content, Context, Item};
use super::css;
use super::pages;
use crate::bibliography::Bibliography;
use crate::citations::{self, Cited};
use crate::config::front_matter::FrontMatter;
use crate::config::resolve::{Inputs, SettingsInput, ThemeInput, resolve};
use crate::config::resolved::Config;
use crate::config::source::Source;
use crate::date::Date;
use crate::layout::fields::Fields;
use crate::layout::structure::Structure;

fn source() -> Source {
    Source::Document("/fake/book.md".into())
}

/// The configuration of a document with front matter `yaml` and the partial `theme`.
pub(super) fn config(yaml: &str, theme: Value) -> Config {
    let settings: FrontMatter = serde_saphyr::from_str(yaml).expect("front matter");
    resolve(Inputs {
        theme: Some(ThemeInput {
            source: Source::Theme("/fake/theme.json".into()),
            value: theme,
        }),
        document: SettingsInput {
            source: source(),
            settings,
        },
        overrides: SettingsInput {
            source: Source::Cli {
                working_dir: "/fake".into(),
            },
            settings: FrontMatter::default(),
        },
    })
    .expect("configuration resolves")
}

/// The body files of `body`, with citations from the BibTeX text `bib`.
fn write(config: &Config, body: &str, bib: Option<&str>) -> (Content, Structure) {
    let document = crate::markdown::parse(body, 1, &source(), crate::config::theme::Lang::En).expect("document parses");
    let bibliography = bib.map(|bib| Bibliography::parse(bib, Path::new("/fake/refs.bib")).expect("bibliography"));
    let cited: Cited = citations::resolve(&document, bibliography.as_ref(), config, &source()).expect("citations");
    let fields = Fields::new(&document, config, Date::from_unix(0));
    let structure = Structure::new(&document, config, cited.references());
    let content = content::write(&Context {
        document: &document,
        cited: &cited,
        config,
        structure: &structure,
        fields: &fields,
        images: &[],
        theme_images: &HashMap::new(),
    });
    (content, structure)
}

fn bodies(content: &Content) -> Vec<&str> {
    content.files.iter().map(|file| file.body.as_str()).collect()
}

#[test]
fn splits_files_at_chapters_parts_page_breaks_and_the_contents() {
    let body = "Copyright.\n\n::: page-break\n\nFor you.\n\n::: front-matter\n\n# Preface {-}\n\nWhy.\n\n::: toc\n\n\
                ::: main-matter\n\n# One\n\nText.\n\n## Inside\n\nMore.\n\n# Two\n\nEnd.\n\n::: back-matter\n\n# About {-}\n\nMe.\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    let matters: Vec<_> = content.files.iter().map(|file| file.matter).collect();
    assert_eq!(
        matters,
        [
            "frontmatter",
            "frontmatter",
            "frontmatter",
            "bodymatter",
            "bodymatter",
            "backmatter"
        ]
    );
    let order = [0, 1, 2]
        .map(Item::File)
        .into_iter()
        .chain([Item::Contents])
        .chain([3, 4, 5].map(Item::File));
    assert_eq!(content.order, order.collect::<Vec<_>>());
    let bodies = bodies(&content);
    assert!(
        bodies[3].starts_with(
            "<section epub:type=\"chapter\" role=\"doc-chapter\"><h1 id=\"a1\" class=\"chapter\">1 One</h1>"
        ),
        "{}",
        bodies[3]
    );
    assert!(bodies[3].contains("<h2 id=\"a2\">1.1 Inside</h2>"), "{}", bodies[3]);
    assert!(
        !bodies[2].contains("<section"),
        "front matter has no chapters: {}",
        bodies[2]
    );
    assert_eq!(content.files[3].title.as_deref(), Some("1 One"));
}

#[test]
fn a_bottom_group_ends_its_file() {
    let body = "::: bottom\nCopyright.\n:::\n\nFor you.\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    assert_eq!(bodies(&content), ["<p>Copyright.</p>", "<p>For you.</p>"]);
}

#[test]
fn places_footnotes_as_notes_at_the_end_of_the_file_of_their_first_reference() {
    let body = "# One\n\nA note.[^a] More.\n\n# Two\n\nB note.[^b]\n\n[^a]: First.\n\n[^b]: Second *note*.\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    let bodies = bodies(&content);
    assert!(
        bodies[0].contains(
            "A note.<a class=\"noteref\" epub:type=\"noteref\" role=\"doc-noteref\" id=\"fnref-1\" href=\"#fn-1\">1</a> More.</p>"
        ),
        "{}",
        bodies[0]
    );
    assert!(
        bodies[0].ends_with(
            "<section class=\"footnotes\" epub:type=\"footnotes\"><aside class=\"footnote\" epub:type=\"footnote\" \
         role=\"doc-footnote\" id=\"fn-1\"><p><a class=\"note-number\" role=\"doc-backlink\" href=\"#fnref-1\">1</a> First.</p></aside></section>"
        ),
        "{}",
        bodies[0]
    );
    assert!(
        bodies[1].contains("id=\"fn-2\"><p><a class=\"note-number\" role=\"doc-backlink\" href=\"#fnref-2\">2</a> Second <em>note</em>.</p>"),
        "{}",
        bodies[1]
    );
    assert!(!bodies[1].contains("id=\"fn-1\""), "{}", bodies[1]);
}

#[test]
fn margin_notes_follow_the_block_that_references_them_and_sidenotes_stay_footnotes() {
    let body = "Aside here.[^m] Note here.[^n]\n\n- Item.[^i]\n\n[^m]: {-} In the margin.\n\n[^n]: At the foot.\n\n\
        [^i]: {-} Beside an item.\n";
    let theme = json!({"version": 1, "margin-notes": {"sidenotes": true}});
    let (content, _) = write(&config("", theme), body, None);
    let body = bodies(&content)[0];
    assert!(
        body.starts_with(
            "<p>Aside here. Note here.<a class=\"noteref\" epub:type=\"noteref\" role=\"doc-noteref\" id=\"fnref-1\" href=\"#fn-1\">1</a></p>\
             <aside class=\"margin-note\"><p>In the margin.</p></aside>\
             <ul><li>Item.<aside class=\"margin-note\"><p>Beside an item.</p></aside></li></ul>"
        ),
        "{body}"
    );
    assert!(body.contains(
        "id=\"fn-1\"><p><a class=\"note-number\" role=\"doc-backlink\" href=\"#fnref-1\">1</a> At the foot.</p>"
    ));
}

#[test]
fn links_cross_references_across_files_and_shows_the_target_for_a_page_reference() {
    let body = "# One {#sec:one}\n\nSee @sec:two and [@sec:two, page].\n\n# Two {#sec:two}\n\nBack to @sec:one.\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    let bodies = bodies(&content);
    assert!(
        bodies[0].contains(
            "See <a href=\"text-002.xhtml#a1\">Section\u{a0}2</a> and <a href=\"text-002.xhtml#a1\">Section\u{a0}2</a>."
        ),
        "{}",
        bodies[0]
    );
    assert!(
        bodies[1].contains("Back to <a href=\"text-001.xhtml#a0\">Section\u{a0}1</a>."),
        "{}",
        bodies[1]
    );
}

#[test]
fn links_citations_to_bibliography_entries_in_their_own_file() {
    let bib = "@book{ada, title = {A <book>}, author = {Ada Lovelace}, publisher = {Press}, year = {2020}}";
    let body = "# One\n\nAs shown [@ada, p. 3].\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, Some(bib));
    let bodies = bodies(&content);
    assert!(
        bodies[0].contains("<a href=\"text-002.xhtml#a2\">Lovelace 2020, p.\u{a0}3</a>"),
        "{}",
        bodies[0]
    );
    assert!(
        bodies[1]
            .starts_with("<section epub:type=\"bibliography\" role=\"doc-bibliography\"><h1 id=\"a1\">References</h1>"),
        "{}",
        bodies[1]
    );
    assert!(
        bodies[1].contains("<p class=\"entry\" id=\"a2\" epub:type=\"biblioentry\">Lovelace, Ada"),
        "{}",
        bodies[1]
    );
    assert!(bodies[1].contains("A &lt;book&gt;"), "{}", bodies[1]);
}

#[test]
fn escapes_text_and_link_targets() {
    let body = "Fish & \\<chips\\> \\\"now\\\" at [shop](https://example.com/?a=1&b=\"2\").\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    assert_eq!(
        bodies(&content)[0],
        "<p>Fish &amp; &lt;chips&gt; \"now\" at <a href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\">shop</a>.</p>"
    );
}

#[test]
fn writes_marks_and_todos_as_elements_and_cuts_comments() {
    let body = "~~Struck~~ H~2~O, m^2^, [under]{.underline}, [caps]{.smallcaps} *[both]{.underline}*, \
        {todo: check <this>}.\n\n<!-- gone -->\n\nNext.\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    assert_eq!(
        bodies(&content)[0],
        "<p><s>Struck</s> H<sub>2</sub>O, m<sup>2</sup>, <u>under</u>, <span class=\"small-caps\">caps</span> \
         <em><u>both</u></em>, <span class=\"todo\">TODO: check &lt;this&gt;</span>.</p><p>Next.</p>"
    );
}

#[test]
fn colors_code_by_token_class_and_leaves_other_blocks_plain() {
    let body = "```json\n{\"a\": true}\n```\n\n```klingon\nnuqneH <b>\n```\n\n```\nplain\n```\n";
    let on = config("", json!({"version": 1}));
    let (content, _) = write(&on, body, None);
    let html = bodies(&content)[0];
    assert!(html.contains("<span class=\"hl-constant\">true</span>"), "{html}");
    assert!(html.contains("<pre><code>nuqneH &lt;b&gt;</code></pre>"), "{html}");
    assert!(html.contains("<pre><code>plain</code></pre>"), "{html}");
    assert!(css::rules(&on).0.contains("pre .hl-constant { color: #8a4b08; }"));

    let off = config("", json!({"version": 1, "highlight": null}));
    let (content, _) = write(&off, body, None);
    assert!(!bodies(&content)[0].contains("hl-"));
    assert!(!css::rules(&off).0.contains("hl-"));
}

#[test]
fn opens_chapters_with_the_number_above_an_ornament_free_heading_and_a_lead_in() {
    let theme = json!({
        "version": 1,
        "document": { "numbering-depth": 1 },
        "custom-styles": { "epigraph": { "based-on": "body", "style": "italic" } },
        "chapters": { "number-format": "upper-roman", "number-position": "above", "number-label": true, "drop-cap": 2, "lead-in": 2 }
    });
    let body = "# The arrival\n\nA quiet line. {.epigraph}\n\nElias came to the island late.\n\nThen more.\n";
    let (content, _) = write(&config("", theme), body, None);
    assert_eq!(
        bodies(&content)[0],
        "<section epub:type=\"chapter\" role=\"doc-chapter\"><h1 id=\"a0\" class=\"chapter\"><span class=\"chapter-number s-heading-1\">\
         Chapter I</span> The arrival</h1><p class=\"s-epigraph\">A quiet line.</p><p class=\"opening\"><span \
         class=\"small-caps\">Elias came</span> to the island late.</p><p>Then more.</p></section>"
    );
}

#[test]
fn writes_tables_with_captions_spans_alignment_and_row_styles() {
    let theme = json!({
        "version": 1,
        "custom-styles": { "total": { "based-on": "table-cell", "weight": "bold" } }
    });
    let body = "::: table {align=\"left right\"}\n- - Item\n  - Cost\n- - Tea\n  - 3\n- {.total}\n  - {span=2} Sum\n:::\n\n\
                : Costs {#tbl:costs}\n";
    let (content, _) = write(&config("", theme), body, None);
    assert_eq!(
        bodies(&content)[0],
        "<table id=\"a0\"><caption>Table 1: Costs</caption><thead><tr><th scope=\"col\" class=\"align-left\">Item</th><th \
         scope=\"col\" class=\"align-right\">Cost</th></tr></thead><tbody><tr><td class=\"align-left\">Tea</td><td \
         class=\"align-right\">3</td></tr><tr class=\"s-total\"><td class=\"s-total align-left\" \
         colspan=\"2\">Sum</td></tr></tbody></table>"
    );
}

#[test]
fn lists_the_listed_headings_up_to_the_contents_depth_in_the_navigation() {
    let body = "# One\n\n## Sub\n\n### Deep\n\n# Hidden {.unlisted}\n\n# Two\n";
    let config = config("toc-depth: 2", json!({"version": 1}));
    let (content, structure) = write(&config, body, None);
    let navigation = pages::navigation(&config, &structure, &content.anchors, &[], ("text-001.xhtml", "Book"));
    assert_eq!(
        navigation,
        "<nav epub:type=\"toc\" role=\"doc-toc\" id=\"toc\"><h1>Contents</h1><ol><li><a href=\"text-001.xhtml#a0\">1 One</a><ol><li>\
         <a href=\"text-001.xhtml#a1\">1.1 Sub</a></li></ol></li><li><a href=\"text-003.xhtml#a4\">3 Two</a></li></ol></nav>"
    );
}

#[test]
fn links_the_first_document_in_the_navigation_of_a_book_without_headings() {
    let config = config("", json!({"version": 1}));
    let (content, structure) = write(&config, "", None);
    assert_eq!(content.order, [Item::File(0)]);
    let navigation = pages::navigation(&config, &structure, &content.anchors, &[], ("text-001.xhtml", "A & B"));
    assert_eq!(
        navigation,
        "<nav epub:type=\"toc\" role=\"doc-toc\" id=\"toc\"><h1>Contents</h1><ol><li><a href=\"text-001.xhtml\">A &amp; B</a></li></ol></nav>"
    );
}

#[test]
fn sets_a_callout_as_an_aside_with_its_label_and_a_class_per_kind() {
    let theme = json!({"version": 1, "callouts": {"tip": {"background": "#eeeeee", "rule": null}}});
    let config = config("lang: de", theme);
    let (content, _) = write(&config, "> [!TIP]\n> Mit `Code` & mehr.\n\n> Plain.\n", None);
    assert_eq!(
        bodies(&content)[0],
        "<aside class=\"callout tip\" epub:type=\"tip\"><p class=\"callout-label\">Tipp</p><p>Mit <code>Code</code> &amp; mehr.</p></aside>\
     <blockquote><p>Plain.</p></blockquote>"
    );
    let (css, _) = super::css::rules(&config);
    assert!(
        css.contains("aside.callout.tip { background-color: #eeeeee; }"),
        "{css}"
    );
    assert!(css.contains("aside.callout.caution { background-color: #f1f1f1; border-left: 4pt solid"));
}

#[test]
fn the_index_links_each_entry_to_its_marks_by_their_headings() {
    let body = "# One\n\nAlpha{^alpha} and [beta]{^}.\n\n## Two\n\nAlpha{^alpha} again{^alpha}.[^n]\n\n\
                [^n]: Beta{^beta!sub} note.\n\n# Three\n\nGamma{^gamma|see alpha}.\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    let bodies = bodies(&content);
    assert_eq!(bodies.len(), 3, "{bodies:?}");
    assert!(
        bodies[0].contains("<p>Alpha<span id=\"a4\"></span> and <span id=\"a5\"></span>beta.</p>"),
        "{}",
        bodies[0]
    );
    let entry = |term: &str, rest: &str| {
        format!(
            "<li class=\"index-entry\" epub:type=\"index-entry\"><span epub:type=\"index-term\">{term}</span>{rest}"
        )
    };
    let locator = |href: &str, text: &str| format!("<a epub:type=\"index-locator\" href=\"{href}\">{text}</a>");
    let expected = [
        "<section epub:type=\"index\" role=\"doc-index\"><h1 id=\"a3\">Index</h1>".to_owned(),
        "<div epub:type=\"index-group\"><h2 class=\"index-letter\">A</h2>".to_owned(),
        entry(
            "alpha",
            &format!(
                ", {}, {}</li>",
                locator("text-001.xhtml#a4", "1 One"),
                locator("text-001.xhtml#a6", "1.1 Two")
            ),
        ),
        entry(
            "beta",
            &format!(
                ", {}<ul class=\"index\" epub:type=\"index-entry-list\"><li class=\"index-sub-entry\" \
                 epub:type=\"index-entry\"><span epub:type=\"index-term\">sub</span>, {}</li></ul></li>",
                locator("text-001.xhtml#a5", "1 One"),
                locator("text-001.xhtml#a8", "1.1 Two")
            ),
        ),
        entry(
            "gamma",
            ", <em>see</em> <span epub:type=\"index-xref-preferred\">alpha</span></li>",
        ),
    ];
    for part in expected {
        assert!(bodies[2].contains(&part), "{part}\n{}", bodies[2]);
    }
}
