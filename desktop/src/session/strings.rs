//! The names and descriptions of what a new section and a new project start from, in English and German.

crate::strings! {
    pub enum Key {
        GroupFront => ("Front matter", "Titelei"),
        GroupBody => ("Body", "Hauptteil"),
        GroupBack => ("Back matter", "Anhang"),

        HalfTitleName => ("Half title", "Schmutztitel"),
        HalfTitleDescription => (
            "Only the title, before the full title page",
            "Nur der Titel, vor der vollen Titelseite"
        ),
        TitlePageName => ("Title page", "Titelseite"),
        TitlePageDescription => ("The title, subtitle, and author", "Titel, Untertitel und Autor"),
        ImprintName => ("Imprint", "Impressum"),
        ImprintDescription => (
            "Copyright and edition, at the bottom of the page",
            "Copyright und Auflage, unten auf der Seite"
        ),
        DedicationName => ("Dedication", "Widmung"),
        DedicationDescription => ("A line for someone, on its own page", "Eine Zeile für jemanden, auf eigener Seite"),
        ContentsName => ("Contents", "Inhalt"),
        ContentsDescription => (
            "The table of contents, built from the headings",
            "Das Inhaltsverzeichnis, aus den Überschriften"
        ),
        AbstractName => ("Abstract", "Abstract"),
        AbstractDescription => ("A brief summary of what this is about", "Eine kurze Zusammenfassung des Inhalts"),
        ChapterName => ("Chapter", "Kapitel"),
        ChapterDescription => ("A numbered chapter with its heading", "Ein nummeriertes Kapitel mit Überschrift"),
        PartName => ("Part", "Teil"),
        PartDescription => (
            "An unnumbered heading that opens a part",
            "Eine Überschrift ohne Nummer, die einen Teil öffnet"
        ),
        BibliographyName => ("Bibliography", "Literatur"),
        BibliographyDescription => (
            "The works cited, placed here instead of at the end",
            "Die zitierten Werke, hier statt am Ende"
        ),
        IndexName => ("Index", "Register"),
        IndexDescription => (
            "The marked terms, placed here instead of at the end",
            "Die markierten Begriffe, hier statt am Ende"
        ),
        BookName => ("Book", "Buch"),
        BookDescription => (
            "Chapters, a title page, and print and ebook exports",
            "Kapitel, Titelseite und Exporte für Druck und E-Book"
        ),
        ReportName => ("Report", "Bericht"),
        ReportDescription => (
            "Numbered headings, contents, and references",
            "Nummerierte Überschriften, Inhalt und Literatur"
        ),
        OfferName => ("Offer", "Angebot"),
        OfferDescription => (
            "A cover with the client's address and a price table",
            "Deckblatt mit Kundenadresse und Preistabelle"
        ),
    }
}
