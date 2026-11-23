//! Every string of the sidebar and its dialogs, in English and German.

crate::strings! {
    pub enum Key {
        Manifest => ("Manifest", "Manifest"),
        NotInBook => ("Not in book", "Nicht im Buch"),
        Notes => ("Notes", "Notizen"),
        Research => ("Research", "Recherche"),
        Passes => ("Passes", "Durchgänge"),

        NewSection => ("New section", "Neuer Abschnitt"),
        Details => ("Details", "Angaben"),
        CloseDialog => ("Close", "Schließen"),
        TheContents => ("Contents", "Inhalt"),
        EditSection => ("Edit \"{title}\"", "\u{201e}{title}\u{201c} bearbeiten"),
        Name => ("Name", "Name"),
        RemoveFromBook => ("Remove from book", "Aus Buch entfernen"),
        Save => ("Save", "Sichern"),
        Outline => ("Outline", "Gliederung"),

        TitleLine => ("Title", "Titel"),
        SubtitleLine => ("Subtitle", "Untertitel"),
        AuthorLine => ("Author", "Autor"),
        PublisherLine => ("Publisher", "Verlag"),
        TitlePageCaption => ("Your title page. Type to change it.", "Deine Titelseite. Tippe, um sie zu ändern."),

        AddPage => ("Add a page", "Seite hinzufügen"),
        PlacedWhereBooksHaveIt => ("Druck puts it where books have it.", "Druck setzt sie, wo Bücher sie haben."),
        DragToMove => (" Drag it in the list to move it.", " Zum Verschieben in der Liste ziehen."),
        OtherSections => ("New section", "Neuer Abschnitt"),
    }
}
