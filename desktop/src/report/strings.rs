//! The strings of the problem report, in English and German.

crate::strings! {
    pub enum Key {
        Panicked => (
            "Druck stopped unexpectedly last time. Nothing leaves your Mac unless you send it.",
            "Druck wurde beim letzten Mal unerwartet beendet. Nichts verlässt deinen Mac, wenn du es nicht sendest."
        ),
        Report => ("Report", "Melden"),
        Dismiss => ("Dismiss", "Schließen"),
    }
}
