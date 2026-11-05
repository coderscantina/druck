//! The theme the panels share. Panels read it and ask it for changes, so they never hold a copy that could
//! disagree. [`Editor`] checks each change at once and the panels show it at once. The write follows on the writer's
//! thread, and the change is announced when it has landed, because the pages read the theme from the file. A change the
//! probe typeset refuses afterwards is taken back and reported as a rejection.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use gpui::{Context, EventEmitter, Task};

use crate::theme_file::editor::{Change, Dirs, Editor, Outcome};
use crate::theme_file::writer::{Done, Writer};
use crate::theme_file::{Problem, ThemeFile};

/// A property of the theme changed and is on disk. The preview and the galley update on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeChanged {
    pub path: PathBuf,
    /// Dotted path of the property, such as `styles.heading-1.size`. Empty when the whole text changed.
    pub property: String,
}

/// Another theme file is open now.
pub struct ThemeSwitched;

/// The question about a shared theme was cancelled, and the change that asked it is dropped.
pub struct QuestionCancelled;

/// The author asked to see the specimen, since the document has no block in the style being edited.
pub struct SpecimenRequested;

/// A commit the writer has not reported yet.
struct Sent {
    /// The file it was for, so a report about an earlier file is not taken for the open one.
    generation: u64,
    property: String,
}

pub struct ThemeModel {
    editor: Editor,
    /// What the engine says about the file as it stands, kept so a panel does not ask on every frame.
    problems: Vec<Problem>,
    /// Why the writer refused or could not write the last changes, until the next change.
    rejected: Vec<Problem>,
    writer: Writer,
    /// The commits given to the writer, in the order it reports them.
    sent: VecDeque<Sent>,
    /// Counts the files opened, which ends the meaning of what was sent for an earlier one.
    generation: u64,
    _reports: Task<()>,
    /// A style the document has no block in, for the Styles panel to offer the specimen about.
    missing: Option<String>,
}

impl EventEmitter<ThemeChanged> for ThemeModel {}
impl EventEmitter<ThemeSwitched> for ThemeModel {}
impl EventEmitter<QuestionCancelled> for ThemeModel {}
impl EventEmitter<SpecimenRequested> for ThemeModel {}

impl ThemeModel {
    pub fn new(path: Option<&Path>, dirs: Dirs, cx: &mut Context<Self>) -> Self {
        let (reports, mut incoming) = futures::channel::mpsc::unbounded::<Done>();
        let writer = Writer::new(move |done| {
            reports.unbounded_send(done).ok();
        });
        let task = cx.spawn(async move |model, cx| {
            // Parked while nothing is being written.
            while let Some(done) = incoming.next().await {
                if model.update(cx, |model, cx| model.finished(done, cx)).is_err() {
                    return;
                }
            }
        });
        let mut model = Self {
            editor: Editor::open(path, dirs),
            problems: Vec::new(),
            rejected: Vec::new(),
            writer,
            sent: VecDeque::new(),
            generation: 0,
            _reports: task,
            missing: None,
        };
        model.refresh();
        model
    }

    /// Waits until every change is on disk, for an app that is about to quit.
    pub fn flush(&self) {
        self.writer.flush();
    }

    /// The reason the last change was taken back after it was accepted, if it was.
    pub fn rejection(&self) -> Option<&Problem> {
        self.rejected.first()
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
        self.problems.iter().chain(&self.rejected).find(|problem| {
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
        self.generation += 1;
        self.rejected.clear();
        self.refresh();
        cx.emit(ThemeSwitched);
        cx.notify();
    }

    /// Reads the file again after another program changed it. Its own saves change nothing here. A file that does not
    /// parse just now keeps the last good text in the panels, and the pages say what is wrong.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        // What is on disk lags the panels while a write is on its way, and the write's own echo is no change.
        if !self.sent.is_empty() || !self.editor.reload() {
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
        self.rejected.clear();
        let result = self.editor.stage(change);
        self.after(result, property, cx)
    }

    /// Copies a common theme into the project and makes the waiting change there.
    pub fn fork(&mut self, cx: &mut Context<Self>) -> Result<Outcome, String> {
        let property = self.editor.pending().map(Change::property).unwrap_or_default();
        let result = self.editor.fork();
        if result.is_ok() {
            self.generation += 1;
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
        cx.emit(QuestionCancelled);
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
            match self.editor.take_commit() {
                Some(commit) => {
                    let disk = self
                        .editor
                        .file()
                        .map(|file| file.disk().to_owned())
                        .unwrap_or_default();
                    self.sent.push_back(Sent {
                        generation: self.generation,
                        property,
                    });
                    self.writer.submit(commit, disk);
                }
                // The text is what it was, so there is nothing to wait for.
                None => self.announce(property, cx),
            }
        }
        cx.notify();
        result
    }

    fn announce(&mut self, property: String, cx: &mut Context<Self>) {
        if let Ok(file) = self.editor.file() {
            cx.emit(ThemeChanged {
                path: file.path().to_owned(),
                property,
            });
        }
    }

    /// The writer reports its commits in the order they were sent.
    fn finished(&mut self, done: Done, cx: &mut Context<Self>) {
        let Some(sent) = self.sent.pop_front() else {
            return;
        };
        if sent.generation == self.generation {
            match done.result {
                Ok(()) => {
                    self.editor.written(&done.commit.text);
                    self.announce(sent.property, cx);
                }
                Err(problem) => self.rejected.push(problem),
            }
            // With nothing more to come, what the disk refused is no longer what the panels show.
            if self.sent.is_empty() && self.editor.take_back_unwritten() {
                self.refresh();
            }
        }
        cx.notify();
    }
}
