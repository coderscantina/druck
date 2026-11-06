//! Every string the directives show in Write, in English and German.

crate::strings! {
    pub enum Key {
        TwoColumns => ("Two columns", "Zwei Spalten"),
        FullWidth => ("Full width", "Volle Breite"),
        KeptTogether => ("Kept together", "Zusammen"),
        BottomOfPage => ("Bottom of page", "Seitenende"),
        NewPage => ("New page", "Neue Seite"),
        Table => ("Table", "Tabelle"),
        Contents => ("Contents", "Inhalt"),
        Bibliography => ("Bibliography", "Literatur"),
        Index => ("Index", "Register"),
        FrontMatter => ("Front matter", "Vorspann"),
        MainMatter => ("Main matter", "Hauptteil"),
        BackMatter => ("Back matter", "Anhang"),
        Kind => ("Kind", "Art"),
        KindColumns => ("Columns", "Spalten"),
        KindFullWidth => ("Wide", "Breit"),
        KindKeep => ("Keep", "Halten"),
        KindBottom => ("Bottom", "Unten"),
        FullWidthFromCaret => ("Full width from the caret", "Ab hier volle Breite"),
        GoToFullWidth => ("Go to the full-width part", "Zum breiten Teil"),
        Unwrap => ("Unwrap, keep the text", "Auflösen, Text bleibt"),
        OptionsFoot => (
            "Changes rewrite the fence lines, never the text between them. Escape returns to the line.",
            "Änderungen schreiben nur die :::-Zeilen, nie den Text dazwischen. Escape führt zur Zeile."
        ),
        Align => ("Align", "Satz"),
        Widths => ("Widths", "Breiten"),
        AlignLeft => ("Left", "Links"),
        AlignCenter => ("Center", "Mitte"),
        AlignRight => ("Right", "Rechts"),
        WidthFill => ("Fill", "Voll"),
        WidthAuto => ("Auto", "Auto"),
        GhostFoot => ("Return inserts it, Escape goes back", "Return fügt ein, Escape geht zurück"),
    }
}
