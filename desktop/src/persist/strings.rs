//! Every string the saving state and the conflict prompt show, in English and German. The Shell slice's table takes
//! these over when it lands.

pub struct Strings {
    pub saved: &'static str,
    pub edited: &'static str,
    pub saving: &'static str,
    pub cannot_save: &'static str,
    pub kept_in_memory: &'static str,
    pub folder_gone: &'static str,
    pub unreadable: &'static str,
    pub overlap: &'static str,
    pub deleted: &'static str,
    pub keep_mine: &'static str,
    pub take_theirs: &'static str,
    pub see_both: &'static str,
    pub merged: &'static str,
    pub reloaded: &'static str,
    pub removed: &'static str,
    pub restored: &'static str,
    pub undo_stopped: &'static str,
    pub undo_done: &'static str,
    pub copy_in: &'static str,
}

const EN: Strings = Strings {
    saved: "Saved",
    edited: "Edited",
    saving: "Saving\u{2026}",
    cannot_save: "Cannot save",
    kept_in_memory: "Your text is safe here and will be saved when it can be.",
    folder_gone: "The document's folder is gone. Your text is safe here and will be saved when the folder is back.",
    unreadable: "Not readable",
    overlap: "changed outside where you are writing. Nothing is lost, whichever you choose:",
    deleted: "was deleted outside, but has text not saved yet. Nothing is lost, whichever you choose:",
    keep_mine: "Keep mine",
    take_theirs: "Take theirs",
    see_both: "See both",
    merged: "Merged changes from outside into",
    reloaded: "Updated from outside:",
    removed: "was deleted outside. Its text is still here.",
    restored: "Restored text not saved before the app stopped:",
    undo_stopped: "The next undo takes back the change from outside. Press again to do so.",
    undo_done: "Took back the change from outside. Redo brings it back.",
    copy_in: "A copy is in",
};

const DE: Strings = Strings {
    saved: "Gesichert",
    edited: "Bearbeitet",
    saving: "Wird gesichert\u{2026}",
    cannot_save: "Sichern nicht möglich",
    kept_in_memory: "Dein Text ist hier sicher und wird gesichert, sobald es geht.",
    folder_gone: "Der Ordner des Dokuments ist weg. Dein Text ist hier sicher und wird gesichert, sobald der Ordner wieder da ist.",
    unreadable: "Nicht lesbar",
    overlap: "wurde von außen an der Stelle geändert, an der du schreibst. Nichts geht verloren, was du auch wählst:",
    deleted: "wurde von außen gelöscht, enthält aber noch nicht gesicherten Text. Nichts geht verloren, was du auch wählst:",
    keep_mine: "Meins behalten",
    take_theirs: "Das andere nehmen",
    see_both: "Beides zeigen",
    merged: "Änderungen von außen eingefügt in",
    reloaded: "Von außen aktualisiert:",
    removed: "wurde von außen gelöscht. Der Text ist noch hier.",
    restored: "Text wiederhergestellt, der beim Beenden nicht gesichert war:",
    undo_stopped: "Der nächste Widerruf nimmt die Änderung von außen zurück. Zum Bestätigen noch einmal drücken.",
    undo_done: "Änderung von außen zurückgenommen. Wiederholen bringt sie zurück.",
    copy_in: "Eine Kopie liegt in",
};

/// The table for the system's language.
pub fn get() -> &'static Strings {
    let language = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
    if language.is_some_and(|language| language.starts_with("de")) {
        &DE
    } else {
        &EN
    }
}
