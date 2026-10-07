//! Fixed words the citation styles generate, per document language.

use crate::config::theme::Lang;

pub struct Words {
    pub and: &'static str,
    pub et_al: &'static str,
    pub editor: &'static str,
    pub editors: &'static str,
    pub page: &'static str,
    pub pages: &'static str,
    pub no_date: &'static str,
    pub accessed: &'static str,
    pub edition: &'static str,
    /// An edition number as an ordinal: "2nd", "2.".
    pub ordinal: fn(u32) -> String,
    pub phd_thesis: &'static str,
    pub masters_thesis: &'static str,
    pub thesis: &'static str,
    pub report: &'static str,
    pub in_: &'static str,
}

const EN: Words = Words {
    and: "and",
    et_al: "et al.",
    editor: "Ed.",
    editors: "Eds.",
    page: "p.",
    pages: "pp.",
    no_date: "n.d.",
    accessed: "Accessed",
    edition: "ed.",
    ordinal: english_ordinal,
    phd_thesis: "PhD thesis",
    masters_thesis: "Master's thesis",
    thesis: "Thesis",
    report: "Technical report",
    in_: "In",
};

const DE: Words = Words {
    and: "und",
    et_al: "et al.",
    editor: "Hrsg.",
    editors: "Hrsg.",
    page: "S.",
    pages: "S.",
    no_date: "o. J.",
    accessed: "Abgerufen am",
    edition: "Aufl.",
    ordinal: |number| format!("{number}."),
    phd_thesis: "Dissertation",
    masters_thesis: "Masterarbeit",
    thesis: "Abschlussarbeit",
    report: "Technischer Bericht",
    in_: "In",
};

fn english_ordinal(number: u32) -> String {
    let suffix = match (number % 10, number % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{number}{suffix}")
}

pub fn words(lang: Lang) -> &'static Words {
    match lang {
        Lang::En => &EN,
        Lang::De => &DE,
    }
}
