//! What the theme files say when something cannot be done, in English and German.

crate::strings! {
    pub enum Key {
        NotAThemeName => (
            "\"{name}\" is not a theme name: use letters, digits, hyphens, and underscores",
            "\"{name}\" ist kein Themenname: erlaubt sind Buchstaben, Ziffern, Binde- und Unterstriche"
        ),
        ThemeExists => ("a theme named \"{name}\" already exists", "ein Thema namens \"{name}\" gibt es schon"),
        NoCommonTheme => ("no common theme named \"{name}\"", "kein gemeinsames Thema namens \"{name}\""),
        ProjectHasTheme => (
            "the project already has a theme named \"{name}\"",
            "das Projekt hat schon ein Thema namens \"{name}\""
        ),
        NoProject => ("there is no project to copy the theme into", "es gibt kein Projekt, in das sich das Thema kopieren lässt"),
        CannotCreate => ("cannot create {path}: {error}", "{path} lässt sich nicht anlegen: {error}"),
        CannotWrite => ("cannot write {path}: {error}", "{path} lässt sich nicht schreiben: {error}"),
        CannotRename => ("cannot rename {path}: {error}", "{path} lässt sich nicht umbenennen: {error}"),
        CannotCopy => ("cannot copy {path}: {error}", "{path} lässt sich nicht kopieren: {error}"),
        CannotDelete => ("cannot delete {path}: {error}", "{path} lässt sich nicht löschen: {error}"),
        CannotRead => ("cannot read {path}: {error}", "{path} lässt sich nicht lesen: {error}"),
        CannotTake => ("the theme cannot take this change", "das Thema lässt diese Änderung nicht zu"),
        NotAFont => (
            "{path} is not an OpenType or TrueType font",
            "{path} ist keine OpenType- oder TrueType-Schrift"
        ),
        StyleExists => ("a style named \"{name}\" already exists", "einen Stil namens \"{name}\" gibt es schon"),
        TokenExists => ("a token named \"{name}\" already exists", "ein Token namens \"{name}\" gibt es schon"),
        NotATokenName => (
            "\"{name}\" is not a token name: use letters, digits, and hyphens",
            "\"{name}\" ist kein Token-Name: erlaubt sind Buchstaben, Ziffern und Bindestriche"
        ),
    }
}
