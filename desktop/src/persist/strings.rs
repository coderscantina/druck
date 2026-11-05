//! Every string the saving state and the conflict prompt show, in English and German.

crate::strings! {
    pub enum Key {
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
            "wurde von außen dort geändert, wo du schreibst. Nichts geht verloren:"
        ),
        OverlapBranch => (
            "In {name}, {branch} changed the same place as you. Nothing is lost, whichever you choose:",
            "In {name} hat {branch} dieselbe Stelle geändert wie du. Nichts geht verloren:"
        ),
        Deleted => (
            "was deleted outside, but has text not saved yet. Nothing is lost, whichever you choose:",
            "wurde von außen gelöscht, hat aber ungesicherten Text. Nichts geht verloren:"
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
        NotUtf8 => ("{file} is not UTF-8 text.", "{file} ist kein UTF-8-Text."),
        NotAsSeen => (
            "the file is not as the app last saw it",
            "die Datei ist nicht mehr so, wie die App sie zuletzt sah"
        ),
        CannotWatch => (
            "Changes from other programs do not show here: {error}",
            "Änderungen aus anderen Programmen erscheinen hier nicht: {error}"
        ),
        CannotRestore => (
            "Cannot bring back the unsaved text of {file}: {error}",
            "Der ungesicherte Text von {file} lässt sich nicht zurückholen: {error}"
        ),
        CannotKeepCopy => (
            "Cannot keep a recovery copy of {file}: {error}",
            "Keine Sicherungskopie von {file} möglich: {error}"
        ),
        CannotMerge => (
            "Cannot merge the change from outside into {file}: {error}",
            "Die Änderung von außen lässt sich nicht in {file} einfügen: {error}"
        ),
        CannotUpdateChapters => (
            "Cannot update the chapter list: {error}",
            "Die Kapitelliste lässt sich nicht aktualisieren: {error}"
        ),
        CannotKeepHistory => (
            "Cannot keep local history of {file}: {error}",
            "Kein lokaler Verlauf für {file} möglich: {error}"
        ),
        CannotMakeRepo => (
            "This book could not become a Git repo, and has local history only: {error}",
            "Dieses Buch konnte kein Git-Repo werden und hat nur den lokalen Verlauf: {error}"
        ),
    }
}
