//! The palette's strings, in English and German.

crate::strings! {
    pub enum Key {
        Chapters => ("Find a chapter", "Kapitel finden"),
        Commands => ("Run a command", "Befehl ausführen"),
        NoMatches => ("Nothing matches this.", "Nichts passt dazu."),
        OpenRecent => ("Open recent: {name}", "Zuletzt benutzt: {name}"),
    }
}
