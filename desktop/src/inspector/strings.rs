//! Every string the inspector shows, in one table. The German table joins this one before the first
//! public release.

pub const STYLES_TITLE: &str = "STYLES";
pub const BACK: &str = "Styles";
pub const INHERITED: &str = "INHERITED";
pub const CUSTOMIZED: &str = "CUSTOMIZED";
pub const RESET: &str = "Reset";
pub const NO_THEME: &str = "No theme file is open. Start the app with --theme PATH.";
pub const ALIGN_LABEL: &str = "Text alignment";
pub const SAVE_FAILED: &str = "Cannot save the theme";

pub const FONT: &str = "Font";
pub const SIZE: &str = "Size";
pub const WEIGHT: &str = "Weight";
pub const LINE_HEIGHT: &str = "Line height";
pub const SPACE_BEFORE: &str = "Space before";
pub const SPACE_AFTER: &str = "Space after";

pub const ALIGN_LEFT: &str = "Left";
pub const ALIGN_CENTER: &str = "Center";
pub const ALIGN_RIGHT: &str = "Right";
pub const ALIGN_JUSTIFY: &str = "Justify";

pub const NOT_A_NUMBER: &str = "Enter a number, such as 1.4";

/// The block styles of the theme, in the order the list shows them, with the key each has in `styles`.
pub const STYLES: &[(&str, &str)] = &[
    ("body", "Regular text"),
    ("heading-1", "Heading 1"),
    ("heading-2", "Heading 2"),
    ("heading-3", "Heading 3"),
    ("heading-4", "Heading 4"),
    ("heading-5", "Heading 5"),
    ("heading-6", "Heading 6"),
    ("quote", "Block quotation"),
    ("list", "List"),
    ("code-block", "Code block"),
    ("caption", "Caption"),
    ("footnote", "Footnote"),
    ("table-cell", "Table cell"),
    ("table-header", "Table header"),
    ("title", "Title"),
    ("subtitle", "Subtitle"),
    ("author", "Author"),
    ("date", "Date"),
    ("abstract-heading", "Abstract heading"),
    ("abstract", "Abstract"),
    ("header", "Running header"),
    ("footer", "Running footer"),
    ("toc-heading", "Contents heading"),
    ("toc-entry", "Contents entry"),
    ("bibliography", "Bibliography"),
    ("scene-break", "Scene break"),
    ("index-letter", "Index letter"),
    ("index-entry", "Index entry"),
    ("index-sub-entry", "Index sub-entry"),
];
