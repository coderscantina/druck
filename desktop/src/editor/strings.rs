//! Every string the editor's commands show, in English and German.

use std::sync::OnceLock;

macro_rules! strings {
    ($($name:ident: $en:literal, $de:literal;)*) => {
        pub struct Strings {
            $(pub $name: &'static str,)*
        }

        pub static EN: Strings = Strings { $($name: $en,)* };
        pub static DE: Strings = Strings { $($name: $de,)* };
    };
}

strings! {
    bold: "Bold", "Fett";
    italic: "Italic", "Kursiv";
    underline: "Underline", "Unterstrichen";
    strike: "Strikethrough", "Durchgestrichen";
    superscript: "Superscript", "Hochgestellt";
    subscript: "Subscript", "Tiefgestellt";
    small_caps: "Small capitals", "Kapitälchen";
    quote: "Quotation", "Zitat";
    bullets: "Bulleted list", "Aufzählung";
    numbers: "Numbered list", "Nummerierte Liste";
    link: "Link", "Link";
    remove_link: "Remove", "Entfernen";
    paragraph: "Text", "Text";
    heading_1: "Heading 1", "Überschrift 1";
    heading_2: "Heading 2", "Überschrift 2";
    heading_3: "Heading 3", "Überschrift 3";
    heading_4: "Heading 4", "Überschrift 4";
    heading_5: "Heading 5", "Überschrift 5";
    heading_6: "Heading 6", "Überschrift 6";
    note: "Note", "Hinweis";
    tip: "Tip", "Tipp";
    important: "Important", "Wichtig";
    warning: "Warning", "Warnung";
    caution: "Caution", "Vorsicht";
    code: "Code block", "Codeblock";
    scene_break: "Scene break", "Szenenumbruch";
    table: "Table", "Tabelle";
    image: "Image", "Bild";
    keep: "Keep together", "Zusammenhalten";
    columns: "Two columns", "Zwei Spalten";
    bottom: "Bottom of page", "Seitenende";
    page_break: "Page break", "Seitenumbruch";
    toc: "Table of contents", "Inhaltsverzeichnis";
    footnote: "Footnote", "Fußnote";
    citation: "Citation", "Quellenangabe";
    new_section: "New section", "Neuer Abschnitt";
    source_view: "Source", "Quelltext";
    notes_title: "NOTES", "NOTIZEN";
    notes_empty: "This chapter opens with no comment. A comment at its very start shows here.",
        "Dieses Kapitel beginnt ohne Kommentar. Ein Kommentar ganz am Anfang erscheint hier.";
    notes_refused: "A note cannot hold -->, which would end it.", "Eine Notiz kann --> nicht enthalten, das würde sie beenden.";
    placeholder_heading: "Heading", "Überschrift";
    placeholder_item: "Item", "Eintrag";
    placeholder_quote: "Quotation", "Zitat";
    placeholder_text: "Text", "Text";
    placeholder_code: "code", "code";
    placeholder_column: "Column", "Spalte";
    placeholder_cell: "Cell", "Zelle";
    placeholder_caption: "Caption", "Bildunterschrift";
    placeholder_footnote: "Footnote text.", "Text der Fußnote.";
}

/// The strings of the app's language: German for a German system, otherwise English.
pub fn strings() -> &'static Strings {
    static CHOSEN: OnceLock<&'static Strings> = OnceLock::new();
    CHOSEN.get_or_init(|| {
        let language = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        if language.starts_with("de") { &DE } else { &EN }
    })
}
