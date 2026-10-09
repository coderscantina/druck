//! The body as XHTML: files, links, notes, chapters, tables, and the navigation document.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Value, json};

use super::content::{self, Content, Context, Item};
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
    let document = crate::markdown::parse(body, 1, &source()).expect("document parses");
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
        bodies[3].starts_with("<section epub:type=\"chapter\"><h1 id=\"a1\" class=\"chapter\">1 One</h1>"),
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
            "A note.<a class=\"noteref\" epub:type=\"noteref\" id=\"fnref-1\" href=\"#fn-1\">1</a> More.</p>"
        ),
        "{}",
        bodies[0]
    );
    assert!(
        bodies[0].ends_with(
            "<section class=\"footnotes\" epub:type=\"footnotes\"><aside class=\"footnote\" epub:type=\"footnote\" \
         id=\"fn-1\"><p><a class=\"note-number\" href=\"#fnref-1\">1</a> First.</p></aside></section>"
        ),
        "{}",
        bodies[0]
    );
    assert!(
        bodies[1].contains("id=\"fn-2\"><p><a class=\"note-number\" href=\"#fnref-2\">2</a> Second <em>note</em>.</p>"),
        "{}",
        bodies[1]
    );
    assert!(!bodies[1].contains("id=\"fn-1\""), "{}", bodies[1]);
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
        bodies[1].starts_with("<section epub:type=\"bibliography\"><h1 id=\"a1\">References</h1>"),
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
    let body = "Fish & \\<chips\\> \"now\" at [shop](https://example.com/?a=1&b=\"2\").\n";
    let (content, _) = write(&config("", json!({"version": 1})), body, None);
    assert_eq!(
        bodies(&content)[0],
        "<p>Fish &amp; &lt;chips&gt; \"now\" at <a href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\">shop</a>.</p>"
    );
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
        "<section epub:type=\"chapter\"><h1 id=\"a0\" class=\"chapter\"><span class=\"chapter-number s-heading-1\">\
         Chapter I</span> The arrival</h1><p class=\"s-epigraph\">A quiet line.</p><p class=\"opening\"><span \
         class=\"lead-in\">Elias came</span> to the island late.</p><p>Then more.</p></section>"
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
        "<table id=\"a0\"><caption>Table 1: Costs</caption><thead><tr><th class=\"align-left\">Item</th><th \
         class=\"align-right\">Cost</th></tr></thead><tbody><tr><td class=\"align-left\">Tea</td><td \
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
        "<nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol><li><a href=\"text-001.xhtml#a0\">1 One</a><ol><li>\
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
        "<nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol><li><a href=\"text-001.xhtml\">A &amp; B</a></li></ol></nav>"
    );
}
