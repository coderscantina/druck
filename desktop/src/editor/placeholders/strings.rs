//! Every string placeholders show, in English and German.

crate::strings! {
    pub enum Key {
        Statistics => ("Statistics", "Statistik"),
        Book => ("Book", "Buch"),
        Date => ("Date", "Datum"),
        StatisticsHint => ("Statistics, updates as you write", "Statistik, folgt dem Schreiben"),
        BookHint => ("Book, from its settings", "Buch, aus den Einstellungen"),
        DateHint => ("Date, the day it is set", "Datum, der Tag des Satzes"),
        NoValue => ("no value", "kein Wert"),
        Counting => ("counting", "wird gezählt"),
        Unknown => ("Unknown placeholder", "Unbekannter Platzhalter"),
        OnlyInBands => ("Only in headers and footers", "Nur in Kopf- und Fußzeilen"),
        NotInHeading => ("Not in a heading", "Nicht in Überschriften"),
        Writes => ("Writes", "Schreibt"),
        Insert => ("Insert", "Einfügen"),
        ToDo => ("To do", "To-do"),
        Words => ("Words", "Wörter"),
        WordsCounts => (
            "Words in the body text and footnotes, so code and the bibliography do not count.",
            "Wörter im Text und in den Fußnoten, ohne Code und Literatur."
        ),
        ReadingTime => ("Reading time", "Lesezeit"),
        ReadingTimeCounts => (
            "Minutes of silent reading, rounded up, at {speed} words a minute.",
            "Minuten stilles Lesen, aufgerundet, bei {speed} Wörtern pro Minute."
        ),
        Sentences => ("Sentences", "Sätze"),
        SentencesCounts => (
            "Sentences in the body text and footnotes.",
            "Sätze im Text und in den Fußnoten."
        ),
        Paragraphs => ("Paragraphs", "Absätze"),
        ParagraphsCounts => (
            "Paragraphs with a word in them, list items and quotations too.",
            "Absätze mit mindestens einem Wort, auch Listenpunkte und Zitate."
        ),
        Chars => ("Characters", "Zeichen"),
        CharsCounts => (
            "Characters a reader sees, spaces included.",
            "Zeichen, die man liest, mit Leerzeichen."
        ),
        CharsNoSpaces => ("Characters without spaces", "Zeichen ohne Leerzeichen"),
        CharsNoSpacesCounts => (
            "Characters a reader sees, without spaces.",
            "Zeichen, die man liest, ohne Leerzeichen."
        ),
        Figures => ("Figures", "Abbildungen"),
        FiguresCounts => ("Figures with a caption.", "Abbildungen mit Bildunterschrift."),
        Tables => ("Tables", "Tabellen"),
        TablesCounts => ("Tables with a caption.", "Tabellen mit Beschriftung."),
        Title => ("Title", "Titel"),
        TitleCounts => ("The book's title, from its settings.", "Der Titel des Buchs aus den Einstellungen."),
        Subtitle => ("Subtitle", "Untertitel"),
        SubtitleCounts => ("The subtitle, from the settings.", "Der Untertitel aus den Einstellungen."),
        Author => ("Author", "Autor"),
        AuthorCounts => ("The authors, joined with commas.", "Die Autoren, mit Kommas getrennt."),
        DateName => ("Date", "Datum"),
        DateCounts => ("The date in the settings, as written.", "Das Datum aus den Einstellungen, wie geschrieben."),
        Abstract => ("Abstract", "Zusammenfassung"),
        AbstractCounts => ("The abstract, from the settings.", "Die Zusammenfassung aus den Einstellungen."),
        Draft => ("Draft", "Entwurf"),
        DraftCounts => (
            "The draft label while the book is a draft, and no value otherwise.",
            "Die Entwurfsangabe, solange das Buch ein Entwurf ist, sonst kein Wert."
        ),
        MetaCounts => ("An entry of meta in the settings.", "Ein Eintrag unter meta in den Einstellungen."),
        Year => ("Year", "Jahr"),
        YearCounts => ("The year of the day the book is set.", "Das Jahr des Tages, an dem das Buch gesetzt wird."),
        BuildDate => ("Build date", "Satzdatum"),
        BuildDateCounts => (
            "The day the book is set, written out.",
            "Der Tag, an dem das Buch gesetzt wird, ausgeschrieben."
        ),
    }
}
