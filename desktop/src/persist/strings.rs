//! Every string the saving state and the conflict prompt show, in English and German.

crate::strings! {
    pub enum Key {
        Saved => ("Saved", "Gesichert"),
        Edited => ("Edited", "Bearbeitet"),
        Saving => ("Saving\u{2026}", "Wird gesichert\u{2026}"),
        CannotSave => ("Cannot save", "Sichern nicht möglich"),
        KeptInMemory => (
            "Your text is safe here and will be saved when it can be.",
            "Dein Text ist hier sicher und wird gesichert, sobald es geht."
        ),
        FolderGone => (
            "The document's folder is gone. Your text is safe here and will be saved when the folder is back.",
            "Der Ordner des Dokuments ist weg. Dein Text ist hier sicher und wird gesichert, sobald der Ordner wieder da ist."
        ),
        Unreadable => ("Not readable", "Nicht lesbar"),
        Overlap => (
            "changed outside where you are writing. Nothing is lost, whichever you choose:",
            "wurde von außen an der Stelle geändert, an der du schreibst. Nichts geht verloren, was du auch wählst:"
        ),
        Deleted => (
            "was deleted outside, but has text not saved yet. Nothing is lost, whichever you choose:",
            "wurde von außen gelöscht, enthält aber noch nicht gesicherten Text. Nichts geht verloren, was du auch wählst:"
        ),
        KeepMine => ("Keep mine", "Meins behalten"),
        TakeTheirs => ("Take theirs", "Das andere nehmen"),
        SeeBoth => ("See both", "Beides zeigen"),
        Merged => ("Merged changes from outside into", "Änderungen von außen eingefügt in"),
        Reloaded => ("Updated from outside:", "Von außen aktualisiert:"),
        Removed => (
            "was deleted outside. Its text is still here.",
            "wurde von außen gelöscht. Der Text ist noch hier."
        ),
        Restored => (
            "Restored text not saved before the app stopped:",
            "Text wiederhergestellt, der beim Beenden nicht gesichert war:"
        ),
        UndoStopped => (
            "The next undo takes back the change from outside. Press again to do so.",
            "Der nächste Widerruf nimmt die Änderung von außen zurück. Zum Bestätigen noch einmal drücken."
        ),
        UndoDone => (
            "Took back the change from outside. Redo brings it back.",
            "Änderung von außen zurückgenommen. Wiederholen bringt sie zurück."
        ),
        CopyIn => ("A copy is in", "Eine Kopie liegt in"),
    }
}
