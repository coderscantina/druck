//! Every string the inspector shows, in English and German: the tab rail, the details panel, and the theme panels. The
//! tab labels and section titles are shown as written, so they are uppercase in both languages.

crate::strings! {
    #[derive(Hash)]
    pub enum Key {
        TabSelection => ("SELECTION", "AUSWAHL"),
        TabStyles => ("STYLES", "STILE"),
        TabFonts => ("FONTS", "SCHRIFTEN"),
        TabImages => ("IMAGES", "BILDER"),
        TabDetails => ("DETAILS", "DETAILS"),
        TabNotes => ("NOTES", "NOTIZEN"),
        TabDiagnostics => ("DIAGNOSTICS", "DIAGNOSE"),
        TabExport => ("EXPORT", "EXPORT"),
        TabAgent => ("AGENT", "AGENT"),

        StyleBody => ("Regular text", "Fließtext"),
        StyleHeading1 => ("Heading 1", "Überschrift 1"),
        StyleHeading2 => ("Heading 2", "Überschrift 2"),
        StyleHeading3 => ("Heading 3", "Überschrift 3"),
        StyleHeading4 => ("Heading 4", "Überschrift 4"),
        StyleHeading5 => ("Heading 5", "Überschrift 5"),
        StyleHeading6 => ("Heading 6", "Überschrift 6"),
        StyleQuote => ("Block quotation", "Blockzitat"),
        StyleCallout => ("Callout", "Hinweisbox"),
        StyleCalloutLabel => ("Callout label", "Hinweisbox-Titel"),
        StyleList => ("List", "Liste"),
        StyleCodeBlock => ("Code block", "Codeblock"),
        StyleCaption => ("Caption", "Beschriftung"),
        StyleFootnote => ("Footnote", "Fußnote"),
        StyleTableCell => ("Table cell", "Tabellenzelle"),
        StyleTableHeader => ("Table header", "Tabellenkopf"),
        StyleTitle => ("Title", "Titel"),
        StyleSubtitle => ("Subtitle", "Untertitel"),
        StyleAuthor => ("Author", "Autor"),
        StyleDate => ("Date", "Datum"),
        StyleAbstractHeading => ("Abstract heading", "Abstract-Überschrift"),
        StyleAbstract => ("Abstract", "Abstract"),
        StyleHeader => ("Running header", "Kopfzeile"),
        StyleFooter => ("Running footer", "Fußzeile"),
        StyleWatermark => ("Watermark", "Wasserzeichen"),
        StyleTocHeading => ("Contents heading", "Inhaltsüberschrift"),
        StyleTocEntry => ("Contents entry", "Inhaltseintrag"),
        StyleBibliography => ("Bibliography", "Literaturverzeichnis"),
        StyleSceneBreak => ("Scene break", "Szenenwechsel"),
        StyleIndexLetter => ("Index letter", "Registerbuchstabe"),
        StyleIndexEntry => ("Index entry", "Registereintrag"),
        StyleIndexSubEntry => ("Index sub-entry", "Register-Untereintrag"),

        DetailsTitle => ("DETAILS", "DETAILS"),
        CoverImageTitle => ("COVER IMAGE", "TITELBILD"),
        MetaTitle => ("META", "META"),
        FieldTitle => ("Title", "Titel"),
        FieldSubtitle => ("Subtitle", "Untertitel"),
        FieldAuthors => ("Authors", "Autoren"),
        FieldDate => ("Date", "Datum"),
        FieldLanguage => ("Language", "Sprache"),
        NoCover => ("No cover", "Kein Cover"),
        Choose => ("Choose\u{2026}", "Auswählen\u{2026}"),
        Remove => ("Remove", "Entfernen"),
        Isbn => ("ISBN", "ISBN"),
        Publisher => ("Publisher", "Verlag"),
        Other => ("Other", "Weitere"),
        NewKey => ("New key", "Neuer Schlüssel"),
        SeveralLines => (
            "{value} (several lines, edit in the file)",
            "{value} (mehrzeilig, in der Datei bearbeiten)"
        ),

        // Shared
        Back => ("Back", "Zurück"),
        Reset => ("Reset", "Zurücksetzen"),
        Inherited => ("DEFAULT", "STANDARD"),
        Customized => ("CUSTOM", "EIGEN"),
        NoTheme => (
            "No theme file is open. Start the app with --theme PATH or name a theme in the document.",
            "Keine Themendatei geöffnet. Starte die App mit --theme PFAD oder nenne ein Thema im Dokument."
        ),
        Cancel => ("Cancel", "Abbrechen"),
        Delete => ("Delete", "Löschen"),
        Rename => ("Rename", "Umbenennen"),
        NotANumber => ("Enter a number, such as 1.4", "Gib eine Zahl ein, etwa 1,4"),
        Yes => ("On", "An"),
        No => ("Off", "Aus"),
        PanelStyles => ("Styles", "Stile"),
        PanelTokens => ("Tokens", "Tokens"),
        PanelPage => ("Page", "Seite"),
        PanelSource => ("Source", "Quelle"),

        // Styles
        StylesTitle => ("STYLES", "STILE"),
        CustomStyles => ("CUSTOM STYLES", "EIGENE STILE"),
        NewStyleHint => ("Name, such as pull-quote", "Name, etwa zitat-gross"),
        BasedOn => ("Based on", "Basiert auf"),
        UsedAs => ("Use in a document with {.name}", "Im Dokument mit {.name} verwenden"),
        RenameStyleNote => (
            "Documents that use {.old} must change to {.new}.",
            "Dokumente, die {.old} nutzen, müssen auf {.new} wechseln."
        ),
        Font => ("Font", "Schrift"),
        Size => ("Size", "Größe"),
        Weight => ("Weight", "Schnitt"),
        FontStyle => ("Font style", "Schriftstil"),
        Color => ("Color", "Farbe"),
        LineHeight => ("Line height", "Zeilenabstand"),
        AlignLabel => ("Text alignment", "Textausrichtung"),
        Hyphenate => ("Hyphenation", "Silbentrennung"),
        SpaceBefore => ("Space before", "Abstand davor"),
        SpaceAfter => ("Space after", "Abstand danach"),
        Indent => ("Indent", "Einzug"),
        FirstLineIndent => ("First line indent", "Erstzeileneinzug"),
        Tracking => ("Letter spacing", "Laufweite"),
        Uppercase => ("Capitals", "Großbuchstaben"),
        KeepWithNext => ("Keep with next", "Mit nächstem halten"),
        FiguresStyle => ("Figures", "Ziffern"),
        FiguresSpacing => ("Figure width", "Ziffernbreite"),
        NumberGap => ("Number gap", "Nummernabstand"),
        Bullets => ("Bullets per level", "Aufzählungszeichen je Ebene"),
        Marker => ("Marker style", "Markerstil"),
        RuleBelow => ("Rule below", "Linie darunter"),
        Normal => ("Normal", "Normal"),
        Italic => ("Italic", "Kursiv"),
        Regular => ("Regular", "Normal"),
        Bold => ("Bold", "Fett"),
        AlignLeft => ("Left", "Links"),
        AlignCenter => ("Center", "Mitte"),
        AlignRight => ("Right", "Rechts"),
        AlignJustify => ("Justify", "Blocksatz"),
        DefaultWord => ("Default", "Standard"),
        Lining => ("Lining", "Versalziffern"),
        Oldstyle => ("Oldstyle", "Mediävalziffern"),
        Proportional => ("Proportional", "Proportional"),
        Tabular => ("Tabular", "Tabellarisch"),
        RuleNone => ("None", "Keine"),
        RuleHeader => ("Header rule", "Kopflinie"),
        RuleRow => ("Row rule", "Zeilenlinie"),
        MissingStyleBlock => ("No block in this document uses {value}.", "Kein Block dieses Dokuments nutzt {value}."),
        ShowSpecimen => ("Show the specimen", "Muster zeigen"),

        // Fonts
        FontsTitle => ("FONTS", "SCHRIFTEN"),
        AddFont => ("Add a font file", "Schriftdatei hinzufügen"),
        ChooseFile => ("Choose", "Wählen"),
        Typeface => ("Typeface", "Schriftart"),
        TotalSize => ("Total size", "Gesamtgröße"),
        Format => ("Format", "Format"),
        Source => ("Source", "Quelle"),
        License => ("LICENSE", "LIZENZ"),
        NoLicense => ("The font file carries no license text.", "Die Schriftdatei enthält keinen Lizenztext."),
        Truetype => ("TrueType", "TrueType"),
        Opentype => ("OpenType", "OpenType"),
        CollectionFormat => ("{value} faces in the file", "{value} Schnitte in der Datei"),
        SourceBundled => ("Bundled", "Mitgeliefert"),
        SourceInstalled => ("Installed", "Installiert"),
        SourceFile => ("Project file", "Projektdatei"),
        EmbeddingWarning => (
            "This font's license restricts embedding. Druck embeds it anyway, so check that the license allows this.",
            "Die Lizenz dieser Schrift schränkt das Einbetten ein. Druck bettet sie trotzdem ein, prüfe daher die Lizenz."
        ),
        Axes => ("VARIABLE AXES", "VARIABLE ACHSEN"),
        AxesNote => (
            "Shown for reference. The weights a theme asks for set the axis.",
            "Zur Ansicht. Die angeforderten Schnitte setzen die Achse."
        ),
        UsedBy => ("Used by", "Verwendet von"),
        Faces => ("Faces", "Schnitte"),
        FontSpecimen => ("Aa", "Aa"),
        FaceName => ("Face", "Schnitt"),
        NotAFont => ("Not a font file", "Keine Schriftdatei"),
        BoldItalic => ("Bold italic", "Fett kursiv"),
        InlineCode => ("Inline code", "Code im Text"),
        Add => ("Add", "Hinzufügen"),
        AddAsFamily => ("Adds a family named {value}.", "Fügt eine Familie namens {value} hinzu."),
        AddToFamily => ("Adds the file as a face of {value}.", "Fügt die Datei als Schnitt von {value} hinzu."),
        NoFonts => ("The fonts appear after the first layout.", "Die Schriften erscheinen nach dem ersten Satz."),
        SizeKb => ("{value} kB", "{value} kB"),
        SizeMb => ("{value} MB", "{value} MB"),
        DecimalPoint => (".", ","),

        // Tokens
        TokensTitle => ("TOKENS", "TOKENS"),
        GroupFonts => ("Fonts", "Schriften"),
        GroupSizes => ("Sizes", "Größen"),
        GroupSpacing => ("Spacing", "Abstände"),
        GroupColors => ("Colors", "Farben"),
        NewTokenHint => ("Name, such as accent", "Name, etwa akzent"),
        TokenUsed => ("Used in {value} places", "An {value} Stellen verwendet"),
        TokenUnused => ("Not used", "Nicht verwendet"),
        TokenBlocked => (
            "In use, so it cannot be deleted: {value}",
            "In Verwendung und daher nicht löschbar: {value}"
        ),

        // Page geometry
        GeometryTitle => ("PAGE", "SEITE"),
        PageSize => ("Page size", "Seitenformat"),
        CustomSize => ("Custom", "Eigenes"),
        Width => ("Width", "Breite"),
        Height => ("Height", "Höhe"),
        Margins => ("MARGINS", "RÄNDER"),
        MarginTop => ("Top", "Oben"),
        MarginBottom => ("Bottom", "Unten"),
        MarginInner => ("Inner", "Innen"),
        MarginOuter => ("Outer", "Außen"),
        Mirror => ("Mirror margins on even pages", "Ränder auf geraden Seiten spiegeln"),
        Columns => ("TEXT", "TEXT"),
        ColumnGap => ("Column gap", "Spaltenabstand"),
        TextWidth => ("Narrower prose width", "Schmalere Textbreite"),
        TextWidthHint => ("Empty for the full frame", "Leer für den ganzen Satzspiegel"),
        FullFrame => ("Full frame", "Ganzer Satzspiegel"),
        PaperA4 => ("A4", "A4"),
        PaperA5 => ("A5", "A5"),
        PaperB5 => ("B5", "B5"),
        PaperLetter => ("Letter", "US-Letter"),
        PaperLegal => ("Legal", "US-Legal"),
        LengthNote => ("{mm} mm · {pt} pt", "{mm} mm · {pt} pt"),
        FrameSize => ("Frame {value}", "Satzspiegel {value}"),
        Decimal => (".", ","),

        // JSON view
        JsonTitle => ("THEME SOURCE", "THEMENQUELLE"),
        JsonValid => ("Valid. Changes apply as you type.", "Gültig. Änderungen gelten sofort."),
        JsonInvalid => ("Not applied: {value}", "Nicht übernommen: {value}"),
        JsonDesigned => ("Own panel", "Eigenes Panel"),
        JsonOnly => ("Source view only", "Nur Quellansicht"),
        JsonNote => (
            "These sections have no panel yet. Edit them here.",
            "Für diese Abschnitte gibt es noch kein Panel. Bearbeite sie hier."
        ),

        // Picker
        Theme => ("Theme", "Thema"),
        ProjectThemes => ("PROJECT", "PROJEKT"),
        CommonThemes => ("COMMON", "ALLGEMEIN"),
        NewCommonTheme => ("New common theme", "Neues allgemeines Thema"),
        Duplicate => ("Duplicate", "Duplizieren"),
        DefaultTheme => ("Built-in default", "Eingebaute Vorgabe"),
        DeleteTheme => ("Delete the common theme \"{value}\"?", "Das allgemeine Thema „{value}“ löschen?"),
        DeleteThemeDetail => (
            "Documents that name it fall back to errors until they name another theme. This cannot be undone.",
            "Dokumente, die es nennen, melden Fehler, bis sie ein anderes Thema nennen. Das lässt sich nicht rückgängig machen."
        ),
        DeleteThemeOpen => ("The open document uses it.", "Das geöffnete Dokument nutzt es."),
        Specimen => ("Specimen", "Muster"),

        // Fork
        ForkTitle => ("Change a shared theme", "Gemeinsames Thema ändern"),
        ForkBody => (
            "\"{value}\" is a common theme. Other documents may use it too.",
            "„{value}“ ist ein allgemeines Thema. Auch andere Dokumente können es nutzen."
        ),
        ForkCopy => ("Copy it into this project (recommended)", "In dieses Projekt kopieren (empfohlen)"),
        ForkCopyNote => (
            "The copy is changed. The shared theme and its other documents stay as they are.",
            "Die Kopie wird geändert. Das gemeinsame Thema und seine anderen Dokumente bleiben wie sie sind."
        ),
        ForkShared => ("Change the shared theme", "Gemeinsames Thema ändern"),
        ForkSharedNote => ("Every document that uses it changes.", "Jedes Dokument, das es nutzt, ändert sich."),
        ForkAffected => ("Documents known to use it: {value}.", "Bekannte Dokumente, die es nutzen: {value}."),
        ForkUnknown => ("Others on this machine are not known.", "Weitere auf diesem Rechner sind nicht bekannt."),
    }
}

/// The block styles of the theme, in the order the list shows them, with the key each has in `styles`.
pub const STYLES: &[(&str, Key)] = &[
    ("body", Key::StyleBody),
    ("heading-1", Key::StyleHeading1),
    ("heading-2", Key::StyleHeading2),
    ("heading-3", Key::StyleHeading3),
    ("heading-4", Key::StyleHeading4),
    ("heading-5", Key::StyleHeading5),
    ("heading-6", Key::StyleHeading6),
    ("quote", Key::StyleQuote),
    ("callout", Key::StyleCallout),
    ("callout-label", Key::StyleCalloutLabel),
    ("list", Key::StyleList),
    ("code-block", Key::StyleCodeBlock),
    ("caption", Key::StyleCaption),
    ("footnote", Key::StyleFootnote),
    ("table-cell", Key::StyleTableCell),
    ("table-header", Key::StyleTableHeader),
    ("title", Key::StyleTitle),
    ("subtitle", Key::StyleSubtitle),
    ("author", Key::StyleAuthor),
    ("date", Key::StyleDate),
    ("abstract-heading", Key::StyleAbstractHeading),
    ("abstract", Key::StyleAbstract),
    ("header", Key::StyleHeader),
    ("footer", Key::StyleFooter),
    ("watermark", Key::StyleWatermark),
    ("toc-heading", Key::StyleTocHeading),
    ("toc-entry", Key::StyleTocEntry),
    ("bibliography", Key::StyleBibliography),
    ("scene-break", Key::StyleSceneBreak),
    ("index-letter", Key::StyleIndexLetter),
    ("index-entry", Key::StyleIndexEntry),
    ("index-sub-entry", Key::StyleIndexSubEntry),
];

impl gpui::IntoElement for Key {
    type Element = <&'static str as gpui::IntoElement>::Element;

    fn into_element(self) -> Self::Element {
        crate::t!(self).into_element()
    }
}

impl From<Key> for gpui::SharedString {
    fn from(key: Key) -> Self {
        Self::new_static(crate::t!(key))
    }
}
