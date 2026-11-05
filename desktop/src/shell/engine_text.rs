//! The engine's messages in the app's languages. The engine says what it found as a [`Code`] with its values, so nothing
//! here parses its English. A diagnostic with no code yet shows the engine's own words, see [the decision](../../../docs/DECISIONS.md#2026-11-05-review-findings).

use druck::diagnostic::{Code, Diagnostic, PageName};

use super::strings::{Language, language};

crate::strings! {
    pub enum Key {
        Page => ("page {page}", "Seite {page}"),
        UnnumberedPage => ("unnumbered page {page}", "Seite {page} ohne Nummer"),
        OneLine => ("1 line", "1 Zeile"),
        Lines => ("{count} lines", "{count} Zeilen"),
        PageShort => ("{page} ends {lines} lines short of the bottom", "{page} endet {lines} Zeilen über dem Seitenfuß"),
        PageUneven => (
            "{page} ends {lines} lines off the height of the page facing it",
            "{page} endet {lines} Zeilen versetzt zur gegenüberliegenden Seite"
        ),
        ChapterEndShort => (
            "{page} ends a chapter with {lines}, fewer than the {wanted} wanted",
            "{page} beendet ein Kapitel mit {lines}, gewünscht sind {wanted}"
        ),
        OneLinerAtTop => (
            "this one-line paragraph stands alone at the top of {page}",
            "dieser einzeilige Absatz steht allein oben auf {page}"
        ),
        OneLinerAtBottom => (
            "this one-line paragraph stands alone at the bottom of {page}",
            "dieser einzeilige Absatz steht allein unten auf {page}"
        ),
        HeadingAtFoot => (
            "a heading at the foot of {page} has {lines} of this text below it, fewer than the {wanted} wanted",
            "eine Überschrift am Fuß von {page} hat {lines} Text unter sich, gewünscht sind {wanted}"
        ),
        LooseLine => (
            "a line of this paragraph on {page} has its spaces stretched loose",
            "eine Zeile dieses Absatzes auf {page} hat zu weite Wortabstände"
        ),
        TooManyHyphens => (
            "this paragraph has more than {limit} hyphenated lines in a row on {page}",
            "dieser Absatz hat mehr als {limit} Trennungen in Folge auf {page}"
        ),
        ShortLastLine => (
            "this paragraph ends in a short last line on {page}",
            "dieser Absatz endet auf {page} in einer kurzen letzten Zeile"
        ),
        NotInChapterList => (
            "excluded from the book: not listed under chapters in druck.yaml",
            "nicht im Buch: fehlt unter chapters in druck.yaml"
        ),
        UndefinedLabel => (
            "@{label} refers to an undefined label; define it with {#{label}}",
            "@{label} verweist auf eine unbekannte Marke; lege sie mit {#{label}} an"
        ),
        UndefinedFootnote => ("footnote [^{label}] has no definition", "Fußnote [^{label}] hat keinen Text"),
        NoBibliography => (
            "a citation needs a bibliography; set `bibliography` to a BibTeX file in the front matter",
            "ein Zitat braucht ein Literaturverzeichnis; nenne unter `bibliography` eine BibTeX-Datei"
        ),
        MissingCitation => ("no entry in {file} has the key `{key}`", "kein Eintrag in {file} hat den Schlüssel `{key}`"),
        UnknownStyle => (
            "unknown style \"{name}\"; the theme defines no custom styles",
            "unbekannter Stil \"{name}\"; das Thema hat keine eigenen Stile"
        ),
        ResourceNotFound => (
            "{property}: the file {file} is not there",
            "{property}: die Datei {file} gibt es nicht"
        ),
        UnknownStyleOf => (
            "unknown style \"{name}\"; the theme's custom styles are {known}",
            "unbekannter Stil \"{name}\"; die eigenen Stile des Themas sind {known}"
        ),
    }
}

fn page_of(page: &PageName) -> String {
    match page {
        PageName::Numbered(page) => crate::t!(Key::Page, page = page),
        PageName::Unnumbered(page) => crate::t!(Key::UnnumberedPage, page = page),
    }
}

fn lines_of(count: usize) -> String {
    if count == 1 {
        crate::t!(Key::OneLine).to_owned()
    } else {
        crate::t!(Key::Lines, count = count)
    }
}

/// A number of lines as the language writes it, with one decimal.
fn decimal(lines: f64) -> String {
    let text = format!("{lines:.1}");
    match language() {
        Language::German => text.replace('.', ","),
        Language::English => text,
    }
}

/// A diagnostic's message in the language now set: its code's words, or the engine's own with no code.
pub fn message(diagnostic: &Diagnostic) -> String {
    let Some(code) = &diagnostic.code else {
        return diagnostic.message.clone();
    };
    match code {
        Code::PageShort { page, lines } => crate::t!(Key::PageShort, page = page_of(page), lines = decimal(*lines)),
        Code::PageUneven { page, lines } => crate::t!(Key::PageUneven, page = page_of(page), lines = decimal(*lines)),
        Code::ChapterEndShort { page, lines, wanted } => {
            crate::t!(
                Key::ChapterEndShort,
                page = page_of(page),
                lines = lines_of(*lines),
                wanted = wanted
            )
        }
        Code::OneLinerAtTop { page } => crate::t!(Key::OneLinerAtTop, page = page_of(page)),
        Code::OneLinerAtBottom { page } => crate::t!(Key::OneLinerAtBottom, page = page_of(page)),
        Code::HeadingAtFoot { page, lines, wanted } => {
            crate::t!(
                Key::HeadingAtFoot,
                page = page_of(page),
                lines = lines_of(*lines),
                wanted = wanted
            )
        }
        Code::LooseLine { page } => crate::t!(Key::LooseLine, page = page_of(page)),
        Code::TooManyHyphens { page, limit } => crate::t!(Key::TooManyHyphens, page = page_of(page), limit = limit),
        Code::ShortLastLine { page } => crate::t!(Key::ShortLastLine, page = page_of(page)),
        Code::NotInChapterList => crate::t!(Key::NotInChapterList).to_owned(),
        Code::UndefinedLabel { label } => crate::t!(Key::UndefinedLabel, label = label),
        Code::UndefinedFootnote { label } => crate::t!(Key::UndefinedFootnote, label = label),
        Code::NoBibliography => crate::t!(Key::NoBibliography).to_owned(),
        Code::MissingCitation { file, key } => crate::t!(Key::MissingCitation, file = file.display(), key = key),
        Code::ResourceNotFound { property, file } => {
            crate::t!(Key::ResourceNotFound, property = property, file = file.display())
        }
        Code::UnknownStyle { name, known } if known.is_empty() => crate::t!(Key::UnknownStyle, name = name),
        Code::UnknownStyle { name, known } => {
            crate::t!(Key::UnknownStyleOf, name = name, known = known.join(", "))
        }
        _ => diagnostic.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::strings::{LANGUAGE_TEST, set_language};

    fn with_code(code: Code) -> Diagnostic {
        let mut diagnostic = Diagnostic::error("the engine's words");
        diagnostic.code = Some(code);
        diagnostic
    }

    #[test]
    fn a_coded_message_reads_in_the_language_set_and_english_matches_the_engine() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let short = with_code(Code::PageShort {
            page: PageName::Numbered("12".into()),
            lines: 1.4,
        });
        set_language(Language::German);
        assert_eq!(message(&short), "Seite 12 endet 1,4 Zeilen über dem Seitenfuß");
        set_language(Language::English);
        assert_eq!(message(&short), "page 12 ends 1.4 lines short of the bottom");
    }

    #[test]
    fn a_message_with_no_code_keeps_the_engines_words() {
        assert_eq!(message(&Diagnostic::error("the engine's words")), "the engine's words");
    }
}
