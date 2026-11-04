//! Every string the theme panels show, in English and German, in one table. A string is a [`Text`], which reads the
//! language of the machine once, or `DRUCK_LANG`. The Shell slice's table takes this one over when the two meet.

use std::sync::OnceLock;

use gpui::{IntoElement, SharedString};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    English,
    German,
}

fn language() -> Language {
    static LANGUAGE: OnceLock<Language> = OnceLock::new();
    *LANGUAGE.get_or_init(|| {
        let asked = std::env::var("DRUCK_LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        if asked.to_ascii_lowercase().starts_with("de") {
            Language::German
        } else {
            Language::English
        }
    })
}

/// A string with its two languages.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Text {
    english: &'static str,
    german: &'static str,
}

impl Text {
    pub const fn new(english: &'static str, german: &'static str) -> Self {
        Self { english, german }
    }

    pub fn get(self) -> &'static str {
        match language() {
            Language::English => self.english,
            Language::German => self.german,
        }
    }

    pub fn upper(self) -> SharedString {
        SharedString::from(self.get().to_uppercase())
    }

    /// The text with `{}` replaced by `value`.
    pub fn with(self, value: &str) -> String {
        self.get().replace("{}", value)
    }
}

impl IntoElement for Text {
    type Element = <&'static str as IntoElement>::Element;

    fn into_element(self) -> Self::Element {
        self.get().into_element()
    }
}

impl From<Text> for SharedString {
    fn from(text: Text) -> Self {
        SharedString::new_static(text.get())
    }
}

macro_rules! strings {
    ($($name:ident = $english:literal / $german:literal;)*) => {
        $(pub const $name: Text = Text::new($english, $german);)*
    };
}

strings! {
    // Shared
    BACK = "Back" / "Zurück";
    RESET = "Reset" / "Zurücksetzen";
    INHERITED = "DEFAULT" / "STANDARD";
    CUSTOMIZED = "CUSTOM" / "EIGEN";
    NO_THEME = "No theme file is open. Start the app with --theme PATH or name a theme in the document." / "Keine Themendatei geöffnet. Starte die App mit --theme PFAD oder nenne ein Thema im Dokument.";
    SAVE_FAILED = "Cannot save the theme" / "Das Thema lässt sich nicht speichern";
    CANCEL = "Cancel" / "Abbrechen";
    DELETE = "Delete" / "Löschen";
    RENAME = "Rename" / "Umbenennen";
    CREATE = "Create" / "Anlegen";
    NAME = "Name" / "Name";
    NOT_A_NUMBER = "Enter a number, such as 1.4" / "Gib eine Zahl ein, etwa 1,4";
    NONE = "None" / "Keine";
    YES = "On" / "An";
    NO = "Off" / "Aus";
    TOKEN = "Token" / "Token";
    PICK_TOKEN = "Pick a token" / "Token wählen";
    PROBLEMS = "PROBLEMS" / "PROBLEME";
    EDIT = "Edit" / "Bearbeiten";

    // Styles
    STYLES_TITLE = "STYLES" / "STILE";
    CUSTOM_STYLES = "CUSTOM STYLES" / "EIGENE STILE";
    NEW_STYLE = "New style" / "Neuer Stil";
    NEW_STYLE_HINT = "Name, such as pull-quote" / "Name, etwa zitat-gross";
    STYLE_NAME_TAKEN = "A style with this name exists" / "Ein Stil mit diesem Namen existiert";
    BASED_ON = "Based on" / "Basiert auf";
    USED_AS = "Use in a document with {.name}" / "Im Dokument mit {.name} verwenden";
    RENAME_STYLE_NOTE = "Documents that use {.old} must change to {.new}." / "Dokumente, die {.alt} nutzen, müssen auf {.neu} wechseln.";
    FONT = "Font" / "Schrift";
    SIZE = "Size" / "Größe";
    WEIGHT = "Weight" / "Schnitt";
    FONT_STYLE = "Font style" / "Schriftstil";
    COLOR = "Color" / "Farbe";
    LINE_HEIGHT = "Line height" / "Zeilenabstand";
    ALIGN_LABEL = "Text alignment" / "Textausrichtung";
    HYPHENATE = "Hyphenation" / "Silbentrennung";
    SPACE_BEFORE = "Space before" / "Abstand davor";
    SPACE_AFTER = "Space after" / "Abstand danach";
    INDENT = "Indent" / "Einzug";
    FIRST_LINE_INDENT = "First line indent" / "Erstzeileneinzug";
    TRACKING = "Letter spacing" / "Laufweite";
    UPPERCASE = "Capitals" / "Großbuchstaben";
    KEEP_WITH_NEXT = "Keep with next" / "Mit nächstem halten";
    FIGURES_STYLE = "Figures" / "Ziffern";
    FIGURES_SPACING = "Figure width" / "Ziffernbreite";
    NUMBER_GAP = "Number gap" / "Nummernabstand";
    BULLETS = "Bullets per level" / "Aufzählungszeichen je Ebene";
    BULLETS_HINT = "Separated by commas" / "Durch Kommas getrennt";
    MARKER = "Marker style" / "Markerstil";
    RULE_BELOW = "Rule below" / "Linie darunter";
    NORMAL = "Normal" / "Normal";
    ITALIC = "Italic" / "Kursiv";
    REGULAR = "Regular" / "Normal";
    BOLD = "Bold" / "Fett";
    ALIGN_LEFT = "Left" / "Links";
    ALIGN_CENTER = "Center" / "Mitte";
    ALIGN_RIGHT = "Right" / "Rechts";
    ALIGN_JUSTIFY = "Justify" / "Blocksatz";
    DEFAULT_WORD = "Default" / "Standard";
    LINING = "Lining" / "Versalziffern";
    OLDSTYLE = "Oldstyle" / "Mediävalziffern";
    PROPORTIONAL = "Proportional" / "Proportional";
    TABULAR = "Tabular" / "Tabellarisch";
    RULE_NONE = "None" / "Keine";
    RULE_HEADER = "Header rule" / "Kopflinie";
    RULE_ROW = "Row rule" / "Zeilenlinie";
    MISSING_STYLE_BLOCK = "No block in this document uses {}." / "Kein Block dieses Dokuments nutzt {}.";
    SHOW_SPECIMEN = "Show the specimen" / "Muster zeigen";

    // Fonts
    FONTS_TITLE = "FONTS" / "SCHRIFTEN";
    FONT_FAMILIES = "FAMILIES IN USE" / "VERWENDETE FAMILIEN";
    ADD_FONT = "Add a font file" / "Schriftdatei hinzufügen";
    CHOOSE = "Choose" / "Wählen";
    TYPEFACE = "Typeface" / "Schriftart";
    TOTAL_SIZE = "Total size" / "Gesamtgröße";
    FORMAT = "Format" / "Format";
    SOURCE = "Source" / "Quelle";
    LICENSE = "LICENSE" / "LIZENZ";
    NO_LICENSE = "The font file carries no license text." / "Die Schriftdatei enthält keinen Lizenztext.";
    TRUETYPE = "TrueType" / "TrueType";
    OPENTYPE = "OpenType" / "OpenType";
    COLLECTION_FORMAT = "{} faces in the file" / "{} Schnitte in der Datei";
    SOURCE_BUNDLED = "Bundled" / "Mitgeliefert";
    SOURCE_INSTALLED = "Installed" / "Installiert";
    SOURCE_FILE = "Project file" / "Projektdatei";
    SOURCE_THEME_FILE = "Theme file" / "Themendatei";
    EMBEDDING_WARNING = "This font's license restricts embedding. Druck embeds it anyway, so check that the license allows this." / "Die Lizenz dieser Schrift schränkt das Einbetten ein. Druck bettet sie trotzdem ein, prüfe daher die Lizenz.";
    AXES = "VARIABLE AXES" / "VARIABLE ACHSEN";
    AXES_NOTE = "Shown for reference. The weights a theme asks for set the axis." / "Zur Ansicht. Die angeforderten Schnitte setzen die Achse.";
    USED_BY = "Used by" / "Verwendet von";
    FACES = "Faces" / "Schnitte";
    FONT_SPECIMEN = "Aa" / "Aa";
    FACE_NAME = "Face" / "Schnitt";
    FONT_ADDED = "Added {}" / "{} hinzugefügt";
    NOT_A_FONT = "Not a font file" / "Keine Schriftdatei";

    // Tokens
    TOKENS_TITLE = "TOKENS" / "TOKENS";
    GROUP_FONTS = "Fonts" / "Schriften";
    GROUP_SIZES = "Sizes" / "Größen";
    GROUP_SPACING = "Spacing" / "Abstände";
    GROUP_COLORS = "Colors" / "Farben";
    NEW_TOKEN = "New token" / "Neuer Token";
    NEW_TOKEN_HINT = "Name, such as accent" / "Name, etwa akzent";
    TOKEN_USED = "Used in {} places" / "An {} Stellen verwendet";
    TOKEN_UNUSED = "Not used" / "Nicht verwendet";
    TOKEN_BLOCKED = "In use, so it cannot be deleted: {}" / "In Verwendung und daher nicht löschbar: {}";
    TOKEN_VALUE = "Value" / "Wert";

    // Page geometry
    GEOMETRY_TITLE = "PAGE" / "SEITE";
    PAGE_SIZE = "Page size" / "Seitenformat";
    CUSTOM_SIZE = "Custom" / "Eigenes";
    WIDTH = "Width" / "Breite";
    HEIGHT = "Height" / "Höhe";
    MARGINS = "MARGINS" / "RÄNDER";
    MARGIN_TOP = "Top" / "Oben";
    MARGIN_BOTTOM = "Bottom" / "Unten";
    MARGIN_INNER = "Inner" / "Innen";
    MARGIN_OUTER = "Outer" / "Außen";
    MIRROR = "Mirror margins on even pages" / "Ränder auf geraden Seiten spiegeln";
    COLUMNS = "TEXT" / "TEXT";
    COLUMN_GAP = "Column gap" / "Spaltenabstand";
    TEXT_WIDTH = "Narrower prose width" / "Schmalere Textbreite";
    TEXT_WIDTH_HINT = "Empty for the full frame" / "Leer für den ganzen Satzspiegel";

    // JSON view
    JSON_TITLE = "THEME SOURCE" / "THEMENQUELLE";
    JSON_VALID = "Valid. Changes apply as you type." / "Gültig. Änderungen gelten sofort.";
    JSON_INVALID = "Not applied: {}" / "Nicht übernommen: {}";
    JSON_SECTIONS = "SECTIONS" / "ABSCHNITTE";
    JSON_DESIGNED = "Own panel" / "Eigenes Panel";
    JSON_ONLY = "Source view only" / "Nur Quellansicht";
    JSON_NOTE = "These sections have no panel yet. Edit them here." / "Für diese Abschnitte gibt es noch kein Panel. Bearbeite sie hier.";

    // Picker
    THEME = "Theme" / "Thema";
    PROJECT_THEMES = "PROJECT" / "PROJEKT";
    COMMON_THEMES = "COMMON" / "ALLGEMEIN";
    NEW_COMMON_THEME = "New common theme" / "Neues allgemeines Thema";
    DUPLICATE = "Duplicate" / "Duplizieren";
    DEFAULT_THEME = "Built-in default" / "Eingebaute Vorgabe";
    DELETE_THEME = "Delete the common theme \"{}\"?" / "Das allgemeine Thema „{}“ löschen?";
    DELETE_THEME_DETAIL = "Documents that name it fall back to errors until they name another theme. This cannot be undone." / "Dokumente, die es nennen, melden Fehler, bis sie ein anderes Thema nennen. Das lässt sich nicht rückgängig machen.";
    DELETE_THEME_OPEN = "The open document uses it." / "Das geöffnete Dokument nutzt es.";
    COPY_OF = "{}-copy" / "{}-kopie";
    SPECIMEN = "Specimen" / "Muster";

    // Fork
    FORK_TITLE = "Change a shared theme" / "Gemeinsames Thema ändern";
    FORK_BODY = "\"{}\" is a common theme. Other documents may use it too." / "„{}“ ist ein allgemeines Thema. Auch andere Dokumente können es nutzen.";
    FORK_COPY = "Copy it into this project (recommended)" / "In dieses Projekt kopieren (empfohlen)";
    FORK_COPY_NOTE = "The copy is changed. The shared theme and its other documents stay as they are." / "Die Kopie wird geändert. Das gemeinsame Thema und seine anderen Dokumente bleiben wie sie sind.";
    FORK_SHARED = "Change the shared theme" / "Gemeinsames Thema ändern";
    FORK_SHARED_NOTE = "Every document that uses it changes." / "Jedes Dokument, das es nutzt, ändert sich.";
    FORK_AFFECTED = "Documents known to use it: {}." / "Bekannte Dokumente, die es nutzen: {}.";
    FORK_UNKNOWN = "Others on this machine are not known." / "Weitere auf diesem Rechner sind nicht bekannt.";
}

/// The block styles of the theme, in the order the list shows them, with the key each has in `styles`.
pub const STYLES: &[(&str, Text)] = &[
    ("body", Text::new("Regular text", "Fließtext")),
    ("heading-1", Text::new("Heading 1", "Überschrift 1")),
    ("heading-2", Text::new("Heading 2", "Überschrift 2")),
    ("heading-3", Text::new("Heading 3", "Überschrift 3")),
    ("heading-4", Text::new("Heading 4", "Überschrift 4")),
    ("heading-5", Text::new("Heading 5", "Überschrift 5")),
    ("heading-6", Text::new("Heading 6", "Überschrift 6")),
    ("quote", Text::new("Block quotation", "Blockzitat")),
    ("callout", Text::new("Callout", "Hinweisbox")),
    ("callout-label", Text::new("Callout label", "Hinweisbox-Titel")),
    ("list", Text::new("List", "Liste")),
    ("code-block", Text::new("Code block", "Codeblock")),
    ("caption", Text::new("Caption", "Beschriftung")),
    ("footnote", Text::new("Footnote", "Fußnote")),
    ("table-cell", Text::new("Table cell", "Tabellenzelle")),
    ("table-header", Text::new("Table header", "Tabellenkopf")),
    ("title", Text::new("Title", "Titel")),
    ("subtitle", Text::new("Subtitle", "Untertitel")),
    ("author", Text::new("Author", "Autor")),
    ("date", Text::new("Date", "Datum")),
    ("abstract-heading", Text::new("Abstract heading", "Zusammenfassung, Überschrift")),
    ("abstract", Text::new("Abstract", "Zusammenfassung")),
    ("header", Text::new("Running header", "Kopfzeile")),
    ("footer", Text::new("Running footer", "Fußzeile")),
    ("watermark", Text::new("Watermark", "Wasserzeichen")),
    ("toc-heading", Text::new("Contents heading", "Inhaltsverzeichnis, Überschrift")),
    ("toc-entry", Text::new("Contents entry", "Inhaltsverzeichnis, Eintrag")),
    ("bibliography", Text::new("Bibliography", "Literaturverzeichnis")),
    ("scene-break", Text::new("Scene break", "Szenenwechsel")),
    ("index-letter", Text::new("Index letter", "Register, Buchstabe")),
    ("index-entry", Text::new("Index entry", "Register, Eintrag")),
    ("index-sub-entry", Text::new("Index sub-entry", "Register, Untereintrag")),
];
