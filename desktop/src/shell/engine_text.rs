//! The engine's messages in the app's languages. The engine says what it found as a [`Code`] with its values, so nothing
//! here parses its English. A diagnostic with no code yet shows the engine's own words, see [the decision](../../../docs/DECISIONS.md#2026-11-05-review-findings).

use std::path::Path;

use druck::diagnostic::{Code, Diagnostic, PageName, path_from};

use super::strings::{Language, language};

crate::strings! {
    pub enum Key {
        Page => ("page {page}", "Seite {page}"),
        PagePrinted => ("page {page} ({printed})", "Seite {page} ({printed})"),
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
        ImageNotFound => (
            "the image {file} is not there; a box stands in for it",
            "das Bild {file} gibt es nicht; ein Kasten steht an seiner Stelle"
        ),
        FontNotFound => (
            "the font file {file} of {family} is not there; {stand_in} stands in",
            "die Schriftdatei {file} von {family} gibt es nicht; {stand_in} steht dafür"
        ),
        BibliographyNotFound => (
            "the bibliography {file} is not there; citations show their keys",
            "die Literaturdatei {file} gibt es nicht; Zitate zeigen ihren Schlüssel"
        ),
        UnknownStyleOf => (
            "unknown style \"{name}\"; the theme's custom styles are {known}",
            "unbekannter Stil \"{name}\"; die eigenen Stile des Themas sind {known}"
        ),
    }
}

/// A page as the status line words it: by its place in the file, with its printed number in brackets when that differs.
/// `place` is the page's index, when the diagnostic has one.
fn page_of(page: &PageName, place: Option<usize>) -> String {
    match (page, place.map(|index| index + 1)) {
        (PageName::Numbered(printed), Some(place)) if *printed != place.to_string() => {
            crate::t!(Key::PagePrinted, page = place, printed = printed)
        }
        (_, Some(place)) => crate::t!(Key::Page, page = place),
        (PageName::Numbered(page), None) => crate::t!(Key::Page, page = page),
        (PageName::Unnumbered(page), None) => crate::t!(Key::UnnumberedPage, page = page),
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

/// A diagnostic's message in the language now set: its code's words, or the engine's own with no code. It stands on
/// its own in a list or a line, so it starts with a capital, where the engine's start lower case. With the book's
/// folder `base`, the files a code names are given from there, as the Binder knows them, and paths below it in the
/// engine's own words are shortened the same way.
pub fn message_in(diagnostic: &Diagnostic, base: Option<&Path>) -> String {
    let words = (diagnostic.code.as_ref())
        .and_then(|code| coded(code, diagnostic.page, base))
        .unwrap_or_else(|| match base {
            Some(base) => diagnostic.message.replace(&format!("{}/", base.display()), ""),
            None => diagnostic.message.clone(),
        });
    let mut chars = words.chars();
    chars
        .next()
        .map_or(words.clone(), |first| first.to_uppercase().chain(chars).collect())
}

fn coded(code: &Code, place: Option<usize>, base: Option<&Path>) -> Option<String> {
    let file = |file: &Path| base.map_or_else(|| file.display().to_string(), |base| path_from(file, base));
    Some(match code {
        Code::PageShort { page, lines } => {
            crate::t!(Key::PageShort, page = page_of(page, place), lines = decimal(*lines))
        }
        Code::PageUneven { page, lines } => {
            crate::t!(Key::PageUneven, page = page_of(page, place), lines = decimal(*lines))
        }
        Code::ChapterEndShort { page, lines, wanted } => {
            crate::t!(
                Key::ChapterEndShort,
                page = page_of(page, place),
                lines = lines_of(*lines),
                wanted = wanted
            )
        }
        Code::OneLinerAtTop { page } => crate::t!(Key::OneLinerAtTop, page = page_of(page, place)),
        Code::OneLinerAtBottom { page } => crate::t!(Key::OneLinerAtBottom, page = page_of(page, place)),
        Code::HeadingAtFoot { page, lines, wanted } => {
            crate::t!(
                Key::HeadingAtFoot,
                page = page_of(page, place),
                lines = lines_of(*lines),
                wanted = wanted
            )
        }
        Code::LooseLine { page } => crate::t!(Key::LooseLine, page = page_of(page, place)),
        Code::TooManyHyphens { page, limit } => {
            crate::t!(Key::TooManyHyphens, page = page_of(page, place), limit = limit)
        }
        Code::ShortLastLine { page } => crate::t!(Key::ShortLastLine, page = page_of(page, place)),
        Code::NotInChapterList => crate::t!(Key::NotInChapterList).to_owned(),
        Code::UndefinedLabel { label } => crate::t!(Key::UndefinedLabel, label = label),
        Code::UndefinedFootnote { label } => crate::t!(Key::UndefinedFootnote, label = label),
        Code::NoBibliography => crate::t!(Key::NoBibliography).to_owned(),
        Code::MissingCitation { file: path, key } => crate::t!(Key::MissingCitation, file = file(path), key = key),
        Code::ResourceNotFound { property, file: path } => {
            crate::t!(Key::ResourceNotFound, property = property, file = file(path))
        }
        Code::ImageNotFound { file: path } => crate::t!(Key::ImageNotFound, file = file(path)),
        Code::FontNotFound {
            family,
            file: path,
            stand_in,
        } => crate::t!(
            Key::FontNotFound,
            file = file(path),
            family = family,
            stand_in = stand_in
        ),
        Code::BibliographyNotFound { file: path } => crate::t!(Key::BibliographyNotFound, file = file(path)),
        Code::UnknownStyle { name, known } if known.is_empty() => crate::t!(Key::UnknownStyle, name = name),
        Code::UnknownStyle { name, known } => {
            crate::t!(Key::UnknownStyleOf, name = name, known = known.join(", "))
        }
        _ => return None,
    })
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
        assert_eq!(message_in(&short, None), "Seite 12 endet 1,4 Zeilen über dem Seitenfuß");
        set_language(Language::English);
        assert_eq!(message_in(&short, None), "Page 12 ends 1.4 lines short of the bottom");
    }

    #[test]
    fn a_page_is_worded_by_its_place_with_a_differing_printed_number_in_brackets() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_language(Language::English);
        let at = |printed: &str, index: usize| {
            let mut diagnostic = with_code(Code::LooseLine {
                page: PageName::Numbered(printed.into()),
            });
            diagnostic.page = Some(index);
            message_in(&diagnostic, None)
        };
        assert_eq!(
            at("iii", 2),
            "A line of this paragraph on page 3 (iii) has its spaces stretched loose"
        );
        assert_eq!(
            at("5", 4),
            "A line of this paragraph on page 5 has its spaces stretched loose"
        );
    }

    #[test]
    fn a_missing_image_names_its_file_from_the_book() {
        let _guard = LANGUAGE_TEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_language(Language::German);
        let missing = with_code(Code::ImageNotFound {
            file: Path::new("/books/one/assets/gone.png").to_owned(),
        });
        assert_eq!(
            message_in(&missing, Some(Path::new("/books/one"))),
            "Das Bild assets/gone.png gibt es nicht; ein Kasten steht an seiner Stelle"
        );
        set_language(Language::English);
    }

    #[test]
    fn a_message_with_no_code_keeps_the_engines_words_with_a_capital() {
        assert_eq!(
            message_in(&Diagnostic::error("the engine's words"), None),
            "The engine's words"
        );
    }
}
