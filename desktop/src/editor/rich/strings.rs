//! Every string the rich blocks show, in English and German.

crate::strings! {
    pub enum Key {
        FootnoteTitle => ("Footnote {label}", "Fußnote {label}"),
        FootnoteElsewhere => ("In {chapter}", "In {chapter}"),
        FootnotePlaceholder => ("Footnote text.", "Text der Fußnote."),
        FootnoteHint => ("Blank lines start paragraphs. Esc closes.", "Leerzeilen trennen Absätze. Esc schließt."),
        FootnoteRemove => ("Remove footnote", "Fußnote entfernen"),
        OrphanOffer => ("The text of note {label} stays. Remove it too?", "Der Text von Fußnote {label} bleibt. Auch entfernen?"),
        OrphanRemove => ("Remove text", "Text entfernen"),
        OrphanKeep => ("Keep", "Behalten"),
        CitationTitle => ("Citation", "Quellenangabe"),
        ReferenceTitle => ("Cross-reference", "Querverweis"),
        SearchWorks => ("Search key, author, or title", "Schlüssel, Autor oder Titel suchen"),
        SearchLabels => ("Search labels and titles", "Marken und Titel suchen"),
        NoWorks => ("No work matches.", "Kein Werk passt."),
        NoLabels => ("No label matches.", "Keine Marke passt."),
        NoLabelsYet => (
            "This book has no labels yet. A heading gets one with {#sec:name} at its end.",
            "Dieses Buch hat noch keine Marken. Eine Überschrift bekommt eine mit {#sec:name} an ihrem Ende."
        ),
        NoBibliography => ("No bibliography is set. Name one under Details.", "Kein Literaturverzeichnis gesetzt. Unter Details angeben."),
        LocatorLabel => ("Locator", "Fundstelle"),
        LocatorPlaceholder => ("p. 12", "S. 12"),
        NarrativeOption => ("In the sentence", "Im Satz"),
        PageOption => ("Page number", "Seitenzahl"),
        AddWork => ("Add work", "Werk hinzufügen"),
        RemoveWork => ("Remove work", "Werk entfernen"),
        UnknownKey => ("No entry in the bibliography has the key {key}.", "Kein Eintrag im Literaturverzeichnis hat den Schlüssel {key}."),
        UnknownLabel => ("No heading, figure, or table has the label {label}.", "Keine Überschrift, Abbildung oder Tabelle hat die Marke {label}."),
        UnknownNote => ("The footnote {label} has no text yet.", "Die Fußnote {label} hat noch keinen Text."),
        PageOf => ("page of {target}", "Seite von {target}"),
        KindHeading => ("Heading", "Überschrift"),
        KindFigure => ("Figure", "Abbildung"),
        KindTable => ("Table", "Tabelle"),
        RowLabel => ("Row", "Zeile"),
        ColumnLabel => ("Column", "Spalte"),
        AlignDefault => ("As the theme sets it", "Wie im Theme"),
        AlignLeft => ("Left", "Links"),
        AlignCenter => ("Center", "Mitte"),
        AlignRight => ("Right", "Rechts"),
        MergeRight => ("Span the next column", "Nächste Spalte mitnehmen"),
        SplitCell => ("Give a column back", "Spalte zurückgeben"),
        CaptionLabel => ("Caption", "Beschriftung"),
        CaptionPlaceholder => ("No caption", "Keine Beschriftung"),
        LabelLabel => ("Label", "Marke"),
        LabelPlaceholder => ("name", "name"),
        PlacementLabel => ("Placement", "Platzierung"),
        PlacementHere => ("Here", "Hier"),
        PlacementTop => ("Top", "Oben"),
        PlacementBottom => ("Bottom", "Unten"),
        ShowSource => ("Show source", "Quelltext zeigen"),
        DeleteTable => ("Delete table", "Tabelle löschen"),
        DeleteFigure => ("Delete image", "Bild löschen"),
        CloseLabel => ("Done", "Fertig"),
        TableBodyNotList => (
            "This table holds something other than one list or one pipe table.",
            "Diese Tabelle enthält etwas anderes als eine Liste oder eine Pipe-Tabelle."
        ),
        TableTextOutsideCells => (
            "A line of this table stands outside its cells.",
            "Eine Zeile dieser Tabelle steht außerhalb ihrer Zellen."
        ),
        TableAttributes => (
            "A cell has attributes other than a style and a span.",
            "Eine Zelle hat andere Attribute als Stil und Spanne."
        ),
        TableHeaderStays => ("The first row is the header and stays.", "Die erste Zeile ist der Kopf und bleibt."),
        TableNoRowThere => ("There is no row to move it past.", "Dort ist keine Zeile mehr."),
        TableNoColumnThere => ("There is no column there.", "Dort ist keine Spalte."),
        TableOneColumnStays => ("A table needs one column.", "Eine Tabelle braucht eine Spalte."),
        TablePipeNoSpans => (
            "Pipe tables cannot span columns. List tables can.",
            "Pipe-Tabellen können keine Spalten verbinden. Listentabellen können es."
        ),
        TableMergeNotEmpty => ("Empty the next cell first.", "Zuerst die nächste Zelle leeren."),
        TableNoSpan => ("This cell covers one column.", "Diese Zelle deckt eine Spalte ab."),
        TableSpanInTheWay => (
            "A cell spanning these columns keeps them from moving.",
            "Eine verbundene Zelle hält diese Spalten fest."
        ),
        ImagePrompt => ("Choose an image", "Bild wählen"),
        ImageMissing => ("The file {path} is missing.", "Die Datei {path} fehlt."),
        ImageUnsupported => (
            "{name} is not a PNG, JPEG, or SVG file.",
            "{name} ist keine PNG-, JPEG- oder SVG-Datei."
        ),
        ImageSize => ("{width} × {height}", "{width} × {height}"),
        ImagesTitle => ("Images", "Bilder"),
        ImagesEmpty => ("This document has no images yet.", "Dieses Dokument hat noch keine Bilder."),
        ImagesInsert => ("Insert image", "Bild einfügen"),
        ImageUses => ("Used {count} times", "{count}-mal verwendet"),
        ImageUsedOnce => ("Used once", "Einmal verwendet"),
        ImageUnused => ("Not used", "Nicht verwendet"),
        ImageNoFile => ("File missing", "Datei fehlt"),
    }
}
