//! End-to-end behaviour of the bibliography: the fixture, diagnostics, both styles, and entry formatting.

use std::path::Path;

use crate::config::source::Source;
use crate::config::theme::{CitationStyle, Lang};
use crate::document::Inline;

use super::cite::{CitationId, Rendered};
use super::syntax::{Segment, find};
use super::*;

const FIXTURE: &str = include_str!("../../tests/fixtures/bib/all-types.bib");

fn fixture() -> Bibliography {
    Bibliography::parse(FIXTURE, Path::new("/docs/all-types.bib")).expect("the fixture is valid")
}

/// Cites each text in order and returns the rendered result with the ids.
fn render(texts: &[&str], style: CitationStyle, lang: Lang) -> (Rendered, Vec<CitationId>) {
    let bibliography = fixture();
    let mut citations = Citations::new(&bibliography);
    let ids = texts
        .iter()
        .flat_map(|text| find(text))
        .filter_map(|segment| match segment {
            Segment::Citation(citation) => Some(citation),
            _ => None,
        })
        .map(|citation| citations.add(&citation).expect("known keys"))
        .collect();
    (citations.finish(style, lang), ids)
}

fn cited(texts: &[&str], style: CitationStyle, lang: Lang) -> Vec<String> {
    let (rendered, ids) = render(texts, style, lang);
    ids.iter().map(|&id| text(&rendered, id)).collect()
}

fn text(rendered: &Rendered, id: CitationId) -> String {
    rendered.citation(id).iter().map(|part| part.text.as_str()).collect()
}

fn plain(content: &[Inline]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text { text, style } if style.emphasis => format!("*{text}*"),
            Inline::Text { text, style } if style.link.is_some() => format!("<{text}>"),
            Inline::Text { text, .. } => text.clone(),
            _ => String::new(),
        })
        .collect()
}

fn reference_texts(keys_text: &str, style: CitationStyle, lang: Lang) -> Vec<String> {
    let (rendered, _) = render(&[keys_text], style, lang);
    rendered
        .references()
        .iter()
        .map(|reference| {
            format!(
                "{}{}",
                reference.label.as_deref().map(|l| format!("{l} ")).unwrap_or_default(),
                plain(&reference.content)
            )
        })
        .collect()
}

#[test]
fn reads_every_supported_type_from_the_fixture() {
    let bibliography = fixture();
    assert_eq!(bibliography.entries().len(), 11);
    assert_eq!(bibliography.index("lee2022"), Some(4));
}

#[test]
fn reports_unsupported_types_duplicates_and_syntax_with_the_path_and_location() {
    let text = "@inbook{a, title = {T}}\n@misc{b, title = {T}, url = {u}}\n  @misc{b, title = {T}, url = {v}}";
    let diagnostics = Bibliography::parse(text, Path::new("/docs/refs.bib")).unwrap_err();
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
        diagnostics[0].source,
        Some(Source::Bibliography("/docs/refs.bib".into()))
    );
    assert_eq!(diagnostics[0].location, Some((1, 1)));
    assert!(diagnostics[0].message.contains("unsupported type `@inbook`"));
    assert_eq!(diagnostics[1].location, Some((3, 3)));
    assert!(diagnostics[1].message.contains("defined twice"));
    assert_eq!(
        diagnostics[0].to_string(),
        "error: /docs/refs.bib:1:1: entry `a` has the unsupported type `@inbook`"
    );
}

#[test]
fn reports_missing_required_fields_per_entry_in_file_order() {
    let text = "@book{b, title = {T}, year = 2020}\n@article{a, author = {A}, title = {T}, journal = {J}}";
    let diagnostics = Bibliography::parse(text, Path::new("refs.bib")).unwrap_err();
    assert_eq!(diagnostics[0].location, Some((1, 1)));
    assert!(diagnostics[0].message.contains("`author or editor`, `publisher`"));
    assert_eq!(diagnostics[1].location, Some((2, 1)));
    assert!(diagnostics[1].message.contains("`year`"));
}

#[test]
fn reports_unknown_keys_at_the_citation() {
    let bibliography = fixture();
    let mut citations = Citations::new(&bibliography);
    let text = "see [@lee2022; @nobody2000]";
    let Segment::Citation(citation) = &find(text)[1] else {
        panic!("expected a citation")
    };
    let missing = citations.add(citation).unwrap_err();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].key, "nobody2000");
    assert_eq!(missing[0].item, 1);
}

#[test]
fn formats_author_date_citations_in_english() {
    let texts =
        ["[@weber2020] [@lee2022, p. 12] [@mueller2019; @ito2021, pp. 3-5] @who2023 [p. 7] @typst [@knuth1984]"];
    assert_eq!(
        cited(&texts, CitationStyle::AuthorDate, Lang::En),
        [
            "(Weber 2020)",
            "(Lee et al. 2022, p.\u{a0}12)",
            "(Müller et al. 2019; Ito 2021, pp.\u{a0}3–5)",
            "World Health Organization (2023, p.\u{a0}7)",
            "Typst documentation (n.d.)",
            "(Knuth 1984)",
        ]
    );
}

#[test]
fn formats_author_date_citations_in_german() {
    let texts = ["[@smith2024a, S. 12] @smith2024a [S. 3-4]"];
    assert_eq!(
        cited(&texts, CitationStyle::AuthorDate, Lang::De),
        [
            "(Smith und Jones 2024, S.\u{a0}12)",
            "Smith und Jones (2024, S.\u{a0}3–4)"
        ]
    );
    assert_eq!(
        cited(&["@typst"], CitationStyle::AuthorDate, Lang::De),
        ["Typst documentation (o. J.)"]
    );
}

#[test]
fn adds_letters_to_works_with_the_same_label_and_year() {
    let texts = ["[@smith2024a] [@smith2024b] [@lee2022]"];
    // "Smith and Jones" and "Smith" differ, so neither needs a letter.
    assert_eq!(
        cited(&texts, CitationStyle::AuthorDate, Lang::En),
        ["(Smith and Jones 2024)", "(Smith 2024)", "(Lee et al. 2022)"]
    );

    let bibliography = Bibliography::parse(
        "@misc{x, author = {Ada Smith}, title = {Zeta}, url = {u}, year = 2024}\n@misc{y, author = {Ada Smith}, title = {Alpha}, url = {u}, year = 2024}",
        Path::new("d.bib"),
    )
    .unwrap();
    let mut citations = Citations::new(&bibliography);
    let ids: Vec<_> = find("[@x] [@y]")
        .into_iter()
        .filter_map(|s| match s {
            Segment::Citation(c) => Some(citations.add(&c).unwrap()),
            _ => None,
        })
        .collect();
    let rendered = citations.finish(CitationStyle::AuthorDate, Lang::En);
    assert_eq!(text(&rendered, ids[0]), "(Smith 2024b)");
    assert_eq!(text(&rendered, ids[1]), "(Smith 2024a)");
    assert_eq!(rendered.references()[0].key, "y");
}

#[test]
fn sorts_the_author_date_bibliography_by_author_year_and_title() {
    let (rendered, _) = render(
        &["[@weber2020; @mueller2019; @knuth1984; @smith2024b; @smith2024a; @lee2022; @typst; @lab2018; @who2023]"],
        CitationStyle::AuthorDate,
        Lang::En,
    );
    let keys: Vec<&str> = rendered.references().iter().map(|r| r.key.as_str()).collect();
    // Knuth, Kurrent Lab, Lee, Müller (as Mueller), Smith Ada, Smith and Jones, Typst documentation, Weber, World.
    assert_eq!(
        keys,
        [
            "knuth1984",
            "lab2018",
            "lee2022",
            "mueller2019",
            "smith2024b",
            "smith2024a",
            "typst",
            "weber2020",
            "who2023"
        ]
    );
    assert!(rendered.references().iter().all(|r| r.label.is_none()));
}

#[test]
fn formats_numeric_citations_with_grouping_locators_and_narrative_form() {
    let texts = [
        "[@weber2020] [@lee2022, p. 12] [@ito2021] [@weber2020; @ito2021; @lee2022; @knuth1984] [@knuth1984; @weber2020, pp. 3-5] @lee2022 [p. 2] @ito2021",
    ];
    assert_eq!(
        cited(&texts, CitationStyle::Numeric, Lang::En),
        [
            "[1]",
            "[2, p.\u{a0}12]",
            "[3]",
            "[1–4]",
            "[1, pp.\u{a0}3–5; 4]",
            "Lee et al. [2, p.\u{a0}2]",
            "Ito [3]",
        ]
    );
}

#[test]
fn puts_prefixes_before_the_work_they_belong_to() {
    let texts = ["[see @weber2020, p. 3; also @lee2022] [see @lee2022; also @weber2020] [@lee2022]"];
    assert_eq!(
        cited(&texts, CitationStyle::AuthorDate, Lang::En),
        [
            "(see Weber 2020, p.\u{a0}3; also Lee et al. 2022)",
            "(see Lee et al. 2022; also Weber 2020)",
            "(Lee et al. 2022)",
        ]
    );
    assert_eq!(
        cited(&texts, CitationStyle::Numeric, Lang::En),
        ["[see 1, p.\u{a0}3; also 2]", "[see 2; also 1]", "[2]"]
    );
}

#[test]
fn puts_suffixes_after_the_locator() {
    let texts = ["[@weber2020, p. 3, emphasis added; @lee2022, see also p. 4] @weber2020 [p. 3, passim]"];
    assert_eq!(
        cited(&texts, CitationStyle::AuthorDate, Lang::En),
        [
            "(Weber 2020, p.\u{a0}3, emphasis added; Lee et al. 2022, see also p. 4)",
            "Weber (2020, p.\u{a0}3, passim)",
        ]
    );
    assert_eq!(
        cited(&texts, CitationStyle::Numeric, Lang::En),
        [
            "[1, p.\u{a0}3, emphasis added; 2, see also p. 4]",
            "Weber [1, p.\u{a0}3, passim]",
        ]
    );
}

/// The linked pieces of a citation as text and the key of the work each points to.
fn links(rendered: &Rendered, id: CitationId) -> Vec<(String, &str)> {
    rendered
        .citation(id)
        .iter()
        .filter_map(|part| Some((part.text.clone(), rendered.references()[part.target?].key.as_str())))
        .collect()
}

#[test]
fn links_each_work_of_a_citation_to_its_own_entry() {
    let (rendered, ids) = render(
        &["[@lee2022, p. 12; @weber2020] [@weber2020; @ito2021; @lee2022; @knuth1984]"],
        CitationStyle::AuthorDate,
        Lang::En,
    );
    let works = links(&rendered, ids[0]);
    assert_eq!(works.len(), 2);
    assert_eq!(works[0].1, "lee2022");
    assert_eq!(works[1].1, "weber2020");

    let (rendered, ids) = render(
        &["[@weber2020; @ito2021; @lee2022; @knuth1984] [@lee2022, p. 3; @weber2020]"],
        CitationStyle::Numeric,
        Lang::En,
    );
    let numbers: Vec<_> = links(&rendered, ids[0]);
    assert_eq!(
        numbers,
        [("1", "weber2020"), ("4", "knuth1984")].map(|(text, key)| (text.to_owned(), key))
    );
    assert_eq!(links(&rendered, ids[1]).len(), 2);
}

#[test]
fn groups_numbers_into_ranges_of_three_or_more() {
    assert_eq!(cite::runs(&[1, 2, 3, 5]), [(1, 3), (5, 5)]);
    assert_eq!(cite::runs(&[1, 2, 4, 5, 6, 9]), [(1, 1), (2, 2), (4, 6), (9, 9)]);
    assert_eq!(cite::runs(&[2, 2]), [(2, 2)]);
}

#[test]
fn orders_the_numeric_bibliography_by_first_citation_with_labels() {
    let texts = "[@weber2020] [@ito2021] [@weber2020]";
    let entries = reference_texts(texts, CitationStyle::Numeric, Lang::En);
    assert_eq!(entries.len(), 2);
    assert!(entries[0].starts_with("[1] Weber, Lena (2020)."));
    assert!(entries[1].starts_with("[2] Ito, Mei (2021)."));
}

#[test]
fn formats_an_article_and_a_book() {
    let all = "[@smith2024a; @smith2024b; @mueller2019; @knuth1984]";
    let entries = reference_texts(all, CitationStyle::AuthorDate, Lang::En);
    assert_eq!(
        entries[0],
        "Knuth, Donald E. (Ed.) (1984). *The TeXbook*. Addison-Wesley."
    );
    assert_eq!(
        entries[1],
        "Müller, Hans, Eva Großmann, and Cy Lee (2019). *Satz und Schrift: Grundlagen der digitalen Typografie*. 2nd ed. Berlin: Springer Vieweg."
    );
    assert_eq!(
        entries[2],
        "Smith, Ada (2024). Line breaking revisited. *Journal of Document Engineering* 12, 101–118."
    );
    assert_eq!(
        entries[3],
        "Smith, Ada and Bob Jones (2024). Typesetting at scale: PDF generation without a browser. *Journal of Document Engineering* 12(3), 45–67. <https://doi.org/10.1000/jde.2024.12>"
    );
}

#[test]
fn formats_a_german_book_edition_and_labels() {
    let entries = reference_texts("[@mueller2019]", CitationStyle::AuthorDate, Lang::De);
    assert_eq!(
        entries[0],
        "Müller, Hans, Eva Großmann und Cy Lee (2019). *Satz und Schrift: Grundlagen der digitalen Typografie*. 2. Aufl. Berlin: Springer Vieweg."
    );
}

#[test]
fn formats_conference_papers_and_theses() {
    let entries = reference_texts("[@lee2022; @weber2020; @ito2021]", CitationStyle::AuthorDate, Lang::En);
    assert_eq!(
        entries[0],
        "Ito, Mei (2021). *Glyph caching in PDF writers*. Master's thesis, University of Tokyo."
    );
    assert_eq!(
        entries[1],
        "Lee, Cy et al. (2022). Balanced columns with dynamic programming. In: *Proceedings of the ACM Symposium on Document Engineering*, 1–10. New York: ACM."
    );
    assert_eq!(
        entries[2],
        "Weber, Lena (2020). *Zeilenumbruch und Silbentrennung in öffentlichen Dokumenten*. PhD thesis, Technische Universität München."
    );
    let german = reference_texts("[@weber2020]", CitationStyle::AuthorDate, Lang::De);
    assert!(german[0].ends_with(". Dissertation, Technische Universität München."));
}

#[test]
fn formats_reports_and_web_resources() {
    let entries = reference_texts(
        "[@who2023; @lab2018; @typst; @pdfspec]",
        CitationStyle::AuthorDate,
        Lang::En,
    );
    assert_eq!(
        entries[0],
        "Adobe Systems (2008). PDF reference. <https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf>"
    );
    assert_eq!(
        entries[1],
        "Kurrent Lab (2018). *Font metrics in practice*. Technical report."
    );
    assert_eq!(
        entries[2],
        "Typst documentation (n.d.). Typst GmbH. <https://typst.app/docs/> Accessed 2024-05-05."
    );
    assert_eq!(
        entries[3],
        "World Health Organization (2023). *Guidance on accessible documents*. Technical report WHO/DOC/2023.1, World Health Organization, Geneva."
    );
}
