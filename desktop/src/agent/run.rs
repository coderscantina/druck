//! Running an editing pass: one turn per chapter, each in a CLI process of its own, so a chapter that fails costs only
//! itself and no chapter's turn carries the ones before it. Results come in as any turn's do, as changes under review
//! and margin notes.

use std::collections::VecDeque;
use std::time::Duration;

use gpui::{App, Context, Window};

use super::conversation::Outcome;
use super::passes::{self, Listed, Origin, Pass, Returns, Target};
use super::session::Access;
use super::strings::{self, Key};
use super::{AgentView, relative, style_sheet};
use crate::session::Kind;
use crate::workspace::Current;

/// How long the line at the window's foot says how a run ended.
const RESULT_SHOWN: Duration = Duration::from_secs(8);

/// What a pass runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Book,
    Chapter,
    Selection,
}

/// What a palette row about passes does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassAction {
    Run(String, Scope),
    /// Copies a built-in pass into the project and opens the copy.
    Customize(String),
}

/// A row for the palette.
pub struct PassCommand {
    pub label: String,
    pub action: PassAction,
}

/// The palette's rows about passes, for the book on screen. None with the agent off.
pub fn pass_commands(cx: &App) -> Vec<PassCommand> {
    Current::<AgentView>::view(cx)
        .map(|view| view.read(cx).pass_commands(cx))
        .unwrap_or_default()
}

pub fn run_pass_command(action: PassAction, cx: &mut App) {
    Current::<AgentView>::defer(cx, move |view, window, cx| match action {
        PassAction::Run(slug, scope) => view.run_pass(&slug, scope, window, cx),
        PassAction::Customize(slug) => view.customize_pass(&slug, cx),
    });
}

/// One chapter of a run.
struct Step {
    title: String,
    target: Target,
}

/// A pass under way.
pub(super) struct PassRun {
    pass: Pass,
    steps: VecDeque<Step>,
    total: usize,
    /// The title of the chapter being worked on.
    current: String,
    done: usize,
    /// The chapters whose turn failed.
    failed: Vec<String>,
    stopped: bool,
    /// The conversation the chat card had, which it gets back when the run ends.
    chat_session: Option<String>,
}

impl PassRun {
    pub(super) fn access(&self) -> Access {
        match self.pass.returns {
            Returns::Changes => Access::Edits,
            Returns::Notes => Access::ReadOnly,
        }
    }

    /// The quiet line at the window's foot.
    pub(super) fn progress(&self) -> String {
        let at = self.total - self.steps.len();
        strings::pass_progress(&self.pass.name, at, self.total, &self.current)
    }
}

/// The name a pass goes by: a built-in's in the app's language, a project's as its file says.
fn name_of(listed: &Listed) -> String {
    let built_in = match (listed.origin, listed.slug.as_str()) {
        (Origin::BuiltIn, "copyedit") => Some(Key::PassCopyedit),
        (Origin::BuiltIn, "consistency") => Some(Key::PassConsistency),
        (Origin::BuiltIn, "references") => Some(Key::PassReferences),
        (Origin::BuiltIn, "line-edit") => Some(Key::PassLineEdit),
        _ => None,
    };
    match (built_in, &listed.pass) {
        (Some(key), _) => crate::t!(key).to_owned(),
        (None, Ok(pass)) => pass.name.clone(),
        (None, Err(_)) => listed.slug.clone(),
    }
}

impl AgentView {
    fn pass_commands(&self, cx: &App) -> Vec<PassCommand> {
        if !self.enabled {
            return Vec::new();
        }
        let editor = self.editor.read(cx);
        let session = editor.session();
        let whole_book = matches!(session.kind(), Kind::Project { .. }) && session.book().len() > 1;
        let selected = !editor.selection_range().is_empty();
        let listed = passes::all(&self.base);
        let mut commands = Vec::new();
        for pass in &listed {
            let name = name_of(pass);
            let scopes = [
                (Scope::Selection, Key::PassOnSelection, selected),
                (Scope::Chapter, Key::PassOnChapter, true),
                (Scope::Book, Key::PassOnBook, whole_book),
            ];
            for (scope, key, offered) in scopes {
                if offered {
                    commands.push(PassCommand {
                        label: crate::t!(key, pass = name),
                        action: PassAction::Run(pass.slug.clone(), scope),
                    });
                }
            }
        }
        for pass in listed.iter().filter(|pass| pass.origin == Origin::BuiltIn) {
            commands.push(PassCommand {
                label: crate::t!(Key::PassCustomize, pass = name_of(pass)),
                action: PassAction::Customize(pass.slug.clone()),
            });
        }
        commands
    }

    /// Copies a built-in pass into the project's `passes/` and opens the copy in the author's editor.
    fn customize_pass(&mut self, slug: &str, cx: &mut Context<Self>) {
        match passes::customize(&self.base, slug) {
            // A scripted run only wants the copy.
            Ok(_) if crate::gate::measuring() => {}
            Ok(file) => cx.open_with_system(&file),
            Err(error) => self.say(strings::pass_not_copied(&error.to_string()), cx),
        }
    }

    /// A line the app says about a pass, at the window's foot for a while and in the conversation for good.
    fn say(&mut self, message: String, cx: &mut Context<Self>) {
        self.conversation.note(message.clone());
        self.pass_result = Some(message);
        self.pass_result_end = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(RESULT_SHOWN).await;
            view.update(cx, |view, cx| {
                view.pass_result = None;
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    /// What the window's foot says about a pass: where it is, or for a while how it ended.
    pub(super) fn pass_line(&self) -> Option<String> {
        (self.pass_run.as_ref().map(PassRun::progress)).or_else(|| self.pass_result.clone())
    }

    /// Starts a pass, once the CLI has been looked for. Nothing else starts one.
    pub(super) fn run_pass(&mut self, slug: &str, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        if self.cli.is_none() {
            self.pass_asked = Some((slug.to_owned(), scope));
            self.ensure_checked(window, cx);
            return;
        }
        if !self.cli_usable() {
            // The card says what is wrong and how to set it up.
            self.open_chat(window, cx);
            return;
        }
        if !self.conversation.is_idle() || self.pass_run.is_some() {
            self.say(strings::turn_busy(self.backend().name()), cx);
            return;
        }
        let Some(listed) = passes::find(&self.base, slug) else {
            return;
        };
        let pass = match &listed.pass {
            Ok(pass) => Pass {
                name: name_of(&listed),
                ..pass.clone()
            },
            Err(problem) => {
                let file = format!("{}/{slug}.md", passes::FOLDER);
                self.say(strings::pass_unusable(&file, problem), cx);
                return;
            }
        };
        self.snapshot_context(cx);
        let steps = self.steps(scope, cx);
        if steps.is_empty() {
            return;
        }
        if scope == Scope::Selection {
            // The text under the selection is about to change, and a key must not type over it.
            let end = self.editor.read(cx).selection_range().end;
            self.editor.update(cx, |editor, cx| editor.jump_to(end, window, cx));
        }
        let label = crate::versions::Label::Words(strings::before_pass(&pass.name));
        crate::versions::label_history(self.editor.read(cx).session(), label, cx);
        self.pass_result = None;
        self.pass_run = Some(PassRun {
            pass,
            total: steps.len(),
            steps: steps.into(),
            current: String::new(),
            done: 0,
            failed: Vec::new(),
            stopped: false,
            chat_session: self.conversation.session_id().map(str::to_owned),
        });
        self.next_step(window, cx);
    }

    /// The chapters a run goes through, in the book's order. A file with several parts is one step.
    fn steps(&self, scope: Scope, cx: &App) -> Vec<Step> {
        let session = self.editor.read(cx).session();
        let chapters = match scope {
            Scope::Book => session.book().to_vec(),
            Scope::Chapter | Scope::Selection => vec![session.active()],
        };
        let mut steps: Vec<Step> = Vec::new();
        for id in chapters {
            let Some(chapter) = session.chapter(id) else {
                continue;
            };
            let file = relative(&self.base, chapter.path());
            if steps.iter().all(|step| step.target.file != file) {
                steps.push(Step {
                    title: session.title_of(id),
                    target: Target { file, selection: None },
                });
            }
        }
        if scope == Scope::Selection {
            let selection = self.context.selection.clone();
            steps.retain(|_| selection.is_some());
            if let Some(step) = steps.first_mut() {
                step.target.selection = selection;
            }
        }
        steps
    }

    /// Starts the next chapter's turn in a new process, or ends the run.
    fn next_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        loop {
            let next = (self.pass_run.as_mut())
                .filter(|run| !run.stopped)
                .and_then(|run| Some((run.steps.pop_front()?, run.pass.clone())));
            let Some((step, pass)) = next else {
                return self.end_pass(cx);
            };
            if let Some(run) = &mut self.pass_run {
                run.current = step.title.clone();
            }
            self.running = None;
            self.conversation.set_session_id(None);
            let sheet = style_sheet::read(&self.base);
            let message = passes::prompt(&pass, &self.base, &step.target, sheet.as_deref());
            let shown = strings::pass_row(&pass.name, &step.title);
            if self.start_turn(shown, message, None, window, cx) {
                return;
            }
            // The CLI did not start. The conversation says why, and the other chapters still get their turn.
            if let Some(run) = &mut self.pass_run {
                run.failed.push(step.title);
            }
        }
    }

    /// A turn of the run ended.
    pub(super) fn pass_turn_ended(&mut self, outcome: Outcome, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = &mut self.pass_run else {
            return;
        };
        match outcome {
            Outcome::Done => run.done += 1,
            Outcome::Failed => run.failed.push(run.current.clone()),
            Outcome::Stopped => run.stopped = true,
        }
        self.next_step(window, cx);
    }

    /// Stop was pressed: the chapters still to come are left out.
    pub(super) fn stop_pass(&mut self) {
        if let Some(run) = &mut self.pass_run {
            run.stopped = true;
        }
    }

    fn end_pass(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.pass_run.take() else {
            return;
        };
        // The chat card's conversation goes on where it was, in a process of its own.
        self.running = None;
        self.conversation.set_session_id(run.chat_session);
        let ended = strings::pass_ended(&run.pass.name, run.done, run.total, &run.failed, run.stopped);
        self.say(ended, cx);
    }
}
