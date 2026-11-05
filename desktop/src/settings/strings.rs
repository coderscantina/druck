//! The settings window's strings, in English and German.

crate::strings! {
    pub enum Key {
        Title => ("Settings", "Einstellungen"),
        Search => ("Search settings", "Einstellungen suchen"),
        EditFile => ("Edit settings.json", "settings.json bearbeiten"),
        NoMatches => ("No setting matches this.", "Keine Einstellung passt dazu."),
        Invalid => (
            "settings.json cannot be used: {problem}. It was left as it is.",
            "settings.json ist nicht verwendbar: {problem}. Die Datei bleibt, wie sie ist."
        ),

        ShouldBe => ("\"{key}\" should be {what}", "\"{key}\" sollte {what} sein"),
        OneObject => ("the file should hold one object", "die Datei sollte ein Objekt enthalten"),
        WantedSwitch => ("true or false", "true oder false"),
        WantedAppearance => ("\"system\", \"dark\", or \"light\"", "\"system\", \"dark\" oder \"light\""),
        WantedAccent => ("a color such as \"#2f8fb8\"", "eine Farbe wie \"#2f8fb8\""),
        WantedLanguage => ("\"system\", \"english\", or \"german\"", "\"system\", \"english\" oder \"german\""),
        WantedLineLength => (
            "\"book\" or a number of characters from 20 to 240",
            "\"book\" oder eine Zeichenzahl von 20 bis 240"
        ),

        CategoryAppearance => ("Appearance", "Darstellung"),
        CategoryWriting => ("Writing", "Schreiben"),
        CategoryLanguage => ("Language and spelling", "Sprache"),
        CategoryAgent => ("Agent", "Agent"),
        CategoryKeys => ("Keys", "Tasten"),

        Appearance => ("Appearance", "Erscheinungsbild"),
        AppearanceHint => ("Follows the system unless you pick one.", "Folgt dem System, wenn du nichts wählst."),
        AppearanceSystem => ("System", "System"),
        AppearanceLight => ("Light", "Hell"),
        AppearanceDark => ("Dark", "Dunkel"),
        Accent => ("Accent color", "Akzentfarbe"),
        AccentHint => (
            "Caret, selection, marks, and the action that moves on.",
            "Cursor, Auswahl, Markierungen und die Aktion, die weiterführt."
        ),
        AccentBad => ("Use a color such as #2f8fb8.", "Eine Farbe wie #2f8fb8 verwenden."),

        LineLength => ("Line length", "Zeilenlänge"),
        LineLengthHint => ("The book's measure, or your own.", "Die Satzbreite des Buchs oder deine eigene."),
        LineLengthBook => ("Book", "Buch"),
        LineLengthCount => ("{count} characters", "{count} Zeichen"),
        Typewriter => ("Typewriter scrolling", "Schreibmaschinen-Scrollen"),
        TypewriterHint => ("Keeps the current line at the same height.", "Hält die aktuelle Zeile auf gleicher Höhe."),
        Dim => ("Dim other paragraphs", "Andere Absätze abblenden"),
        DimHint => ("Keeps only the current paragraph in full ink.", "Nur der aktuelle Absatz bleibt in voller Farbe."),
        SelectionToolbar => ("Toolbar on selection", "Werkzeugleiste bei Auswahl"),
        SelectionToolbarHint => (
            "Marks and lists over selected text. The keys work either way.",
            "Auszeichnungen und Listen über dem markierten Text. Die Tasten gehen immer."
        ),
        Binder => ("Binder", "Binder"),
        WarningMarks => ("Warning marks", "Warnungen markieren"),
        WarningMarksHint => (
            "Mark typesetting warnings on the pages with the Proof tab closed.",
            "Satzwarnungen auf den Seiten markieren, auch wenn die Korrektur zu ist."
        ),
        BinderHint => ("A slim contents list on the left, on Cmd+\\.", "Eine schmale Inhaltsliste links, auf Cmd+\\."),

        Language => ("Language", "Sprache"),
        LanguageHint => ("The app's own words. A book's language is in the book.", "Die Wörter der App. Die Sprache eines Buchs steht im Buch."),
        LanguageSystem => ("System", "System"),
        LanguageEnglish => ("English", "English"),
        LanguageGerman => ("Deutsch", "Deutsch"),
        Spelling => ("Check spelling", "Rechtschreibung prüfen"),
        Agent => ("Writing partner", "Schreibpartner"),
        AgentHint => (
            "Claude Code on Cmd+J, on this computer. Set it up with an API key. Druck holds no keys and starts nothing while this is off.",
            "Claude Code auf Cmd+J, auf diesem Computer. Mit einem API-Schlüssel einrichten. Druck speichert keine Schlüssel und startet nichts, solange das aus ist."
        ),
        SpellingHint => ("Underlines misspelled words while you write.", "Unterstreicht falsche Wörter beim Schreiben."),
    }
}
