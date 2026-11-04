//! The theme the panels share. Panels read it and ask it for changes, so they never hold a copy that could
//! disagree. Each change is checked and saved by [`Editor`], then announced.

use std::path::{Path, PathBuf};

use gpui::{Context, EventEmitter};

use crate::theme_file::editor::{Change, Dirs, Editor, Outcome};
use crate::theme_file::{Problem, ThemeFile};

/// A property of the theme changed and was saved. The preview and the galley update on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeChanged {
    pub path: PathBuf,
    /// Dotted path of the property, such as `styles.heading-1.size`. Empty when the whole text changed.
    pub property: String,
}

/// Another theme file is open now.
pub struct ThemeSwitched;

/// The author asked to see the specimen, since the document has no block in the style being edited.
pub struct SpecimenRequested;

pub struct ThemeModel {
    editor: Editor,
    /// What the engine says about the file as it stands, kept so a panel does not ask on every frame.
    problems: Vec<Problem>,
    /// A style the document has no block in, for the Styles panel to offer the specimen about.
    missing: Option<String>,
}

impl EventEmitter<ThemeChanged> for ThemeModel {}
impl EventEmitter<ThemeSwitched> for ThemeModel {}
impl EventEmitter<SpecimenRequested> for ThemeModel {}

impl ThemeModel {
    pub fn new(path: Option<&Path>, dirs: Dirs) -> Self {
        let mut model = Self {
            editor: Editor::open(path, dirs),
            problems: Vec::new(),
            missing: None,
        };
        model.refresh();
        model
    }

    fn refresh(&mut self) {
        self.problems = self
            .editor
            .file()
            .map(|file| file.problems().to_vec())
            .unwrap_or_default();
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// The open file, or why there is none.
    pub fn file(&self) -> Result<&ThemeFile, &str> {
        self.editor.file()
    }

    /// The first problem at a property, or under it.
    pub fn problem_at(&self, property: &str) -> Option<&Problem> {
        self.problems.iter().find(|problem| {
            problem
                .property
                .as_deref()
                .is_some_and(|at| at == property || at.strip_prefix(property).is_some_and(|rest| rest.starts_with('.')))
        })
    }

    /// The style the document has no block in, after the last jump looked for one.
    pub fn missing(&self) -> Option<&str> {
        self.missing.as_deref()
    }

    pub fn set_missing(&mut self, style: Option<String>, cx: &mut Context<Self>) {
        if self.missing != style {
            self.missing = style;
            cx.notify();
        }
    }

    /// Asks for the specimen, for the style that was missing.
    pub fn request_specimen(&mut self, cx: &mut Context<Self>) {
        cx.emit(SpecimenRequested);
    }

    /// Opens another theme file, dropping any change that waited for a choice.
    pub fn open(&mut self, path: Option<&Path>, cx: &mut Context<Self>) {
        let dirs = self.editor.dirs().clone();
        self.editor = Editor::open(path, dirs);
        self.refresh();
        cx.emit(ThemeSwitched);
        cx.notify();
    }

    /// Reads the file again after another program changed it. Its own saves change nothing here. A file that does not
    /// parse just now keeps the last good text in the panels, and the pages say what is wrong.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if !self.editor.reload() {
            return;
        }
        self.refresh();
        if let Ok(file) = self.editor.file() {
            cx.emit(ThemeChanged {
                path: file.path().to_owned(),
                property: String::new(),
            });
        }
        cx.notify();
    }

    /// Makes a change. The reason is the engine's when it refuses.
    pub fn change(&mut self, change: Change, cx: &mut Context<Self>) -> Result<Outcome, String> {
        let property = change.property();
        let result = self.editor.apply(change);
        self.after(result, property, cx)
    }

    /// Copies a common theme into the project and makes the waiting change there.
    pub fn fork(&mut self, cx: &mut Context<Self>) -> Result<Outcome, String> {
        let property = self.editor.pending().map(Change::property).unwrap_or_default();
        let result = self.editor.fork();
        if result.is_ok() {
            cx.emit(ThemeSwitched);
        }
        self.after(result, property, cx)
    }

    pub fn edit_shared(&mut self, cx: &mut Context<Self>) -> Result<Outcome, String> {
        let property = self.editor.pending().map(Change::property).unwrap_or_default();
        let result = self.editor.edit_shared();
        self.after(result, property, cx)
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.editor.cancel();
        cx.notify();
    }

    fn after(
        &mut self,
        result: Result<Outcome, String>,
        property: String,
        cx: &mut Context<Self>,
    ) -> Result<Outcome, String> {
        if matches!(result, Ok(Outcome::Saved)) {
            self.refresh();
            if let Ok(file) = self.editor.file() {
                cx.emit(ThemeChanged {
                    path: file.path().to_owned(),
                    property,
                });
            }
        }
        cx.notify();
        result
    }
}
