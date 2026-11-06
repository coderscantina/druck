//! Every string of the bookshelf, in English and German.

crate::strings! {
    pub enum Key {
        TheBookshelf => ("Bookshelf", "Bücherregal"),
        NotInBook => ("Not in book", "Nicht im Buch"),
        WordsOne => ("{count} word", "{count} Wort"),
        WordsMany => ("{count} words", "{count} Wörter"),
        PagesOne => ("{count} page", "{count} Seite"),
        PagesMany => ("{count} pages", "{count} Seiten"),
        OpenOne => ("1 open placeholder", "1 offener Platzhalter"),
        OpenMany => ("{count} open placeholders", "{count} offene Platzhalter"),
        HintBook => ("Click to open, drag to move", "Klick öffnet, Ziehen verschiebt"),
        HintLoose => ("Click to open", "Klick öffnet"),
    }
}
