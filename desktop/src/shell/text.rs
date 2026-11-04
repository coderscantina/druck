//! The strings of the shell: menus, the diagnostics, export, and search panels, the find bar, the footer, and settings.
//! German follows the words of macOS: Ablage, Bearbeiten, Darstellung, Sichern, Widerrufen.

crate::strings! {
    pub enum Key {
        // Window and canvas
        Write => ("Write", "Schreiben"),
        Pages => ("Pages", "Seiten"),
        Split => ("Split", "Geteilt"),
        SaveBeforeOpening => (
            "Save your changes before opening another document?",
            "Änderungen sichern, bevor ein anderes Dokument geöffnet wird?"
        ),
        SaveBeforeClosing => (
            "Save your changes before closing the document?",
            "Änderungen sichern, bevor das Dokument geschlossen wird?"
        ),
        Save => ("Save", "Sichern"),
        Discard => ("Discard", "Verwerfen"),
        Cancel => ("Cancel", "Abbrechen"),
        OpenPrompt => ("Open", "Öffnen"),

        // Menus
        MenuFile => ("File", "Ablage"),
        MenuEdit => ("Edit", "Bearbeiten"),
        MenuView => ("View", "Darstellung"),
        MenuWindow => ("Window", "Fenster"),
        MenuHelp => ("Help", "Hilfe"),
        About => ("About Druck", "Über Druck"),
        Settings => ("Settings\u{2026}", "Einstellungen\u{2026}"),
        Services => ("Services", "Dienste"),
        HideApp => ("Hide Druck", "Druck ausblenden"),
        HideOthers => ("Hide Others", "Andere ausblenden"),
        Quit => ("Quit Druck", "Druck beenden"),
        New => ("New", "Neu"),
        NewBook => ("Book\u{2026}", "Buch\u{2026}"),
        NewReport => ("Report\u{2026}", "Bericht\u{2026}"),
        NewOffer => ("Offer\u{2026}", "Angebot\u{2026}"),
        Open => ("Open\u{2026}", "Öffnen\u{2026}"),
        OpenRecent => ("Open Recent", "Benutzte Dokumente"),
        ClearRecent => ("Clear Menu", "Menü löschen"),
        Close => ("Close", "Schließen"),
        SaveItem => ("Save", "Sichern"),
        ExportItem => ("Export\u{2026}", "Exportieren\u{2026}"),
        Undo => ("Undo", "Widerrufen"),
        Redo => ("Redo", "Wiederholen"),
        Cut => ("Cut", "Ausschneiden"),
        Copy => ("Copy", "Kopieren"),
        Paste => ("Paste", "Einsetzen"),
        SelectAll => ("Select All", "Alles auswählen"),
        Find => ("Find\u{2026}", "Suchen\u{2026}"),
        FindInProject => ("Find in Project\u{2026}", "Im Projekt suchen\u{2026}"),
        FindNext => ("Find Next", "Weitersuchen"),
        FindPrevious => ("Find Previous", "Rückwärts suchen"),
        ViewWrite => ("Write", "Schreiben"),
        ViewPages => ("Pages", "Seiten"),
        ViewSplit => ("Split", "Geteilte Ansicht"),
        ZoomIn => ("Zoom In", "Vergrößern"),
        ZoomOut => ("Zoom Out", "Verkleinern"),
        ZoomFit => ("Fit to Width", "An Breite anpassen"),
        HideSidebar => ("Hide Sidebar", "Seitenleiste ausblenden"),
        ShowSidebar => ("Show Sidebar", "Seitenleiste einblenden"),
        ShowDiagnostics => ("Show Diagnostics", "Diagnose einblenden"),
        FocusSidebar => ("Focus Sidebar", "Seitenleiste aktivieren"),
        HideInspector => ("Hide Inspector", "Inspektor ausblenden"),
        ShowInspector => ("Show Inspector", "Inspektor einblenden"),
        Minimize => ("Minimize", "Im Dock ablegen"),
        WindowZoom => ("Zoom", "Zoomen"),
        HelpWeb => ("Druck Documentation", "Druck-Dokumentation"),

        // Footer
        WordsFooter => (
            "{chapter} words in this chapter \u{b7} {book} in the book",
            "{chapter} Wörter im Kapitel \u{b7} {book} im Buch"
        ),
        WordsChapterOnly => ("{chapter} words in this chapter", "{chapter} Wörter im Kapitel"),

        // Diagnostics
        DiagnosticsTitle => ("DIAGNOSTICS", "DIAGNOSE"),
        Summary => (
            "Errors {errors} \u{b7} Warnings {warnings} \u{b7} Open {todos}",
            "Fehler {errors} \u{b7} Warnungen {warnings} \u{b7} Offen {todos}"
        ),
        NothingToReport => (
            "Nothing to report. The pages are set without problems.",
            "Nichts zu melden. Die Seiten sind ohne Probleme gesetzt."
        ),
        KindError => ("Error", "Fehler"),
        KindWarning => ("Warning", "Warnung"),
        KindTodo => ("To do", "Offen"),
        LineNumber => ("Line {line}", "Zeile {line}"),
        ProjectSection => ("Project", "Projekt"),
        ExportFailed => ("Export \"{name}\" failed", "Export \u{201e}{name}\u{201c} fehlgeschlagen"),
        Dismiss => ("Dismiss", "Schließen"),

        // Export
        ExportThisDocument => ("This document", "Dieses Dokument"),
        ExportPdf => ("Export as PDF", "Als PDF exportieren"),
        ExportEpub => ("Export as EPUB", "Als EPUB exportieren"),
        NamedExports => ("NAMED EXPORTS", "BENANNTE EXPORTE"),
        NoExports => (
            "This project has no named exports yet. An export is a way to build the book, such as a print PDF.",
            "Dieses Projekt hat noch keine benannten Exporte. Ein Export ist eine Art, das Buch zu bauen, etwa als Druck-PDF."
        ),
        ExportRun => ("Export", "Exportieren"),
        ExportCancel => ("Cancel", "Abbrechen"),
        ExportAdd => ("Add export", "Export hinzufügen"),
        ExportName => ("Name of the export", "Name des Exports"),
        ExportCreate => ("Add", "Hinzufügen"),
        ExportBadName => (
            "Use letters, digits, - and _ for the name.",
            "Für den Namen Buchstaben, Ziffern, - und _ verwenden."
        ),
        ExportExists => ("An export with this name exists.", "Ein Export mit diesem Namen gibt es schon."),
        FieldTheme => ("Theme", "Theme"),
        FieldOutput => ("Output file", "Ausgabedatei"),
        FieldBleed => ("Bleed", "Beschnitt"),
        FieldCropMarks => ("Crop marks", "Schnittmarken"),
        FieldDraft => ("Draft", "Entwurf"),
        FieldFinal => ("Final", "Endfassung"),
        FinalHint => ("fails while placeholders are open", "scheitert bei offenen Platzhaltern"),
        StagePreparing => ("Preparing\u{2026}", "Wird vorbereitet\u{2026}"),
        StageTypesetting => ("Typesetting\u{2026}", "Wird gesetzt\u{2026}"),
        StageWriting => ("Writing the file\u{2026}", "Datei wird geschrieben\u{2026}"),
        ExportCancelled => ("Cancelled.", "Abgebrochen."),
        ExportFailedSee => (
            "The export failed. The reasons are in Diagnostics.",
            "Der Export ist fehlgeschlagen. Die Gründe stehen unter Diagnose."
        ),
        ExportDonePages => ("Pages: {parts} \u{b7} {seconds} s", "Seiten: {parts} \u{b7} {seconds} s"),
        ExportDoneSections => ("Sections: {parts} \u{b7} {seconds} s", "Abschnitte: {parts} \u{b7} {seconds} s"),
        OpenFile => ("Open", "Öffnen"),
        RevealFile => ("Show in Finder", "Im Finder zeigen"),
        ManifestProblem => ("Cannot read the exports.", "Die Exporte lassen sich nicht lesen."),

        // Search
        SearchTitle => ("Search", "Suchen"),
        SearchPlaceholder => ("Search chapters, notes, research", "Kapitel, Notizen, Recherche durchsuchen"),
        ReplacePlaceholder => ("Replace with", "Ersetzen durch"),
        OptionRegex => ("Regular expression", "Regulärer Ausdruck"),
        OptionCase => ("Match case", "Groß-/Kleinschreibung"),
        OptionWord => ("Whole word", "Ganzes Wort"),
        ReplaceAll => ("Replace all", "Alle ersetzen"),
        ReplaceInFile => ("Replace in file", "In Datei ersetzen"),
        SearchCount => ("Matches: {count} \u{b7} Files: {files}", "Treffer: {count} \u{b7} Dateien: {files}"),
        NoMatches => ("No matches", "Keine Treffer"),
        Searching => ("Searching\u{2026}", "Suche läuft\u{2026}"),
        Replaced => (
            "Replaced: {count}. Undo in a file takes back its part.",
            "Ersetzt: {count}. Widerrufen in einer Datei macht ihren Teil rückgängig."
        ),

        // Find bar
        FindPlaceholder => ("Find in chapter", "Im Kapitel suchen"),
        FindPosition => ("{current} of {total}", "{current} von {total}"),

        // Settings
        SettingsTitle => ("Settings", "Einstellungen"),
        Appearance => ("Appearance", "Erscheinungsbild"),
        AppearanceSystem => ("Automatic", "Automatisch"),
        AppearanceDark => ("Dark", "Dunkel"),
        AppearanceLight => ("Light", "Hell"),
        Language => ("Language", "Sprache"),
        LanguageSystem => ("System", "Systemsprache"),
        LanguageEnglish => ("English", "English"),
        LanguageGerman => ("Deutsch", "Deutsch")
    }
}
