//! Every string of the sidebar and its dialogs, in English and German.

crate::strings! {
    pub enum Key {
        Manifest => ("MANIFEST", "MANIFEST"),
        Contents => ("CONTENTS", "INHALT"),
        NotInBook => ("NOT IN BOOK", "NICHT IM BUCH"),
        Notes => ("NOTES", "NOTIZEN"),
        Research => ("RESEARCH", "RECHERCHE"),

        NewSection => ("New section", "Neuer Abschnitt"),
        EditSection => ("Edit \"{title}\"", "\u{201e}{title}\u{201c} bearbeiten"),
        Name => ("Name", "Name"),
        RemoveFromBook => ("Remove from book", "Aus Buch entfernen"),
        Save => ("Save", "Sichern"),
    }
}
