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
        CopyrightName => ("Copyright", "Impressum"),
        PlainPage => ("Page", "Seite"),
        DedicationName => ("Dedication", "Widmung"),
        DedicationDescription => ("A line for someone, on its own page", "Eine Zeile für jemanden, auf eigener Seite"),
        EpigraphName => ("Epigraph", "Motto"),
        EpigraphDescription => ("A quotation that sets the tone", "Ein Zitat, das den Ton setzt"),
        ContentsName => ("Contents", "Inhalt"),
        ContentsDescription => (
            "The table of contents, built from the headings",
            "Das Inhaltsverzeichnis, aus den Überschriften"
        ),
        AbstractName => ("Abstract", "Abstract"),
        AbstractDescription => ("A brief summary of what this is about", "Eine kurze Zusammenfassung des Inhalts"),
        ChapterName => ("Chapter", "Kapitel"),
        ChapterDescription => ("A numbered chapter with its heading", "Ein nummeriertes Kapitel mit Überschrift"),
        SectionName => ("Section", "Abschnitt"),
        SectionDescription => ("A second-level heading with its text", "Eine Überschrift der zweiten Ebene mit Text"),
        ColumnsName => ("Two-column section", "Abschnitt in zwei Spalten"),
        ColumnsDescription => ("A heading and text set in two columns", "Überschrift und Text in zwei Spalten"),
        PartName => ("Part", "Teil"),
        PartDescription => (
            "An unnumbered heading that opens a part",
            "Eine Überschrift ohne Nummer, die einen Teil öffnet"
        ),
        AcknowledgmentsName => ("Acknowledgments", "Danksagung"),
        AcknowledgmentsDescription => ("Thanks to those who helped", "Dank an alle, die geholfen haben"),
        AboutAuthorName => ("About the author", "Über den Autor"),
        AboutAuthorDescription => ("A few lines on who wrote the book", "Ein paar Zeilen zu dem, der es schrieb"),
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
        NovelName => ("Novel", "Roman"),
        NovelDescription => ("Chapters with ornaments, for print and ebook", "Kapitel mit Ornament, für Druck und E-Book"),
        NovelFolder => ("Untitled novel", "Unbenannter Roman"),
        NonfictionName => ("Non-fiction", "Sachbuch"),
        NonfictionDescription => ("Numbered chapters, notes, and sources", "Nummerierte Kapitel, Fußnoten und Quellen"),
        NonfictionFolder => ("Untitled book", "Unbenanntes Sachbuch"),
        ArticleName => ("Article", "Artikel"),
        ArticleDescription => ("One column under a title", "Eine Spalte unter einem Titel"),
        ArticleFolder => ("Untitled article", "Unbenannter Artikel"),
        PaperName => ("Paper", "Paper"),
        PaperDescription => ("Two columns under an abstract", "Zwei Spalten unter dem Abstract"),
        PaperFolder => ("Untitled paper", "Unbenanntes Paper"),
        ThesisName => ("Thesis", "Abschlussarbeit"),
        ThesisDescription => ("Title page, contents, chapters, and references", "Titelseite, Inhalt, Kapitel und Literatur"),
        ThesisFolder => ("Untitled thesis", "Unbenannte Abschlussarbeit"),

        CannotFindWorkingDir => ("cannot find the working directory: {error}", "Der Arbeitsordner ist nicht zu finden: {error}"),
        NoSuchFile => ("cannot open {path}: no such file or folder", "{path} lässt sich nicht öffnen: Datei oder Ordner gibt es nicht"),
        CannotRead => ("cannot read {path}: {error}", "{path} lässt sich nicht lesen: {error}"),
        NotUtf8 => ("{path} is not UTF-8 text.", "{path} ist kein UTF-8-Text."),
        NoManifest => (
            "{path} has no {manifest}. A project folder needs one, or open a Markdown file.",
            "{path} hat keine {manifest}. Ein Projektordner braucht eine, oder öffne eine Markdown-Datei."
        ),
        NotThereYet => ("{listed} is not there yet. Saving creates it.", "{listed} gibt es noch nicht. Sichern legt die Datei an."),
        ChangedNotSaved => ("{path} changed on disk, so it was not saved.", "{path} wurde auf der Platte geändert und daher nicht gesichert."),
        CannotSave => ("cannot save {path}: {error}", "{path} lässt sich nicht sichern: {error}"),
        SingleFileChapters => (
            "A single file cannot change a chapter list: its order is its text.",
            "Eine einzelne Datei hat keine Kapitelliste: Ihre Reihenfolge ist ihr Text."
        ),
        OnlyBookChaptersMove => ("Only a chapter in the book can move.", "Nur ein Kapitel im Buch lässt sich verschieben."),
        OnlyBookChaptersHaveStatus => ("Only a chapter in the book has a state.", "Nur ein Kapitel im Buch hat einen Stand."),
        UnknownStatus => ("That state is not one of the book's statuses.", "Diesen Stand kennt das Buch nicht."),
        StatusDraft => ("Draft", "Entwurf"),
        StatusRevised => ("Revised", "Überarbeitet"),
        StatusFinal => ("Final", "Final"),
        BookNeedsChapter => ("A book needs at least one chapter.", "Ein Buch braucht mindestens ein Kapitel."),
        NameEmpty => ("A name cannot be empty.", "Ein Name darf nicht leer sein."),
        AlreadyExists => ("{path} already exists.", "{path} gibt es schon."),
        StayInFolder => ("A chapter must stay in the project folder.", "Ein Kapitel muss im Projektordner bleiben."),
        CannotRename => ("cannot rename {path}: {error}", "{path} lässt sich nicht umbenennen: {error}"),
        CannotCreate => ("cannot create {path}: {error}", "{path} lässt sich nicht anlegen: {error}"),
        AlreadyHoldsFiles => ("{path} already holds files", "In {path} liegen schon Dateien"),
        NotAFile => ("{path} is not a file", "{path} ist keine Datei"),
        CannotCopy => ("cannot copy {path}: {error}", "{path} lässt sich nicht kopieren: {error}"),
        CannotReadManifest => ("cannot read {manifest}: {error}", "{manifest} lässt sich nicht lesen: {error}"),
        CannotChangeManifest => ("cannot change {manifest} here. Edit the file directly.", "{manifest} lässt sich hier nicht ändern. Bearbeite die Datei direkt."),
        ManifestChanged => ("{manifest} just changed on disk. Try again.", "{manifest} wurde eben auf der Platte geändert. Versuche es noch einmal."),
        CannotWriteManifest => ("cannot write {manifest}: {error}", "{manifest} lässt sich nicht schreiben: {error}"),
        CannotChangeFrontMatter => ("cannot change the front matter here. Edit it directly.", "Die Front Matter lässt sich hier nicht ändern. Bearbeite sie direkt."),
        OutsideProject => ("{path} is outside the project.", "{path} liegt außerhalb des Projekts."),

        CannotReadYaml => ("cannot read this YAML line: {line}", "diese YAML-Zeile lässt sich nicht lesen: {line}"),
        MakeProject => ("Make this a project", "Daraus ein Projekt machen"),
        MakeProjectHint => (
            "Writes {manifest} with {chapters} in name order. Your files stay where they are.",
            "Schreibt {manifest} mit {chapters} nach Namen. Deine Dateien bleiben, wo sie sind."
        ),
        MakeProjectHintMoves => (
            "Writes {manifest} with {chapters} in name order, and moves the front matter of {file} into it.",
            "Schreibt {manifest} mit {chapters} nach Namen und übernimmt die Front Matter aus {file}."
        ),
        MakeProjectMissing => (
            "It names {paths}, which cannot be found, so there are no pages until the file is there.",
            "Sie nennt {paths}, nicht auffindbar, also gibt es keine Seiten, bis die Datei da ist."
        ),
        MadeProject => ("Wrote {manifest} with {chapters}.", "{manifest} mit {chapters} geschrieben."),
        OneChapter => ("1 chapter", "1 Kapitel"),
        Chapters => ("{count} chapters", "{count} Kapiteln"),
        MadeProjectMoved => (
            "Wrote {manifest} with {chapters}, and moved the front matter of {file} into it.",
            "{manifest} mit {chapters} geschrieben und die Front Matter aus {file} übernommen."
        ),
        NoManifestInFolder => ("{folder} has no {manifest} yet.", "{folder} hat noch keine {manifest}."),
        ManifestExists => ("{path} already has a {manifest}.", "{path} hat schon eine {manifest}."),
    }
}
