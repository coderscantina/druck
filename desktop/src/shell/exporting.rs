//! Running exports for the hub: the export panel's setup from `druck.yaml`, changes to an export's values, and a run
//! with its progress and result.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use druck::Failure;
use druck::cancel::Cancel;
use druck::diagnostic::Diagnostic;
use druck::input::MANIFEST;
use gpui::{AppContext as _, Context, Entity, Focusable, Task, Window};

use super::Hub;
use super::export::{self, Request, Stage, Target};
use super::export_panel::{ExportEvent, ExportPanel, RunState, Setup};
use super::manifest;
use super::text::Key;
use crate::inspector::Tab;
use crate::session::Kind;

/// How often the export panel hears how far a run is.
const PROGRESS_PAUSE: Duration = Duration::from_millis(100);
/// Stack of the export thread, as the preview's layout thread has: the engine reads a nested theme.
const EXPORT_STACK: usize = 64 * 1024 * 1024;

/// The export panel and what the hub knows of its runs.
pub(super) struct Exporting {
    pub panel: Entity<ExportPanel>,
    /// Why the last export failed, with its errors, for the diagnostics panel.
    pub failure: Option<(String, Vec<Diagnostic>)>,
    /// What the book's diagnostics were when the last export failed. The panel says it failed until they change.
    pub failed_with: Option<u64>,
    /// Why the last change to the exports did not take.
    edit_error: Option<String>,
    running: Option<Running>,
}

impl Exporting {
    pub fn new(panel: Entity<ExportPanel>) -> Self {
        Self {
            panel,
            failure: None,
            failed_with: None,
            edit_error: None,
            running: None,
        }
    }
}

/// A running export.
struct Running {
    target: Target,
    cancel: Cancel,
    stage: Arc<AtomicU8>,
    _tasks: Vec<Task<()>>,
}

impl Hub {
    /// Reads the exports from the manifest as it is on disk, which every change to it writes at once.
    pub(super) fn refresh_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.parts.editor.read(cx).session();
        let setup = match session.kind() {
            Kind::Single { .. } => Setup {
                single: true,
                ..Setup::default()
            },
            Kind::Project { dir } => {
                let read = std::fs::read_to_string(dir.join(MANIFEST))
                    .map_err(|error| error.to_string())
                    .and_then(|text| manifest::read(&text));
                match read {
                    Ok(defs) => Setup {
                        single: false,
                        defs,
                        problem: self.exports.edit_error.clone(),
                        built_in_output: export::built_in_output(dir),
                    },
                    Err(problem) => Setup {
                        single: false,
                        defs: Vec::new(),
                        problem: Some(format!("{} {problem}", crate::t!(Key::ManifestProblem))),
                        built_in_output: String::new(),
                    },
                }
            }
        };
        self.exports
            .panel
            .update(cx, |panel, cx| panel.set_setup(setup, window, cx));
    }

    pub(super) fn export_event(
        &mut self,
        _: &Entity<ExportPanel>,
        event: &ExportEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExportEvent::Run(target) => self.start_export(target.clone(), window, cx),
            ExportEvent::Cancel => {
                if let Some(running) = &self.exports.running {
                    running.cancel.cancel();
                }
            }
            ExportEvent::Set { name, key, value } => self.set_export_field(name, key, value.clone(), window, cx),
            ExportEvent::Add(name) => {
                let output = format!("build/{name}.pdf");
                self.set_export_field(name, "output", Some(output), window, cx);
            }
            ExportEvent::Open(path) => cx.open_with_system(path),
            ExportEvent::Reveal(path) => cx.reveal_path(path),
        }
    }

    /// Changes one value of an export in `druck.yaml` as a text splice the engine must read back as asked.
    fn set_export_field(
        &mut self,
        name: &str,
        key: &'static str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (edit_name, check_name) = (name.to_owned(), name.to_owned());
        let written = value.as_deref().map(|value| manifest::written(key, value));
        let result = self.parts.editor.update(cx, |editor, cx| {
            editor.update_session(
                |session| {
                    session.write_settings(
                        |text| manifest::set_field(text, &edit_name, key, written.as_deref()),
                        |settings| manifest::agrees(settings, &check_name, key, value.as_deref()),
                    )
                },
                window,
                cx,
            )
        });
        self.exports.edit_error = result.err();
        self.refresh_export(window, cx);
    }

    fn start_export(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        if self.exports.running.is_some() {
            return;
        }
        let session = self.parts.editor.read(cx).session();
        let request = Request {
            kind: session.kind().clone(),
            joined: session.joined(),
            target: target.clone(),
        };
        let cancel = Cancel::default();
        let stage = Arc::new(AtomicU8::new(0));
        let started = Instant::now();
        self.exports.failure = None;
        self.exports.failed_with = None;
        self.refresh_report(cx);
        self.exports.panel.update(cx, |panel, cx| {
            panel.set_run(
                RunState::Running {
                    target: target.clone(),
                    stage: Stage::Preparing,
                },
                cx,
            );
        });
        let work = {
            let (cancel, stage) = (cancel.clone(), stage.clone());
            cx.background_spawn(async move {
                std::thread::scope(|scope| {
                    std::thread::Builder::new()
                        .name("druck-export".into())
                        .stack_size(EXPORT_STACK)
                        .spawn_scoped(scope, || {
                            export::run(request, &cancel, &|reached| {
                                stage.store(reached as u8, Ordering::Relaxed)
                            })
                        })
                        .expect("the system starts a thread")
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
            })
        };
        let finish = cx.spawn_in(window, {
            let target = target.clone();
            async move |hub, cx| {
                let result = work.await;
                hub.update_in(cx, |hub, window, cx| {
                    hub.finish_export(&target, result, started, window, cx)
                })
                .ok();
            }
        });
        let progress = cx.spawn(async move |hub, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_PAUSE).await;
                let alive = hub
                    .update(cx, |hub, cx| {
                        let Some(running) = &hub.exports.running else {
                            return false;
                        };
                        let stage = match running.stage.load(Ordering::Relaxed) {
                            0 => Stage::Preparing,
                            1 => Stage::Typesetting,
                            _ => Stage::Writing,
                        };
                        let target = running.target.clone();
                        hub.exports
                            .panel
                            .update(cx, |panel, cx| panel.set_run(RunState::Running { target, stage }, cx));
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        });
        self.exports.running = Some(Running {
            target,
            cancel,
            stage,
            _tasks: vec![finish, progress],
        });
    }

    fn finish_export(
        &mut self,
        target: &Target,
        result: Result<export::Written, Failure>,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.exports.running = None;
        let state = match result {
            Ok(written) => RunState::Done {
                epub: written
                    .output
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("epub")),
                output: written.output,
                parts: written.parts,
                seconds: started.elapsed().as_secs_f32(),
            },
            Err(Failure::Cancelled) => RunState::Cancelled,
            Err(Failure::Errors(errors)) => {
                let name = match target {
                    Target::Named(name) => name.clone(),
                    Target::Pdf => "PDF".to_owned(),
                    Target::Epub => "EPUB".to_owned(),
                };
                self.exports.failure = Some((name, errors));
                let preview = self.parts.preview.read(cx);
                self.exports.failed_with = Some(seen(preview.diagnostics(), preview.warnings()));
                self.parts
                    .inspector
                    .update(cx, |inspector, cx| inspector.show(Tab::Proof, cx));
                RunState::Failed
            }
        };
        let finished = matches!(state, RunState::Done { .. });
        let failed = matches!(state, RunState::Failed);
        self.exports.panel.update(cx, |panel, cx| panel.set_run(state, cx));
        self.refresh_report(cx);
        // The export's errors are the report's first group, so its first error is why it did not run.
        if failed && !self.problem_places.is_empty() {
            self.go_to_problem(0, window, cx);
        }
        self.refresh_export(window, cx);
        // The run button that had the keys is gone. An author who went on to the text keeps them there.
        let in_the_text = self.parts.editor.read(cx).focus_handle(cx).contains_focused(window, cx)
            || self
                .parts
                .preview
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx);
        if finished && !in_the_text {
            self.exports
                .panel
                .update(cx, |panel, cx| panel.focus_result(window, cx));
        }
    }
}

/// A fingerprint of the book's diagnostics: what each says and where.
pub(super) fn seen(errors: &[Diagnostic], warnings: &[Diagnostic]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for diagnostic in errors.iter().chain(warnings) {
        diagnostic.message.hash(&mut hasher);
        (diagnostic.location.map(|at| (at.line, at.column)), diagnostic.page).hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diagnostics_seen_change_with_what_one_says() {
        let before = [Diagnostic::error("no entry has the key `x`")];
        let after = [Diagnostic::error("no entry has the key `y`")];
        assert_eq!(seen(&before, &[]), seen(&before.clone(), &[]));
        assert_ne!(seen(&before, &[]), seen(&after, &[]));
        assert_ne!(seen(&before, &[]), seen(&[], &[]));
    }
}
