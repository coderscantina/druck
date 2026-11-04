//! The spike's gate, measured with everything running. Off unless `DRUCK_GATE=1`. Any other value only opens the window
//! as a measurement does, floating without focus, for screenshots.
//!
//! Types into the middle of the open document on a timer, through the same input path as the keyboard, while pages
//! refresh in the background, then prints typing latency, pause to painted pages, theme edit to updated pages, and
//! memory over many refreshes, and quits. `DRUCK_GATE_THEME_EDITS=0` skips the theme edits, for a read-only theme.
//!
//! Times run from when a key was due, so they include any wait for the main thread, to the end of the first paint
//! showing it. Presenting takes one more frame at most. `DRUCK_GATE_REFRESHES` sets the memory run's length.

use std::cell::Cell;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, App, AsyncApp, Entity, EntityInputHandler};

use crate::editor::EditorView;
use crate::inspector::InspectorView;
use crate::preview::{PreviewView, memory};

/// Bursts of typing at about 130 words a minute, with pauses that let a refresh start.
const BURSTS: usize = 60;
const BURST_KEYS: usize = 6;
const KEY_INTERVAL: Duration = Duration::from_millis(90);
/// Longer than the shell's pause, so the next burst lands while the layout runs.
const BETWEEN_BURSTS: Duration = Duration::from_millis(300);
const PAUSE_SAMPLES: usize = 30;
const THEME_SAMPLES: usize = 12;
const REFRESHES: usize = 200;
const POLL: Duration = Duration::from_millis(1);
/// Longer than any layout should take, so a stuck run reports rather than hangs.
const GIVE_UP: Duration = Duration::from_secs(10);
const MARK: &str = "QQQ";

struct Gate {
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    window: AnyWindowHandle,
    /// Keys typed by [`Gate::letter`].
    letters: Cell<usize>,
}

pub fn start(
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    if std::env::var("DRUCK_GATE").as_deref() != Ok("1") {
        return;
    }
    let gate = Gate {
        editor,
        preview,
        inspector,
        window,
        letters: Cell::new(0),
    };
    cx.spawn(async move |cx| {
        gate.run(cx).await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

impl Gate {
    async fn run(&self, cx: &mut AsyncApp) {
        self.wait_for_pages(cx).await;
        eprintln!("gate: first pages; {}", memory());
        self.place_caret(cx).await;
        if std::env::var_os("DRUCK_GATE_THEME_ONLY").is_none() {
            self.typing(cx).await;
            self.pause_to_pages(cx).await;
        }
        if std::env::var("DRUCK_GATE_THEME_EDITS").as_deref() != Ok("0") {
            self.theme_edits(cx).await;
        }
        self.memory(cx).await;
    }

    async fn sleep(&self, duration: Duration, cx: &mut AsyncApp) {
        cx.background_executor().timer(duration).await;
    }

    /// Waits until the newest request's pages are on screen, and returns when that was.
    async fn wait_for_pages(&self, cx: &mut AsyncApp) -> Instant {
        let start = Instant::now();
        loop {
            let painted = self.preview.read_with(cx, |preview, _| preview.painted_at());
            if let Some(painted) = painted {
                return painted;
            }
            if start.elapsed() > GIVE_UP {
                let problem = self
                    .preview
                    .read_with(cx, |preview, _| preview.diagnostics().first().map(ToString::to_string));
                eprintln!("gate: no pages after {GIVE_UP:?}: {problem:?}");
                return Instant::now();
            }
            self.sleep(POLL, cx).await;
        }
    }

    fn key(&self, text: &str, cx: &mut AsyncApp) -> u64 {
        let editor = self.editor.clone();
        self.window
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.replace_text_in_range(None, text, window, cx);
                    editor.revision()
                })
            })
            .unwrap_or_default()
    }

    /// Types words of six letters. One endless word would stop fitting on a line, and the engine would reject it.
    fn letter(&self, cx: &mut AsyncApp) {
        let typed = self.letters.get();
        self.letters.set(typed + 1);
        self.key(if typed % 7 == 6 { " " } else { "e" }, cx);
    }

    /// Puts the caret at a paragraph end in the middle of the middle chapter, and shows that page.
    async fn place_caret(&self, cx: &mut AsyncApp) {
        let editor = self.editor.clone();
        self.window
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| {
                    let book = editor.session().book();
                    let middle_chapter = book[book.len() / 2];
                    editor.show_chapter(middle_chapter, window, cx);
                    let text = editor.text();
                    // Chapters open with long comments, whose text never reaches a page.
                    let body = text.rfind("-->").map_or(0, |end| end + 3);
                    let middle = text.floor_char_boundary(body + (text.len() - body) / 2);
                    let at = text[middle..].find(".\n").map_or(middle, |found| middle + found + 1);
                    editor.jump_to(at, window, cx);
                })
            })
            .ok();
        self.key(&format!(" {MARK} "), cx);
        self.sleep(Duration::from_millis(200), cx).await;
        self.wait_for_pages(cx).await;
        let page = self.preview.update(cx, |preview, cx| preview.show_text(MARK, cx));
        eprintln!("gate: typing on page {page:?}");
        self.sleep(Duration::from_millis(200), cx).await;
    }

    async fn typing(&self, cx: &mut AsyncApp) {
        let mut idle = Vec::new();
        let mut during = Vec::new();
        let mut late = Vec::new();
        let mut next = Instant::now();
        for _ in 0..BURSTS {
            for key in 0..BURST_KEYS {
                self.sleep(next.saturating_duration_since(Instant::now()), cx).await;
                let due = next;
                late.push(Instant::now() - due);
                let laying_out = self.preview.read_with(cx, |preview, _| preview.is_laying_out());
                let revision = self.key(if key + 1 == BURST_KEYS { " " } else { "e" }, cx);
                loop {
                    let painted = self
                        .editor
                        .read_with(cx, |editor, _| editor.revision_painted_at(revision));
                    if let Some(painted) = painted {
                        let latency = painted.saturating_duration_since(due);
                        if laying_out { &mut during } else { &mut idle }.push(latency);
                        break;
                    }
                    if due.elapsed() > GIVE_UP {
                        eprintln!("gate: a key was not painted after {GIVE_UP:?}");
                        return;
                    }
                    self.sleep(POLL, cx).await;
                }
                next = due + KEY_INTERVAL;
            }
            next += BETWEEN_BURSTS;
        }
        let all: Vec<_> = idle.iter().chain(&during).copied().collect();
        report("typing, all keys", all);
        report("typing, during a layout", during);
        report("typing, no layout running", idle);
        report("key timer lateness", late);
        eprintln!("gate: after typing; {}", memory());
    }

    /// Lets everything settle so the next sample starts with nothing running.
    async fn settle(&self, cx: &mut AsyncApp) {
        self.sleep(Duration::from_millis(250), cx).await;
        self.wait_for_pages(cx).await;
    }

    async fn pause_to_pages(&self, cx: &mut AsyncApp) {
        self.settle(cx).await;
        let mut quiet = Vec::new();
        for _ in 0..PAUSE_SAMPLES {
            self.letter(cx);
            let last = Instant::now();
            self.sleep(Duration::from_millis(200), cx).await;
            quiet.push(self.wait_for_pages(cx).await.saturating_duration_since(last));
            self.settle(cx).await;
        }
        report("last key to painted pages, nothing running at the key", quiet);

        // The last key lands just after a layout started, whose request then cancels that layout.
        let mut busy = Vec::new();
        for _ in 0..PAUSE_SAMPLES {
            self.letter(cx);
            while !self.preview.read_with(cx, |preview, _| preview.is_laying_out()) {
                self.sleep(POLL, cx).await;
            }
            self.letter(cx);
            let last = Instant::now();
            self.sleep(Duration::from_millis(200), cx).await;
            busy.push(self.wait_for_pages(cx).await.saturating_duration_since(last));
            self.settle(cx).await;
        }
        report("last key to painted pages, key just after a layout started", busy);
    }

    async fn theme_edits(&self, cx: &mut AsyncApp) {
        let mut pages = Vec::new();
        let mut galley = Vec::new();
        for sample in 0..THEME_SAMPLES {
            self.settle(cx).await;
            let generation = self.editor.read_with(cx, |editor, _| editor.style_generation());
            // Differs from the last sample every time, and stays near a real body size.
            let size = format!("{}pt", 10.5 + (sample % 4) as f32 * 0.25 + 0.1);
            let start = Instant::now();
            self.inspector
                .update(cx, |inspector, cx| inspector.set_body_size(&size, cx));
            self.sleep(Duration::from_millis(5), cx).await;
            pages.push(self.wait_for_pages(cx).await.saturating_duration_since(start));
            loop {
                let painted = self
                    .editor
                    .read_with(cx, |editor, _| editor.style_painted_at(generation + 1));
                if let Some(painted) = painted {
                    galley.push(painted.saturating_duration_since(start));
                    break;
                }
                if start.elapsed() > GIVE_UP {
                    eprintln!("gate: the galley did not change after {size}");
                    break;
                }
                self.sleep(POLL, cx).await;
            }
        }
        report("theme edit to painted pages", pages);
        report("theme edit to painted galley", galley);
    }

    async fn memory(&self, cx: &mut AsyncApp) {
        let refreshes = std::env::var("DRUCK_GATE_REFRESHES")
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(REFRESHES);
        self.settle(cx).await;
        eprintln!("gate: refresh 0; {}", memory());
        for refresh in 1..=refreshes {
            self.letter(cx);
            self.settle(cx).await;
            if refresh % 25 == 0 {
                eprintln!("gate: refresh {refresh}; {}", memory());
            }
        }
    }
}

fn report(name: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let at = |share: f64| {
        let index = ((samples.len() as f64 - 1.0) * share).round() as usize;
        samples.get(index).copied().unwrap_or_default()
    };
    eprintln!(
        "gate: {name}: {} samples, median {:?}, p95 {:?}, max {:?}",
        samples.len(),
        at(0.5),
        at(0.95),
        at(1.0)
    );
}
