//! For scripted runs: a debug-only way to do what a person does with a mouse, which automated mouse events cannot reach on
//! this platform. `DRUCK_AGENT_SCRIPT` names a file the app reads new lines from, one action per line. A shipped build has
//! none of this, see [`crate::gate::var`]. What it finds is written to `<script>.out`.
//!
//! Actions: `ask`, `prompt <text>` (into the card or the selection prompt that is open), `send`, `select <text>`, `show <text>`,
//! `type <text>`, `accept <n>`, `reject <n>`, `again <n>`, `accept-all`, `reject-all`, `next`, `note-apply <n>`,
//! `note-reply <n>`, `note-dismiss <n>`, `allow`, `deny`, `chapter <n>`, `mode write|publish`, `state`.

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{Context, Entity, EntityInputHandler as _, Window};

use super::AgentView;
use crate::editor::review::NoteAction;
use crate::workspace::Workspace;

const POLL: Duration = Duration::from_millis(100);

pub fn start(agent: &Entity<AgentView>, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(script) = crate::gate::var("DRUCK_AGENT_SCRIPT").map(PathBuf::from) else {
        return;
    };
    let agent = agent.clone();
    cx.spawn_in(window, async move |workspace, cx| {
        let mut done = 0;
        loop {
            cx.background_executor().timer(POLL).await;
            let text = std::fs::read_to_string(&script).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            for line in lines.iter().skip(done) {
                let alive = cx.update(|window, cx| {
                    let answer = agent.update(cx, |agent, cx| agent.act(line, &workspace, window, cx));
                    let unreviewed: Vec<String> = (super::unreviewed_changes(cx).iter())
                        .map(|entry| format!("{}={}", entry.name, entry.count))
                        .collect();
                    report(
                        &script,
                        line,
                        &format!("{answer} unreviewed=[{}]", unreviewed.join(", ")),
                    );
                });
                if alive.is_err() {
                    return;
                }
                done += 1;
            }
        }
    })
    .detach();
}

fn report(script: &std::path::Path, line: &str, answer: &str) {
    let mut out = script.as_os_str().to_owned();
    out.push(".out");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(out) {
        writeln!(file, "{line} => {answer}").ok();
    }
}

impl AgentView {
    fn act(
        &mut self,
        line: &str,
        workspace: &gpui::WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let passage = |view: &Self, cx: &Context<Self>| {
            rest.parse::<usize>()
                .ok()
                .and_then(|at| view.editor.read(cx).review().get(at).map(|passage| passage.id))
        };
        match verb {
            "ask" => self.ask(window, cx),
            "prompt" => {
                let input = if self.selection_card.is_some() {
                    self.selection_prompt.clone()
                } else {
                    self.prompt.clone()
                };
                input.update(cx, |input, cx| input.set_text(rest, cx));
            }
            "send" if self.selection_card.is_some() => self.send_selection(window, cx),
            "send" => self.send(window, cx),
            #[cfg(any(debug_assertions, feature = "measure"))]
            "select" => self.editor.update(cx, |editor, cx| {
                if let Some(at) = editor.text().find(rest) {
                    editor.select_for_screenshot(at..at + rest.len(), window, cx);
                }
            }),
            "show" => self.editor.update(cx, |editor, cx| {
                if let Some(at) = editor.text().find(rest) {
                    editor.jump_to(at, window, cx);
                }
            }),
            "type" => self.editor.update(cx, |editor, cx| {
                editor.replace_text_in_range(None, rest, window, cx);
            }),
            "accept" | "reject" | "again" => {
                if let Some(id) = passage(self, cx) {
                    use crate::editor::review::ReviewAction::{Accept, Reject, TryAgain};
                    let action = match verb {
                        "accept" => Accept,
                        "reject" => Reject,
                        _ => TryAgain,
                    };
                    self.review_clicked(id, action, window, cx);
                }
            }
            "accept-all" => self.accept_all(window, cx),
            "reject-all" => self.reject_all(window, cx),
            "next" => self.next_change(window, cx),
            "note-apply" | "note-reply" | "note-dismiss" => {
                let at = rest.parse::<usize>().unwrap_or(0);
                if let Some(id) = self.notes.all().get(at).map(|note| note.id) {
                    let action = match verb {
                        "note-apply" => NoteAction::Apply,
                        "note-reply" => NoteAction::Reply,
                        _ => NoteAction::Dismiss,
                    };
                    self.note_clicked(id, action, window, cx);
                }
            }
            "allow" => {
                if let Some(id) = self.first_waiting() {
                    self.allow(&id, false, cx);
                }
            }
            "deny" => {
                if let Some(id) = self.first_waiting() {
                    self.deny(&id, "", cx);
                }
            }
            "chapter" => {
                let at = rest.parse::<usize>().unwrap_or(0);
                let id = self.editor.read(cx).session().book().get(at).copied();
                if let Some(id) = id {
                    self.editor.update(cx, |editor, cx| editor.show_chapter(id, window, cx));
                }
            }
            "mode" => {
                let mode = if rest == "publish" {
                    crate::workspace::Mode::Publish
                } else {
                    crate::workspace::Mode::Write
                };
                workspace
                    .update(cx, |workspace, cx| workspace.show(mode, window, cx))
                    .ok();
            }
            "state" => return self.state(cx),
            _ => return "unknown".to_owned(),
        }
        "ok".to_owned()
    }

    fn first_waiting(&self) -> Option<String> {
        self.conversation
            .approvals()
            .next()
            .map(|approval| approval.request.request_id.clone())
    }

    fn state(&self, cx: &Context<Self>) -> String {
        let editor = self.editor.read(cx);
        format!(
            "working={} waiting={} passages={} notes={} chat={} process={} chapter={}",
            !self.conversation.is_idle(),
            self.conversation.approvals().count(),
            editor.review().len(),
            self.notes.all().len(),
            self.chat_open,
            self.running.is_some(),
            editor.session().title_of(editor.session().active()),
        )
    }
}
