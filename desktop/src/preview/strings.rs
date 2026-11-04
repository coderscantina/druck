//! The strings of the preview's status strip, in English and German.

crate::strings! {
    pub enum Key {
        NoPagesYet => ("No pages yet", "Noch keine Seiten"),
        NoPages => ("No pages", "Keine Seiten"),
        PageOf => (
            "Page {page} of {pages} \u{00b7} {percent}%",
            "Seite {page} von {pages} \u{00b7} {percent}\u{a0}%"
        ),
    }
}
