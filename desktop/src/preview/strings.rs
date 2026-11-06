//! The strings of Publish's pages: the status line and the proof slip, in English and German.

crate::strings! {
    pub enum Key {
        NoPagesYet => ("No pages yet", "Noch keine Seiten"),
        NoPages => ("No pages", "Keine Seiten"),
        OpenDetails => ("Book details", "Buchdaten"),
        ShowInText => ("Show in text", "Im Text zeigen"),
        AtLine => ("{file}, line {line}: {message}", "{file}, Zeile {line}: {message}"),
        OpenProof => ("Proof", "Korrektur"),
        PageOf => ("Page {page} of {pages}", "Seite {page} von {pages}"),
        PagesOf => ("Pages {first}\u{2013}{last} of {pages}", "Seiten {first}\u{2013}{last} von {pages}"),
        OnePage => ("1 page", "1 Seite"),
        PageCount => ("{pages} pages", "{pages} Seiten"),
        ToLookAt => ("{count} to look at", "{count} zu prüfen"),
        OneWarning => ("1 warning", "1 Warnung"),
        Warnings => ("{count} warnings", "{count} Warnungen"),
        WarningMarks => ("Marks", "Markierungen"),
        ZoomOut => ("Zoom out", "Verkleinern"),
        ZoomIn => ("Zoom in", "Vergrößern"),
        ZoomFit => ("Fit", "Einpassen"),
        SlipLabel => ("Proof slip", "Korrekturzettel"),
        SlipText => ("Paragraph text", "Absatztext"),
        SlipPageBreak => ("Page break before", "Seitenumbruch davor"),
        SlipKeep => ("Keep together", "Zusammenhalten"),
        SlipEscape => ("Esc closes", "Esc schließt"),
        SlipStyles => ("Styles of this paragraph", "Stile dieses Absatzes"),
        SlipAlignLeft => ("Left", "Links"),
        SlipAlignCenter => ("Center", "Mitte"),
        SlipAlignRight => ("Right", "Rechts"),
        SlipAlignJustify => ("Justify", "Block"),
        SlipShowStyle => ("Edit this style", "Stil bearbeiten"),
        SlipStyleWaits => (
            "Choose where the theme is changed, and the style follows.",
            "Wähle, wo das Theme geändert wird, dann folgt der Stil."
        ),
        EngineStopped => (
            "The typesetter stopped on an internal error: {message}",
            "Der Satz hat mit einem internen Fehler angehalten: {message}"
        ),
        SlipNoTheme => ("A style needs a theme file to live in.", "Ein Stil braucht eine Theme-Datei."),
    }
}
