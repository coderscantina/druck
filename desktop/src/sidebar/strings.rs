//! Every string of the sidebar and its dialogs, in English and German.

crate::strings! {
    pub enum Key {
        Manifest => ("Manifest", "Manifest"),
        Contents => ("Contents", "Inhalt"),
        NotInBook => ("Not in book", "Nicht im Buch"),
        Notes => ("Notes", "Notizen"),
        Research => ("Research", "Recherche"),

        NewSection => ("New section", "Neuer Abschnitt"),
        Details => ("Details", "Angaben"),
        CloseDialog => ("Close", "Schließen"),
        TheContents => ("Contents", "Inhalt"),
        EditSection => ("Edit \"{title}\"", "\u{201e}{title}\u{201c} bearbeiten"),
        Name => ("Name", "Name"),
        RemoveFromBook => ("Remove from book", "Aus Buch entfernen"),
        Save => ("Save", "Sichern"),
        Outline => ("Outline", "Gliederung"),
    }
}
